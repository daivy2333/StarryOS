use alloc::string::String;
use core::{
    sync::atomic::{AtomicUsize, Ordering},
    task::Waker,
};

use axdriver::prelude::*;
use axdriver_net::{NetQueueControl, TxCookie};
use axtask::future::register_irq_waker;
use hashbrown::HashMap;
use smoltcp::{
    storage::PacketBuffer,
    time::{Duration, Instant},
    wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetAddress, EthernetFrame, EthernetProtocol,
        EthernetRepr, IpAddress, Ipv4Address, Ipv4Cidr,
    },
};

use crate::{
    consts::{ETHERNET_MAX_PENDING_PACKETS, STANDARD_MTU},
    device::{
        Device, RxCopyStep, RxStep, TxDropReason, TxOutcome, TxPreflight, TxReclaimStep,
        TxSubmitStep,
        fixed_queue::{FixedFrameQueue, MAX_FRAME_SIZE, TicketTracker},
    },
};

fn tx_outcome_from_err(err: DevError) -> TxOutcome {
    match err {
        DevError::Again => TxOutcome::Full,
        DevError::InvalidParam => TxOutcome::Dropped(TxDropReason::FrameTooLarge),
        err => TxOutcome::Fault(err),
    }
}

const EMPTY_MAC: EthernetAddress = EthernetAddress([0; 6]);

/// Outcome of processing one RX frame in the transactional slot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameStep {
    /// The frame was fully processed and its RX slot may be released.
    Consumed,
    /// An IP payload was delivered to the Router RX buffer.
    Delivered,
    /// A TX obligation (the ARP reply) was deferred; the RX slot must be
    /// retained so a later retry commits it exactly once.
    Deferred,
}

/// Whether processing an ARP frame fully committed or deferred a TX
/// obligation owed by that frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArpProgress {
    /// All obligations of this frame committed.
    Complete,
    /// The owed ARP reply could not be submitted; the RX frame is retained.
    Deferred,
}

struct Neighbor {
    hardware_address: EthernetAddress,
    expires_at: Instant,
}

/// How the device submits complete frames to the hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TxMode {
    /// Synchronous recycle→alloc→transmit against the raw driver.
    Polling,
    /// Emit into the fixed dormant TX slot storage (host-test seam only).
    /// The product cutover to this mode is owned by Task 3.1.
    #[allow(dead_code)]
    DormantSlots,
}

pub struct EthernetDevice {
    name: String,
    inner: AxNetDevice,
    neighbors: HashMap<IpAddress, Option<Neighbor>>,
    ip: Ipv4Cidr,

    pending_packets: FixedFrameQueue<ETHERNET_MAX_PENDING_PACKETS, IpAddress>,

    /// Fixed RX slot storage (Task 2.1). Consumed only in dormant mode.
    #[allow(dead_code)]
    rx_slots: FixedFrameQueue<64>,
    /// Fixed TX slot storage (Task 2.1). Consumed only in dormant mode.
    tx_slots: FixedFrameQueue<64>,
    /// Checked monotonic tickets for accepted dormant TX frames.
    tx_tickets: TicketTracker,
    tx_mode: TxMode,
    /// Host-test witness for deferred RX retry counting (Task 3.6).
    #[cfg(test)]
    recv_dormant_calls: AtomicUsize,
}
impl EthernetDevice {
    const NEIGHBOR_TTL: Duration = Duration::from_secs(60);

