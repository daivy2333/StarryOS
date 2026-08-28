use core::{
    sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    time::Duration,
};

use axerrno::{AxError, AxResult, LinuxError};
use axpoll::{IoEvents, Pollable};
use axtask::future::{block_on, poll_io, timeout};

use crate::options::{Configurable, GetSocketOption, SetSocketOption};

/// General options for all sockets.
pub(crate) struct GeneralOptions {
    /// Whether the socket is non-blocking.
    nonblock: AtomicBool,
    /// Whether the socket should reuse the address.
    reuse_address: AtomicBool,

    send_timeout_nanos: AtomicU64,
    recv_timeout_nanos: AtomicU64,
    /// Task 3.1: saved stable socket error as a Linux errno number
    /// (0 = none). Exposed by `SO_ERROR` without being consumed.
    saved_error: AtomicI32,
}
impl Default for GeneralOptions {
    fn default() -> Self {
        Self::new()
    }
}
impl GeneralOptions {
    pub fn new() -> Self {
        Self {
            nonblock: AtomicBool::new(false),
            reuse_address: AtomicBool::new(false),

            send_timeout_nanos: AtomicU64::new(0),
            recv_timeout_nanos: AtomicU64::new(0),
            saved_error: AtomicI32::new(0),
        }
    }

    /// Saves the Linux errno number backing the non-consuming `SO_ERROR` view.
    pub(crate) fn record_socket_error(&self, err: &AxError) {
        let errno = LinuxError::from(*err).code();
        self.saved_error.store(errno, Ordering::Release);
    }

    pub fn nonblocking(&self) -> bool {
        self.nonblock.load(Ordering::Relaxed)
    }

    pub fn reuse_address(&self) -> bool {
        self.reuse_address.load(Ordering::Relaxed)
    }

    pub fn send_timeout(&self) -> Option<Duration> {
        let nanos = self.send_timeout_nanos.load(Ordering::Relaxed);
        (nanos > 0).then(|| Duration::from_nanos(nanos))
    }

    pub fn recv_timeout(&self) -> Option<Duration> {
        let nanos = self.recv_timeout_nanos.load(Ordering::Relaxed);
        (nanos > 0).then(|| Duration::from_nanos(nanos))
    }

    pub fn send_poller<P: Pollable, F: FnMut() -> AxResult<T>, T>(
        &self,
        pollable: &P,
        f: F,
    ) -> AxResult<T> {
        block_on(timeout(
            self.send_timeout(),
            poll_io(pollable, IoEvents::OUT, self.nonblocking(), f),
        ))?
    }

    pub fn recv_poller<P: Pollable, F: FnMut() -> AxResult<T>, T>(
        &self,
        pollable: &P,
        f: F,
    ) -> AxResult<T> {
        block_on(timeout(
            self.recv_timeout(),
            poll_io(pollable, IoEvents::IN, self.nonblocking(), f),
        ))?
    }
}
impl Configurable for GeneralOptions {
    fn get_option_inner(&self, option: &mut GetSocketOption) -> AxResult<bool> {
        use GetSocketOption as O;
        match option {
            O::Error(error) => {
                **error = self.saved_error.load(Ordering::Acquire);
            }
            O::NonBlocking(nonblock) => {
                **nonblock = self.nonblocking();
            }
            O::ReuseAddress(reuse) => {
                **reuse = self.reuse_address();
            }
            O::SendTimeout(timeout) => {
                **timeout = Duration::from_nanos(self.send_timeout_nanos.load(Ordering::Relaxed));
            }
            O::ReceiveTimeout(timeout) => {
                **timeout = Duration::from_nanos(self.recv_timeout_nanos.load(Ordering::Relaxed));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn set_option_inner(&self, option: SetSocketOption) -> AxResult<bool> {
        use SetSocketOption as O;

        match option {
            O::NonBlocking(nonblock) => {
                self.nonblock.store(*nonblock, Ordering::Relaxed);
            }
            O::ReuseAddress(reuse) => {
                self.reuse_address.store(*reuse, Ordering::Relaxed);
            }
            O::SendTimeout(timeout) => {
                self.send_timeout_nanos
                    .store(timeout.as_nanos() as u64, Ordering::Relaxed);
            }
            O::ReceiveTimeout(timeout) => {
                self.recv_timeout_nanos
                    .store(timeout.as_nanos() as u64, Ordering::Relaxed);
            }
            O::SendBuffer(_) | O::ReceiveBuffer(_) => {
                // TODO(mivik): implement buffer size options
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use axerrno::{AxError, LinuxError};

    use super::GeneralOptions;
    use crate::options::{Configurable, GetSocketOption};

    #[test]
    fn so_error_returns_saved_errno_without_consuming() {
        let general = GeneralOptions::new();

        let mut initial = -1i32;
        assert!(
            general
                .get_option_inner(&mut GetSocketOption::Error(&mut initial))
                .unwrap()
        );
        assert_eq!(initial, 0);

        general.record_socket_error(&AxError::ConnectionRefused);

        let mut first = 0i32;
        general
            .get_option_inner(&mut GetSocketOption::Error(&mut first))
            .unwrap();
        assert_eq!(first, LinuxError::ECONNREFUSED.code());

        let mut second = 0i32;
        general
            .get_option_inner(&mut GetSocketOption::Error(&mut second))
            .unwrap();
        assert_eq!(second, first);
    }
}
