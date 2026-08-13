use alloc::{boxed::Box, vec, vec::Vec};

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueControl;
use smoltcp::{
    phy::{DeviceCapabilities, Medium},
    storage::PacketMetadata,
    time::Instant,
    wire::{IpAddress, IpCidr, IpVersion},
};

use crate::{
    consts::{SOCKET_BUFFER_SIZE, STANDARD_MTU},
    device::{Device, RxStep, TxDropReason, TxOutcome, TxPreflight},
};

#[derive(Debug)]
pub struct Rule {
    pub filter: IpCidr,
    pub via: Option<IpAddress>,
    pub dev: usize,
    pub src: IpAddress,
}

impl Rule {
    pub fn new(filter: IpCidr, via: Option<IpAddress>, dev: usize, src: IpAddress) -> Self {
        Self {
            filter,
            via,
            dev,
            src,
        }
    }
}

type PacketBuffer = smoltcp::storage::PacketBuffer<'static, ()>;

/// Outcome of a single target-device RX-only service step.
pub enum RxOutcome {
    /// The Router RX buffer is full; the device was not touched.
    Full,
    /// The device reported no completion.
    Empty,
    /// One completion was reaped and refilled without delivering a packet.
    Consumed,
    /// One IP packet was delivered to the Router RX buffer.
    Delivered,
    /// A device or queue fault.
    Fault(DevError),
}

/// Who currently holds the right to consume a target device's RX.
///
/// This is a consumption-right view, not a lifecycle state. The lifecycle maps
/// `Polling/Spawned/Unavailable` to [`Self::PollingOwned`] and
/// `Active/Faulted` to [`Self::AsyncOwned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxOwnerView {
    /// Ordinary Router polling may consume the target device's RX.
    PollingOwned,
    /// Only the queue task may consume the target device's RX; ordinary
    /// Router polling must skip it.
    AsyncOwned,
}

// TODO(mivik): optimize
pub struct RouteTable {
    rules: Vec<Rule>,
}
impl RouteTable {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        let idx = self
            .rules
            .binary_search_by(|it| rule.filter.prefix_len().cmp(&it.filter.prefix_len()))
            .unwrap_or_else(|idx| idx);
        self.rules.insert(idx, rule);
    }

    pub fn lookup(&self, dst: &IpAddress) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|rule| rule.filter.contains_addr(dst))
    }
}

pub struct Router {
    rx_buffer: PacketBuffer,
    tx_buffer: PacketBuffer,
    pub(crate) devices: Vec<Box<dyn Device>>,
    pub(crate) table: RouteTable,
    /// Stable TX fault: once set, dispatch stops forwarding until recovery
    /// (recovery is outside Iteration 003).
    tx_fault: Option<DevError>,
    /// Per-reason drop counters; each logical packet increments exactly one.
    tx_drop_counts: [u64; TxDropReason::COUNT],
}
impl Router {
    pub fn new() -> Self {
        let rx_buffer = PacketBuffer::new(
            vec![PacketMetadata::EMPTY; SOCKET_BUFFER_SIZE],
            vec![0u8; STANDARD_MTU * SOCKET_BUFFER_SIZE],
        );
        let tx_buffer = PacketBuffer::new(
            vec![PacketMetadata::EMPTY; SOCKET_BUFFER_SIZE],
            vec![0u8; STANDARD_MTU * SOCKET_BUFFER_SIZE],
        );
        Self {
            rx_buffer,
            tx_buffer,
            devices: Vec::new(),
            table: RouteTable::new(),
            tx_fault: None,
            tx_drop_counts: [0; TxDropReason::COUNT],
        }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        self.table.add_rule(rule);
    }

    pub fn add_device(&mut self, device: Box<dyn Device>) -> usize {
        self.devices.push(device);
        self.devices.len() - 1
    }

    pub fn poll(&mut self, owner: RxOwnerView, target_dev: Option<usize>, timestamp: Instant) {
        for (i, dev) in self.devices.iter_mut().enumerate() {
            if owner == RxOwnerView::AsyncOwned && Some(i) == target_dev {
                continue;
            }
            while !self.rx_buffer.is_full() {
                match dev.recv(&mut self.rx_buffer, timestamp) {
                    RxStep::Consumed | RxStep::Delivered => {}
                    RxStep::Empty => break,
                    RxStep::Fault(err) => {
                        warn!("receive failed: {err}");
                        break;
                    }
                }
            }
        }
    }

    /// Services the target device's RX by at most one physical completion.
    ///
    /// Returns `Full` before touching the device when the Router RX buffer is
    /// full, so a queue task stops reaping while there is no room for the
    /// handoff. An out-of-range `dev` index yields `Fault(BadState)` instead
    /// of panicking.
    pub fn rx_one_step(&mut self, dev: usize, timestamp: Instant) -> RxOutcome {
        let Some(device) = self.devices.get_mut(dev) else {
            return RxOutcome::Fault(DevError::BadState);
        };
        if self.rx_buffer.is_full() {
            return RxOutcome::Full;
        }
        match device.recv(&mut self.rx_buffer, timestamp) {
            RxStep::Empty => RxOutcome::Empty,
            RxStep::Consumed => RxOutcome::Consumed,
            RxStep::Delivered => RxOutcome::Delivered,
            RxStep::Fault(err) => RxOutcome::Fault(err),
        }
    }

