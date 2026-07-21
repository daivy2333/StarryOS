// kernel/src/drivers/mod.rs

//! Platform drivers module.
//!
//! After async UART removal (Q30), this module no longer contains
//! UART driver code. Console I/O is handled by the platform polling
//! port and Console TTY in `pseudofs/dev/tty/`.

// Placeholder: no async UART modules remain.
// Console I/O: platform polling port (kernel/src/platform/) +
//              Console TTY (pseudofs/dev/tty/)
