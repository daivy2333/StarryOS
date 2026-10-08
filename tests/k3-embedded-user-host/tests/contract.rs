//! MS17 Iteration 002 (T6) host contract tests.
//!
//! Drive the production embedded loader and stdio publication with fakes
//! replacing only the backend (map/populate/write) and the open boundary.
//! RED = missing production module or failing assertions.

use k3_embedded_user_host::embedded_loader::{ElfLoadParams, SegFlags, load_elf_image};
use k3_embedded_user_host::fake::{FakeBackend, FakeFile, FakeOpener, FakeTable, Op};
use k3_embedded_user_host::stdio::{StdioTable, publish_stdio};
use memory_addr::VirtAddr;

/// Platform constants standing in for the kernel's config values; the
/// production loader receives them as parameters.
fn params() -> ElfLoadParams {
    ElfLoadParams {
        user_space_base: 0,
        user_stack_top: 0x0000_0000_8000_0000,
        user_stack_size: 0x4_0000,
        user_heap_base: 0x0000_0001_0000_0000,
        user_heap_size: 0x4_0000,
    }
}

const RX_VADDR: u64 = 0x1000;
const RW_VADDR: u64 = 0x2000;

/// One program-header spec for [`elf64`].
struct Phdr {
    r#type: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
}

fn load(flags: u32, offset: u64, vaddr: u64, filesz: u64, memsz: u64) -> Phdr {
    Phdr {
        r#type: 1, // PT_LOAD
        flags,
        offset,
        vaddr,
        filesz,
        memsz,
    }
}

