use alloc::boxed::Box;
use core::{
    pin::Pin,
    task::{Context, Waker},
};

use axhal::time::{NANOS_PER_MICROS, TimeValue, wall_time_nanos};
use axtask::future::sleep_until;
use smoltcp::{
    iface::{Interface, PollIngressSingleResult, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{HardwareAddress, IpAddress, IpListenEndpoint},
};

use crate::{LISTEN_TABLE, SOCKET_SET, router::Router};

const POLLING_FALLBACK: Duration = Duration::from_millis(10);

fn now() -> Instant {
    Instant::from_micros_const((wall_time_nanos() / NANOS_PER_MICROS) as i64)
}

fn select_wake_deadline(
    protocol_deadline: Option<Instant>,
    polling_deadline: Option<Instant>,
) -> Option<Instant> {
    match (protocol_deadline, polling_deadline) {
        (Some(protocol), Some(polling)) => Some(protocol.min(polling)),
        (Some(protocol), None) => Some(protocol),
        (None, Some(polling)) => Some(polling),
        (None, None) => None,
    }
}

/// `polling_capabilities` yields one `requires_polling()` result per device,
/// where bit `i` in `mask` selects device `i`.
fn any_masked_device_requires_polling(
    mask: u32,
    polling_capabilities: impl IntoIterator<Item = bool>,
) -> bool {
    polling_capabilities
        .into_iter()
        .enumerate()
        .any(|(i, requires_polling)| mask & (1 << i) != 0 && requires_polling)
}

pub struct Service {
    pub iface: Interface,
    router: Router,
    timeout: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}
impl Service {
    pub fn new(mut router: Router) -> Self {
        let config = smoltcp::iface::Config::new(HardwareAddress::Ip);
        let iface = Interface::new(config, &mut router, now());

        Self {
            iface,
            router,
            timeout: None,
        }
    }

    pub fn poll(&mut self, sockets: &mut SocketSet) -> bool {
        let timestamp = now();
        let mut changed = false;

        self.router.poll(timestamp);
        self.iface.poll_maintenance(timestamp);
        LISTEN_TABLE.reconcile(sockets);
        loop {
            match self
                .iface
                .poll_ingress_single(timestamp, &mut self.router, sockets)
            {
                PollIngressSingleResult::None => break,
                PollIngressSingleResult::PacketProcessed => {}
                PollIngressSingleResult::SocketStateChanged => changed = true,
            }
            LISTEN_TABLE.reconcile(sockets);
        }
        loop {
            match self.iface.poll_egress(timestamp, &mut self.router, sockets) {
                PollResult::None => break,
                PollResult::SocketStateChanged => changed = true,
            }
        }
        LISTEN_TABLE.reconcile(sockets);
        self.router.dispatch(timestamp) || changed
    }

    pub fn get_source_address(&self, dst_addr: &IpAddress) -> IpAddress {
        let Some(rule) = self.router.table.lookup(dst_addr) else {
            panic!("no route to destination: {dst_addr}");
        };
        rule.src
    }

    pub fn device_mask_for(&self, endpoint: &IpListenEndpoint) -> u32 {
        match endpoint.addr {
            Some(addr) => self
                .router
                .table
                .lookup(&addr)
                .map_or(0, |it| 1u32 << it.dev),
            None => u32::MAX,
        }
    }

    pub fn register_waker(&mut self, mask: u32, waker: &Waker) {
        let timestamp = now();
        let protocol_deadline = self.iface.poll_at(timestamp, &SOCKET_SET.inner.lock());
        let polling_deadline = any_masked_device_requires_polling(
            mask,
            self.router.devices.iter().map(|d| d.requires_polling()),
        )
        .then_some(timestamp + POLLING_FALLBACK);
        let next = select_wake_deadline(protocol_deadline, polling_deadline);

        if let Some(t) = next {
            let next = TimeValue::from_micros(t.total_micros() as _);

            // drop old timeout future
            self.timeout = None;

            let mut fut = Box::pin(sleep_until(next));
            let mut cx = Context::from_waker(waker);

            if fut.as_mut().poll(&mut cx).is_ready() {
                waker.wake_by_ref();
                return;
            } else {
                self.timeout = Some(fut);
            }
        }

        for (i, device) in self.router.devices.iter().enumerate() {
            if mask & (1 << i) != 0 {
                device.register_waker(waker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use smoltcp::time::Instant;

    use super::{any_masked_device_requires_polling, select_wake_deadline};

    #[test]
    fn no_deadline_without_protocol_timer_or_polling_fallback() {
        assert_eq!(select_wake_deadline(None, None), None);
    }

    #[test]
    fn preserves_protocol_deadline_without_polling_fallback() {
        let protocol = Instant::from_millis_const(25);

        assert_eq!(select_wake_deadline(Some(protocol), None), Some(protocol));
    }

    #[test]
    fn uses_polling_fallback_without_protocol_deadline() {
        let fallback = Instant::from_millis_const(10);

        assert_eq!(select_wake_deadline(None, Some(fallback)), Some(fallback));
    }

    #[test]
    fn chooses_earlier_protocol_or_polling_deadline() {
        let earlier = Instant::from_millis_const(10);
        let later = Instant::from_millis_const(25);

        assert_eq!(
            select_wake_deadline(Some(later), Some(earlier)),
            Some(earlier)
        );
        assert_eq!(
            select_wake_deadline(Some(earlier), Some(later)),
            Some(earlier)
        );
    }

    #[test]
    fn masked_non_polling_device_does_not_trigger_fallback() {
        let mask = 0b001;
        let capabilities = [false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn unmasked_polling_device_does_not_trigger_fallback() {
        let mask = 0b010;
        let capabilities = [true, false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn masked_polling_device_triggers_fallback() {
        let mask = 0b001;
        let capabilities = [true];

        assert!(any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn mixed_devices_only_masked_polling_decides() {
        let mask = 0b101;
        let capabilities = [true, true, false];

        assert!(any_masked_device_requires_polling(mask, capabilities));

        let mask = 0b101;
        let capabilities = [false, true, false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }
}
