//! A test-only `Hal` that returns unzeroed pages pre-filled with a
//! deterministic non-zero pattern.
//!
//! This makes the zeroing postcondition of [`crate::hal::Dma::new`] observable
//! even though a particular production HAL might choose to honour it itself:
//! unless the driver zeroes the DMA region before exposing it to a virtqueue or
//! device, the pattern survives, and stale queue state (for example a used-ring
//! index) read back from a rebuilt queue would look like a spurious completion.

#![deny(unsafe_op_in_unsafe_fn)]

use crate::{BufferDirection, Hal, PAGE_SIZE, PhysAddr};
use alloc::alloc::{alloc, dealloc, handle_alloc_error};
use core::{alloc::Layout, ptr::NonNull};

/// The byte value that [`DirtyHal`] writes across every allocated page.
const DIRTY_PATTERN: u8 = 0xA5;

#[derive(Debug)]
pub(crate) struct DirtyHal;

unsafe impl Hal for DirtyHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        assert_ne!(pages, 0);
        let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE).unwrap();
        // Safe because the size and alignment of the layout are non-zero.
        let ptr = unsafe { alloc(layout) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| handle_alloc_error(layout));
        // `alloc` does not initialise the memory; fill the whole region with a
        // deterministic non-zero pattern so a zeroing bug in `Dma::new` is
        // clearly visible.
        // Safe because the whole allocation is valid and exclusive for now.
        unsafe {
            ptr.as_ptr().write_bytes(DIRTY_PATTERN, pages * PAGE_SIZE);
        }
        (ptr.as_ptr() as PhysAddr, ptr)
    }

    unsafe fn dma_dealloc(_paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        assert_ne!(pages, 0);
        let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE).unwrap();
        // Safe because the layout is the same as was used when the memory was allocated by
        // `dma_alloc` above.
        unsafe {
            dealloc(vaddr.as_ptr(), layout);
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as _).unwrap()
    }

    unsafe fn share(_buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        unimplemented!("DirtyHal is only exercised through Dma::new")
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        unimplemented!("DirtyHal is only exercised through Dma::new")
    }
}
