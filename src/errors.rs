use std::io;

use thiserror::Error;

use crate::eventfd::EventFdError;

/// AIO command error
#[derive(Error, Debug)]
pub enum AioCommandError {
    /// AIO context was stopped
    #[error("AioContext stopped")]
    AioStopped,

    /// Error from [`io_submit`]
    ///
    /// [`io_submit`]: https://manpages.debian.org/testing/manpages-dev/io_submit.2.en.html
    #[error("io_submit error: {0}")]
    IoSubmit(#[source] io::Error),

    /// Bad result received
    #[error("bad result: `{0}`")]
    BadResult(#[source] io::Error),

    /// Non-zero length returned
    #[error("non-zero code returned")]
    NonZeroCode,

    /// The capacity of AIO context exceeded. Happens if `use_semaphore` set to `false`
    /// and the code attempts to send more requests than kernel-threads.
    #[error("capacity exceeded")]
    CapacityExceeded,
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
