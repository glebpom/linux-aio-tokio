use std::fs::{Metadata, OpenOptions, Permissions};
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::{fmt, io};

use crate::errors::AioCommandError;
use crate::fs::AioOpenOptionsExt;
use crate::{AioContextHandle, AioResult, LockedBuf, Opcode, RawCommand, ReadFlags, WriteFlags};

/// AIO version of tokio [`File`], to work through [`AioContextHandle`]
///
/// [`File`]: ../tokio/fs/struct.File.html
/// [`AioContextHandle`]: struct.AioContextHandle.html
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
    /// [`buffer`]: struct.LockedBuf.html
    /// [`flags`]: struct.ReadFlags.html
    pub async fn read_at(
        &self,
        aio_handle: &AioContextHandle,
        offset: u64,
        buffer: LockedBuf,
        flags: ReadFlags,
    ) -> Result<(AioResult, LockedBuf), AioCommandError> {
        aio_handle
            .submit_request(
                self,
                RawCommand {
                    opcode: Opcode::Pread,
                    offset,
                    buf: Some(buffer),
                    flags: flags.bits() as _,
                },
            )
            .await
            .map(|(code, buf)| (code, buf.unwrap()))
    }

    /// Write to the file through AIO at `offset` from the [`buffer`] with provided [`flags`].
    ///
    /// [`buffer`]: struct.LockedBuf.html
    /// [`flags`]: struct.ReadFlags.html
    pub async fn write_at(
        &self,
        aio_handle: &AioContextHandle,
        offset: u64,
        buffer: LockedBuf,
        flags: WriteFlags,
    ) -> Result<(AioResult, LockedBuf), AioCommandError> {
        aio_handle
            .submit_request(
                self,
                RawCommand {
                    opcode: Opcode::Pwrite,
                    offset,
                    buf: Some(buffer),
                    flags: flags.bits() as _,
                },
            )
            .await
            .map(|(code, buf)| (code, buf.unwrap()))
    }

    /// Sync data and metadata through AIO
    pub async fn sync_all(
        &self,
        aio_handle: &AioContextHandle,
    ) -> Result<AioResult, AioCommandError> {
        aio_handle
            .submit_request(
                self,
                RawCommand {
                    opcode: Opcode::Fsync,
                    buf: None,
                    offset: 0,
                    flags: 0,
                },
            )
            .await
            .map(|(res, _)| res)
    }

    /// Sync only data through AIO
    pub async fn sync_data(
        &self,
        aio_handle: &AioContextHandle,
    ) -> Result<AioResult, AioCommandError> {
        aio_handle
            .submit_request(
                self,
                RawCommand {
                    opcode: Opcode::Fdsync,
                    buf: None,
                    offset: 0,
                    flags: 0,
                },
            )
            .await
            .map(|(res, _)| res)
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
