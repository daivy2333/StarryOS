use alloc::{boxed::Box, vec, vec::Vec};

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueDirection;
use smoltcp::{
    phy::{DeviceCapabilities, Medium},
    storage::PacketMetadata,
    time::Instant,
    wire::{IpAddress, IpCidr, IpVersion},
};

use crate::{
    consts::{SOCKET_BUFFER_SIZE, STANDARD_MTU},
    device::{
        Device, RxCopyStep, RxStep, TxDropReason, TxOutcome, TxPreflight, TxReclaimStep,
        TxSubmitStep,
    },
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

/// Lifecycle-derived consumption-right snapshot of the async RX task.
///
/// The lifecycle maps `Polling/Spawned/Unavailable` to [`Self::PollingOwned`]
/// and `Active/Faulted` to [`Self::AsyncOwned`]. Since MS05 Task 3.2 the
/// consumption decision itself lives in each device's `recv` dispatch
/// (slot mode vs polling mode); this view is retained as observable
/// lifecycle telemetry for the V2 snapshot and caller observability, not as
/// a per-device skip selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxOwnerView {
    /// Ordinary Router polling may consume the target device's RX.
    PollingOwned,
    /// The async queue task owns the raw directions; ordinary Router
    /// polling still calls `recv`, which drains the fixed RX slots.
    AsyncOwned,
}

/// Result of one bounded Router RX stage.
#[derive(Debug)]
pub(crate) struct RouterRxOutcome {
    pub(crate) processed: usize,
    pub(crate) budget_exhausted: bool,
    pub(crate) backlog: bool,
    pub(crate) fault: Option<DevError>,
}

/// Result of one bounded Router TX dispatch stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouterDispatchOutcome {
    pub(crate) processed: usize,
    pub(crate) budget_exhausted: bool,
    pub(crate) backlog: bool,
    pub(crate) rx_ready: bool,
    pub(crate) faulted: bool,
}

