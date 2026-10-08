//! MS18 Iteration 000 (tasks 1.1/1.2) host contract tests for the K3
//! RAM-rootfs boot layout.
//!
//! Drive the production pure module `axplat-riscv64-k3/src/ram_layout.rs`
//! (included via `#[path]`, same pattern as the embedded loader): the
//! `initrdmem` bootargs item, the rootfs extent checks, the DTB handoff
//! bounds, and the allocator reservation difference. RED = missing
//! production module or failing assertions.

#[path = "../../../crates/axplat-riscv64-k3/src/ram_layout.rs"]
pub mod ram_layout;

use ram_layout::{
    RAM_DTB_BASE, RAM_DTB_WINDOW_END, RAM_ROOTFS_ALIGN, RAM_ROOTFS_BASE,
    RAM_ROOTFS_RESERVED, RAM_ROOTFS_WINDOW_END, RamRootfsError,
};

/// K3 board fact (axconfig): phys_ram_ranges spans kernel base ..= safe end.
const PHYS_RAM_START: usize = 0x1_8000_0000;
const PHYS_RAM_END: usize = 0x2_f000_0000;
/// Kernel image stand-in for the difference test (.bin is ~934 KiB at MS17).
const KERNEL_IMAGE: (usize, usize) = (PHYS_RAM_START, 0xE_4000);

// ── rootfs extent validation ─────────────────────────────────────────

#[test]
fn full_window_extent_is_accepted() {
    let len = RAM_ROOTFS_WINDOW_END - RAM_ROOTFS_BASE;
    assert_eq!(
        len, 0x1000_0000,
        "board plan: 256 MiB carrying window (D1-r1/ISS01)"
    );
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE, len),
        Ok((RAM_ROOTFS_BASE, len))
    );
}

#[test]
fn zero_length_extent_is_rejected() {
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE, 0),
        Err(RamRootfsError::ZeroLength)
    );
}

#[test]
fn wrong_start_extent_is_rejected() {
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE + RAM_ROOTFS_ALIGN, 0x1000),
        Err(RamRootfsError::BadStart)
    );
}

#[test]
fn misaligned_length_extent_is_rejected() {
    assert_eq!(RAM_ROOTFS_ALIGN, 512);
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE, 0x1001),
        Err(RamRootfsError::Misaligned)
    );
}

#[test]
fn extent_beyond_window_is_rejected() {
    let len = (RAM_ROOTFS_WINDOW_END - RAM_ROOTFS_BASE) + RAM_ROOTFS_ALIGN;
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE, len),
        Err(RamRootfsError::OutOfWindow)
    );
}

#[test]
fn overflowing_extent_end_is_rejected_without_panic() {
    // checked end arithmetic: base + (usize::MAX rounded down to the block
    // alignment) must be an Err, not a panic.
    let len = usize::MAX - (usize::MAX % RAM_ROOTFS_ALIGN);
    assert_eq!(len % RAM_ROOTFS_ALIGN, 0);
    assert_eq!(
        ram_layout::validate_rootfs_extent(RAM_ROOTFS_BASE, len),
        Err(RamRootfsError::OutOfWindow)
    );
}

// ── initrdmem bootargs item ──────────────────────────────────────────

#[test]
fn bootargs_item_is_parsed_from_surrounding_arguments() {
    let bootargs =
        b"console=ttyS0 earlycon initrdmem=0x240000000,0x40000000 quiet";
    assert_eq!(
        ram_layout::parse_initrdmem(bootargs),
        Ok((0x2_4000_0000, 0x4000_0000))
    );
}

#[test]
fn bootargs_item_without_0x_prefix_is_parsed() {
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem=240000000,40000000"),
        Ok((0x2_4000_0000, 0x4000_0000))
    );
}

#[test]
fn bootargs_missing_item_is_rejected() {
    assert_eq!(
        ram_layout::parse_initrdmem(b"console=ttyS0 quiet"),
        Err(RamRootfsError::MissingInitrdmem)
    );
    assert_eq!(
        ram_layout::parse_initrdmem(b""),
        Err(RamRootfsError::MissingBootargs)
    );
}

