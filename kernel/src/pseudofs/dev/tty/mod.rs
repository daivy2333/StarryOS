#![allow(dead_code)]
mod ptm;
mod pts;
mod pty;
pub mod terminal;
mod write;

use alloc::sync::{Arc, Weak};
use core::{
    any::Any,
    ops::Deref,
    sync::atomic::{AtomicBool, Ordering},
    task::Context,
};

use axerrno::{AxError, AxResult};
use axfs_ng_vfs::NodeFlags;
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;
use axtask::{
    current,
    future::{block_on, poll_io},
};
use linux_raw_sys::general::{ONLCR, OPOST};
use starry_process::Process;
use starry_vm::{VmMutPtr, VmPtr};

pub use self::{ptm::Ptmx, pts::PtsDir, pty::PtyDriver};
use self::{
    terminal::{
        Terminal, WindowSize,
        ldisc::{LineDiscipline, ProcessMode, TtyConfig, TtyRead, TtyWrite, TtyWriteReady},
        termios::{Termios, Termios2},
    },
    write::{ONLCR_BUF_SIZE, OnlcrChunk, ShortWriteAction, classify_short_write},
};
use crate::{pseudofs::DeviceOps, task::AsThread};

/// Tty device
pub struct Tty<R, W> {
    this: Weak<Self>,
    terminal: Arc<Terminal>,
    ldisc: Mutex<LineDiscipline<R, W>>,
    writer: W,
    is_ptm: bool,
    nonblocking: AtomicBool,
}

impl<R: TtyRead, W: TtyWrite + Clone> Tty<R, W> {
    pub fn new(terminal: Arc<Terminal>, config: TtyConfig<R, W>) -> Arc<Self> {
        let writer = config.writer.clone();
        let is_ptm = matches!(&config.process_mode, ProcessMode::None(_));
        let ldisc = Mutex::new(LineDiscipline::new(terminal.clone(), config));
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            terminal,
            ldisc,
            writer,
            is_ptm,
            nonblocking: AtomicBool::new(false),
        })
    }
}

impl<R: TtyRead, W: TtyWrite> Tty<R, W> {
    pub fn bind_to(self: &Arc<Self>, proc: &Process) -> AxResult<()> {
        let pg = proc.group();
        if pg.session().sid() != proc.pid() {
            return Err(AxError::OperationNotPermitted);
        }
        assert!(pg.session().set_terminal_with(|| {
            self.terminal.job_control.set_session(&pg.session());
            self.clone()
        }));

        self.terminal.job_control.set_foreground(&pg)?;
        Ok(())
    }

    pub fn pty_number(&self) -> u32 {
        self.terminal.pty_number.load(Ordering::Acquire)
    }
}

impl<R: TtyRead, W: TtyWriteReady> Tty<R, W> {
    fn finish_write(&self, buf: &[u8], written: usize) -> AxResult<usize> {
        let mut total = written;
        block_on(poll_io(self, IoEvents::OUT, false, || {
            total += self.writer.write(&buf[total..]);
            if total == buf.len() {
                Ok(total)
            } else {
                Err(AxError::WouldBlock)
            }
        }))
    }

    fn write_mapped_chunk(&self, chunk: &OnlcrChunk) -> AxResult<()> {
        let bytes = chunk.bytes();
        let written = self.writer.write(bytes);
        if written == bytes.len() {
            return Ok(());
        }
        self.finish_write(bytes, written).map(|_| ())
    }

    fn write_onlcr_blocking(&self, buf: &[u8]) -> AxResult<usize> {
        let mut consumed = 0;
        while consumed < buf.len() {
            let chunk = OnlcrChunk::new(&buf[consumed..], ONLCR_BUF_SIZE);
            self.write_mapped_chunk(&chunk)?;
            consumed += chunk.source_len();
        }
        Ok(consumed)
    }