const DEFAULT_ROUTER_BUDGET: usize = 32;

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
    /// Delivered IP frames since the last `take_*` (stack RX path).
    rx_delivered_delta: u64,
    /// Non-IP / malformed completions consumed since the last `take_*`.
    rx_consumed_delta: u64,
    /// Next device considered by the bounded RX stage.
    rx_cursor: usize,
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
            rx_delivered_delta: 0,
            rx_consumed_delta: 0,
            rx_cursor: 0,
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
        let _ = self.poll_bounded(owner, target_dev, timestamp, DEFAULT_ROUTER_BUDGET);
    }

    /// Polls at most `budget` RX items, rotating after every attempt so one
    /// continuously-ready device cannot hide a later device.
    pub(crate) fn poll_bounded(
        &mut self,
        owner: RxOwnerView,
        target_dev: Option<usize>,
        timestamp: Instant,
        budget: usize,
    ) -> RouterRxOutcome {
        // `recv` decides consumption by the device's own mode: slot mode
        // drains the fixed RX slots, polling mode reaps raw completions.
        // The owner view no longer selects a per-device skip (MS05 T3.2).
        let _ = (owner, target_dev);
        let device_count = self.devices.len();
        let mut processed = 0usize;
        let mut inactive = 0usize;
        let mut fault = None;
        let mut blocked = false;

        while processed < budget
            && inactive < device_count
            && !self.rx_buffer.is_full()
            && device_count != 0
        {
            let dev_idx = self.rx_cursor % device_count;
            self.rx_cursor = (dev_idx + 1) % device_count;
            match self.devices[dev_idx].recv(&mut self.rx_buffer, timestamp) {
                RxStep::Consumed => {
                    self.rx_consumed_delta += 1;
                    processed += 1;
                    inactive = 0;
                }
                RxStep::Delivered => {
                    self.rx_delivered_delta += 1;
                    processed += 1;
                    inactive = 0;
                }
                // Blocked and Empty are quiescent for this device in this
                // round. The cursor still advances so later devices run.
                RxStep::Blocked => {
                    blocked = true;
                    inactive += 1;
                }
                RxStep::Empty => inactive += 1,
                RxStep::Fault(err) => {
                    warn!("receive failed: {err}");
                    if fault.is_none() {
                        fault = Some(err);
                    }
                    inactive += 1;
                }
            }
        }

        let budget_exhausted = budget != 0 && processed == budget;
        RouterRxOutcome {
            processed,
            budget_exhausted,
            backlog: budget_exhausted || blocked || self.rx_buffer.is_full(),
            fault,
        }
    }

    /// Takes the IP frames delivered by the stack RX path since the last
    /// call. MS05 Task 3.2: the queue task no longer delivers frames
    /// directly, so the delivered counter is produced by stack polling.
    pub(crate) fn take_rx_delivered_delta(&mut self) -> u64 {
        core::mem::take(&mut self.rx_delivered_delta)
    }

    /// Takes the non-IP completions consumed by the stack RX path since the
    /// last call.
    pub(crate) fn take_rx_consumed_delta(&mut self) -> u64 {
        core::mem::take(&mut self.rx_consumed_delta)
    }

    /// Suppresses used-buffer notifications for BOTH directions on device
    /// `dev`. A device without explicit notification control maps to
    /// `Unsupported`.
    pub fn control_suppress_both(&mut self, dev: usize) -> DevResult {
        let Some(device) = self.devices.get_mut(dev) else {
            return Err(DevError::BadState);
        };
        let control = device.queue_control().ok_or(DevError::Unsupported)?;
        control.suppress_notify(NetQueueDirection::BOTH)
    }

    /// Rearms BOTH directions on device `dev` and reports which directions
    /// still have a pending completion after the transport barrier.
    pub fn control_arm_and_check_both(&mut self, dev: usize) -> DevResult<NetQueueDirection> {
        let Some(device) = self.devices.get_mut(dev) else {
            return Err(DevError::BadState);
        };
        let control = device.queue_control().ok_or(DevError::Unsupported)?;
        control.arm_notify_and_check(NetQueueDirection::BOTH)
    }

    /// Returns which requested directions currently have visible completions
    /// on device `dev`.
    pub fn control_completion_pending_both(&mut self, dev: usize) -> DevResult<NetQueueDirection> {
        let Some(device) = self.devices.get_mut(dev) else {
            return Err(DevError::BadState);
        };
        let control = device.queue_control().ok_or(DevError::Unsupported)?;
        control.completion_pending(NetQueueDirection::BOTH)
    }

    /// Switches device `dev` to the slot data path (Task 3.1 activation).
    pub fn activate_slot_mode(&mut self, dev: usize) -> DevResult {
        let Some(device) = self.devices.get_mut(dev) else {
            return Err(DevError::BadState);
        };
        device.activate_slot_mode()
    }

    /// Advances the raw→RX-slot copy on device `dev` by at most one frame.
    pub fn rx_copy_one(&mut self, dev: usize) -> RxCopyStep {
        let Some(device) = self.devices.get_mut(dev) else {
            return RxCopyStep::Fault(DevError::BadState);
        };
        device.rx_copy_one()
    }

    /// Advances the TX-slot→raw submit on device `dev` by at most one frame.
    pub fn tx_submit_one(&mut self, dev: usize) -> TxSubmitStep {
        let Some(device) = self.devices.get_mut(dev) else {
            return TxSubmitStep::Fault(DevError::BadState);
        };
        device.tx_submit_one()
    }

    /// Advances the TX completion reclaim on device `dev` by at most one
    /// completion.
    pub fn tx_reclaim_one(&mut self, dev: usize) -> TxReclaimStep {
        let Some(device) = self.devices.get_mut(dev) else {
            return TxReclaimStep::Fault(DevError::BadState);
        };
        device.tx_reclaim_one()
    }

    /// Whether device `dev`'s fixed RX slots currently have room for a frame.
    pub fn rx_slot_has_space(&self, dev: usize) -> bool {
        self.devices
            .get(dev)
            .is_some_and(|device| device.rx_slot_has_space())
    }

    /// Whether device `dev`'s fixed TX slots currently hold a pending frame.
    pub fn tx_slot_pending(&self, dev: usize) -> bool {
        self.devices
            .get(dev)
            .is_some_and(|device| device.tx_slot_pending())
    }

    /// Most recently accepted TX ticket on device `dev` (D8 flush target).
    pub fn tx_last_accepted(&self, dev: usize) -> Option<u64> {
        self.devices
            .get(dev)
            .and_then(|device| device.tx_last_accepted())
    }

    /// Whether a C4 flush to `target` is complete on device `dev`.
    pub fn tx_flush_done(&self, dev: usize, target: Option<u64>) -> bool {
        self.devices
            .get(dev)
            .is_some_and(|device| device.tx_flush_done(target))
    }

    /// Slot/ticket ledger of device `dev` for the V3 diagnostic snapshot.
    pub fn slot_ledger(&self, dev: usize) -> crate::device::SlotLedger {
        self.devices
            .get(dev)
            .map(|device| device.slot_ledger())
            .unwrap_or_default()
    }

    /// Real driver TX resource ledger of device `dev` (RW-2).
    pub fn tx_resource_ledger(&mut self, dev: usize) -> Option<crate::device::TxResourceLedger> {
        self.devices.get_mut(dev)?.tx_resource_ledger()
    }

    #[cfg(test)]
    pub(crate) fn fill_rx_buffer_for_test(&mut self) {
        while self.rx_buffer.enqueue(1, ()).is_ok() {}
    }

    /// Returns whether the Router RX buffer currently holds any packet.
    #[cfg(test)]
    pub(crate) fn rx_buffer_pending_for_test(&self) -> bool {
        !self.rx_buffer.is_empty()
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
    #[allow(dead_code)]
    pub(crate) fn count_drop(&mut self, reason: TxDropReason) {
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

    /// Forwards the TX queue head with peek → all-target preflight → commit.
    ///
    /// Returns whether the caller should poll again (loopback RX became
    /// ready). The head bytes and the target list are never copied: the
    /// packet is borrowed from the TX buffer and the target plan is recomputed
    /// deterministically for each of the two passes. Any target `Full` keeps
    /// the head and stops this round with no commit. A preflight `Drop` counts
    /// once and dequeues. A preflight `Fault` keeps the head and enters the
    /// stable fault. A commit that violates the Ready promise enters the
    /// stable fault and removes the head, so the possibly-delivered packet is
    /// never forwarded twice.
    pub fn dispatch(&mut self, timestamp: Instant) -> bool {
        self.dispatch_bounded(timestamp, DEFAULT_ROUTER_BUDGET)
            .rx_ready
    }

    /// Dispatches at most `budget` logical packets while preserving the
    /// existing peek/preflight/commit ownership rules.
    pub(crate) fn dispatch_bounded(
        &mut self,
        timestamp: Instant,
        budget: usize,
    ) -> RouterDispatchOutcome {
        if self.tx_fault.is_some() {
            return RouterDispatchOutcome {
                processed: 0,
                budget_exhausted: false,
                backlog: !self.tx_buffer.is_empty(),
                rx_ready: false,
                faulted: true,
            };
        }
        let mut processed = 0usize;
        let mut rx_ready = false;
        let mut faulted = false;
        let Self {
            tx_buffer,
            devices,
            table,
            tx_fault,
            tx_drop_counts,
            ..
        } = self;

        enum Action {
            /// All targets committed; dequeue and continue.
            DequeueContinue,
            /// A target is Full; keep the head and stop this round.
            KeepHeadStop,
            /// A preflight fault; keep the head, faulted.
            FaultKeepHead,
            /// A commit drifted after Ready; remove the head, faulted.
            FaultRemoveHead,
        }

        while processed < budget {
            // The packet borrow lives only inside this loop so `tx_buffer`
            // can be dequeued afterwards (split borrows: `tx_buffer` vs
            // `devices`).
            let action = loop {
                let Ok(((), packet)) = tx_buffer.peek() else {
                    break Action::KeepHeadStop;
                };
                let mut targets = match plan_packet(table, devices.len(), packet) {
                    Ok(targets) => targets,
                    Err(reason) => {
                        tx_drop_counts[reason.index()] += 1;
                        break Action::DequeueContinue;
                    }
                };

                // All-target preflight; the plan is recomputed for the commit
                // pass below, so the iterator is consumed here.
                let mut preflight = TxPreflight::Ready;
                for (dev_idx, next_hop) in targets.by_ref() {
                    let dev = &mut devices[dev_idx];
                    match dev.preflight_send(next_hop, packet, timestamp) {
                        TxPreflight::Ready => {}
                        other => {
                            preflight = other;
                            break;
                        }
                    }
                }
                let preflight_action = match preflight {
                    TxPreflight::Ready => None,
                    // Keep the head; retry when the device frees capacity.
                    TxPreflight::Full => Some(Action::KeepHeadStop),
                    TxPreflight::Drop(reason) => {
                        tx_drop_counts[reason.index()] += 1;
                        Some(Action::DequeueContinue)
                    }
                    TxPreflight::Fault(err) => {
                        *tx_fault = Some(err);
                        Some(Action::FaultKeepHead)
                    }
                };
                if let Some(action) = preflight_action {
                    break action;
                }

                // Commit every planned target; the head is dequeued exactly
                // once only after every target accepted. The plan is
                // recomputed so the target iterator stays allocation-free.
                let targets = match plan_packet(table, devices.len(), packet) {
                    Ok(targets) => targets,
                    Err(_) => unreachable!("plan succeeded in preflight"),
                };
                let mut drift = None;
                for (dev_idx, next_hop) in targets {
                    let dev = &mut devices[dev_idx];
                    match dev.send(next_hop, packet, timestamp) {
                        TxOutcome::Accepted { rx_became_ready } => {
                            rx_ready |= rx_became_ready;
                        }
                        // A non-Accepted commit after preflight Ready is an
                        // invariant violation: enter the stable fault.
                        outcome => {
                            let err = match outcome {
                                TxOutcome::Fault(err) => err,
                                _ => DevError::BadState,
                            };
                            drift = Some(err);
                            break;
                        }
                    }
                }
                break match drift {
                    Some(err) => {
                        *tx_fault = Some(err);
                        Action::FaultRemoveHead
                    }
                    None => Action::DequeueContinue,
                };
            };

            match action {
                Action::DequeueContinue => {
                    let _ = tx_buffer.dequeue();
                    processed += 1;
                }
                Action::KeepHeadStop => break,
                Action::FaultKeepHead => {
                    faulted = true;
                    break;
                }
                Action::FaultRemoveHead => {
                    let _ = tx_buffer.dequeue();
                    processed += 1;
                    faulted = true;
                    break;
                }
            }
        }
        let backlog = !tx_buffer.is_empty();
        RouterDispatchOutcome {
            processed,
            budget_exhausted: budget != 0 && processed == budget && backlog,
            backlog,
            rx_ready,
            faulted,
        }
    }
}

/// A deterministic, allocation-free enumeration of a packet's delivery
/// targets. The same plan is recomputed for the preflight pass and the commit
/// pass, so no target list is ever materialized in the data path.
enum TargetIter {
    /// Exactly one target device.
    Single(Option<(usize, IpAddress)>),
    /// Every device (broadcast/multicast fanout).
    Range {
        next: usize,
        count: usize,
        next_hop: IpAddress,
    },
}

impl Iterator for TargetIter {
    type Item = (usize, IpAddress);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single(item) => item.take(),
            Self::Range {
                next,
                count,
                next_hop,
            } => {
                if *next < *count {
                    let i = *next;
                    *next += 1;
                    Some((i, *next_hop))
                } else {
                    None
                }
            }
        }
    }
}