#[test]
fn bootargs_duplicate_item_is_rejected() {
    let bootargs =
        b"initrdmem=0x240000000,0x1000 initrdmem=0x240000000,0x2000";
    assert_eq!(
        ram_layout::parse_initrdmem(bootargs),
        Err(RamRootfsError::DuplicateInitrdmem)
    );
}

#[test]
fn bootargs_illegal_digit_is_rejected() {
    // Linux K/M/G suffixes are not supported by this mode (design D1).
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem=0x240000000,0x40000K"),
        Err(RamRootfsError::InvalidInitrdmem)
    );
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem=zzz,0x1000"),
        Err(RamRootfsError::InvalidInitrdmem)
    );
}

#[test]
fn bootargs_garbled_payload_is_rejected() {
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem="),
        Err(RamRootfsError::InvalidInitrdmem)
    );
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem=0x240000000"),
        Err(RamRootfsError::InvalidInitrdmem)
    );
    assert_eq!(
        ram_layout::parse_initrdmem(b"initrdmem=0x240000000,"),
        Err(RamRootfsError::InvalidInitrdmem)
    );
}

#[test]
fn bootargs_number_overflow_is_rejected() {
    // 16 F's × 17 digits exceeds usize::MAX without wrapping.
    let big = "initrdmem=0x".to_string() + &"f".repeat(17) + ",0x1000";
    assert_eq!(
        ram_layout::parse_initrdmem(big.as_bytes()),
        Err(RamRootfsError::NumberOverflow)
    );
}

// ── DTB handoff bounds ───────────────────────────────────────────────

fn stock_header(totalsize: u32) -> [u8; 40] {
    let mut header = [0u8; 40];
    header[0..4].copy_from_slice(&[0xd0, 0x0d, 0xfe, 0xed]); // FDT magic, BE
    header[4..8].copy_from_slice(&totalsize.to_be_bytes());
    header
}

#[test]
fn valid_dtb_handoff_is_accepted() {
    // Board fact: the stock DTB is 150998 bytes; its load address is the
    // carrying-window base (D1-r1: 0x2ef000000, in-place U-Boot rewrite).
    let header = stock_header(150_998);
    assert_eq!(
        ram_layout::validate_dtb_pointer(RAM_DTB_BASE),
        Ok(())
    );
    assert_eq!(
        ram_layout::validate_dtb_header(RAM_DTB_BASE, &header),
        Ok(150_998)
    );
}

#[test]
fn dtb_pointer_outside_window_is_rejected() {
    assert_eq!(
        ram_layout::validate_dtb_pointer(RAM_DTB_BASE - 8),
        Err(RamRootfsError::DtbOutOfRange)
    );
    assert_eq!(
        ram_layout::validate_dtb_pointer(RAM_DTB_WINDOW_END),
        Err(RamRootfsError::DtbOutOfRange)
    );
    // A pointer whose 40-byte header would cross the window end.
    assert_eq!(
        ram_layout::validate_dtb_pointer(RAM_DTB_WINDOW_END - 8),
        Err(RamRootfsError::DtbOutOfRange)
    );
}

#[test]
fn dtb_header_bad_magic_is_rejected() {
    let mut header = stock_header(4096);
    header[0] = 0;
    assert_eq!(
        ram_layout::validate_dtb_header(RAM_DTB_BASE, &header),
        Err(RamRootfsError::DtbBadHeader)
    );
}

#[test]
fn dtb_totalsize_out_of_bounds_is_rejected() {
    // Below the minimal 40-byte FDT header.
    assert_eq!(
        ram_layout::validate_dtb_header(RAM_DTB_BASE, &stock_header(39)),
        Err(RamRootfsError::DtbBadHeader)
    );
    // Beyond the 1 MiB carrying window.
    let window = RAM_DTB_WINDOW_END - RAM_DTB_BASE;
    assert_eq!(
        ram_layout::validate_dtb_header(RAM_DTB_BASE, &stock_header(window as u32 + 1)),
        Err(RamRootfsError::DtbBadHeader)
    );
    // Blob would cross the window end from its own start.
    assert_eq!(
        ram_layout::validate_dtb_header(RAM_DTB_WINDOW_END - 0x1_0000, &stock_header(0x1_0001)),
        Err(RamRootfsError::DtbBadHeader)
    );
}

// ── D1-r1 layout invariants (Iteration 001 replan, task 2.3) ─────────