    pub fn new(name: String, inner: AxNetDevice, ip: Ipv4Cidr) -> Self {
        Self {
            name,
            inner,
            neighbors: HashMap::new(),
            ip,
            pending_packets: FixedFrameQueue::new_with(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED)),
            rx_slots: FixedFrameQueue::new(),
            tx_slots: FixedFrameQueue::new(),
            tx_tickets: TicketTracker::new(),
            tx_mode: TxMode::Polling,
            #[cfg(test)]
            recv_dormant_calls: AtomicUsize::new(0),
        }
    }

    /// Borrows the dormant slot storage for host tests. Product builds never
    /// touch the slots.
    #[cfg(test)]
    pub(super) fn slots_for_test(&self) -> (&FixedFrameQueue<64>, &FixedFrameQueue<64>) {
        (&self.rx_slots, &self.tx_slots)
    }

    /// Pushes a raw frame into the RX slot storage (host-test seam for the
    /// dormant RX transaction path). Product activation is owned by Task 3.1.
    #[cfg(test)]
    pub(super) fn push_rx_frame_for_test(&mut self, frame: &[u8]) -> bool {
        self.rx_slots.enqueue(frame, (), None).is_ok()
    }

    /// Pops one TX slot (host-test seam to free capacity for the dormant RX
    /// retry path).
    #[cfg(test)]
    pub(super) fn pop_tx_slot_for_test(&mut self) -> bool {
        self.tx_slots.pop().is_some()
    }

    /// Activates dormant slot mode for host tests. Product activation is
    /// owned by Task 3.1.
    #[cfg(test)]
    pub(super) fn set_dormant_slots_for_test(&mut self) {
        self.tx_mode = TxMode::DormantSlots;
    }

    #[inline]
    fn hardware_address(&self) -> EthernetAddress {
        EthernetAddress(self.inner.mac_address().0)
    }

    /// Emits and submits one complete Ethernet frame.
    ///
    /// In polling mode this recycles, allocates and transmits synchronously.
    /// In dormant mode it writes the frame into a fixed TX slot and returns a
    /// checked ticket. The caller must treat every non-`Accepted` result as
    /// "frame not submitted" and must not advance any packet/neighbor state.
    fn emit_frame<F>(
        &mut self,
        dst: EthernetAddress,
        size: usize,
        f: F,
        proto: EthernetProtocol,
    ) -> TxOutcome
    where
        F: FnOnce(&mut [u8]),
    {
        match self.tx_mode {
            TxMode::Polling => self.emit_frame_polling(dst, size, f, proto),
            TxMode::DormantSlots => self.emit_frame_dormant(dst, size, f, proto),
        }
    }

    fn emit_frame_polling<F>(
        &mut self,
        dst: EthernetAddress,
        size: usize,
        f: F,
        proto: EthernetProtocol,
    ) -> TxOutcome
    where
        F: FnOnce(&mut [u8]),
    {
        let outcome = (|| -> DevResult {
            self.inner.recycle_tx_buffers()?;
            let repr = EthernetRepr {
                src_addr: EthernetAddress(self.inner.mac_address().0),
                dst_addr: dst,
                ethertype: proto,
            };
            let mut tx_buf = self.inner.alloc_tx_buffer(repr.buffer_len() + size)?;
            let mut frame = EthernetFrame::new_unchecked(tx_buf.packet_mut());
            repr.emit(&mut frame);
            f(frame.payload_mut());
            trace!(
                "SEND {} bytes: {:02X?}",
                tx_buf.packet_len(),
                tx_buf.packet()
            );
            self.inner.transmit(tx_buf)
        })();
        match outcome {
            Ok(()) => TxOutcome::Accepted {
                rx_became_ready: false,
            },
            Err(err) => tx_outcome_from_err(err),
        }
    }

    fn emit_frame_dormant<F>(
        &mut self,
        dst: EthernetAddress,
        size: usize,
        f: F,
        proto: EthernetProtocol,
    ) -> TxOutcome
    where
        F: FnOnce(&mut [u8]),
    {
        let repr = EthernetRepr {
            src_addr: self.hardware_address(),
            dst_addr: dst,
            ethertype: proto,
        };
        let frame_len = repr.buffer_len() + size;
        if self.tx_slots.preflight(frame_len).is_err() {
            return TxOutcome::Full;
        }
        let ticket = match self.tx_tickets.alloc() {
            Ok(ticket) => ticket,
            Err(_) => return TxOutcome::Full,
        };
        // Write the frame directly into the reserved vacant slot and publish
        // length/ticket only after emission succeeds (no intermediate Vec).
        match self.tx_slots.fill((), Some(ticket), |region| {
            let mut frame = EthernetFrame::new_unchecked(&mut region[..frame_len]);
            repr.emit(&mut frame);
            f(frame.payload_mut());
            Ok(frame_len)
        }) {
            Ok(_) => TxOutcome::Accepted {
                rx_became_ready: false,
            },
            Err(_) => {
                // Preflight promised room; a failed fill is invariant drift,
                // so release the ticket and report a stable fault.
                let _ = self.tx_tickets.release(ticket);
                TxOutcome::Fault(DevError::BadState)
            }
        }
    }

    fn handle_frame(
        &mut self,
        frame: &[u8],
        buffer: &mut PacketBuffer<()>,
        timestamp: Instant,
    ) -> Result<FrameStep, DevError> {
        let frame = EthernetFrame::new_unchecked(frame);
        let Ok(repr) = EthernetRepr::parse(&frame) else {
            warn!("Dropping malformed Ethernet frame");
            return Ok(FrameStep::Consumed);
        };

        if !repr.dst_addr.is_broadcast()
            && repr.dst_addr != EMPTY_MAC
            && repr.dst_addr != self.hardware_address()
        {
            return Ok(FrameStep::Consumed);
        }

        match repr.ethertype {
            EthernetProtocol::Ipv4 => {
                let Ok(dst) = buffer.enqueue(frame.payload().len(), ()) else {
                    return Err(DevError::BadState);
                };
                dst.copy_from_slice(frame.payload());
                Ok(FrameStep::Delivered)
            }
            EthernetProtocol::Arp => {
                let progress = self.process_arp(frame.payload(), timestamp);
                match progress {
                    ArpProgress::Complete => Ok(FrameStep::Consumed),
                    // A TX obligation (the ARP reply) was deferred: the RX
                    // frame must be retained for a later retry.
                    ArpProgress::Deferred => Ok(FrameStep::Deferred),
                }
            }
            _ => Ok(FrameStep::Consumed),
        }
    }

    fn request_arp(&mut self, target_ip: IpAddress) -> TxOutcome {
        let IpAddress::Ipv4(target_ipv4) = target_ip else {
            warn!("IPv6 address ARP is not supported: {}", target_ip);
            return TxOutcome::Dropped(TxDropReason::UnsupportedAddress);
        };
        debug!("Requesting ARP for {}", target_ipv4);

        let arp_repr = ArpRepr::EthernetIpv4 {
            operation: ArpOperation::Request,
            source_hardware_addr: self.hardware_address(),
            source_protocol_addr: self.ip.address(),
            target_hardware_addr: EthernetAddress::BROADCAST,
            target_protocol_addr: target_ipv4,
        };

        let outcome = self.emit_frame(
            EthernetAddress::BROADCAST,
            arp_repr.buffer_len(),
            |buf| arp_repr.emit(&mut ArpPacket::new_unchecked(buf)),
            EthernetProtocol::Arp,
        );
        // Only a submitted request records the pending (None) neighbor.
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            self.neighbors.insert(target_ip, None);
        }
        outcome
    }

    fn process_arp(&mut self, payload: &[u8], now: Instant) -> ArpProgress {
        let Ok(repr) = ArpPacket::new_checked(payload).and_then(|packet| ArpRepr::parse(&packet))
        else {
            warn!("Dropping malformed ARP packet");
            return ArpProgress::Complete;
        };

        if let ArpRepr::EthernetIpv4 {
            operation,
            source_hardware_addr,
            source_protocol_addr,
            target_hardware_addr,
            target_protocol_addr,
        } = repr
        {
            let is_unicast_mac =
                target_hardware_addr != EMPTY_MAC && !target_hardware_addr.is_broadcast();
            if is_unicast_mac && self.hardware_address() != target_hardware_addr {
                // Only process packet that are for us
                return ArpProgress::Complete;
            }

            if let ArpOperation::Unknown(_) = operation {
                return ArpProgress::Complete;
            }

            if !source_hardware_addr.is_unicast()
                || source_protocol_addr.is_broadcast()
                || source_protocol_addr.is_multicast()
                || source_protocol_addr.is_unspecified()
            {
                return ArpProgress::Complete;
            }
            if self.ip.address() != target_protocol_addr {
                return ArpProgress::Complete;
            }

            debug!("ARP: {} -> {}", source_protocol_addr, source_hardware_addr);

            // A reply carries no TX obligation: record the neighbor directly.
            if let ArpOperation::Reply = operation {
                self.neighbors.insert(
                    IpAddress::Ipv4(source_protocol_addr),
                    Some(Neighbor {
                        hardware_address: source_hardware_addr,
                        expires_at: now + Self::NEIGHBOR_TTL,
                    }),
                );
            }

            // An ARP request owes a reply; only an accepted reply resolves the
            // neighbor (transactional: a Full reply must not update state).
            if let ArpOperation::Request = operation {
                let response = ArpRepr::EthernetIpv4 {
                    operation: ArpOperation::Reply,
                    source_hardware_addr: self.hardware_address(),
                    source_protocol_addr: self.ip.address(),
                    target_hardware_addr: source_hardware_addr,
                    target_protocol_addr: source_protocol_addr,
                };

                let reply = self.emit_frame(
                    source_hardware_addr,
                    response.buffer_len(),
                    |buf| response.emit(&mut ArpPacket::new_unchecked(buf)),
                    EthernetProtocol::Arp,
                );
                if !matches!(reply, TxOutcome::Accepted { .. }) {
                    warn!("ARP reply not accepted: {reply:?}");
                    // The TX obligation is deferred: the RX frame must stay at
                    // the head so a later retry commits the reply exactly once.
                    return ArpProgress::Deferred;
                }
                self.neighbors.insert(
                    IpAddress::Ipv4(source_protocol_addr),
                    Some(Neighbor {
                        hardware_address: source_hardware_addr,
                        expires_at: now + Self::NEIGHBOR_TTL,
                    }),
                );
            }

            // Flush pending packets to the now-resolved neighbor. Each flush
            // only dequeues after the frame was accepted.
            loop {
                let Some((next_hop, _)) = self.pending_packets.peek_meta() else {
                    break;
                };
                if next_hop != IpAddress::Ipv4(source_protocol_addr) {
                    break;
                }
                let Some(Some(neighbor)) = self.neighbors.get(&next_hop) else {
                    break;
                };
                if neighbor.expires_at <= now {
                    // Neighbor is expired, we need to request ARP again.
                    let outcome = self.request_arp(next_hop);
                    if !matches!(outcome, TxOutcome::Accepted { .. }) {
                        warn!("expired-neighbor ARP request not accepted: {outcome:?}");
                    }
                    break;
                }

                let flush = self.emit_pending_head(neighbor.hardware_address);
                if !matches!(flush, TxOutcome::Accepted { .. }) {
                    warn!("pending flush not accepted: {flush:?}");
                    break;
                }
                let _ = self.pending_packets.pop();
            }
        }
        ArpProgress::Complete
    }

    /// Emits the pending head IPv4 frame to `dst` without copying the payload
    /// out of pending storage.
    ///
    /// The pending slot is read while the TX path is written, so `self` is
    /// field-split and no intermediate buffer is allocated. Only the frame
    /// emission is performed here; dequeuing the pending head stays with the
    /// caller after an `Accepted` result.
    fn emit_pending_head(&mut self, dst: EthernetAddress) -> TxOutcome {
        let Self {
            pending_packets,
            tx_slots,
            tx_tickets,
            inner,
            tx_mode,
            ..
        } = self;
        let Some((_, buf)) = pending_packets.peek_meta() else {
            return TxOutcome::Accepted {
                rx_became_ready: false,
            };
        };
        let repr = EthernetRepr {
            src_addr: EthernetAddress(inner.mac_address().0),
            dst_addr: dst,
            ethertype: EthernetProtocol::Ipv4,
        };
        let frame_len = repr.buffer_len() + buf.len();
        match tx_mode {
            TxMode::Polling => {
                let outcome = (|| -> DevResult {
                    inner.recycle_tx_buffers()?;
                    let mut tx_buf = inner.alloc_tx_buffer(frame_len)?;
                    let mut frame = EthernetFrame::new_unchecked(tx_buf.packet_mut());
                    repr.emit(&mut frame);
                    frame.payload_mut().copy_from_slice(buf);
                    inner.transmit(tx_buf)
                })();
                match outcome {
                    Ok(()) => TxOutcome::Accepted {
                        rx_became_ready: false,
                    },
                    Err(err) => tx_outcome_from_err(err),
                }
            }
            TxMode::DormantSlots => {
                if tx_slots.preflight(frame_len).is_err() {
                    return TxOutcome::Full;
                }
                let ticket = match tx_tickets.alloc() {
                    Ok(ticket) => ticket,
                    Err(_) => return TxOutcome::Full,
                };
                match tx_slots.fill((), Some(ticket), |region| {
                    let mut frame = EthernetFrame::new_unchecked(&mut region[..frame_len]);
                    repr.emit(&mut frame);
                    frame.payload_mut().copy_from_slice(buf);
                    Ok(frame_len)
                }) {
                    Ok(_) => TxOutcome::Accepted {
                        rx_became_ready: false,
                    },
                    Err(_) => {
                        let _ = tx_tickets.release(ticket);
                        TxOutcome::Fault(DevError::BadState)
                    }
                }
            }
        }
    }

    fn preflight_ready_tx(&mut self) -> TxPreflight {
        match self.tx_mode {
            // The polling stack owns the raw TX path: recycle completed
            // buffers and report the driver's transmit capacity.
            TxMode::Polling => match self.inner.recycle_tx_buffers() {
                Ok(()) if self.inner.can_transmit() => TxPreflight::Ready,
                Ok(()) => TxPreflight::Full,
                Err(err) => TxPreflight::Fault(err),
            },
            // In slot mode the stack never touches the raw queue: the queue
            // task alone owns raw TX completions. Readiness depends only on
            // fixed TX slot capacity and checked ticket headroom.
            TxMode::DormantSlots => {
                if self.tx_slots.is_full() || !self.tx_tickets.can_alloc() {
                    TxPreflight::Full
                } else {
                    TxPreflight::Ready
                }
            }
        }
    }

    fn preflight_unknown_neighbor(
        &mut self,
        _next_hop: IpAddress,
        timestamp: Instant,
    ) -> TxPreflight {
        // ARP request transmit capacity plus pending storage capacity.
        let tx = self.preflight_ready_tx();
        if !matches!(tx, TxPreflight::Ready) {
            return tx;
        }
        if self.pending_packets.is_full() {
            return TxPreflight::Full;
        }
        let _ = timestamp;
        TxPreflight::Ready
    }

    /// Preflight for an already-requested neighbor (pending wait): only the
    /// pending storage capacity matters, never raw TX capacity.
    fn preflight_requested_neighbor(&mut self) -> TxPreflight {
        if self.pending_packets.is_full() {
            TxPreflight::Full
        } else {
            TxPreflight::Ready
        }
    }

    /// Polling RX: reaps one raw completion, handles the frame, recycles the
    /// driver buffer unconditionally.
    fn recv_polling(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep {
        let rx_buf = match self.inner.receive() {
            Ok(buf) => buf,
            Err(DevError::Again) => return RxStep::Empty,
            Err(err) => return RxStep::Fault(err),
        };
        trace!(
            "RECV {} bytes: {:02X?}",
            rx_buf.packet_len(),
            rx_buf.packet()
        );

        let frame_result = self.handle_frame(rx_buf.packet(), buffer, timestamp);
        let recycle_result = self.inner.recycle_rx_buffer(rx_buf);
        match (frame_result, recycle_result) {
            (_, Err(err)) => RxStep::Fault(err),
            // A deferred ARP reply still recycles the polling driver buffer
            // (the raw path has no slot to retain); the peer may re-request.
            (Ok(FrameStep::Consumed | FrameStep::Deferred), Ok(())) => RxStep::Consumed,
            (Ok(FrameStep::Delivered), Ok(())) => RxStep::Delivered,
            (Err(err), Ok(())) => RxStep::Fault(err),
        }
    }

    /// Dormant slot RX: peeks the fixed RX head and pops it only after
    /// `handle_frame` completes transactionally. A deferred ARP reply keeps
    /// the exact frame bytes at the head for a later retry.
    fn recv_dormant(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep {
        #[cfg(test)]
        self.recv_dormant_calls.fetch_add(1, Ordering::Relaxed);
        // Copy the head frame out of the slot so `handle_frame` can mutate
        // `self` (it may emit an ARP reply into the TX slots). The copy is a
        // fixed-size stack buffer, never a heap allocation.
        let Some((_, ticket, head)) = self.rx_slots.peek_full() else {
            return RxStep::Empty;
        };
        let len = head.len();
        let mut scratch = [0u8; MAX_FRAME_SIZE];
        scratch[..len].copy_from_slice(head);
        let _ = ticket;
        match self.handle_frame(&scratch[..len], buffer, timestamp) {
            Ok(FrameStep::Consumed) => {
                let _ = self.rx_slots.pop();
                RxStep::Consumed
            }
            Ok(FrameStep::Delivered) => {
                let _ = self.rx_slots.pop();
                RxStep::Delivered
            }
            // The owed reply could not be submitted: retain the exact frame
            // bytes at the head and report the distinct `Blocked` step so the
            // Router stops its RX loop for this device (Task 3.6). A later
            // poll retries the reply exactly once when TX capacity frees.
            Ok(FrameStep::Deferred) => RxStep::Blocked,
            Err(err) => RxStep::Fault(err),
        }
    }
}

impl Device for EthernetDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep {
        match self.tx_mode {
            TxMode::Polling => self.recv_polling(buffer, timestamp),
            TxMode::DormantSlots => self.recv_dormant(buffer, timestamp),
        }
    }

    fn preflight_send(
        &mut self,
        next_hop: IpAddress,
        packet: &[u8],
        timestamp: Instant,
    ) -> TxPreflight {
        // The L3 payload is bounded by the IP MTU; the fixed queue separately
        // checks the complete L2 frame against MAX_FRAME_SIZE.
        if packet.len() > STANDARD_MTU {
            return TxPreflight::Drop(TxDropReason::FrameTooLarge);
        }
        let is_broadcast =
            next_hop.is_broadcast() || self.ip.broadcast().map(IpAddress::Ipv4) == Some(next_hop);
        if !is_broadcast {
            match self.neighbors.get(&next_hop) {
                // IPv6 always requires ARP, which this device cannot serve.
                None if matches!(next_hop, IpAddress::Ipv6(_)) => {
                    return TxPreflight::Drop(TxDropReason::UnsupportedAddress);
                }
                None => return self.preflight_unknown_neighbor(next_hop, timestamp),
                // Request already sent: only pending capacity matters, so raw
                // TX full does not falsely backpressure the enqueue.
                Some(None) => return self.preflight_requested_neighbor(),
                Some(Some(neighbor)) if neighbor.expires_at <= timestamp => {
                    return self.preflight_unknown_neighbor(next_hop, timestamp);
                }
                _ => {}
            }
        }
        self.preflight_ready_tx()
    }

    fn send(&mut self, next_hop: IpAddress, packet: &[u8], timestamp: Instant) -> TxOutcome {
        if packet.len() > STANDARD_MTU {
            return TxOutcome::Dropped(TxDropReason::FrameTooLarge);
        }
        if next_hop.is_broadcast() || self.ip.broadcast().map(IpAddress::Ipv4) == Some(next_hop) {
            return self.emit_frame(
                EthernetAddress::BROADCAST,
                packet.len(),
                |buf| buf.copy_from_slice(packet),
                EthernetProtocol::Ipv4,
            );
        }

        let need_request = match self.neighbors.get(&next_hop) {
            Some(Some(neighbor)) => {
                if neighbor.expires_at > timestamp {
                    return self.emit_frame(
                        neighbor.hardware_address,
                        packet.len(),
                        |buf| buf.copy_from_slice(packet),
                        EthernetProtocol::Ipv4,
                    );
                } else {
                    true
                }
            }
            // Request already sent (pending wait) or unknown: re-request only
            // when the request was not already accepted.
            Some(None) => false,
            None => true,
        };
        // Only send ARP request if we haven't already requested it.
        if need_request {
            match self.request_arp(next_hop) {
                TxOutcome::Accepted { .. } => {}
                outcome => return outcome,
            }
        }
        if self.pending_packets.is_full() {
            warn!("Pending packets buffer is full, dropping packet");
            return TxOutcome::Full;
        }
        match self.pending_packets.enqueue(packet, next_hop, None) {
            Ok(()) => TxOutcome::Accepted {
                rx_became_ready: false,
            },
            Err(_) => {
                warn!("Failed to enqueue packet in pending packets buffer");
                TxOutcome::Full
            }
        }
    }

    fn requires_polling(&self) -> bool {
        self.inner.irq_num().is_none()
    }

    fn activate_slot_mode(&mut self) -> DevResult {
        self.tx_mode = TxMode::DormantSlots;
        Ok(())
    }

    fn rx_copy_one(&mut self) -> RxCopyStep {
        // A full RX slot never reaps a raw completion: no frame is dropped.
        if self.rx_slots.is_full() {
            return RxCopyStep::Full;
        }
        let rx_buf = match self.inner.receive() {
            Ok(buf) => buf,
            Err(DevError::Again) => return RxCopyStep::Empty,
            Err(err) => return RxCopyStep::Fault(err),
        };
        let len = rx_buf.packet_len();
        let payload = rx_buf.packet();
        // Copy the frame into the RX slot and refill the driver buffer.
        let copied = self.rx_slots.fill((), None, |region| {
            region[..len].copy_from_slice(payload);
            Ok(len)
        });
        // SAFETY: `rx_buf` came from `receive` and is returned to the driver
        // exactly once, matching the receive/refill pairing.
        let recycle = self.inner.recycle_rx_buffer(rx_buf);
        match (copied, recycle) {
            (Ok(_), Ok(())) => RxCopyStep::Copied,
            (_, Err(err)) => RxCopyStep::Fault(err),
            (Err(_), Ok(())) => RxCopyStep::Fault(DevError::BadState),
        }
    }

    fn tx_submit_one(&mut self) -> TxSubmitStep {
        let Some((_, ticket, frame)) = self.tx_slots.peek_full() else {
            return TxSubmitStep::Empty;
        };
        // TX slots always carry a ticket; a missing ticket is invariant drift.
        let Some(ticket) = ticket else {
            return TxSubmitStep::Fault(DevError::BadState);
        };
        let mut tx_buf = match self.inner.alloc_tx_buffer(frame.len()) {
            Ok(buf) => buf,
            Err(DevError::Again) => return TxSubmitStep::Full,
            Err(err) => return TxSubmitStep::Fault(err),
        };
        tx_buf.packet_mut().copy_from_slice(frame);
        let Some(tx_queue) = self.inner.tx_queue() else {
            return TxSubmitStep::Fault(DevError::Unsupported);
        };
        match tx_queue.submit_tx(tx_buf, TxCookie::new(ticket)) {
            // On submit the driver owns the buffer; the slot pops and the
            // ticket stays live until the matching completion is reclaimed.
            Ok(()) => {
                let _ = self.tx_slots.pop();
                TxSubmitStep::Submitted
            }
            // A pre-accept `Again` returns the buffer to the driver's free
            // set and retains the slot frame.
            Err(DevError::Again) => TxSubmitStep::Full,
            Err(err) => TxSubmitStep::Fault(err),
        }
    }

    fn tx_reclaim_one(&mut self) -> TxReclaimStep {
        let Some(tx_queue) = self.inner.tx_queue() else {
            return TxReclaimStep::Fault(DevError::Unsupported);
        };
        match tx_queue.reclaim_tx() {
            Ok(Some(cookie)) => {
                // The completion cookie must match exactly one live ticket.
                // An unknown or duplicate cookie is an ownership invariant
                // violation: report a stable fault instead of a success.
                if self.tx_tickets.release(cookie.value()) {
                    TxReclaimStep::Reclaimed
                } else {
                    TxReclaimStep::Fault(DevError::BadState)
                }
            }
            Ok(None) => TxReclaimStep::Empty,
            Err(err) => TxReclaimStep::Fault(err),
        }
    }

    fn rx_slot_has_space(&self) -> bool {
        !self.rx_slots.is_full()
    }

    fn tx_slot_pending(&self) -> bool {
        !self.tx_slots.is_empty()
    }

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        self.inner.queue_control()
    }

    fn register_waker(&self, waker: &Waker) {
        if let Some(irq) = self.inner.irq_num() {
            register_irq_waker(irq, waker);
        }
    }

    #[cfg(test)]
    fn recv_dormant_calls_for_test(&self) -> usize {
        self.recv_dormant_calls.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn rx_slot_len_for_test(&self) -> usize {
        self.rx_slots.len()
    }

    #[cfg(test)]
    fn rx_slot_peek_for_test(&self) -> Option<&[u8]> {
        self.rx_slots.peek()
    }

    #[cfg(test)]
    fn pop_tx_slot_for_test(&mut self) -> bool {
        self.tx_slots.pop().is_some()
    }

    #[cfg(test)]
    fn tx_slot_len_for_test(&self) -> usize {
        self.tx_slots.len()
    }
}
