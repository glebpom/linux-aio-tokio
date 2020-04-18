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
