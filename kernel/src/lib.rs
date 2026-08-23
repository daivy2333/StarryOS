//! The core functionality of a monolithic kernel, including loading user
//! programs and managing processes.

#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![cfg_attr(
    feature = "lichee-d1",
    allow(dead_code, unused_imports, unused_variables)
)]

extern crate alloc;
extern crate axruntime;

#[macro_use]
extern crate axlog;

// ── Feature mode mutual exclusion guards ───────────────────────────────
// Each Lichee fullbench mode (path, command) is a single-mode
// build target. Combining incompatible modes would produce unreachable code
// paths or module exclusion failures.
#[cfg(all(
    feature = "lichee-d1-fullbench",
    feature = "lichee-d1-fullbench-command"
))]
compile_error!(
    "lichee-d1-fullbench and lichee-d1-fullbench-command are mutually exclusive; select exactly \
     one fullbench mode"
);

pub mod entry;

mod config;
// Crate-root shared IRQ restore policy (no feature gate: `lichee-d1-smoke`
// excludes `drivers` but still requires the critical-section impl).
mod critical_section_policy;
#[cfg(not(feature = "lichee-d1-smoke"))]
mod drivers;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod file;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod mm;
pub mod platform;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod pseudofs;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod syscall;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod task;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod time;

// Critical section implementation for embassy-sync AtomicWaker.
//
// IRQ restore uses the `critical-section` crate's official `set_impl!` +
// `Impl` contract with the `restore-state-bool` feature: acquire saves the
// prior IRQ enable state, release re-enables only when the matching acquire
// entered from an enabled state. Nested sections (and ISR wake) therefore
// never re-enable IRQs prematurely.
//
// Restore policy lives in `critical_section_policy` (the seam host-tested by
// `tests/ms04-async-rx-host-harness.rs`); keep no direct `axhal` IRQ calls
// here.
mod critical_impl {
    use axhal::asm::{disable_irqs, enable_irqs, irqs_enabled};

    use crate::critical_section_policy::{self, IrqOps};

    struct AxhalIrqOps;

    impl IrqOps for AxhalIrqOps {
        fn irqs_enabled(&self) -> bool {
            irqs_enabled()
        }

        fn disable_irqs(&self) {
            disable_irqs();
        }

        fn enable_irqs(&self) {
            enable_irqs();
        }
    }

    struct KernelCriticalSection;

    critical_section::set_impl!(KernelCriticalSection);

    unsafe impl critical_section::Impl for KernelCriticalSection {
        unsafe fn acquire() -> critical_section::RawRestoreState {
            critical_section_policy::acquire(&AxhalIrqOps)
        }

        unsafe fn release(restore_state: critical_section::RawRestoreState) {
            critical_section_policy::release(&AxhalIrqOps, restore_state);
        }
    }
}

// Minimal libc ABI shims required by lwext4's C code in the no-std kernel link.
mod libc_abi_shims {
    use core::ffi::c_void;

    #[unsafe(no_mangle)]
    static mut stdout: *mut c_void = core::ptr::null_mut();

    #[unsafe(no_mangle)]
    extern "C" fn fflush(_stream: *mut c_void) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn qsort(
        base: *mut c_void,
        nmemb: usize,
        size: usize,
        compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> i32>,
    ) {
        let Some(compar) = compar else {
            return;
        };
        if base.is_null() || nmemb < 2 || size == 0 {
            return;
        }

        let base = base.cast::<u8>();
        for end in (1..nmemb).rev() {
            for idx in 0..end {
                let left = unsafe { base.add(idx * size) };
                let right = unsafe { base.add((idx + 1) * size) };
                if unsafe { compar(left.cast::<c_void>(), right.cast::<c_void>()) } > 0 {
                    unsafe { swap_bytes(left, right, size) };
                }
            }
        }
    }

    unsafe fn swap_bytes(left: *mut u8, right: *mut u8, len: usize) {
        for offset in 0..len {
            let left_byte = unsafe { left.add(offset) };
            let right_byte = unsafe { right.add(offset) };
            let tmp = unsafe { left_byte.read() };
            unsafe {
                left_byte.write(right_byte.read());
                right_byte.write(tmp);
            }
        }
    }
}
