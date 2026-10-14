//! [ArceOS](https://github.com/rcore-os/arceos) network module.
//!
//! It provides unified networking primitives for TCP/UDP communication
//! using various underlying network stacks. Currently, only [smoltcp] is
//! supported.
//!
//! # Organization
//!
//! - [`tcp::TcpSocket`]: A TCP socket that provides POSIX-like APIs.
//! - [`udp::UdpSocket`]: A UDP socket that provides POSIX-like APIs.
//!
//! [smoltcp]: https://github.com/smoltcp-rs/smoltcp

#![no_std]

#[macro_use]
extern crate log;
extern crate alloc;

mod async_rx;
mod consts;
mod device;
#[cfg(feature = "queue-diagnostics")]
mod diag;
mod flush;
mod general;
mod hart_counter;
mod listen_table;
/// Socket option types and the [`Configurable`](options::Configurable) trait.
pub mod options;
mod readiness;
mod recovery;
mod router;
mod service;
mod socket;
mod stack_runner;
pub(crate) mod state;
/// TCP socket implementation.
pub mod tcp;
/// UDP socket implementation.
pub mod udp;
/// Unix domain socket implementation.
pub mod unix;
/// Vsock socket implementation.
#[cfg(feature = "vsock")]
pub mod vsock;
mod wrapper;

use alloc::{borrow::ToOwned, boxed::Box};

use axdriver::{AxDeviceContainer, prelude::*};
use axsync::Mutex;
use smoltcp::wire::{EthernetAddress, Ipv4Address, Ipv4Cidr};
use spin::{Lazy, Once};

#[cfg(feature = "qemu-diagnostics")]
pub use self::async_rx::{RecoverySnapshotV4, recovery_snapshot_v4};
use self::{
    async_rx::RX_LIFECYCLE,
    consts::{GATEWAY, IP, IP_PREFIX},
    device::{EthernetDevice, LoopbackDevice},
    listen_table::ListenTable,
    router::{Router, Rule},
    service::Service,
    wrapper::SocketSetWrapper,
};
pub use self::{
    async_rx::{
        RX_TASK_NAME, RxSnapshot, RxSnapshotV3, owner_hart_tuple, publish_config_event,
        publish_queue_event, publish_rx_event, rx_snapshot, rx_snapshot_v3, software_nudge,
        start_rx_task, start_rx_task_affinity,
    },
    socket::*,
    stack_runner::{
        StackSnapshot, runner_hart_tuple, runner_software_nudge, stack_snapshot,
        start_stack_runner_affinity,
    },
};

static LISTEN_TABLE: Lazy<ListenTable> = Lazy::new(ListenTable::new);
static SOCKET_SET: Lazy<SocketSetWrapper> = Lazy::new(SocketSetWrapper::new);

static SERVICE: Once<Mutex<Service>> = Once::new();

fn get_service() -> axsync::MutexGuard<'static, Service> {
    SERVICE
        .get()
        .expect("Network service not initialized")
        .lock()
}

/// Initializes the network subsystem by NIC devices.
pub fn init_network(mut net_devs: AxDeviceContainer<AxNetDevice>) {
    info!("Initialize network subsystem...");

    let mut router = Router::new();
    let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));

    let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
    router.add_rule(Rule::new(
        lo_ip.into(),
        None,
        lo_dev,
        lo_ip.address().into(),
    ));

    let eth0_ip = if let Some(dev) = net_devs.take_one() {
        info!("  use NIC 0: {:?}", dev.device_name());

        let eth0_address = EthernetAddress(dev.mac_address().0);
        let eth0_ip = Ipv4Cidr::new(IP.parse().expect("Invalid IPv4 address"), IP_PREFIX);

        let eth0_dev = router.add_device(Box::new(EthernetDevice::new(
            "eth0".to_owned(),
            dev,
            eth0_ip,
        )));

        router.add_rule(Rule::new(
            Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0).into(),
            Some(GATEWAY.parse().expect("Invalid gateway address")),
            eth0_dev,
            eth0_ip.address().into(),
        ));

        info!("eth0:");
        info!("  mac:  {}", eth0_address);
        info!("  ip:   {}", eth0_ip);

        Some((eth0_dev, eth0_ip))
    } else {
        warn!("  No network device found!");
        None
    };

    for dev in &router.devices {
        info!("Device: {}", dev.name());
    }

    let mut service = Service::new(router, eth0_ip.as_ref().map(|(dev, _)| *dev));
    service.iface.update_ip_addrs(|ip_addrs| {
        ip_addrs.push(lo_ip.into()).unwrap();
        if let Some((_, eth0_ip)) = eth0_ip {
            ip_addrs.push(eth0_ip.into()).unwrap();
        }
    });
    SERVICE.call_once(|| Mutex::new(service));
    // Design D5: installation is spawn-free. The owning kernel adapter starts
    // exactly one pinned stack runner and one pinned queue owner only after the
    // secondary schedulers are published (secondary-ready), so no background
    // network task enters a run queue before it is schedulable.
}

/// Init vsock subsystem by vsock devices.
#[cfg(feature = "vsock")]
pub fn init_vsock(mut vsock_devs: AxDeviceContainer<AxVsockDevice>) {
    use self::device::register_vsock_device;
    info!("Initialize vsock subsystem...");
    if let Some(dev) = vsock_devs.take_one() {
        info!("  use vsock 0: {:?}", dev.device_name());
        if let Err(e) = register_vsock_device(dev) {
            warn!("Failed to initialize vsock device: {:?}", e);
        }
    } else {
        warn!("  No vsock device found!");
    }
}

