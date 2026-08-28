use super::{DeviceStatus, DeviceType, Transport};
use crate::{
    queue::{fake_read_write_queue, Descriptor},
    PhysAddr, Result,
};
use alloc::{sync::Arc, vec::Vec};
use core::{
    any::TypeId,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use std::{sync::Mutex, thread};

/// A fake implementation of [`Transport`] for unit tests.
#[derive(Debug)]
pub struct FakeTransport<C: 'static> {
    pub device_type: DeviceType,
    pub max_queue_size: u32,
    pub device_features: u64,
    pub config_space: NonNull<C>,
    pub state: Arc<Mutex<State>>,
}

impl<C> Transport for FakeTransport<C> {
    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn read_device_features(&mut self) -> u64 {
        self.device_features
    }

    fn write_driver_features(&mut self, driver_features: u64) {
        self.state.lock().unwrap().driver_features = driver_features;
    }

    fn max_queue_size(&mut self, _queue: u16) -> u32 {
        self.max_queue_size
    }

    fn notify(&mut self, queue: u16) {
        self.state.lock().unwrap().queues[queue as usize]
            .notified
            .store(true, Ordering::SeqCst);
    }

    fn get_status(&self) -> DeviceStatus {
        self.state.lock().unwrap().status
    }

    fn set_status(&mut self, status: DeviceStatus) {
        let mut state = self.state.lock().unwrap();
        if status.is_empty() && state.defer_reset {
            // A deferred reset does not commit: the observed device status
            // stays non-empty until the test lifts the flag, modelling a device
            // that has not yet confirmed it stopped accessing its queues.
            return;
        }
        state.status = status;
    }

    fn set_guest_page_size(&mut self, guest_page_size: u32) {
        self.state.lock().unwrap().guest_page_size = guest_page_size;
    }

    fn requires_legacy_layout(&self) -> bool {
        false
    }

    fn queue_set(
        &mut self,
        queue: u16,
        size: u32,
        descriptors: PhysAddr,
        driver_area: PhysAddr,
        device_area: PhysAddr,
    ) {
        let mut state = self.state.lock().unwrap();
        state.queues[queue as usize].size = size;
        state.queues[queue as usize].descriptors = descriptors;
        state.queues[queue as usize].driver_area = driver_area;
        state.queues[queue as usize].device_area = device_area;
    }

    fn queue_unset(&mut self, queue: u16) {
        let mut state = self.state.lock().unwrap();
        state.queues[queue as usize].size = 0;
        state.queues[queue as usize].descriptors = 0;
        state.queues[queue as usize].driver_area = 0;
        state.queues[queue as usize].device_area = 0;
    }

    fn queue_used(&mut self, queue: u16) -> bool {
        self.state.lock().unwrap().queues[queue as usize].descriptors != 0
    }

    fn config_generation(&self) -> Option<u8> {
        Some(self.state.lock().unwrap().config_generation)
    }

    fn ack_interrupt(&mut self) -> bool {
        let mut state = self.state.lock().unwrap();
        let pending = state.interrupt_pending;
        if pending {
            state.interrupt_pending = false;
        }
        pending
    }

    fn config_space<T: 'static>(&self) -> Result<NonNull<T>> {
        if TypeId::of::<T>() == TypeId::of::<C>() {
            Ok(self.config_space.cast())
        } else {
            panic!("Unexpected config space type.");
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub status: DeviceStatus,
    pub driver_features: u64,
    pub guest_page_size: u32,
    pub interrupt_pending: bool,
    /// When set, a reset write is held pending and does not clear the observed
    /// device status until the flag is lifted; models a device whose reset
    /// confirmation is deferred or never arrives.
    pub defer_reset: bool,
    /// The current config-generation value reported by the fake device.
    pub config_generation: u8,
    pub queues: Vec<QueueStatus>,
}

impl State {
    /// Simulates the device writing to the given queue.
    ///
    /// The fake device always uses descriptors in order.
    pub fn write_to_queue<const QUEUE_SIZE: usize>(&mut self, queue_index: u16, data: &[u8]) {
        let queue = &self.queues[queue_index as usize];
        assert_ne!(queue.descriptors, 0);
        assert!(fake_read_write_queue(
            queue.descriptors as *const [Descriptor; QUEUE_SIZE],
            queue.driver_area as *const u8,
            queue.device_area as *mut u8,
            |input| {
                assert_eq!(input, Vec::new());
                data.to_owned()
            },
        ));
    }