    /// Whether the Router RX buffer has room for at least one packet.
    pub fn rx_buffer_has_space(&self) -> bool {
        !self.rx_buffer.is_full()
    }

    /// Borrows the queue-control interface of device `dev` for one atomic
    /// operation. A missing device maps to `BadState`; a device without
    /// explicit notification control maps to `Unsupported`.
    fn rx_queue_control(&mut self, dev: usize) -> DevResult<&mut dyn NetQueueControl> {
        let Some(device) = self.devices.get_mut(dev) else {
            return Err(DevError::BadState);
        };
        device.queue_control().ok_or(DevError::Unsupported)
    }

    /// Activation-time preflight: the device must expose queue control and
    /// accept suppression. No completion is reaped.
    pub fn rx_control_preflight(&mut self, dev: usize) -> DevResult {
        self.rx_control_suppress(dev)
    }

    /// Suppresses RX used-buffer notifications on device `dev`.
    pub fn rx_control_suppress(&mut self, dev: usize) -> DevResult {
        self.rx_queue_control(dev)?.suppress_rx_notify()
    }

    /// Rearms RX notifications on device `dev` and reports whether a
    /// completion is still pending after the transport barrier.
    pub fn rx_control_arm_and_check(&mut self, dev: usize) -> DevResult<bool> {
        self.rx_queue_control(dev)?.arm_rx_notify_and_check()
    }

    /// Returns whether device `dev` currently sees an RX completion.
    pub fn rx_control_has_completion(&mut self, dev: usize) -> DevResult<bool> {
        Ok(self.rx_queue_control(dev)?.has_rx_completion())
    }

    #[cfg(test)]
    pub(crate) fn fill_rx_buffer_for_test(&mut self) {
        while self.rx_buffer.enqueue(1, ()).is_ok() {}
    }

    /// Enqueues an IP packet into the TX buffer for dispatch tests.
    #[cfg(test)]
    pub(crate) fn enqueue_tx_for_test(&mut self, packet: &[u8]) -> bool {
        match self.tx_buffer.enqueue(packet.len(), ()) {
            Ok(buf) => {
                buf.copy_from_slice(packet);
                true
            }
            Err(_) => false,
        }
    }

    /// Returns whether packets remain in the TX buffer.
    #[cfg(test)]
    pub(crate) fn tx_pending_for_test(&self) -> bool {
        !self.tx_buffer.is_empty()
    }

    /// Counts a logical drop once. `reason` maps to a fixed counter slot.
    fn count_drop(&mut self, reason: TxDropReason) {
        self.tx_drop_counts[reason.index()] += 1;
    }

    /// Returns how many logical packets were dropped with `reason`.
    ///
    /// Consumed by Iteration 005 V3 telemetry and host tests.
    #[allow(dead_code)]
    pub(crate) fn drop_count(&self, reason: TxDropReason) -> u64 {
        self.tx_drop_counts[reason.index()]
    }

    /// Returns whether dispatch entered a stable TX fault.
    #[allow(dead_code)]
    pub(crate) fn tx_faulted(&self) -> bool {
        self.tx_fault.is_some()
    }

