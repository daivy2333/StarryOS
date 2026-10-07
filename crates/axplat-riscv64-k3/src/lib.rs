#![no_std]

#[macro_use]
extern crate log;
#[macro_use]
extern crate axplat;

mod boot;
/// Early/fault console plus the shared UART0 hardware guard consumed by the
/// kernel-side async PXA port (MS17 Iteration 000, task T2).
pub mod console;
mod init;
#[cfg(feature = "irq-if")]
pub mod irq;
mod mem;
mod power;
mod time;

pub mod aia;

/// Independent supervisor timer handler slot (MS17 Iteration 000, task T1).
pub mod timer_irq;

/// K3 GMAC DMA address/cache foundation (MS11 Iteration 000, task 1.1).
pub mod dma;

pub mod config {
    //! Platform configuration module.
    //!
    //! If the `AX_CONFIG_PATH` environment variable is set, it will load the configuration from the specified path.
    //! Otherwise, it will fall back to the `axconfig.toml` file in the current directory and generate the default configuration.
    //!
    //! If the `PACKAGE` field in the configuration does not match the package name, it will panic with an error message.
    axconfig_macros::include_configs!(path_env = "AX_CONFIG_PATH", fallback = "axconfig.toml");
    assert_str_eq!(
        PACKAGE,
        env!("CARGO_PKG_NAME"),
        "`PACKAGE` field in the configuration does not match the Package name. Please check your \
         configuration file."
    );
}
