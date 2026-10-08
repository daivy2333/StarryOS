//! Fakes for the T6 host contract tests. They replace exactly the two
//! boundaries the Task Contract names: the map/populate/write backend and
//! the stdio open boundary. Everything else is production code.

use alloc::vec::Vec;
use axerrno::{AxError, AxResult};
use memory_addr::VirtAddr;

use crate::embedded_loader::{ElfMapBackend, SegFlags};
use crate::stdio::StdioTable;

/// One recorded backend call, in issue order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Map(u64, usize, SegFlags, bool),
    Populate(u64, usize, SegFlags),
    Write(u64, usize),
}

/// Backend fake: records every call in order and, when `fail_at` is set,
/// fails the n-th call (1-based) with `fail_err` instead of executing it.
pub struct FakeBackend {
    pub ops: Vec<Op>,
    pub fail_at: Option<(usize, AxError)>,
    calls: usize,
}

impl FakeBackend {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            fail_at: None,
            calls: 0,
        }
    }

    pub fn failing(at: usize, err: AxError) -> Self {
        Self {
            fail_at: Some((at, err)),
            ..Self::new()
        }
    }

    fn check(&mut self) -> Result<(), AxError> {
        self.calls += 1;
        match self.fail_at {
            Some((at, ref err)) if at == self.calls => Err(err.clone()),
            _ => Ok(()),
        }
    }
}

impl ElfMapBackend for FakeBackend {
    type Err = AxError;

    fn map_area(
        &mut self,
        vaddr: VirtAddr,
        size: usize,
        flags: SegFlags,
        populate: bool,
    ) -> AxResult<()> {
        self.check()?;
        self.ops.push(Op::Map(vaddr.as_usize() as u64, size, flags, populate));
        Ok(())
    }

    fn populate_area(&mut self, vaddr: VirtAddr, size: usize, flags: SegFlags) -> AxResult<()> {
        self.check()?;
        self.ops.push(Op::Populate(vaddr.as_usize() as u64, size, flags));
        Ok(())
    }

    fn write_mem(&mut self, vaddr: VirtAddr, data: &[u8]) -> AxResult<()> {
        self.check()?;
        self.ops.push(Op::Write(vaddr.as_usize() as u64, data.len()));
        Ok(())
    }
}

/// Opaque handle the fake table hands out (stands in for `Arc<File>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeFile(pub u8);

/// Open-boundary fake: hands out numbered files and, when `fail_on` is
/// set, fails that n-th open call (1-based).
pub struct FakeOpener {
    pub opened: Vec<bool>,
    pub fail_on: Option<usize>,
    calls: usize,
}

impl FakeOpener {
    pub fn new() -> Self {
        Self {
            opened: Vec::new(),
            fail_on: None,
            calls: 0,
        }
    }

    pub fn failing(on: usize) -> Self {
        Self {
            fail_on: Some(on),
            ..Self::new()
        }
    }

    pub fn open(&mut self, read: bool) -> AxResult<FakeFile> {
        self.calls += 1;
        if self.fail_on == Some(self.calls) {
            return Err(AxError::BadFileDescriptor);
        }
        self.opened.push(read);
        Ok(FakeFile(self.calls as u8))
    }
}

/// Table fake with a bounded capacity (`cap`) and recorded insert order.
pub struct FakeTable {
    pub entries: Vec<FakeFile>,
    pub cap: usize,
}

impl FakeTable {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap,
        }
    }
}

impl StdioTable for FakeTable {
    type File = FakeFile;

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn add(&mut self, file: Self::File) -> AxResult<()> {
        if self.entries.len() >= self.cap {
            return Err(AxError::TooManyOpenFiles);
        }
        self.entries.push(file);
        Ok(())
    }
}
