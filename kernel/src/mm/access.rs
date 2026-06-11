use alloc::string::String;
use core::{
    alloc::Layout,
    ffi::c_char,
    hint::unlikely,
    mem::{MaybeUninit, transmute},
    ptr, slice, str,
};

use axerrno::{AxError, AxResult};
use axhal::{
    asm::user_copy,
    paging::MappingFlags,
    trap::{PAGE_FAULT, register_trap_handler},
};
use axio::prelude::*;
use axtask::current;
use extern_trait::extern_trait;
use kernel_guard::IrqSave;
use memory_addr::{MemoryAddr, PAGE_SIZE_4K, VirtAddr};
use starry_vm::{VmError, VmIo, VmResult, vm_load_until_nul, vm_read_slice, vm_write_slice};

use crate::{
    config::{USER_SPACE_BASE, USER_SPACE_SIZE},
    mm::AddrSpace,
    task::AsThread,
};

/// Enables scoped access into user memory, allowing page faults to occur inside
/// kernel.
pub fn access_user_memory<R>(f: impl FnOnce() -> R) -> R {
    let curr = current();
    let Some(thr) = curr.try_as_thread() else {
        panic!("access_user_memory called outside of thread context");
    };

    thr.set_accessing_user_memory(true);
    let result = f();
    thr.set_accessing_user_memory(false);
    result
}

fn check_region(start: VirtAddr, layout: Layout, access_flags: MappingFlags) -> AxResult<()> {
    let align = layout.align();
    if start.as_usize() & (align - 1) != 0 {
        return Err(AxError::BadAddress);
    }

    let curr = current();
    let mut aspace = curr.as_thread().proc_data.aspace.lock();

    if !aspace.can_access_range(start, layout.size(), access_flags) {
        return Err(AxError::BadAddress);
    }

    let page_start = start.align_down_4k();
    let page_end = (start + layout.size()).align_up_4k();
    aspace.populate_area(page_start, page_end - page_start, access_flags)?;

    Ok(())
}

/// Find the maximum contiguous valid size (bytes, 4K-aligned) from `start`
/// that satisfies `access_flags` in the given address space.
///
/// Uses binary search over `can_access_range`, which walks memory areas
/// (O(areas)) rather than individual pages (O(pages)), making boundary
/// discovery logarithmic in the address range.
///
/// Returns 0 if even the first page is not accessible with the requested flags.
fn max_contiguous_valid(
    aspace: &AddrSpace,
    start: VirtAddr,
    max_size: usize,
    access_flags: MappingFlags,
) -> usize {
    if max_size == 0 {
        return 0;
    }
    // Fast path: the entire remaining range is valid.
    if aspace.can_access_range(start, max_size, access_flags) {
        return max_size;
    }
    // At minimum the first page must be accessible.
    let min_check = PAGE_SIZE_4K.min(max_size);
    if !aspace.can_access_range(start, min_check, access_flags) {
        return 0;
    }
    // Binary search for the maximum valid contiguous size.
    let mut lo = min_check;
    let mut hi = max_size;
    while lo + PAGE_SIZE_4K <= hi {
        let mid = ((lo + hi) / 2).align_down_4k();
        if mid >= min_check && aspace.can_access_range(start, mid, access_flags) {
            lo = mid;
        } else {
            hi = mid.saturating_sub(PAGE_SIZE_4K);
        }
    }
    lo
}

fn check_null_terminated<T: PartialEq + Default>(
    start: VirtAddr,
    access_flags: MappingFlags,
) -> AxResult<usize> {
    let align = Layout::new::<T>().align();
    if start.as_usize() & (align - 1) != 0 {
        return Err(AxError::BadAddress);
    }

    let zero = T::default();
    let start_ptr = start.as_ptr_of::<T>();
    let mut len = 0;
    // The highest page-aligned address we have validated up to (exclusive).
    // We batch-validate as many contiguous pages as possible in one lock
    // acquisition, avoiding the per-4KB lock cycling of the original approach.
    let mut validated_boundary = start.align_down_4k();

    access_user_memory(|| {
        loop {
            // SAFETY: This won't overflow the address space — we validate page
            // boundaries below before crossing them.
            let ptr = unsafe { start_ptr.add(len) };

            // We cannot hold the aspace lock across page faults (the fault
            // handler needs the same lock), so we pre-validate a batch of
            // contiguous pages, release the lock, then rely on the page fault
            // handler for any unpopulated pages within the validated range.
            while (ptr as usize) >= validated_boundary.as_usize() {
                let curr = current();
                let aspace = curr.as_thread().proc_data.aspace.lock();
                let max_remaining = aspace
                    .end()
                    .as_usize()
                    .saturating_sub(validated_boundary.as_usize());

                let valid_size = max_contiguous_valid(
                    &aspace,
                    validated_boundary,
                    max_remaining,
                    access_flags,
                );
                if valid_size == 0 {
                    return Err(AxError::BadAddress);
                }
                validated_boundary += valid_size;
            }

            // This might trigger a page fault (unpopulated pages within a
            // valid mapping). The page fault handler populates the page and
            // returns so we can safely retry the read.
            // SAFETY: The pointer lies within memory areas validated above.
            if unsafe { ptr.read_volatile() } == zero {
                break;
            }
            len += 1;
        }
        Ok(())
    })?;

    Ok(len)
}