    /// Simulates the device reading from the given queue.
    ///
    /// Data is read into the `data` buffer passed in. Returns the number of bytes actually read.
    ///
    /// The fake device always uses descriptors in order.
    pub fn read_from_queue<const QUEUE_SIZE: usize>(&mut self, queue_index: u16) -> Vec<u8> {
        let queue = &self.queues[queue_index as usize];
        assert_ne!(queue.descriptors, 0);

        let mut ret = None;

        // Read data from the queue but don't write any response.
        assert!(fake_read_write_queue(
            queue.descriptors as *const [Descriptor; QUEUE_SIZE],
            queue.driver_area as *const u8,
            queue.device_area as *mut u8,
            |input| {
                ret = Some(input);
                Vec::new()
            },
        ));

        ret.unwrap()
    }

    /// Simulates the device reading data from the given queue and then writing a response back.
    ///
    /// The fake device always uses descriptors in order.
    ///
    /// Returns true if a descriptor chain was available and processed, or false if no descriptors were
    /// available.
    pub fn read_write_queue<const QUEUE_SIZE: usize>(
        &mut self,
        queue_index: u16,
        handler: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> bool {
        let queue = &self.queues[queue_index as usize];
        assert_ne!(queue.descriptors, 0);
        fake_read_write_queue(
            queue.descriptors as *const [Descriptor; QUEUE_SIZE],
            queue.driver_area as *const u8,
            queue.device_area as *mut u8,
            handler,
        )
    }

    /// Waits until the given queue is notified.
    pub fn wait_until_queue_notified(state: &Mutex<Self>, queue_index: u16) {
        while !Self::poll_queue_notified(state, queue_index) {
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Checks if the given queue has been notified.
    ///
    /// If it has, returns true and resets the status so this will return false until it is notified
    /// again.
    pub fn poll_queue_notified(state: &Mutex<Self>, queue_index: u16) -> bool {
        state.lock().unwrap().queues[usize::from(queue_index)]
            .notified
            .swap(false, Ordering::SeqCst)
    }
}

#[derive(Debug, Default)]
pub struct QueueStatus {
    pub size: u32,
    pub descriptors: PhysAddr,
    pub driver_area: PhysAddr,
    pub device_area: PhysAddr,
    pub notified: AtomicBool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::NonNull;

    fn fake_transport() -> FakeTransport<[u8; 16]> {
        let mut state = State::default();
        state.queues = (0..2).map(|_| QueueStatus::default()).collect();
        FakeTransport {
            device_type: DeviceType::Network,
            max_queue_size: 4,
            device_features: 0,
            config_space: NonNull::new(Box::into_raw(Box::new([0u8; 16])) as *mut [u8; 16])
                .unwrap(),
            state: Arc::new(Mutex::new(state)),
        }
    }

    #[test]
    fn begin_reset_and_confirmation_are_separate_steps() {
        let mut t = fake_transport();
        // A busy device exposes a non-empty status.
        t.state.lock().unwrap().status =
            DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER_OK;
        assert!(!t.reset_confirmed(), "busy device is not confirmed reset");

        // `begin_reset` is a bounded one-shot empty-status write.
        t.begin_reset();
        assert!(
            t.state.lock().unwrap().status.is_empty(),
            "reset start clears status"
        );
        assert!(t.reset_confirmed(), "immediate reset reads back empty");

        // A device that defers stopping must not be reported confirmed until
        // it actually reads back empty.
        t.state.lock().unwrap().defer_reset = true;
        t.state.lock().unwrap().status =
            DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER_OK;
        t.begin_reset();
        assert!(
            !t.reset_confirmed(),
            "a pending reset must not be reported confirmed"
        );
        assert!(
            !t.state.lock().unwrap().status.is_empty(),
            "deferred reset keeps a non-empty observed status"
        );

        // Simulate the device finishing the reset: it stops and reports empty.
        t.state.lock().unwrap().defer_reset = false;
        t.state.lock().unwrap().status = DeviceStatus::empty();
        assert!(t.reset_confirmed(), "confirmed reset reads back empty");
    }

    #[test]
    fn config_snapshot_retries_when_generation_changes_mid_read() {
        let t = fake_transport();
        t.state.lock().unwrap().config_generation = 3;

        // `read` simulates a device config update racing the read by bumping
        // the generation between the before/after reads of the snapshot.
        let racing = |_t: &FakeTransport<[u8; 16]>| -> crate::Result<u16> {
            t.state.lock().unwrap().config_generation = 4;
            Ok(7)
        };
        assert_eq!(t.read_config_snapshot(racing), Err(crate::Error::Retry));

        // A stable read (no concurrent update) returns the value once.
        let stable = |_t: &FakeTransport<[u8; 16]>| -> crate::Result<u16> { Ok(9) };
        assert_eq!(t.read_config_snapshot(stable), Ok(9));
    }
}
