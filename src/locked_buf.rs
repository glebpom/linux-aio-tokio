use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::{fmt, io, mem};

use memmap::MmapMut;
use thiserror::Error;

/// Error during [`LockedBuf`] creation
///
/// [`LockedBuf`]: struct.LockedBuf.html
#[derive(Error, Debug)]
pub enum LockedBufError {
    /// Error in `mmap` invocation
    #[error("map_anon error: `{0}`")]
    MapAnon(#[from] io::Error),

    /// Error in `mlock` invocation
    #[error("mlock error: `{0}`")]
    MemLock(#[from] region::Error),
}

struct LockedBufInner {
    bytes: ManuallyDrop<MmapMut>,
    mlock_guard: ManuallyDrop<region::LockGuard>,
}

/// Buffer with fixed capacity, locked to RAM. It prevents
/// memory from being paged to the swap area
///
/// This is required to work with AIO operations.
pub struct LockedBuf {
    inner: Arc<UnsafeCell<LockedBufInner>>,
}

impl fmt::Debug for LockedBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LockedBuf")
            .field("size", &self.size())
            .finish()
    }
}

pub(crate) struct LifetimeExtender {
    _inner: Arc<UnsafeCell<LockedBufInner>>,
}

impl fmt::Debug for LifetimeExtender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LifetimeExtender").finish()
    }
}

impl LockedBuf {
    /// Create with desired capacity
    pub fn with_size(size: usize) -> Result<LockedBuf, LockedBufError> {
        let bytes = MmapMut::map_anon(size)?;
        let mlock_guard = region::lock(bytes.as_ref().as_ptr(), size)?;

        Ok(LockedBuf {
            inner: Arc::new(UnsafeCell::new(LockedBufInner {
                bytes: ManuallyDrop::new(bytes),
                mlock_guard: ManuallyDrop::new(mlock_guard),
            })),
        })
    }

    /// Return current capacity
    pub fn size(&self) -> usize {
        unsafe { &*self.inner.get() }.bytes.len()
    }

    pub(crate) fn aio_addr_and_len(&self) -> (u64, u64) {
        let len = unsafe { &*self.inner.get() }.bytes.len() as u64;
        let ptr = unsafe { mem::transmute::<_, usize>((*self.inner.get()).bytes.as_ptr()) } as u64;
        (ptr, len)
    }

    /// Handle, which prevents LockedBuf to drop while request is in-flight
    pub(crate) fn lifetime_extender(&self) -> LifetimeExtender {
        LifetimeExtender {
            _inner: self.inner.clone(),
        }
    }
}

impl AsRef<[u8]> for LockedBuf {
    fn as_ref(&self) -> &[u8] {
        let inner = unsafe { &*self.inner.get() };
        inner.bytes.as_ref()
    }
}

impl AsMut<[u8]> for LockedBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        let inner = unsafe { &mut *self.inner.get() };
        inner.bytes.as_mut()
    }
}

impl Drop for LockedBufInner {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.mlock_guard);
            ManuallyDrop::drop(&mut self.bytes);
        }
    }
}

unsafe impl Send for LockedBuf {}
unsafe impl Sync for LockedBuf {}

unsafe impl Send for LifetimeExtender {}
