//! K3 RAM-rootfs boot layout (MS18 Iteration 000, task 1.1).
//!
//! Pure constants and checked validators for the temporary RAM-rootfs boot
//! shape: the reserved physical carrying windows, the `initrdmem` bootargs
//! item and the DTB handoff bounds. Only `core` is used — the host harness
//! includes this file directly via `#[path]`, while the allocator
//! reservation (`mem.rs`, under the `ram-rootfs` feature) and the kernel
//! entry consume the same production functions.

/// Fixed load address U-Boot `fatload`s the raw rootfs image to.
pub const RAM_ROOTFS_BASE: usize = 0x2_4000_0000;
/// Exclusive end of the 256 MiB rootfs carrying window (D1-r1/ISS01 tier:
/// sized for the shrunken board-test image, not the raw 1 GiB stock one).
pub const RAM_ROOTFS_WINDOW_END: usize = 0x2_5000_0000;
/// FDT load address of the k3-rootfs FIT (`tools/starry-k3-rootfs.its`).
/// Above the rootfs window so the post-kernel free range stays contiguous
/// up to the rootfs base (the page allocator's main region must not be
/// fragmented — MS18 Iteration 001 root cause).
pub const RAM_DTB_BASE: usize = 0x2_ef00_0000;
/// Exclusive end of the 1 MiB DTB carrying window.
pub const RAM_DTB_WINDOW_END: usize = 0x2_ef10_0000;
/// Block alignment required by `axdriver_block::ramdisk_static::RamDisk`.
pub const RAM_ROOTFS_ALIGN: usize = 512;
/// Size of the FDT header this module validates before any parser runs.
pub const FDT_HEADER_SIZE: usize = 40;

/// Physical RAM ranges this mode reserves from the allocator in addition to
/// the kernel image: the DTB carrying window and the rootfs carrying window.
/// Consumed by `reserved_phys_ram_ranges` (only under the platform
/// `ram-rootfs` feature) and by the host difference test.
pub const RAM_ROOTFS_RESERVED: [(usize, usize); 2] = [
    (RAM_DTB_BASE, RAM_DTB_WINDOW_END - RAM_DTB_BASE),
    (RAM_ROOTFS_BASE, RAM_ROOTFS_WINDOW_END - RAM_ROOTFS_BASE),
];

/// Failure modes of the RAM-rootfs boot layout validation. Every variant is
/// a distinct, observable rejection: the entry reports it and stops before
/// constructing any memory slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamRootfsError {
    /// `/chosen bootargs` is empty.
    MissingBootargs,
    /// Bootargs present but carries no `initrdmem` item.
    MissingInitrdmem,
    /// More than one `initrdmem` item.
    DuplicateInitrdmem,
    /// The `initrdmem` payload is not two comma-separated hex numbers.
    InvalidInitrdmem,
    /// A hex number does not fit `usize`.
    NumberOverflow,
    /// Zero rootfs length.
    ZeroLength,
    /// Rootfs start is not the fixed carrying-window base.
    BadStart,
    /// Rootfs length is not 512-byte aligned.
    Misaligned,
    /// Checked rootfs end falls outside the carrying window.
    OutOfWindow,
    /// DTB pointer outside the reserved window (header not fully inside).
    DtbOutOfRange,
    /// DTB magic or totalsize invalid.
    DtbBadHeader,
}

/// Parses the single `initrdmem=<hexaddr>,<hexlen>` item out of the
/// `/chosen bootargs` bytes. Items are ASCII-whitespace separated; `0x`
/// prefixes are optional; Linux K/M/G suffixes are hex-invalid here and
/// rejected.
pub fn parse_initrdmem(bootargs: &[u8]) -> Result<(usize, usize), RamRootfsError> {
    if bootargs.is_empty() {
        return Err(RamRootfsError::MissingBootargs);
    }
    let mut found: Option<(usize, usize)> = None;
    for item in bootargs.split(u8::is_ascii_whitespace) {
        if item.is_empty() {
            continue;
        }
        let Some(rest) = item.strip_prefix(b"initrdmem=") else {
            continue;
        };
        if found.is_some() {
            return Err(RamRootfsError::DuplicateInitrdmem);
        }
        found = Some(parse_addr_len(rest)?);
    }
    found.ok_or(RamRootfsError::MissingInitrdmem)
}

