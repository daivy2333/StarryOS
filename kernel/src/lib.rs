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
#[cfg(not(feature = "lichee-d1"))]
mod drivers;
#[cfg(not(feature = "lichee-d1"))]
mod file;
#[cfg(not(feature = "lichee-d1"))]
mod mm;
pub mod platform;
#[cfg(not(feature = "lichee-d1"))]
mod pseudofs;
#[cfg(not(feature = "lichee-d1"))]
mod syscall;
#[cfg(not(feature = "lichee-d1"))]
mod task;
#[cfg(not(feature = "lichee-d1"))]
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