/// A pointer to user space memory.
#[repr(transparent)]
#[derive(PartialEq, Clone, Copy)]
pub struct UserPtr<T>(*mut T);

impl<T> From<usize> for UserPtr<T> {
    fn from(value: usize) -> Self {
        UserPtr(value as *mut _)
    }
}

impl<T> From<*mut T> for UserPtr<T> {
    fn from(value: *mut T) -> Self {
        UserPtr(value)
    }
}

impl<T> Default for UserPtr<T> {
    fn default() -> Self {
        Self(ptr::null_mut())
    }
}

impl<T> UserPtr<T> {
    const ACCESS_FLAGS: MappingFlags = MappingFlags::READ.union(MappingFlags::WRITE);

    pub fn address(&self) -> VirtAddr {
        VirtAddr::from_ptr_of(self.0)
    }

    pub fn cast<U>(self) -> UserPtr<U> {
        UserPtr(self.0 as *mut U)
    }

    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    pub fn get_as_mut(self) -> AxResult<&'static mut T> {
        check_region(self.address(), Layout::new::<T>(), Self::ACCESS_FLAGS)?;
        Ok(unsafe { &mut *self.0 })
    }

    pub fn get_as_mut_slice(self, len: usize) -> AxResult<&'static mut [T]> {
        check_region(
            self.address(),
            Layout::array::<T>(len).unwrap(),
            Self::ACCESS_FLAGS,
        )?;
        Ok(unsafe { slice::from_raw_parts_mut(self.0, len) })
    }
}

/// An immutable pointer to user space memory.
#[repr(transparent)]
#[derive(PartialEq, Clone, Copy)]
pub struct UserConstPtr<T>(*const T);

impl<T> From<usize> for UserConstPtr<T> {
    fn from(value: usize) -> Self {
        UserConstPtr(value as *const _)
    }
}

impl<T> From<*const T> for UserConstPtr<T> {
    fn from(value: *const T) -> Self {
        UserConstPtr(value)
    }
}

impl<T> Default for UserConstPtr<T> {
    fn default() -> Self {
        Self(ptr::null())
    }
}

impl<T> UserConstPtr<T> {
    const ACCESS_FLAGS: MappingFlags = MappingFlags::READ;

    pub fn address(&self) -> VirtAddr {
        VirtAddr::from_ptr_of(self.0)
    }

    pub fn cast<U>(self) -> UserConstPtr<U> {
        UserConstPtr(self.0 as *const U)
    }

    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    pub fn get_as_ref(self) -> AxResult<&'static T> {
        check_region(self.address(), Layout::new::<T>(), Self::ACCESS_FLAGS)?;
        Ok(unsafe { &*self.0 })
    }

    pub fn get_as_slice(self, len: usize) -> AxResult<&'static [T]> {
        check_region(
            self.address(),
            Layout::array::<T>(len).unwrap(),
            Self::ACCESS_FLAGS,
        )?;
        Ok(unsafe { slice::from_raw_parts(self.0, len) })
    }

    pub fn get_as_null_terminated(self) -> AxResult<&'static [T]>
    where
        T: PartialEq + Default,
    {
        let len = check_null_terminated::<T>(self.address(), Self::ACCESS_FLAGS)?;
        Ok(unsafe { slice::from_raw_parts(self.0, len) })
    }
}

impl UserConstPtr<c_char> {
    /// Get the pointer as `&str`, validating the memory region.
    pub fn get_as_str(self) -> AxResult<&'static str> {
        let slice = self.get_as_null_terminated()?;
        // SAFETY: c_char is u8
        let slice = unsafe { transmute::<&[c_char], &[u8]>(slice) };

        str::from_utf8(slice).map_err(|_| AxError::IllegalBytes)
    }
}

macro_rules! nullable {
    ($ptr:ident.$func:ident($($arg:expr),*)) => {
        if $ptr.is_null() {
            Ok(None)
        } else {
            Some($ptr.$func($($arg),*)).transpose()
        }
    };
}

