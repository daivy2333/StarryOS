//! User address space management and user-space memory access.

mod access;
mod aspace;
mod io;
mod loader;
// Production embedded-ELF loader (MS17 T6): compiled only for the shapes
// that load an embedded payload (the D1 eager modes and the K3 stdio
// entry); the host harness includes the file directly.
#[cfg(any(
    feature = "lichee-d1-userbench",
    feature = "lichee-d1-fullbench",
    feature = "lichee-d1-fullbench-command",
    feature = "k3-stdio",
))]
mod embedded_loader;

pub use self::{access::*, aspace::*, io::*, loader::*};
