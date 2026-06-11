// AsyncUartReader/Writer 实现 TtyRead/TtyWrite trait，用于 Tty 泛型绑定

use crate::pseudofs::dev::tty::terminal::ldisc::{TtyRead, TtyWrite};
use crate::drivers::async_driver::DRIVER;

pub struct AsyncUartReader;

impl TtyRead for AsyncUartReader {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        DRIVER.rx.pop(buf)
    }
}

pub struct AsyncUartWriter;

impl TtyWrite for AsyncUartWriter {
    fn write(&self, buf: &[u8]) {
        if buf.is_empty() { return; }
        DRIVER.tx.push(buf);
    }
}

impl Clone for AsyncUartWriter {
    fn clone(&self) -> Self { Self }
}

// embedded-io-async trait implementations (standard async I/O interface)
impl embedded_io_async::ErrorType for AsyncUartReader {
    type Error = core::convert::Infallible;
}

impl embedded_io_async::Read for AsyncUartReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(DRIVER.rx.pop(buf))
    }
}

impl embedded_io_async::ErrorType for AsyncUartWriter {
    type Error = core::convert::Infallible;
}

impl embedded_io_async::Write for AsyncUartWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        DRIVER.tx.push(buf);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