/// Hand-assemble a minimal little-endian ELF64 image: ELF header (64 B),
/// `phdrs` (56 B each, right after the header), then their bytes laid out
/// at the offsets the caller chose.
fn elf64(etype: u16, entry: u64, phdrs: &[Phdr], segment_bytes: &[u8]) -> Vec<u8> {
    let phoff = 64u64;
    let mut out = Vec::new();
    let mut ident = [0u8; 16];
    ident[..4].copy_from_slice(b"\x7fELF");
    ident[4] = 2; // ELFCLASS64
    ident[5] = 1; // ELFDATA2LSB
    ident[6] = 1; // EV_CURRENT
    out.extend_from_slice(&ident);
    out.extend_from_slice(&etype.to_le_bytes());
    out.extend_from_slice(&243u16.to_le_bytes()); // EM_RISCV
    out.extend_from_slice(&1u32.to_le_bytes()); // e_version
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&phoff.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    out.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    out.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    out.extend_from_slice(&(phdrs.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    out.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

    for ph in phdrs {
        out.extend_from_slice(&ph.r#type.to_le_bytes());
        out.extend_from_slice(&ph.flags.to_le_bytes());
        out.extend_from_slice(&ph.offset.to_le_bytes());
        out.extend_from_slice(&ph.vaddr.to_le_bytes());
        out.extend_from_slice(&ph.vaddr.to_le_bytes()); // p_paddr
        out.extend_from_slice(&ph.filesz.to_le_bytes());
        out.extend_from_slice(&ph.memsz.to_le_bytes());
        out.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
    }

    assert!(out.len() as u64 <= segment_bytes_offset(phdrs));
    out.resize(segment_bytes_offset(phdrs) as usize, 0);
    out.extend_from_slice(segment_bytes);
    out
}

fn segment_bytes_offset(phdrs: &[Phdr]) -> u64 {
    // Segment bytes start at the aligned end of the phdr table.
    64 + 56 * phdrs.len() as u64
}

/// The valid reference image: an executable segment at 0x1000 whose entry
/// is its start, plus a writable segment at 0x2000 with BSS
/// (filesz < memsz). Segment bytes are laid out after the phdr table.
fn valid_image() -> Vec<u8> {
    let code = b"\x13\x00\x00\x00"; // 4 code bytes
    let data = b"AB"; // 2 data bytes; BSS = 6 more bytes
    let code_off = segment_bytes_offset(&[]);
    let data_off = code_off + code.len() as u64;
    let phdrs = [
        load(0x5, code_off, RX_VADDR, code.len() as u64, code.len() as u64), // R|X
        load(0x6, data_off, RW_VADDR, data.len() as u64, data.len() as u64 + 6), // RW+BSS
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(code);
    bytes.extend_from_slice(data);
    elf64(2, RX_VADDR, &phdrs, &bytes) // ET_EXEC
}

const STACK_TOP: u64 = 0x0000_0000_8000_0000;

#[test]
fn valid_static_exec_maps_segments_stack_and_heap_in_d1_order() {
    let image = valid_image();
    let mut backend = FakeBackend::new();
    let args = [std::string::String::from("probe")];

    let loaded = load_elf_image(&mut backend, &image, &params(), &args, &[]).expect("must load");
    assert_eq!(loaded.entry, VirtAddr::from(RX_VADDR as usize));
    // The returned SP lands inside the mapped stack area.
    assert!(loaded.user_sp.as_usize() <= STACK_TOP as usize);
    assert!(loaded.user_sp.as_usize() > STACK_TOP as usize - 0x4_0000);

    let rx_flags = SegFlags {
        read: true,
        write: false,
        execute: true,
    };
    let rw_flags = SegFlags {
        read: true,
        write: true,
        execute: false,
    };
    // The D1 eager sequence, per LOAD then stack then heap; BSS bytes are
    // never written from the file (populate covers the whole region).
    assert_eq!(
        &backend.ops[..6],
        &[
            Op::Map(RX_VADDR, 0x1000, rx_flags, false),
            Op::Populate(RX_VADDR, 0x1000, rx_flags),
            Op::Write(RX_VADDR, 4),
            Op::Map(RW_VADDR, 0x1000, rw_flags, false),
            Op::Populate(RW_VADDR, 0x1000, rw_flags),
            Op::Write(RW_VADDR, 2),
        ]
    );
    // Stack: map the whole area without populate, then populate the used
    // tail and write the stack image; heap: one map-with-populate.
    assert_eq!(backend.ops.len(), 10);
    assert!(
        matches!(backend.ops[6], Op::Map(v, 0x4_0000, _, false) if v == STACK_TOP - 0x4_0000)
    );
    assert!(matches!(backend.ops[7], Op::Populate(_, _, _)));
    assert!(matches!(backend.ops[8], Op::Write(_, n) if n > 0));
    assert!(matches!(
        backend.ops[9],
        Op::Map(v, 0x4_0000, _, true) if v == params().user_heap_base as u64
    ));
}

#[test]
fn truncated_segment_bytes_are_rejected_not_truncated() {
    // The RX header claims 64 file bytes but the image ends earlier.
    let code = [0u8; 4];
    let phdrs = [load(0x5, 120, RX_VADDR, 64, 64)];
    let mut image = elf64(2, RX_VADDR, &phdrs, &code);
    image.truncate(120 + 8); // cut the segment short

    let mut backend = FakeBackend::new();
    let err = load_elf_image(&mut backend, &image, &params(), &[], &[])
        .expect_err("truncated segment must be rejected");
    assert_eq!(err, axerrno::AxError::OutOfRange);
    assert!(
        backend.ops.is_empty(),
        "no mapping may start for an out-of-bounds image"
    );
}

#[test]
fn phdr_table_beyond_the_image_is_rejected() {
    // e_phnum claims 2 headers but the image ends inside the first one.
    let code = [0u8; 4];
    let phdrs = [load(0x5, 120, RX_VADDR, 4, 4)];
    let mut image = elf64(2, RX_VADDR, &phdrs, &code);
    image.truncate(100);

    let mut backend = FakeBackend::new();
    let err = load_elf_image(&mut backend, &image, &params(), &[], &[])
        .expect_err("phdr table beyond the image must be rejected");
    assert_eq!(err, axerrno::AxError::OutOfRange);
    assert!(backend.ops.is_empty());
}

#[test]
fn dynamic_pie_payload_is_rejected() {
    let image = {
        let mut img = valid_image();
        // e_type ET_DYN (3) instead of ET_EXEC (2).
        img[16..18].copy_from_slice(&3u16.to_le_bytes());
        img
    };
    let mut backend = FakeBackend::new();
    let err = load_elf_image(&mut backend, &image, &params(), &[], &[])
        .expect_err("dynamic payload must be rejected");
    assert_eq!(err, axerrno::AxError::InvalidExecutable);
    assert!(backend.ops.is_empty());
}

#[test]
fn entry_outside_executable_segments_is_rejected() {
    let image = {
        let mut img = valid_image();
        // Entry at 0x9000 — inside no executable LOAD.
        let entry_pos = 24; // e_entry offset within the ELF header
        img[entry_pos..entry_pos + 8].copy_from_slice(&0x9000u64.to_le_bytes());
        img
    };
    let mut backend = FakeBackend::new();
    let err = load_elf_image(&mut backend, &image, &params(), &[], &[])
        .expect_err("bad entry must be rejected");
    assert_eq!(err, axerrno::AxError::InvalidExecutable);
    assert!(backend.ops.is_empty());
}

#[test]
fn mapping_failure_propagates_and_stops_the_sequence() {
    let image = valid_image();
    // Fail the second backend call (the RX populate) with a kernel error.
    let mut backend = FakeBackend::failing(2, axerrno::AxError::NoMemory);
    let err = load_elf_image(&mut backend, &image, &params(), &[], &[])
        .expect_err("backend failure must propagate");
    assert_eq!(err, axerrno::AxError::NoMemory);
    // The failed call is not recorded and nothing runs after it.
    assert_eq!(backend.ops.len(), 1);
    assert!(matches!(backend.ops[0], Op::Map(RX_VADDR, 0x1000, _, false)));
}

#[test]
fn stdio_publishes_input_then_output_twice_in_order() {
    let mut table = FakeTable::new(8);
    let mut opener = FakeOpener::new();

    publish_stdio(&mut table, &mut |read| opener.open(read)).expect("publish must succeed");

    assert_eq!(opener.opened, vec![true, false]);
    assert_eq!(
        table.entries,
        vec![FakeFile(1), FakeFile(2), FakeFile(2)],
        "stdin first, then stdout and stderr sharing one endpoint"
    );
}

#[test]
fn stdio_open_failure_publishes_nothing() {
    // First open fails: nothing added.
    let mut table = FakeTable::new(8);
    let mut opener = FakeOpener::failing(1);
    assert!(publish_stdio(&mut table, &mut |read| opener.open(read)).is_err());
    assert!(table.is_empty());

    // Second open fails: the already-opened input is dropped, nothing added.
    let mut table = FakeTable::new(8);
    let mut opener = FakeOpener::failing(2);
    assert!(publish_stdio(&mut table, &mut |read| opener.open(read)).is_err());
    assert!(table.is_empty());
}

#[test]
fn stdio_add_failure_propagates() {
    let mut table = FakeTable::new(1);
    let mut opener = FakeOpener::new();
    let err = publish_stdio(&mut table, &mut |read| opener.open(read))
        .expect_err("a full table must fail the publication");
    assert_eq!(err, axerrno::AxError::TooManyOpenFiles);
    assert_eq!(table.entries.len(), 1);
}
