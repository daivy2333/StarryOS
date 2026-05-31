// Placeholder file - will be implemented in subsequent tasks

use crate::pseudofs::dev::tty::terminal::ldisc::{TtyRead, TtyWrite};
use crate::drivers::async_driver::DRIVER;

pub struct AsyncUartReader;

impl TtyRead for AsyncUartReader {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        DRIVER.buffer.lock().pop_rx(buf)
    }
}

pub struct AsyncUartWriter;

impl TtyWrite for AsyncUartWriter {
    fn write(&self, buf: &[u8]) {
        if buf.is_empty() { return; }
        DRIVER.buffer.lock().push_tx(buf);
    }
}

impl Clone for AsyncUartWriter {
    fn clone(&self) -> Self { Self }
}

#[derive(Clone)]
pub struct ConsoleWriter;
impl TtyWrite for ConsoleWriter {
    fn write(&self, buf: &[u8]) {
        axhal::console::write_bytes(buf);
    }
}