/// Poll all network interfaces for new events.
///
/// The owner view is lifecycle telemetry; consumption is decided by each
/// device's `recv` dispatch (slot mode after activation, polling before).
pub fn poll_interfaces() {
    let owner = RX_LIFECYCLE.owner_view();
    while get_service().poll(owner, &mut SOCKET_SET.inner.lock()) {}
    LISTEN_TABLE.drain_accept_wakes();
}

/// Applies a bounded pressure control (D9).
///
/// `op` is `HoldTxSubmit=1`, `HoldTxReclaim=2` (lease 1..=2000 ms) or
/// `Release=3` (lease 0). The control acquires the Service with one
/// genuinely nonblocking `try_lock`: while the Service is busy it returns
/// `ResourceBusy` (mapped to `WouldBlock`) without changing state or
/// publishing an event. On success it commits the lease under the guard,
/// drops the guard, then publishes exactly one queue-work event so the sole
/// owner pauses (or resumes) the matching stage.
#[cfg(feature = "queue-diagnostics")]
pub fn diagnostic_control(op: u64, lease_ms: u64) -> axdriver::prelude::DevResult {
    use crate::async_rx::{QUEUE_EVENT, ServiceAccess, diagnostic_control_shared};
    diagnostic_control_shared(ServiceAccess::Global, &QUEUE_EVENT, op, lease_ms)
}

/// Observable facts of one diagnostic enqueue (MS12 Iteration 001 T5).
///
/// Every field is read from an existing ledger under the Service guard —
/// the fixed slot/ticket ledger and the driver's real TX resource ledger —
/// never synthesized from software capacities.
#[cfg(feature = "queue-diagnostics")]
pub struct DiagnosticEnqueueReport {
    /// The frame took a TX slot (its ticket is live until reclaim).
    pub accepted: bool,
    /// Fixed TX slot occupancy after this call.
    pub tx_slot_occupancy: u64,
    /// Fixed TX slot high-water mark.
    pub tx_slot_high_water: u64,
    /// Fixed TX slot full events (the 64/65 boundary counter).
    pub tx_slot_full: u64,
    /// Fixed RX slot occupancy.
    pub rx_slot_occupancy: u64,
    /// Live TX tickets (queued + device-owned).
    pub tickets_live: u64,
    /// Driver TX buffers available / in flight, when observable.
    pub tx_buffer_available: Option<u64>,
    pub tx_buffer_inflight: Option<u64>,
    /// Driver TX descriptors available / in flight, when observable.
    pub tx_descriptor_available: Option<u64>,
    pub tx_descriptor_inflight: Option<u64>,
    /// Legal OWN=0 completions the driver has synchronously observed.
    pub completions_seen: Option<u64>,
}

/// Diagnostic minimal complete-frame enqueue (MS12 Iteration 001 T5).
///
/// One frame per call into the target's fixed TX slot storage through the
/// device's own checked-ticket path, the ledgers read under the same guard,
/// then exactly one software nudge after the guard drops so the owner
/// submits it. Never injects a ticket, never modifies a descriptor, never
/// fabricates a completion.
#[cfg(feature = "queue-diagnostics")]
pub fn diagnostic_enqueue_frame(
    dst: [u8; 6],
    ethertype: u16,
    payload: &[u8],
) -> axdriver::prelude::DevResult<DiagnosticEnqueueReport> {
    let mut service = get_service();
    let accepted = match service.diagnostic_enqueue_target(dst, ethertype, payload) {
        Ok(()) => true,
        Err(axdriver::prelude::DevError::Again) => false,
        Err(e) => return Err(e),
    };
    // The ledgers report even on a refused frame: the boundary observation
    // is exactly what the payload prints.
    let slot = service.v3_slot_ledger();
    let tx_ledger = service.v3_tx_resource_ledger();
    drop(service);
    async_rx::software_nudge();
    Ok(DiagnosticEnqueueReport {
        accepted,
        tx_slot_occupancy: slot.tx_occupancy,
        tx_slot_high_water: slot.tx_high_water,
        tx_slot_full: slot.tx_full,
        rx_slot_occupancy: slot.rx_occupancy,
        tickets_live: slot.live,
        tx_buffer_available: tx_ledger.map(|l| l.buffer_available),
        tx_buffer_inflight: tx_ledger.map(|l| l.buffer_inflight),
        tx_descriptor_available: tx_ledger.map(|l| l.descriptor_available),
        tx_descriptor_inflight: tx_ledger.map(|l| l.descriptor_inflight),
        completions_seen: tx_ledger.map(|l| l.completions_seen),
    })
}

/// Queues one reset request for the unique resident queue owner.
#[cfg(feature = "queue-diagnostics")]
pub fn recovery_reset_request() -> axdriver::prelude::DevResult {
    async_rx::recovery_reset_request_shared()
}

/// Reserves the sole C4 flush waiter and returns its future (D8).
///
/// The kernel ioctl wraps this in a fixed deadline; dropping the future
/// clears the waiter without changing packet ownership.
#[cfg(feature = "queue-diagnostics")]
pub fn flush() -> axdriver::prelude::DevResult<flush::FlushFuture> {
    use crate::async_rx::ServiceAccess;
    flush::flush_new(ServiceAccess::Global)
}
