use std::mem::ManuallyDrop;
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

/// Buffer with fixed capacity, locked to RAM. It prevents
/// memory from being paged to the swap area
///
/// This is required to work with AIO operations.
pub struct LockedBuf {
    bytes: ManuallyDrop<MmapMut>,
    mlock_guard: ManuallyDrop<region::LockGuard>,
}

impl fmt::Debug for LockedBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LockedBuf")
            .field("size", &self.size())
            .finish()
    }
}

impl LockedBuf {
    /// Create with desired capacity
    pub fn with_size(size: usize) -> Result<LockedBuf, LockedBufError> {
        let bytes = MmapMut::map_anon(size)?;
        let mlock_guard = region::lock(bytes.as_ref().as_ptr(), size)?;

        Ok(LockedBuf {
            bytes: ManuallyDrop::new(bytes),
            mlock_guard: ManuallyDrop::new(mlock_guard),
        })
    }

    /// Return current capacity
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn aio_addr_and_len(&self) -> (u64, u64) {
        let len = self.bytes.len() as u64;
        let ptr = unsafe { mem::transmute::<_, usize>(self.bytes.as_ptr()) } as u64;
        (ptr, len)
    }
}

impl AsRef<[u8]> for LockedBuf {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl AsMut<[u8]> for LockedBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        self.bytes.as_mut()
    }
}

impl Drop for LockedBuf {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.mlock_guard);
            ManuallyDrop::drop(&mut self.bytes);
        }
    }
}
