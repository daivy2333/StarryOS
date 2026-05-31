//! The core functionality of a monolithic kernel, including loading user
//! programs and managing processes.

#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;
extern crate axruntime;

#[macro_use]
extern crate axlog;

pub mod entry;

mod config;
mod drivers;
mod file;
mod mm;
mod pseudofs;
mod syscall;
mod task;
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
