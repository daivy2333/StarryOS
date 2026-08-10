use alloc::vec;
use core::task::Waker;

use axdriver::prelude::DevError;
use axpoll::PollSet;
use smoltcp::{
    storage::{PacketBuffer, PacketMetadata},
    time::Instant,
    wire::IpAddress,
};

use crate::{
    consts::{SOCKET_BUFFER_SIZE, STANDARD_MTU},
    device::{Device, RxStep},
};

pub struct LoopbackDevice {
    buffer: PacketBuffer<'static, ()>,
    poll: PollSet,
}
impl LoopbackDevice {
    pub fn new() -> Self {
        let buffer = PacketBuffer::new(
            vec![PacketMetadata::EMPTY; SOCKET_BUFFER_SIZE],
            vec![0u8; STANDARD_MTU * SOCKET_BUFFER_SIZE],
        );
        Self {
            buffer,
            poll: PollSet::new(),
        }
    }
}

impl Device for LoopbackDevice {
    fn name(&self) -> &str {
        "lo"
    }

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
        let Some((_, rx_buf)) = self.buffer.dequeue().ok() else {
            return RxStep::Empty;
        };
        match buffer.enqueue(rx_buf.len(), ()) {
            Ok(dst) => {
                dst.copy_from_slice(rx_buf);
                RxStep::Delivered
            }
            Err(_) => RxStep::Fault(DevError::BadState),
        }
    }

    fn send(&mut self, next_hop: IpAddress, packet: &[u8], _timestamp: Instant) -> bool {
        match self.buffer.enqueue(packet.len(), ()) {
            Ok(tx_buf) => {
                tx_buf.copy_from_slice(packet);
                self.poll.wake();
                true
            }
            Err(_) => {
                warn!(
                    "Loopback device buffer is full, dropping packet to {}",
                    next_hop
                );
                false
            }
        }
    }

    fn register_waker(&self, waker: &Waker) {
        self.poll.register(waker);
    }
}
