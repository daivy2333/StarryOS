use alloc::{string::String, vec};
use core::task::Waker;

use axdriver::prelude::*;
use axdriver_net::NetQueueControl;
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
    consts::ETHERNET_MAX_PENDING_PACKETS,
    device::{
        Device, RxStep, TxDropReason, TxOutcome, TxPreflight,
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
        }
    }

    /// Borrows the dormant slot storage for host tests. Product builds never
    /// touch the slots.
    #[cfg(test)]
    pub(super) fn slots_for_test(&self) -> (&FixedFrameQueue<64>, &FixedFrameQueue<64>) {
        (&self.rx_slots, &self.tx_slots)
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
        let mut frame_bytes = vec![0u8; frame_len];
        let mut frame = EthernetFrame::new_unchecked(&mut frame_bytes);
        repr.emit(&mut frame);
        f(frame.payload_mut());
        match self.tx_slots.enqueue(&frame_bytes, (), Some(ticket)) {
            Ok(()) => TxOutcome::Accepted {
                rx_became_ready: false,
            },
            Err(_) => {
                // Preflight promised room; a failed enqueue is invariant
                // drift, so release the ticket and report a stable fault.
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
    ) -> Result<RxStep, DevError> {
        let frame = EthernetFrame::new_unchecked(frame);
        let Ok(repr) = EthernetRepr::parse(&frame) else {
            warn!("Dropping malformed Ethernet frame");
            return Ok(RxStep::Consumed);
        };

        if !repr.dst_addr.is_broadcast()
            && repr.dst_addr != EMPTY_MAC
            && repr.dst_addr != self.hardware_address()
        {
            return Ok(RxStep::Consumed);
        }

        match repr.ethertype {
            EthernetProtocol::Ipv4 => {
                let Ok(dst) = buffer.enqueue(frame.payload().len(), ()) else {
                    return Err(DevError::BadState);
                };
                dst.copy_from_slice(frame.payload());
                Ok(RxStep::Delivered)
            }
            EthernetProtocol::Arp => {
                self.process_arp(frame.payload(), timestamp);
                Ok(RxStep::Consumed)
            }
            _ => Ok(RxStep::Consumed),
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

    fn process_arp(&mut self, payload: &[u8], now: Instant) {
        let Ok(repr) = ArpPacket::new_checked(payload).and_then(|packet| ArpRepr::parse(&packet))
        else {
            warn!("Dropping malformed ARP packet");
            return;
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
                return;
            }

            if let ArpOperation::Unknown(_) = operation {
                return;
            }

            if !source_hardware_addr.is_unicast()
                || source_protocol_addr.is_broadcast()
                || source_protocol_addr.is_multicast()
                || source_protocol_addr.is_unspecified()
            {
                return;
            }
            if self.ip.address() != target_protocol_addr {
                return;
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
                    return;
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
                let Some((next_hop, buf)) = self.pending_packets.peek_meta() else {
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

                let payload = buf.to_vec();
                let flush = self.emit_frame(
                    neighbor.hardware_address,
                    payload.len(),
                    |b| b.copy_from_slice(&payload),
                    EthernetProtocol::Ipv4,
                );
                if !matches!(flush, TxOutcome::Accepted { .. }) {
                    warn!("pending flush not accepted: {flush:?}");
                    break;
                }
                let _ = self.pending_packets.pop();
            }
        }
    }

    fn preflight_ready_tx(&mut self) -> TxPreflight {
        match self.inner.recycle_tx_buffers() {
            Ok(()) if self.inner.can_transmit() => TxPreflight::Ready,
            Ok(()) => TxPreflight::Full,
            Err(err) => TxPreflight::Fault(err),
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
}

impl Device for EthernetDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep {
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
            (Ok(step), Ok(())) => step,
            (Err(err), Ok(())) => RxStep::Fault(err),
        }
    }

    fn preflight_send(
        &mut self,
        next_hop: IpAddress,
        packet: &[u8],
        timestamp: Instant,
    ) -> TxPreflight {
        if packet.len() > MAX_FRAME_SIZE {
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
                Some(None) => return self.preflight_unknown_neighbor(next_hop, timestamp),
                Some(Some(neighbor)) if neighbor.expires_at <= timestamp => {
                    return self.preflight_unknown_neighbor(next_hop, timestamp);
                }
                _ => {}
            }
        }
        self.preflight_ready_tx()
    }

    fn send(&mut self, next_hop: IpAddress, packet: &[u8], timestamp: Instant) -> TxOutcome {
        if packet.len() > MAX_FRAME_SIZE {
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

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        self.inner.queue_control()
    }

    fn register_waker(&self, waker: &Waker) {
        if let Some(irq) = self.inner.irq_num() {
            register_irq_waker(irq, waker);
        }
    }
}
