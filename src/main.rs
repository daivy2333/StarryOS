#![no_std]
#![no_main]
#![doc = include_str!("../README.md")]

extern crate alloc;

use alloc::{borrow::ToOwned, vec::Vec};

pub const CMDLINE: &[&str] = &["/bin/sh", "-c", include_str!("init.sh")];

#[unsafe(no_mangle)]
fn main() {
    let args = CMDLINE
        .iter()
        .copied()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let envs = [];

    starry_kernel::entry::init(&args, &envs);
}

#[cfg(feature = "vf2")]
extern crate axplat_riscv64_visionfive2;

#[cfg(any(feature = "lichee-d1", feature = "lichee-d1-async-uart"))]
extern crate axplat_riscv64_lichee_d1;

#[cfg(feature = "k3")]
extern crate axplat_riscv64_k3;

// MS17 Iteration 000: the K3 async UART mode is platform-exclusive; no other
// platform's build shape may combine with it (the kernel-level k3+qemu /
// k3+lichee guards in platform/mod.rs stay authoritative for those pairs).
#[cfg(all(
    feature = "k3-async-uart",
    any(
        feature = "qemu",
        feature = "vf2",
        feature = "lichee-d1",
        feature = "lichee-d1-async-uart",
        feature = "lichee-d1-kbench",
        feature = "lichee-d1-userbench",
        feature = "lichee-d1-fullbench",
        feature = "lichee-d1-fullbench-command",
    )
))]
compile_error!(
    "k3-async-uart cannot be combined with qemu/vf2/lichee platform features; select exactly \
     one platform mode"
);

// MS12 Iteration 000: the K3 async GMAC mode is platform-exclusive the same
// way (the kernel-level k3-async-net+smp / k3-async-net+k3-async-uart guards
// in lib.rs stay authoritative for those pairs).
#[cfg(all(
    feature = "k3-async-net",
    any(
        feature = "qemu",
        feature = "vf2",
        feature = "lichee-d1",
        feature = "lichee-d1-async-uart",
        feature = "lichee-d1-kbench",
        feature = "lichee-d1-userbench",
        feature = "lichee-d1-fullbench",
        feature = "lichee-d1-fullbench-command",
    )
))]
compile_error!(
    "k3-async-net cannot be combined with qemu/vf2/lichee platform features; select exactly \
     one platform mode"
);
