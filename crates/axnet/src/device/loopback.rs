use core::task::Waker;

use axdriver::prelude::DevError;
use axpoll::PollSet;
use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

use crate::{
    consts::SOCKET_BUFFER_SIZE,
    device::{Device, RxStep, TxOutcome, TxPreflight, fixed_queue::FixedFrameQueue},
};

pub struct LoopbackDevice {
    buffer: FixedFrameQueue<SOCKET_BUFFER_SIZE>,
    poll: PollSet,
}
impl LoopbackDevice {
    pub fn new() -> Self {
        Self {
            buffer: FixedFrameQueue::new(),
            poll: PollSet::new(),
        }
    }
}

impl Device for LoopbackDevice {
    fn name(&self) -> &str {
        "lo"
    }

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
        let Some(rx_buf) = self.buffer.peek() else {
            return RxStep::Empty;
        };
        match buffer.enqueue(rx_buf.len(), ()) {
            Ok(dst) => {
                dst.copy_from_slice(rx_buf);
                let _ = self.buffer.pop();
                RxStep::Delivered
            }
            Err(_) => RxStep::Fault(DevError::BadState),
        }
    }

    fn preflight_send(
        &mut self,
        _next_hop: IpAddress,
        packet: &[u8],
        _timestamp: Instant,
    ) -> TxPreflight {
        match self.buffer.preflight(packet.len()) {
            Ok(()) => TxPreflight::Ready,
            Err(_) => TxPreflight::Full,
        }
    }

    fn send(&mut self, next_hop: IpAddress, packet: &[u8], _timestamp: Instant) -> TxOutcome {
        match self.buffer.enqueue(packet, (), None) {
            Ok(()) => {
                self.poll.wake();
                TxOutcome::Accepted {
                    rx_became_ready: true,
                }
            }
            Err(_) => {
                warn!(
                    "Loopback device buffer is full, dropping packet to {}",
                    next_hop
                );
                TxOutcome::Full
            }
        }
    }

    fn register_waker(&self, waker: &Waker) {
        self.poll.register(waker);
    }
}
