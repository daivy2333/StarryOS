use alloc::{boxed::Box, vec, vec::Vec};

use axdriver::prelude::DevError;
use smoltcp::{
    phy::{DeviceCapabilities, Medium},
    storage::PacketMetadata,
    time::Instant,
    wire::{IpAddress, IpCidr, IpVersion},
};

use crate::{
    consts::{SOCKET_BUFFER_SIZE, STANDARD_MTU},
    device::{Device, RxStep},
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
/// This is a consumption-right view, not a lifecycle state. T5 later maps
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

    #[cfg(test)]
    pub(crate) fn fill_rx_buffer_for_test(&mut self) {
        while self.rx_buffer.enqueue(1, ()).is_ok() {}
    }

    pub fn dispatch(&mut self, timestamp: Instant) -> bool {
        let mut poll_next = false;
        while let Ok(((), packet)) = self.tx_buffer.dequeue() {
            match IpVersion::of_packet(packet).expect("got invalid IP packet") {
                IpVersion::Ipv4 => {
                    let packet = smoltcp::wire::Ipv4Packet::new_checked(packet)
                        .expect("got invalid IPv4 packet");
                    let dst_addr = IpAddress::Ipv4(packet.dst_addr());
                    if packet.dst_addr().is_broadcast() {
                        let buf = packet.into_inner();
                        for dev in &mut self.devices {
                            poll_next |= dev.send(dst_addr, buf, timestamp);
                        }
                    } else {
                        let Some(rule) = self.table.lookup(&dst_addr) else {
                            warn!("No route found for destination: {}", dst_addr);
                            continue;
                        };
                        assert_eq!(rule.src, IpAddress::Ipv4(packet.src_addr()));

                        let next_hop = rule.via.unwrap_or(dst_addr);
                        let dev = &mut self.devices[rule.dev];
                        poll_next |= dev.send(next_hop, packet.into_inner(), timestamp);
                    }
                }
                IpVersion::Ipv6 => {
                    let packet = smoltcp::wire::Ipv6Packet::new_checked(packet)
                        .expect("got invalid IPv6 packet");
                    let dst_addr = IpAddress::Ipv6(packet.dst_addr());
                    if packet.dst_addr().is_multicast() {
                        let buf = packet.into_inner();
                        for dev in &mut self.devices {
                            poll_next |= dev.send(dst_addr, buf, timestamp);
                        }
                    } else {
                        let Some(rule) = self.table.lookup(&dst_addr) else {
                            warn!("No route found for destination: {}", dst_addr);
                            continue;
                        };
                        assert_eq!(rule.src, IpAddress::Ipv6(packet.src_addr()));

                        let next_hop = rule.via.unwrap_or(dst_addr);
                        let dev = &mut self.devices[rule.dev];
                        poll_next |= dev.send(next_hop, packet.into_inner(), timestamp);
                    }
                }
            }
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
