//! An unsafe (non-thread-safe) lock, equivalent to UnsafeCell from futures_intrusive,
//! until https://github.com/Matthias247/futures-intrusive/pull/38 is merged

use core::marker::PhantomData;
use parking_lot::lock_api::{GuardSend, RawMutex};

/// An unsafe (non-thread-safe) lock, equivalent to UnsafeCell
#[derive(Debug)]
pub struct NoopLock {
    /// Assigned in order to make the type !Sync
    _phantom: PhantomData<*mut ()>,
}

unsafe impl RawMutex for NoopLock {
    const INIT: NoopLock = NoopLock {
        _phantom: PhantomData,
    };

    type GuardMarker = GuardSend;

    fn lock(&self) {}

    fn try_lock(&self) -> bool {
        true
    }

    unsafe fn unlock(&self) {}
}
