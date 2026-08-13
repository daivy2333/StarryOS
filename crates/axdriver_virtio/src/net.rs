use alloc::{sync::Arc, vec::Vec};

use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};
use axdriver_net::{
    EthernetAddress, NetBuf, NetBufBox, NetBufPool, NetBufPtr, NetDriverOps, NetQueueControl,
    NetQueueDirection, NetTxQueue, TxCookie,
};
use virtio_drivers::{Hal, device::net::VirtIONetRaw as InnerDev, transport::Transport};

use crate::as_dev_err;

const NET_BUF_LEN: usize = 1526;

/// The VirtIO network device driver.
///
/// `QS` is the VirtIO queue size.
pub struct VirtIoNetDev<H: Hal, T: Transport, const QS: usize> {
    rx_buffers: [Option<NetBufBox>; QS],
    tx_buffers: [Option<NetBufBox>; QS],
    tx_cookies: [Option<TxCookie>; QS],
    free_tx_bufs: Vec<NetBufBox>,
    buf_pool: Arc<NetBufPool>,
    inner: InnerDev<H, T, QS>,
    irq: Option<usize>,
}

unsafe impl<H: Hal, T: Transport, const QS: usize> Send for VirtIoNetDev<H, T, QS> {}
unsafe impl<H: Hal, T: Transport, const QS: usize> Sync for VirtIoNetDev<H, T, QS> {}

impl<H: Hal, T: Transport, const QS: usize> VirtIoNetDev<H, T, QS> {
    /// Creates a new driver instance and initializes the device, or returns
    /// an error if any step fails.
    pub fn try_new(transport: T, irq: Option<usize>) -> DevResult<Self> {
        // 0. Create a new driver instance.
        const NONE_BUF: Option<NetBufBox> = None;
        const NONE_COOKIE: Option<TxCookie> = None;
        let inner = InnerDev::new(transport).map_err(as_dev_err)?;
        let rx_buffers = [NONE_BUF; QS];
        let tx_buffers = [NONE_BUF; QS];
        let tx_cookies = [NONE_COOKIE; QS];
        let buf_pool = NetBufPool::new(2 * QS, NET_BUF_LEN)?;
        let free_tx_bufs = Vec::with_capacity(QS);

        let mut dev = Self {
            rx_buffers,
            inner,
            tx_buffers,
            tx_cookies,
            free_tx_bufs,
            buf_pool,
            irq,
        };

        // 1. Fill all rx buffers.
        for (i, rx_buf_place) in dev.rx_buffers.iter_mut().enumerate() {
            let mut rx_buf = dev.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Safe because the buffer lives as long as the queue.
            let token = unsafe {
                dev.inner
                    .receive_begin(rx_buf.raw_buf_mut())
                    .map_err(as_dev_err)?
            };
            assert_eq!(token, i as u16);
            *rx_buf_place = Some(rx_buf);
        }

        // 2. Allocate all tx buffers.
        for _ in 0..QS {
            let mut tx_buf = dev.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Fill header
            let hdr_len = dev
                .inner
                .fill_buffer_header(tx_buf.raw_buf_mut())
                .or(Err(DevError::InvalidParam))?;
            tx_buf.set_header_len(hdr_len);
            dev.free_tx_bufs.push(tx_buf);
        }

        // 3. Return the driver instance.
        Ok(dev)
    }
}

fn recover_submit_error(
    free_tx_bufs: &mut Vec<NetBufBox>,
    tx_buf: NetBufBox,
    err: virtio_drivers::Error,
) -> DevError {
    free_tx_bufs.push(tx_buf);
    as_dev_err(err)
}

fn install_tx_submission<const QS: usize>(
    tx_buffers: &mut [Option<NetBufBox>; QS],
    tx_cookies: &mut [Option<TxCookie>; QS],
    token: u16,
    tx_buf: NetBufBox,
    cookie: TxCookie,
) {
    let slot = token as usize;
    assert!(slot < QS, "VirtIO returned an out-of-range TX token");
    assert!(
        tx_buffers[slot].is_none() && tx_cookies[slot].is_none(),
        "VirtIO reused an in-flight TX token"
    );
    tx_buffers[slot] = Some(tx_buf);
    tx_cookies[slot] = Some(cookie);
}

fn take_tx_completion<const QS: usize>(
    tx_buffers: &mut [Option<NetBufBox>; QS],
    tx_cookies: &mut [Option<TxCookie>; QS],
    token: u16,
) -> DevResult<(NetBufBox, TxCookie)> {
    let slot = token as usize;
    if slot >= QS {
        return Err(DevError::BadState);
    }
    let tx_buf = tx_buffers[slot].take().ok_or(DevError::BadState)?;
    let cookie = match tx_cookies[slot].take() {
        Some(cookie) => cookie,
        None => {
            tx_buffers[slot] = Some(tx_buf);
            return Err(DevError::BadState);
        }
    };
    Ok((tx_buf, cookie))
}

