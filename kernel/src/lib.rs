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

pub mod entry;

mod config;
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

// Critical section implementation for embassy-sync AtomicWaker
mod critical_impl {
    #[unsafe(no_mangle)]
    unsafe fn _critical_section_1_0_acquire() {
        axhal::asm::disable_irqs();
    }
    #[unsafe(no_mangle)]
    unsafe fn _critical_section_1_0_release(_: ()) {
        axhal::asm::enable_irqs();
    }
}
