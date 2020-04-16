use std::io;

use thiserror::Error;

use crate::eventfd::EventFdError;
use crate::LockedBuf;

/// AIO command error
#[derive(Error, Debug)]
pub enum AioCommandError {
    /// AIO context was stopped
    #[error("AioContext stopped")]
    AioStopped {
        /// LockedBuf
        buf: Option<LockedBuf>,
    },

    /// Error from [`io_submit`]
    ///
    /// [`io_submit`]: https://manpages.debian.org/testing/manpages-dev/io_submit.2.en.html
    #[error("io_submit error: {err}")]
    IoSubmit {
        /// Error
        err: io::Error,
        /// LockedBuf
        buf: Option<LockedBuf>,
    },

    /// Bad result received
    #[error("bad result: `{err}`")]
    BadResult {
        /// Error
        err: io::Error,
        /// LockedBuf
        buf: Option<LockedBuf>,
    },
}

impl AioCommandError {
    /// Take LockedBuf out of the error
    pub fn take_buf(&mut self) -> Option<LockedBuf> {
        use AioCommandError::*;

        match self {
            AioStopped { buf, .. } => buf.take(),
            IoSubmit { buf, .. } => buf.take(),
            BadResult { buf, .. } => buf.take(),
        }
    }
}

/// AIO context creation error
#[derive(Error, Debug)]
pub enum AioContextError {
    /// Could not create [`EventFd`]
    ///
    /// [`EventFd`]: struct.EventFd.html
    #[error("eventfd error: `{0}`")]
    EventFd(#[from] EventFdError),

    /// Error from `io_setup`
    #[error("io_setup error: `{0}`")]
    IoSetup(#[from] io::Error),
}