    fn write_onlcr_once(&self, buf: &[u8], zero_would_block: bool) -> AxResult<usize> {
        let chunk = OnlcrChunk::new(buf, self.writer.writable_len());
        if chunk.source_len() == 0 {
            return if zero_would_block {
                Err(AxError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        let accepted = chunk.accepted_source_len(self.writer.write(chunk.bytes()));
        if accepted == 0 && zero_would_block {
            Err(AxError::WouldBlock)
        } else {
            Ok(accepted)
        }
    }

    fn write_onlcr(&self, buf: &[u8], nonblocking: bool) -> AxResult<usize> {
        if !nonblocking && self.writer.waits_for_write_completion() {
            self.write_onlcr_blocking(buf)
        } else {
            self.write_onlcr_once(buf, nonblocking)
        }
    }
}

impl<R: TtyRead, W: TtyWriteReady> DeviceOps for Tty<R, W> {
    fn read_at(&self, buf: &mut [u8], _offset: u64) -> AxResult<usize> {
        let nb = self.nonblocking.load(Ordering::Acquire);
        block_on(poll_io(
            &self.terminal.job_control,
            IoEvents::IN,
            nb,
            || {
                if self.is_ptm || self.terminal.job_control.current_in_foreground() {
                    self.ldisc.lock().read(buf, nb)
                } else {
                    Err(AxError::WouldBlock)
                }
            },
        ))
    }

    fn write_at(&self, buf: &[u8], _offset: u64) -> AxResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let term = self.terminal.load_termios();
        let onlcr = term.has_oflag(OPOST) && term.has_oflag(ONLCR);

        if !onlcr {
            let written = self.writer.write(buf);
            if written == buf.len() {
                return Ok(written);
            }
            let nb = self.nonblocking.load(Ordering::Acquire);
            match classify_short_write(
                written,
                buf.len(),
                nb,
                self.writer.waits_for_write_completion(),
            ) {
                ShortWriteAction::Return(written) => Ok(written),
                ShortWriteAction::WouldBlock => Err(AxError::WouldBlock),
                ShortWriteAction::Wait => self.finish_write(buf, written),
            }
        } else {
            let nb = self.nonblocking.load(Ordering::Acquire);
            self.write_onlcr(buf, nb)
        }
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> AxResult<usize> {
        use linux_raw_sys::ioctl::*;
        match cmd {
            TCGETS => {
                (arg as *mut Termios).vm_write(*self.terminal.termios.lock().as_ref().deref())?;
            }
            TCGETS2 => {
                (arg as *mut Termios2).vm_write(*self.terminal.termios.lock().as_ref())?;
            }
            TCSETS | TCSETSF | TCSETSW => {
                // TODO: drain output?
                *self.terminal.termios.lock() =
                    Arc::new(Termios2::new((arg as *const Termios).vm_read()?));
                if cmd == TCSETSF {
                    self.ldisc.lock().drain_input();
                }
            }
            TCSETS2 | TCSETSF2 | TCSETSW2 => {
                // TODO: drain output?
                *self.terminal.termios.lock() = Arc::new((arg as *const Termios2).vm_read()?);
                if cmd == TCSETSF2 {
                    self.ldisc.lock().drain_input();
                }
            }
            TIOCGPGRP => {
                let foreground = self
                    .terminal
                    .job_control
                    .foreground()
                    .ok_or(AxError::NoSuchProcess)?;
                (arg as *mut u32).vm_write(foreground.pgid())?;
            }
            TIOCSPGRP => {
                let curr = current();
                self.terminal
                    .job_control
                    .set_foreground(&curr.as_thread().proc_data.proc.group())?;
            }
            TIOCGWINSZ => {
                (arg as *mut WindowSize).vm_write(*self.terminal.window_size.lock())?;
            }
            TIOCSWINSZ => {
                *self.terminal.window_size.lock() = (arg as *const WindowSize).vm_read()?;
            }
            TIOCSPTLCK => {}
            TIOCGPTN => {
                (arg as *mut u32).vm_write(self.pty_number())?;
            }
            TIOCSCTTY => {
                self.this
                    .upgrade()
                    .ok_or(AxError::NotFound)?
                    .bind_to(&current().as_thread().proc_data.proc)?;
            }
            TIOCNOTTY => {
                if current()
                    .as_thread()
                    .proc_data
                    .proc
                    .group()
                    .session()
                    .unset_terminal(&(self.this.upgrade().ok_or(AxError::NotFound)? as _))
                {
                } else {
                    warn!("Failed to unset terminal");
                }
            }
            FIONBIO => {
                self.nonblocking.store(arg != 0, Ordering::Release);
            }
            _ => return Err(AxError::NotATty),
        }
        Ok(0)
    }

    fn as_pollable(&self) -> Option<&dyn Pollable> {
        Some(self)
    }

    /// Casts the device operations to a dynamic type.
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn flags(&self) -> NodeFlags {
        NodeFlags::NON_CACHEABLE | NodeFlags::STREAM
    }
}

impl<R: TtyRead, W: TtyWriteReady> Pollable for Tty<R, W> {
    fn poll(&self) -> IoEvents {
        let mut events = self.terminal.job_control.poll();
        if self.writer.can_write() {
            events |= IoEvents::OUT;
        }
        if self.is_ptm || events.contains(IoEvents::IN) {
            events.set(IoEvents::IN, self.ldisc.lock().poll_read());
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if !self.is_ptm {
            self.terminal.job_control.register(context, events);
        }
        if events.contains(IoEvents::IN) {
            self.ldisc.lock().register_rx_waker(context.waker());
        }
        if events.contains(IoEvents::OUT) {
            self.writer.register_writable_waker(context.waker());
        }
    }
}

pub struct CurrentTty;
impl DeviceOps for CurrentTty {
    fn read_at(&self, _buf: &mut [u8], _offset: u64) -> AxResult<usize> {
        unreachable!()
    }

    fn write_at(&self, _buf: &[u8], _offset: u64) -> AxResult<usize> {
        Ok(0)
    }

    fn ioctl(&self, _cmd: u32, _arg: usize) -> AxResult<usize> {
        unreachable!()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