impl<H: Hal, T: Transport, const QS: usize> BaseDriverOps for VirtIoNetDev<H, T, QS> {
    fn device_name(&self) -> &str {
        "virtio-net"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Net
    }

    fn irq_num(&self) -> Option<usize> {
        self.irq
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetQueueControl for VirtIoNetDev<H, T, QS> {
    fn completion_pending(&self, directions: NetQueueDirection) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && self.inner.poll_rx_completion() {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) && self.inner.poll_tx_completion() {
            pending |= NetQueueDirection::TX;
        }
        Ok(pending)
    }

    fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
        if directions.contains(NetQueueDirection::RX) {
            self.inner.suppress_rx_notify();
        }
        if directions.contains(NetQueueDirection::TX) {
            self.inner.suppress_tx_notify();
        }
        Ok(())
    }

    fn arm_notify_and_check(
        &mut self,
        directions: NetQueueDirection,
    ) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && self.inner.arm_rx_notify_and_check() {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) && self.inner.arm_tx_notify_and_check() {
            pending |= NetQueueDirection::TX;
        }
        Ok(pending)
    }

    fn has_rx_completion(&self) -> bool {
        self.inner.poll_rx_completion()
    }

    fn suppress_rx_notify(&mut self) -> DevResult {
        self.inner.suppress_rx_notify();
        Ok(())
    }

    fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
        Ok(self.inner.arm_rx_notify_and_check())
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetDriverOps for VirtIoNetDev<H, T, QS> {
    #[inline]
    fn mac_address(&self) -> EthernetAddress {
        EthernetAddress(self.inner.mac_address())
    }

    #[inline]
    fn can_transmit(&self) -> bool {
        !self.free_tx_bufs.is_empty() && self.inner.can_send()
    }

    #[inline]
    fn can_receive(&self) -> bool {
        self.inner.poll_receive().is_some()
    }

    #[inline]
    fn rx_queue_size(&self) -> usize {
        QS
    }

    #[inline]
    fn tx_queue_size(&self) -> usize {
        QS
    }

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        Some(self)
    }

    fn tx_queue(&mut self) -> Option<&mut dyn NetTxQueue> {
        Some(self)
    }

    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult {
        let mut rx_buf = unsafe { NetBuf::from_buf_ptr(rx_buf) };
        // Safe because we take the ownership of `rx_buf` back to `rx_buffers`,
        // it lives as long as the queue.
        let new_token = unsafe {
            self.inner
                .receive_begin(rx_buf.raw_buf_mut())
                .map_err(as_dev_err)?
        };
        // `rx_buffers[new_token]` is expected to be `None` since it was taken
        // away at `Self::receive()` and has not been added back.
        if self.rx_buffers[new_token as usize].is_some() {
            return Err(DevError::BadState);
        }
        self.rx_buffers[new_token as usize] = Some(rx_buf);
        Ok(())
    }

    fn recycle_tx_buffers(&mut self) -> DevResult {
        while let Some(token) = self.inner.poll_transmit() {
            if self.tx_cookies[token as usize].is_some() {
                return Err(DevError::BadState);
            }
            let tx_buf = self.tx_buffers[token as usize]
                .take()
                .ok_or(DevError::BadState)?;
            unsafe {
                self.inner
                    .transmit_complete(token, tx_buf.packet_with_header())
                    .map_err(as_dev_err)?;
            }
            // Recycle the buffer.
            self.free_tx_bufs.push(tx_buf);
        }
        Ok(())
    }

    fn transmit(&mut self, tx_buf: NetBufPtr) -> DevResult {
        // 0. prepare tx buffer.
        let tx_buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
        // 1. transmit packet.
        let token = match unsafe { self.inner.transmit_begin(tx_buf.packet_with_header()) } {
            Ok(token) => token,
            Err(err) => return Err(recover_submit_error(&mut self.free_tx_bufs, tx_buf, err)),
        };
        self.tx_buffers[token as usize] = Some(tx_buf);
        Ok(())
    }

    fn receive(&mut self) -> DevResult<NetBufPtr> {
        self.inner.ack_interrupt();
        if let Some(token) = self.inner.poll_receive() {
            let mut rx_buf = self.rx_buffers[token as usize]
                .take()
                .ok_or(DevError::BadState)?;
            // Safe because the buffer lives as long as the queue.
            let (hdr_len, pkt_len) = unsafe {
                self.inner
                    .receive_complete(token, rx_buf.raw_buf_mut())
                    .map_err(as_dev_err)?
            };
            rx_buf.set_header_len(hdr_len);
            rx_buf.set_packet_len(pkt_len);

            Ok(rx_buf.into_buf_ptr())
        } else {
            Err(DevError::Again)
        }
    }

    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr> {
        // 0. Allocate a buffer from the queue.
        let mut net_buf = self.free_tx_bufs.pop().ok_or(DevError::NoMemory)?;
        let pkt_len = size;

        // 1. Check if the buffer is large enough.
        let hdr_len = net_buf.header_len();
        if hdr_len + pkt_len > net_buf.capacity() {
            self.free_tx_bufs.push(net_buf);
            return Err(DevError::InvalidParam);
        }
        net_buf.set_packet_len(pkt_len);

        // 2. Return the buffer.
        Ok(net_buf.into_buf_ptr())
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetTxQueue for VirtIoNetDev<H, T, QS> {
    fn submit_tx(&mut self, tx_buf: NetBufPtr, cookie: TxCookie) -> DevResult {
        let tx_buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
        let token = match unsafe { self.inner.transmit_begin(tx_buf.packet_with_header()) } {
            Ok(token) => token,
            Err(err) => return Err(recover_submit_error(&mut self.free_tx_bufs, tx_buf, err)),
        };
        install_tx_submission(
            &mut self.tx_buffers,
            &mut self.tx_cookies,
            token,
            tx_buf,
            cookie,
        );
        Ok(())
    }

    fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>> {
        let Some(token) = self.inner.poll_transmit() else {
            return Ok(None);
        };
        let slot = token as usize;
        let tx_buf = self
            .tx_buffers
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(DevError::BadState)?;
        if self.tx_cookies.get(slot).and_then(Option::as_ref).is_none() {
            return Err(DevError::BadState);
        }
        // Keep the ledger intact until the transport has accepted the
        // completion. A fatal completion error therefore still has one clear
        // owner for both the buffer and cookie.
        unsafe {
            self.inner
                .transmit_complete(token, tx_buf.packet_with_header())
                .map_err(as_dev_err)?;
        }
        let (tx_buf, cookie) =
            take_tx_completion(&mut self.tx_buffers, &mut self.tx_cookies, token)?;
        self.free_tx_bufs.push(tx_buf);
        Ok(Some(cookie))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_full_recovers_buffer_and_maps_to_again() {
        let pool = NetBufPool::new(1, NET_BUF_LEN).unwrap();
        let buffer = pool.alloc_boxed().unwrap();
        let mut free = Vec::new();
        let err = recover_submit_error(&mut free, buffer, virtio_drivers::Error::QueueFull);
        assert!(matches!(err, DevError::Again));
        assert_eq!(free.len(), 1);
    }

    #[test]
    fn token_slot_round_trips_opaque_cookie_once() {
        const NONE_BUF: Option<NetBufBox> = None;
        const NONE_COOKIE: Option<TxCookie> = None;
        let pool = NetBufPool::new(1, NET_BUF_LEN).unwrap();
        let mut buffers = [NONE_BUF; 2];
        let mut cookies = [NONE_COOKIE; 2];
        install_tx_submission(
            &mut buffers,
            &mut cookies,
            1,
            pool.alloc_boxed().unwrap(),
            TxCookie::new(41),
        );
        let (_buffer, cookie) = take_tx_completion(&mut buffers, &mut cookies, 1).unwrap();
        assert_eq!(cookie, TxCookie::new(41));
        assert!(take_tx_completion(&mut buffers, &mut cookies, 1).is_err());
    }

    #[test]
    fn token_slots_preserve_out_of_order_cookie_identity() {
        const NONE_BUF: Option<NetBufBox> = None;
        const NONE_COOKIE: Option<TxCookie> = None;
        let pool = NetBufPool::new(2, NET_BUF_LEN).unwrap();
        let mut buffers = [NONE_BUF; 2];
        let mut cookies = [NONE_COOKIE; 2];
        install_tx_submission(
            &mut buffers,
            &mut cookies,
            0,
            pool.alloc_boxed().unwrap(),
            TxCookie::new(10),
        );
        install_tx_submission(
            &mut buffers,
            &mut cookies,
            1,
            pool.alloc_boxed().unwrap(),
            TxCookie::new(11),
        );
        assert_eq!(
            take_tx_completion(&mut buffers, &mut cookies, 1).unwrap().1,
            TxCookie::new(11)
        );
        assert_eq!(
            take_tx_completion(&mut buffers, &mut cookies, 0).unwrap().1,
            TxCookie::new(10)
        );
    }

    #[test]
    #[should_panic(expected = "reused an in-flight TX token")]
    fn token_slot_overwrite_is_fatal_without_losing_existing_owner() {
        const NONE_BUF: Option<NetBufBox> = None;
        const NONE_COOKIE: Option<TxCookie> = None;
        let pool = NetBufPool::new(2, NET_BUF_LEN).unwrap();
        let mut buffers = [NONE_BUF; 1];
        let mut cookies = [NONE_COOKIE; 1];
        install_tx_submission(
            &mut buffers,
            &mut cookies,
            0,
            pool.alloc_boxed().unwrap(),
            TxCookie::new(10),
        );
        install_tx_submission(
            &mut buffers,
            &mut cookies,
            0,
            pool.alloc_boxed().unwrap(),
            TxCookie::new(11),
        );
    }
}