/// Plans the delivery targets for `packet` without allocating.
///
/// Parsing and route lookup happen before any device is touched. A broadcast
/// or multicast destination fans out to every device; a unicast destination
/// resolves to the single matching rule.
fn plan_packet(
    table: &RouteTable,
    device_count: usize,
    packet: &[u8],
) -> Result<TargetIter, TxDropReason> {
    match IpVersion::of_packet(packet) {
        Ok(IpVersion::Ipv4) => {
            let ipv4 = smoltcp::wire::Ipv4Packet::new_checked(packet)
                .map_err(|_| TxDropReason::MalformedIp)?;
            let dst = IpAddress::Ipv4(ipv4.dst_addr());
            if ipv4.dst_addr().is_broadcast() {
                return Ok(TargetIter::Range {
                    next: 0,
                    count: device_count,
                    next_hop: dst,
                });
            }
            let rule = table.lookup(&dst).ok_or(TxDropReason::NoRoute)?;
            if rule.src != IpAddress::Ipv4(ipv4.src_addr()) {
                return Err(TxDropReason::RouteSourceMismatch);
            }
            let next_hop = rule.via.unwrap_or(dst);
            Ok(TargetIter::Single(Some((rule.dev, next_hop))))
        }
        Ok(IpVersion::Ipv6) => {
            let ipv6 = smoltcp::wire::Ipv6Packet::new_checked(packet)
                .map_err(|_| TxDropReason::MalformedIp)?;
            let dst = IpAddress::Ipv6(ipv6.dst_addr());
            if ipv6.dst_addr().is_multicast() {
                return Ok(TargetIter::Range {
                    next: 0,
                    count: device_count,
                    next_hop: dst,
                });
            }
            let rule = table.lookup(&dst).ok_or(TxDropReason::NoRoute)?;
            if rule.src != IpAddress::Ipv6(ipv6.src_addr()) {
                return Err(TxDropReason::RouteSourceMismatch);
            }
            let next_hop = rule.via.unwrap_or(dst);
            Ok(TargetIter::Single(Some((rule.dev, next_hop))))
        }
        Err(_) => Err(TxDropReason::MalformedIp),
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
