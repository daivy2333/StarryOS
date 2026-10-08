//! Production embedded-ELF loader (MS17 Iteration 002, task T6).
//!
//! Extracted from the D1 eager path (`loader.rs::load_user_app_from_elf_data`)
//! with an explicit rejection contract: a malformed header, a program
//! header table or segment bytes beyond the image, an entry outside every
//! executable `PT_LOAD`, and non-`ET_EXEC` (dynamic) payloads are all
//! rejected *before* any mapping call, instead of the previous silent
//! `min(len)` truncation. The mapping sequence for a valid image is
//! byte-for-byte the D1 eager sequence.
//!
//! The module depends only on `core`/`alloc` plus `axerrno`,
//! `memory_addr`, `kernel-elf-parser` and `xmas-elf`, so the native
//! `tests/k3-embedded-user-host` package drives it directly; the harness
//! fakes replace exactly the [`ElfMapBackend`] map/populate/write calls.

use alloc::{string::String, vec::Vec};
use axerrno::{AxError, AxResult};
use kernel_elf_parser::{ELFHeadersBuilder, ELFParser, app_stack_region};
use memory_addr::{MemoryAddr, PAGE_SIZE_4K, VirtAddr};
use xmas_elf::header::Type as ElfType;
use xmas_elf::program::Type as PhType;

/// Segment access rights, computed from the ELF program header flags and
/// translated to platform mapping flags by the backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// Mapping backend: the kernel adapts [`crate::mm::aspace::AddrSpace`];
/// the host harness records the calls in order and can fail the n-th one.
pub trait ElfMapBackend {
    type Err;

    /// Map one area (`populate == true` maps and populates immediately,
    /// matching the heap shape of the eager path).
    fn map_area(
        &mut self,
        vaddr: VirtAddr,
        size: usize,
        flags: SegFlags,
        populate: bool,
    ) -> Result<(), Self::Err>;

    /// Populate a previously mapped area (zero-fill for fresh backends).
    fn populate_area(&mut self, vaddr: VirtAddr, size: usize, flags: SegFlags)
        -> Result<(), Self::Err>;

    /// Write bytes into a populated area.
    fn write_mem(&mut self, vaddr: VirtAddr, data: &[u8]) -> Result<(), Self::Err>;
}

/// Loader inputs that are kernel config constants in production and
/// harness parameters on the host.
#[derive(Clone, Copy, Debug)]
pub struct ElfLoadParams {
    /// `ELFParser` bias; `ET_EXEC` images are unaffected (base 0).
    pub user_space_base: usize,
    pub user_stack_top: usize,
    pub user_stack_size: usize,
    pub user_heap_base: usize,
    pub user_heap_size: usize,
}

/// A successfully loaded image: user entry point and initial stack pointer.
#[derive(Clone, Copy, Debug)]
pub struct LoadedImage {
    pub entry: VirtAddr,
    pub user_sp: VirtAddr,
}

fn seg_flags(flags: xmas_elf::program::Flags) -> SegFlags {
    SegFlags {
        read: flags.is_read(),
        write: flags.is_write(),
        execute: flags.is_execute(),
    }
}

/// Load a static `ET_EXEC` image from embedded bytes. Every rejection
/// happens before the first backend call; backend errors propagate
/// immediately and stop the sequence.
pub fn load_elf_image<B: ElfMapBackend>(
    backend: &mut B,
    elf_data: &[u8],
    params: &ElfLoadParams,
    args: &[String],
    envs: &[String],
) -> AxResult<LoadedImage>
where
    AxError: From<B::Err>,
{
    let builder = ELFHeadersBuilder::new(elf_data).map_err(|_| AxError::InvalidExecutable)?;
    let range = builder.ph_range();
    // The program header table itself must lie inside the image; the
    // previous eager path would slice-panic here on a truncated image.
    if range.end > elf_data.len() as u64 {
        return Err(AxError::OutOfRange);
    }
    let headers = builder
        .build(&elf_data[range.start as usize..range.end as usize])
        .map_err(|_| AxError::InvalidExecutable)?;

    // Static executables only: a dynamic (ET_DYN/PIE) payload needs a
    // linker and relocation support this loader does not provide.
    if headers.header.pt2.type_().as_type() != ElfType::Executable {
        return Err(AxError::InvalidExecutable);
    }

    let elf_parser = ELFParser::new(&headers, params.user_space_base)
        .map_err(|_| AxError::InvalidData)?;
    let loads: Vec<_> = headers
        .ph
        .iter()
        .filter(|ph| ph.get_type() == Ok(PhType::Load))
        .collect();

    // Segment file ranges must be inside the image — the old
    // `file_end.min(elf_data.len())` silently truncated corrupt images.
    for ph in &loads {
        if ph.file_size > 0 {
            let file_end = ph.offset as usize + ph.file_size as usize;
            if ph.offset as usize > elf_data.len() || file_end > elf_data.len() {
                return Err(AxError::OutOfRange);
            }
        }
    }

    // The entry point must name executable code inside a loaded segment.
    let entry = elf_parser.entry();
    let entry_in_exec_segment = loads.iter().any(|ph| {
        ph.flags.is_execute()
            && {
                let start = ph.virtual_addr as usize + elf_parser.base();
                let end = start + ph.mem_size as usize;
                (start..end).contains(&entry)
            }
    });
    if !entry_in_exec_segment {
        return Err(AxError::InvalidExecutable);
    }

    // Mapping sequence: per LOAD map → populate → write, then stack, then
    // heap (the D1 eager order, kept byte-for-byte for valid images).
    for ph in &loads {
        let vaddr = ph.virtual_addr as usize + elf_parser.base();
        let seg_pad = vaddr.align_offset_4k();
        let seg_align_size =
            (ph.mem_size as usize + seg_pad + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);
        let seg_start = VirtAddr::from_usize(vaddr);
        let flags = seg_flags(ph.flags);

        backend.map_area(seg_start.align_down_4k(), seg_align_size, flags, false)?;
        backend.populate_area(seg_start.align_down_4k(), seg_align_size, flags)?;

        if ph.file_size > 0 {
            let file_start = ph.offset as usize;
            let file_end = file_start + ph.file_size as usize;
            backend.write_mem(seg_start, &elf_data[file_start..file_end])?;
        }
    }

    let auxv = elf_parser.aux_vector(PAGE_SIZE_4K, None).collect::<Vec<_>>();

    let ustack_top = VirtAddr::from_usize(params.user_stack_top);
    let ustack_start = ustack_top - params.user_stack_size;

    let rw = SegFlags {
        read: true,
        write: true,
        execute: false,
    };

    backend.map_area(ustack_start, params.user_stack_size, rw, false)?;

    let stack_data = app_stack_region(args, envs, &auxv, ustack_top.into());
    let user_sp = ustack_top - stack_data.len();
    let user_sp_aligned = user_sp.align_down_4k();
    backend.populate_area(
        user_sp_aligned,
        (ustack_top - user_sp_aligned).align_up_4k(),
        rw,
    )?;
    backend.write_mem(user_sp, stack_data.as_slice())?;

    let heap_start = VirtAddr::from_usize(params.user_heap_base);
    backend.map_area(heap_start, params.user_heap_size, rw, true)?;

    Ok(LoadedImage {
        entry: VirtAddr::from_usize(entry),
        user_sp,
    })
}
