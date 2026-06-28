//! D1 early smoke test — minimal UART0 polling output before
//! the async UART stack, rootfs, or shell are available.
//!
//! Runs only when `feature = "lichee-d1"` is enabled.

use super::early_console::{DwApbUart32EarlyConsole, EarlyConsole};

/// Print boot facts via the D1 early console, then halt.
///
/// # Safety
///
/// The caller MUST ensure:
/// - The platform MMIO is identity-mapped (virtual == physical).
/// - The UART0 clock and pins are configured (bootloader responsibility).
pub unsafe fn run_lichee_d1_smoke() -> ! {
    let desc = super::descriptor();
    // SAFETY: D1 platform MMIO is identity-mapped by OpenSBI; UART0 clock
    // and pins are configured by U-Boot.
    let console = unsafe { DwApbUart32EarlyConsole::from_config(&desc.console) };

    console.write_str("[starry-d1] early boot\n");

    console.write_str("hart_id: unavailable in S-mode\n");

    match sbi_get_spec_version() {
        Ok(sbi_version) => {
            let major = (sbi_version >> 24) & 0x7f;
            let minor = sbi_version & 0xffffff;
            console.write_str("sbi_version: ");
            print_decimal(&console, major as u32);
            console.write_str(".");
            print_decimal(&console, minor as u32);
            console.write_str("\n");
        }
        Err(error) => {
            console.write_str("sbi_version_error: ");
            print_decimal(&console, error.unsigned_abs() as u32);
            console.write_str("\n");
        }
    }

    console.write_str("[starry-d1] smoke complete, halting.\n");
    loop {
        riscv::asm::wfi();
    }
}

fn sbi_get_spec_version() -> Result<usize, isize> {
    let error: isize;
    let value: usize;
    // SAFETY: This is a standard SBI v0.2+ base extension call.
    // a0 returns the SBI error code, a1 returns the spec version value.
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") 0x10usize,
            in("a6") 0usize,
            inlateout("a0") 0usize => error,
            lateout("a1") value,
            options(nostack),
        );
    }
    if error == 0 { Ok(value) } else { Err(error) }
}

fn print_decimal(console: &impl EarlyConsole, val: u32) {
    if val == 0 {
        console.write_str("0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut pos = 0;
    let mut v = val;
    while v > 0 {
        buf[9 - pos] = b'0' + (v % 10) as u8;
        pos += 1;
        v /= 10;
    }
    // SAFETY: buf[10-pos..10] contains only ASCII decimal digits.
    let s = unsafe { core::str::from_utf8_unchecked(&buf[10 - pos..10]) };
    console.write_str(s);
}