    /// Returns the stable TX fault category name, if dispatch entered one.
    #[allow(dead_code)]
    pub(crate) fn tx_fault_kind(&self) -> Option<&'static str> {
        match self.tx_fault {
            Some(DevError::BadState) => Some("BadState"),
            Some(DevError::Again) => Some("Again"),
            Some(DevError::Io) => Some("Io"),
            Some(DevError::NoMemory) => Some("NoMemory"),
            Some(DevError::Unsupported) => Some("Unsupported"),
            Some(DevError::AlreadyExists) => Some("AlreadyExists"),
            Some(DevError::InvalidParam) => Some("InvalidParam"),
            Some(DevError::ResourceBusy) => Some("ResourceBusy"),
            None => None,
        }
    }

    /// Plans the unique delivery targets for the packet at the TX head.
    ///
    /// Returns `Ok(targets)` where each entry is `(device index, next hop)`,
    /// or a stable drop reason when the packet cannot be delivered. Parsing
    /// and route lookup happen before any device is touched.
    fn plan_packet(&self, packet: &[u8]) -> Result<Vec<(usize, IpAddress)>, TxDropReason> {
        match IpVersion::of_packet(packet) {
            Ok(IpVersion::Ipv4) => {
                let ipv4 = smoltcp::wire::Ipv4Packet::new_checked(packet)
                    .map_err(|_| TxDropReason::MalformedIp)?;
                let dst = IpAddress::Ipv4(ipv4.dst_addr());
                if ipv4.dst_addr().is_broadcast() {
                    let targets = self
                        .devices
                        .iter()
                        .enumerate()
                        .map(|(idx, _)| (idx, dst))
                        .collect();
                    return Ok(targets);
                }
                let rule = self.table.lookup(&dst).ok_or(TxDropReason::NoRoute)?;
                if rule.src != IpAddress::Ipv4(ipv4.src_addr()) {
                    return Err(TxDropReason::RouteSourceMismatch);
                }
                let next_hop = rule.via.unwrap_or(dst);
                Ok(vec![(rule.dev, next_hop)])
            }
            Ok(IpVersion::Ipv6) => {
                let ipv6 = smoltcp::wire::Ipv6Packet::new_checked(packet)
                    .map_err(|_| TxDropReason::MalformedIp)?;
                let dst = IpAddress::Ipv6(ipv6.dst_addr());
                if ipv6.dst_addr().is_multicast() {
                    let targets = self
                        .devices
                        .iter()
                        .enumerate()
                        .map(|(idx, _)| (idx, dst))
                        .collect();
                    return Ok(targets);
                }
                let rule = self.table.lookup(&dst).ok_or(TxDropReason::NoRoute)?;
                if rule.src != IpAddress::Ipv6(ipv6.src_addr()) {
                    return Err(TxDropReason::RouteSourceMismatch);
                }
                let next_hop = rule.via.unwrap_or(dst);
                Ok(vec![(rule.dev, next_hop)])
            }
            Err(_) => Err(TxDropReason::MalformedIp),
        }
    }

    /// Forwards the TX queue head with peek → all-target preflight → commit.
    ///
    /// Returns whether the caller should poll again (loopback RX became
    /// ready). Any target `Full` keeps the head and stops this round with no
    /// commit. A preflight `Drop` counts once and dequeues. A preflight
    /// `Fault`, or a commit that violates the Ready promise, enters the
    /// stable Router TX fault and stops all forwarding.
    pub fn dispatch(&mut self, timestamp: Instant) -> bool {
        let mut poll_next = false;
        if self.tx_fault.is_some() {
            return false;
        }
        while let Ok(((), packet)) = self.tx_buffer.peek() {
            // Copy the head out of the TX buffer so planning, preflight and
            // commit can borrow `self` independently of the peek borrow.
            let packet = packet.to_vec();
            let targets = match self.plan_packet(&packet) {
                Ok(targets) => targets,
                Err(reason) => {
                    self.count_drop(reason);
                    let _ = self.tx_buffer.dequeue();
                    continue;
                }
            };

            // All-target preflight under one lock scope.
            let mut preflight = TxPreflight::Ready;
            for &(dev_idx, next_hop) in &targets {
                let dev = &mut self.devices[dev_idx];
                match dev.preflight_send(next_hop, &packet, timestamp) {
                    TxPreflight::Ready => {}
                    other => {
                        preflight = other;
                        break;
                    }
                }
            }
            match preflight {
                TxPreflight::Ready => {}
                // Keep the head; retry when the device frees capacity.
                TxPreflight::Full => break,
                TxPreflight::Drop(reason) => {
                    self.count_drop(reason);
                    let _ = self.tx_buffer.dequeue();
                    continue;
                }
                TxPreflight::Fault(err) => {
                    self.tx_fault = Some(err);
                    return poll_next;
                }
            }

            // Commit every planned target; the head is dequeued exactly once
            // only after every target accepted.
            for &(dev_idx, next_hop) in &targets {
                let dev = &mut self.devices[dev_idx];
                match dev.send(next_hop, &packet, timestamp) {
                    TxOutcome::Accepted { rx_became_ready } => {
                        poll_next |= rx_became_ready;
                    }
                    // A non-Accepted commit after preflight Ready is an
                    // invariant violation: enter the stable fault.
                    outcome => {
                        let err = match outcome {
                            TxOutcome::Fault(err) => err,
                            _ => DevError::BadState,
                        };
                        self.tx_fault = Some(err);
                        return poll_next;
                    }
                }
            }
            let _ = self.tx_buffer.dequeue();
        }
        poll_next
    }
}

pub struct TxToken<'a>(&'a mut PacketBuffer);

impl smoltcp::phy::TxToken for TxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(self
            .0
            .enqueue(len, ())
            .expect("This was checked before creating the TxToken"))
    }
}

pub struct RxToken<'a>(&'a [u8]);

impl<'a> smoltcp::phy::RxToken for RxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.0)
    }
}

impl smoltcp::phy::Device for Router {
    type RxToken<'a> = RxToken<'a>;
    type TxToken<'a> = TxToken<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.rx_buffer.is_empty() || self.tx_buffer.is_full() {
            None
        } else {
            Some((
                RxToken(self.rx_buffer.dequeue().unwrap().1),
                TxToken(&mut self.tx_buffer),
            ))
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.tx_buffer.is_full() {
            None
        } else {
            Some(TxToken(&mut self.tx_buffer))
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = STANDARD_MTU;
        caps.max_burst_size = Some(SOCKET_BUFFER_SIZE);
        caps
    }
}