/// `<hexaddr>,<hexlen>` — both parts required, no extra parts tolerated.
fn parse_addr_len(payload: &[u8]) -> Result<(usize, usize), RamRootfsError> {
    let mut parts = payload.splitn(2, |&b| b == b',');
    let addr = parts.next().ok_or(RamRootfsError::InvalidInitrdmem)?;
    let len = parts.next().ok_or(RamRootfsError::InvalidInitrdmem)?;
    Ok((parse_hex(addr)?, parse_hex(len)?))
}

fn parse_hex(digits: &[u8]) -> Result<usize, RamRootfsError> {
    let digits = digits
        .strip_prefix(b"0x")
        .or_else(|| digits.strip_prefix(b"0X"))
        .unwrap_or(digits);
    if digits.is_empty() {
        return Err(RamRootfsError::InvalidInitrdmem);
    }
    digits.iter().try_fold(0usize, |acc, &d| {
        let v = (d as char)
            .to_digit(16)
            .ok_or(RamRootfsError::InvalidInitrdmem)? as usize;
        acc.checked_mul(16)
            .and_then(|a| a.checked_add(v))
            .ok_or(RamRootfsError::NumberOverflow)
    })
}

/// Validates the bootargs-provided rootfs extent against the fixed carrying
/// window: non-zero 512-aligned length pinned to [`RAM_ROOTFS_BASE`], with
/// the end computed by checked arithmetic. Returns the checked extent.
pub fn validate_rootfs_extent(base: usize, len: usize) -> Result<(usize, usize), RamRootfsError> {
    if len == 0 {
        return Err(RamRootfsError::ZeroLength);
    }
    if base != RAM_ROOTFS_BASE {
        return Err(RamRootfsError::BadStart);
    }
    if len % RAM_ROOTFS_ALIGN != 0 {
        return Err(RamRootfsError::Misaligned);
    }
    let end = base.checked_add(len).ok_or(RamRootfsError::OutOfWindow)?;
    if end > RAM_ROOTFS_WINDOW_END {
        return Err(RamRootfsError::OutOfWindow);
    }
    Ok((base, len))
}

/// First handoff check: the DTB pointer must sit in the reserved 1 MiB
/// window with room for the complete FDT header. Callers read the 40 header
/// bytes only after this passes.
pub fn validate_dtb_pointer(ptr: usize) -> Result<(), RamRootfsError> {
    if ptr < RAM_DTB_BASE || ptr > RAM_DTB_WINDOW_END - FDT_HEADER_SIZE {
        return Err(RamRootfsError::DtbOutOfRange);
    }
    Ok(())
}

/// Second handoff check: the FDT header must carry the big-endian magic and
/// a totalsize that covers at least the header and keeps the blob inside
/// the carrying window from its own start. Returns the validated totalsize.
pub fn validate_dtb_header(ptr: usize, header: &[u8; FDT_HEADER_SIZE]) -> Result<usize, RamRootfsError> {
    if header[0..4] != [0xd0, 0x0d, 0xfe, 0xed] {
        return Err(RamRootfsError::DtbBadHeader);
    }
    let totalsize = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    if totalsize < FDT_HEADER_SIZE {
        return Err(RamRootfsError::DtbBadHeader);
    }
    match ptr.checked_add(totalsize) {
        Some(end) if end <= RAM_DTB_WINDOW_END => Ok(totalsize),
        _ => Err(RamRootfsError::DtbBadHeader),
    }
}
