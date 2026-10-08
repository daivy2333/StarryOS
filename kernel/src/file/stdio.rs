//! Stdio publication contract (MS17 Iteration 002, task T6).
//!
//! The publication order (stdin, then stdout, then stderr sharing one
//! endpoint) and its failure semantics live here so the native
//! `tests/k3-embedded-user-host` package can drive them with fakes that
//! replace only the open boundary. The kernel-side table adapter and the
//! `/dev/console` opener stay in `file/mod.rs`.

use axerrno::AxResult;

/// A descriptor table that can receive the three initial stdio endpoints.
pub trait StdioTable {
    type File: Clone;

    /// Whether no descriptor has been published yet (the publication
    /// asserts this, like the original `add_stdio` did).
    fn is_empty(&self) -> bool;

    /// Insert one endpoint as the next descriptor. Fails closed when the
    /// table cannot accept the entry.
    fn add(&mut self, file: Self::File) -> AxResult<()>;
}

/// Publish stdin/stdout/stderr into an empty descriptor table.
///
/// `open` is the only injected boundary: production opens the console
/// twice (input first), and the K3 stdio entry replays pre-opened files
/// through it so an open failure can never happen past the process commit
/// point. An open failure leaves the table untouched; an insertion
/// failure propagates with the table in the partial state the failed
/// insert left (identical to the previous `add_stdio` behavior).
pub fn publish_stdio<T: StdioTable>(
    table: &mut T,
    open: &mut dyn FnMut(bool) -> AxResult<T::File>,
) -> AxResult<()> {
    assert!(
        table.is_empty(),
        "stdio requires an empty descriptor table"
    );
    let input = open(true)?;
    let output = open(false)?;
    table.add(input)?;
    let stderr = output.clone();
    table.add(stderr)?;
    table.add(output)?;
    Ok(())
}
