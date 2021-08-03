use std::fs::{Metadata, OpenOptions, Permissions};
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::{fmt, io};

use intrusive_collections::linked_list::LinkedListOps;
use intrusive_collections::DefaultLinkOps;
use parking_lot::lock_api::RawMutex;

use crate::errors::AioCommandError;
use crate::fs::AioOpenOptionsExt;
use crate::{GenericAioContextHandle, LockedBuf, RawCommand, ReadFlags, WriteFlags};

/// AIO version of tokio [`File`], to work through [`GenericAioContextHandle`]
///
/// [`File`]: ../tokio/fs/struct.File.html
/// [`GenericAioContextHandle`]: struct.GenericAioContextHandle.html
pub struct File {
    pub(crate) inner: tokio::fs::File,
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("File").field("inner", &self.inner).finish()
    }
}

impl File {
    /// Open the file. See tokio [`File::open`]
    ///
    /// [`File::open`]: ../tokio/fs/struct.File.html#method.open
    pub async fn open(path: impl AsRef<Path>, is_sync: bool) -> io::Result<File> {
        let mut open_options = OpenOptions::new();
        open_options.read(true).write(false);

        let mut path_buf = PathBuf::new();
        path_buf.push(path);

        open_options.aio_open(path_buf, is_sync).await
    }

    /// Open the file. See tokio [`File::create`]
    ///
    /// [`File::create`]: ../tokio/fs/struct.File.html#method.create
    pub async fn create(path: impl AsRef<Path>, is_sync: bool) -> io::Result<File> {
        let mut open_options = OpenOptions::new();
        open_options.write(true).truncate(true).create(true);

        let mut path_buf = PathBuf::new();
        path_buf.push(path);

        open_options.aio_open(path_buf, is_sync).await
    }

    /// Set file let. See tokio [`set_len`]
    ///
    /// [`set_len`]: ../tokio/fs/struct.File.html#method.set_len
    pub async fn set_len(&mut self, size: u64) -> io::Result<()> {
        self.inner.set_len(size).await
    }

    /// Retrieves file metadata. See tokio [`metadata`]
    ///
    /// [`metadata`]: ../tokio/fs/struct.File.html#method.metadata
    pub async fn metadata(&self) -> io::Result<Metadata> {
        self.inner.metadata().await
    }

    /// Set file permissions. See tokio [`set_permissions`]
    ///
    /// [`set_permissions`]: ../tokio/fs/struct.File.html#method.set_permissions
    pub async fn set_permissions(&self, perm: Permissions) -> io::Result<()> {
        self.inner.set_permissions(perm).await
    }

    /// Read the file through AIO at `offset` to the [`buffer`] with provided [`flags`].
    ///
    /// See [`submit_request`] for more information
    ///
    /// [`submit_request`]: struct.GenericAioContextHandle.html#method.submit_request
    /// [`buffer`]: struct.LockedBuf.html
    /// [`flags`]: struct.ReadFlags.html
    pub async fn read_at<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    >(
        &self,
        aio_handle: &GenericAioContextHandle<M, A, L>,
        offset: u64,
        buffer: &mut LockedBuf,
        len: u64,
        flags: ReadFlags,
    ) -> Result<u64, AioCommandError>
    where
        A::LinkOps: LinkedListOps + Default,
    {
        assert!(len <= buffer.size() as u64);
        aio_handle
            .submit_request(
                self,
                RawCommand::Pread {
                    offset,
                    buffer,
                    flags,
                    len,
                },
            )
            .await
    }

    /// Write to the file through AIO at `offset` from the [`buffer`] with provided [`flags`].
    ///
    /// See [`submit_request`] for more information
    ///
    /// [`submit_request`]: struct.GenericAioContextHandle.html#method.submit_request
    /// [`buffer`]: struct.LockedBuf.html
    /// [`flags`]: struct.ReadFlags.html
    pub async fn write_at<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    >(
        &self,
        aio_handle: &GenericAioContextHandle<M, A, L>,
        offset: u64,
        buffer: &LockedBuf,
        len: u64,
        flags: WriteFlags,
    ) -> Result<u64, AioCommandError>
    where
        A::LinkOps: LinkedListOps + Default,
    {
        assert!(len <= buffer.size() as u64);
        aio_handle
            .submit_request(
                self,
                RawCommand::Pwrite {
                    offset,
                    buffer,
                    flags,
                    len,
                },
            )
            .await
    }

    /// Sync data and metadata through AIO
    ///
    /// See [`submit_request`] for more information
    ///
    /// [`submit_request`]: struct.GenericAioContextHandle.html#method.submit_request
    pub async fn sync_all<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    >(
        &self,
        aio_handle: &GenericAioContextHandle<M, A, L>,
    ) -> Result<(), AioCommandError>
    where
        A::LinkOps: LinkedListOps + Default,
    {
        let r = aio_handle.submit_request(self, RawCommand::Fsync).await?;
        if r != 0 {
            return Err(AioCommandError::NonZeroCode);
        }
        Ok(())
    }

    /// Sync only data through AIO
    ///
    /// See [`submit_request`] for more information
    ///
    /// [`submit_request`]: struct.GenericAioContextHandle.html#method.submit_request
    pub async fn sync_data<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    >(
        &self,
        aio_handle: &GenericAioContextHandle<M, A, L>,
    ) -> Result<(), AioCommandError>
    where
        A::LinkOps: LinkedListOps + Default,
    {
        let r = aio_handle.submit_request(self, RawCommand::Fdsync).await?;
        if r != 0 {
            return Err(AioCommandError::NonZeroCode);
        }
        Ok(())
    }
}

impl AsRawFd for File {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl AsRawFd for &'_ File {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}
