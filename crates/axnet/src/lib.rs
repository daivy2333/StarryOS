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
#[cfg(feature = "qemu-diagnostics")]
mod diag;
mod flush;
mod general;
mod listen_table;
/// Socket option types and the [`Configurable`](options::Configurable) trait.
pub mod options;
mod router;
mod service;
mod socket;
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
        RX_TASK_NAME, RxSnapshot, RxSnapshotV3, publish_queue_event, publish_rx_event, rx_snapshot,
        rx_snapshot_v3, software_nudge, start_rx_task,
    },
    socket::*,
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
}

/// Applies a QEMU-only bounded pressure control (D9).
///
/// `op` is `HoldTxSubmit=1`, `HoldTxReclaim=2` (lease 1..=2000 ms) or
/// `Release=3` (lease 0). Committing a hold publishes queue work so the sole
/// owner pauses the matching stage; `Release` and lease expiry resume it.
#[cfg(feature = "qemu-diagnostics")]
pub fn diagnostic_control(op: u64, lease_ms: u64) -> axdriver::prelude::DevResult {
    use crate::async_rx::QUEUE_EVENT;
    diag::DIAGNOSTIC.control(op, lease_ms, diag::diag_now())?;
    // Wake the queue owner so a sleeping task observes the new hold/release.
    QUEUE_EVENT.publish_queue_work();
    Ok(())
}

/// Reserves the sole C4 flush waiter and returns its future (D8).
///
/// The kernel ioctl wraps this in a fixed deadline; dropping the future
/// clears the waiter without changing packet ownership.
#[cfg(feature = "qemu-diagnostics")]
pub fn flush() -> axdriver::prelude::DevResult<flush::FlushFuture> {
    use crate::async_rx::ServiceAccess;
    flush::flush_new(ServiceAccess::Global)
}