pub(crate) use nullable;

#[register_trap_handler(PAGE_FAULT)]
fn handle_page_fault(vaddr: VirtAddr, access_flags: MappingFlags) -> bool {
    debug!("Page fault at {vaddr:#x}, access_flags: {access_flags:#x?}");

    let curr = current();
    let Some(thr) = curr.try_as_thread() else {
        return false;
    };

    if unlikely(!thr.is_accessing_user_memory()) {
        return false;
    }

    thr.proc_data
        .aspace
        .lock()
        .handle_page_fault(vaddr, access_flags)
}

pub fn vm_load_string(ptr: *const c_char) -> AxResult<String> {
    #[allow(clippy::unnecessary_cast)]
    let bytes = vm_load_until_nul(ptr as *const u8)?;
    String::from_utf8(bytes).map_err(|_| AxError::IllegalBytes)
}

#[allow(dead_code)]
struct Vm(IrqSave);

/// Briefly checks if the given memory region is valid user memory.
pub fn check_access(start: usize, len: usize) -> VmResult {
    const USER_SPACE_END: usize = USER_SPACE_BASE + USER_SPACE_SIZE;
    let ok = (USER_SPACE_BASE..USER_SPACE_END).contains(&start) && (USER_SPACE_END - start) >= len;
    if unlikely(!ok) {
        Err(VmError::AccessDenied)
    } else {
        Ok(())
    }
}

#[extern_trait]
unsafe impl VmIo for Vm {
    fn new() -> Self {
        Self(IrqSave::new())
    }

    fn read(&mut self, start: usize, buf: &mut [MaybeUninit<u8>]) -> VmResult {
        check_access(start, buf.len())?;
        let failed_at = access_user_memory(|| unsafe {
            user_copy(buf.as_mut_ptr() as *mut _, start as _, buf.len())
        });
        if unlikely(failed_at != 0) {
            Err(VmError::AccessDenied)
        } else {
            Ok(())
        }
    }

    fn write(&mut self, start: usize, buf: &[u8]) -> VmResult {
        check_access(start, buf.len())?;
        let failed_at = access_user_memory(|| unsafe {
            user_copy(start as _, buf.as_ptr() as *const _, buf.len())
        });
        if unlikely(failed_at != 0) {
            Err(VmError::AccessDenied)
        } else {
            Ok(())
        }
    }
}

/// A read-only buffer in the VM's memory.
///
/// It implements the `axio::Read` trait, allowing it to be used with other I/O
/// operations.
pub struct VmBytes {
    /// The pointer to the start of the buffer in the VM's memory.
    pub ptr: *const u8,
    /// The length of the buffer.
    pub len: usize,
}

impl VmBytes {
    /// Creates a new `VmBytes` from a raw pointer and a length.
    pub fn new(ptr: *const u8, len: usize) -> Self {
        Self { ptr, len }
    }
}

impl Read for VmBytes {
    /// Reads bytes from the VM's memory into the provided buffer.
    fn read(&mut self, buf: &mut [u8]) -> axio::Result<usize> {
        let len = self.len.min(buf.len());
        vm_read_slice(self.ptr, unsafe {
            transmute::<&mut [u8], &mut [MaybeUninit<u8>]>(&mut buf[..len])
        })?;
        self.ptr = self.ptr.wrapping_add(len);
        self.len -= len;
        Ok(len)
    }
}

impl IoBuf for VmBytes {
    fn remaining(&self) -> usize {
        self.len
    }
}

/// A mutable buffer in the VM's memory.
///
/// It implements the `axio::Write` trait, allowing it to be used with other I/O
/// operations.
pub struct VmBytesMut {
    /// The pointer to the start of the buffer in the VM's memory.
    pub ptr: *mut u8,
    /// The length of the buffer.
    pub len: usize,
}

impl VmBytesMut {
    /// Creates a new `VmBytesMut` from a raw pointer and a length.
    pub fn new(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }
}

impl Write for VmBytesMut {
    /// Writes bytes from the provided buffer into the VM's memory.
    fn write(&mut self, buf: &[u8]) -> axio::Result<usize> {
        let len = self.len.min(buf.len());
        vm_write_slice(self.ptr, &buf[..len])?;
        self.ptr = self.ptr.wrapping_add(len);
        self.len -= len;
        Ok(len)
    }

    /// Flushes the buffer. This is a no-op for `VmBytesMut`.
    fn flush(&mut self) -> axio::Result {
        Ok(())
    }
}

impl IoBufMut for VmBytesMut {
    fn remaining_mut(&self) -> usize {
        self.len
    }
}
