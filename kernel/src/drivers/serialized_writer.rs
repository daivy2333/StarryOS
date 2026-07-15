use alloc::sync::Arc;

use kspin::SpinNoPreempt;

pub(crate) struct SerializedWriter<W>(Arc<SpinNoPreempt<W>>);

impl<W> SerializedWriter<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self(Arc::new(SpinNoPreempt::new(writer)))
    }

    pub(crate) fn with_lock<T>(&self, f: impl FnOnce(&mut W) -> T) -> T {
        f(&mut self.0.lock())
    }
}

impl<W> Clone for SerializedWriter<W> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