/// axruntime `init_allocator` picks the FIRST free range after the kernel
/// image — not the largest — as the page allocator's only main region; the
/// rest of RAM only reaches the slab byte allocator. Any reserved window
/// between the kernel image and the rootfs window fragments that main
/// region and starves demand paging (MS18 Iteration 001 board OOM root
/// cause). The first free range must therefore run contiguously up to the
/// rootfs carrying-window base.
#[test]
fn first_post_kernel_free_range_is_contiguous_to_rootfs_base() {
    let mut reserved = RAM_ROOTFS_RESERVED.to_vec();
    reserved.push(KERNEL_IMAGE);
    let free = free_ranges(&[(PHYS_RAM_START, PHYS_RAM_END - PHYS_RAM_START)], &reserved);
    let &(first_start, first_size) = free.first().expect("kernel must still have free RAM");
    assert_eq!(
        first_start + first_size,
        RAM_ROOTFS_BASE,
        "first free range after the kernel image must end at the rootfs window base"
    );
}

/// The rootfs carrying window is exactly 256 MiB (D1-r1 / ISS01 tier).
#[test]
fn rootfs_window_is_exactly_256mib() {
    assert_eq!(RAM_ROOTFS_WINDOW_END - RAM_ROOTFS_BASE, 0x1000_0000);
}

/// The DTB carrying window stays 1 MiB and sits at or above the rootfs
/// window (top of managed RAM), never between kernel and rootfs.
#[test]
fn dtb_window_stays_1mib_at_or_above_rootfs_window() {
    assert_eq!(RAM_DTB_WINDOW_END - RAM_DTB_BASE, 0x10_0000);
    assert!(
        RAM_DTB_BASE >= RAM_ROOTFS_WINDOW_END,
        "DTB window {RAM_DTB_BASE:#x} must not sit below the rootfs window end"
    );
}

// ── allocator reservation difference (axhal mem.rs semantics) ────────

/// Native difference set mirroring axhal's free-memory computation: phys
/// ranges minus the sorted reserved list. Test fixture, not production.
fn free_ranges(phys: &[(usize, usize)], reserved: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut reserved = reserved.to_vec();
    reserved.sort_unstable_by_key(|&(start, _)| start);
    let mut free = Vec::new();
    for &(pstart, psize) in phys {
        let mut cur = pstart;
        let end = pstart + psize;
        for &(rstart, rsize) in &reserved {
            let rend = rstart + rsize;
            if rend <= cur || rstart >= end {
                continue;
            }
            if rstart > cur {
                free.push((cur, rstart - cur));
            }
            cur = cur.max(rend);
            if cur >= end {
                break;
            }
        }
        if cur < end {
            free.push((cur, end - cur));
        }
    }
    free
}

#[test]
fn allocator_free_excludes_rootfs_and_dtb_windows() {
    let mut reserved = RAM_ROOTFS_RESERVED.to_vec();
    reserved.push(KERNEL_IMAGE);
    let free = free_ranges(&[(PHYS_RAM_START, PHYS_RAM_END - PHYS_RAM_START)], &reserved);

    assert!(!free.is_empty(), "kernel must still have free RAM");
    let overlaps = |(s, e): (usize, usize), ws: usize, we: usize| s < we && ws < e;
    for &(start, size) in &free {
        let end = start + size;
        assert!(
            !overlaps((start, end), RAM_ROOTFS_BASE, RAM_ROOTFS_WINDOW_END),
            "free range {start:#x}..{end:#x} intersects the rootfs window"
        );
        assert!(
            !overlaps((start, end), RAM_DTB_BASE, RAM_DTB_WINDOW_END),
            "free range {start:#x}..{end:#x} intersects the DTB window"
        );
    }
    // Both carrying windows live inside the managed phys RAM.
    assert!(RAM_ROOTFS_BASE >= PHYS_RAM_START && RAM_ROOTFS_WINDOW_END <= PHYS_RAM_END);
    assert!(RAM_DTB_BASE >= PHYS_RAM_START && RAM_DTB_WINDOW_END <= PHYS_RAM_END);
    // ...and stay clear of the kernel image (overlap would panic on boot).
    assert!(RAM_DTB_BASE >= KERNEL_IMAGE.0 + KERNEL_IMAGE.1);
}
