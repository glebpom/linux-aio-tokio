use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::pin::Pin;
use std::task::Poll;
use std::{fmt, mem, slice};

use futures::task::Context;
use futures::{self, ready, Sink, Stream};
use libc::eventfd;
use mio::{self, Ready};
use thiserror::Error;
use tokio::io::PollEvented;

#[derive(Error, Debug)]
pub enum EventFdError {
    #[error("error creating EventFd: `{0}`")]
    CreateError(#[source] io::Error),
    #[error("Poll error: `{0}`")]
    PollError(#[source] io::Error),
    #[error("Read error: `{0}`")]
    ReadError(#[source] io::Error),
}

pub struct EventFdInner {
    pub inner: File,
}

impl mio::Evented for EventFdInner {
    fn register(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> io::Result<()> {
        mio::unix::EventedFd(&self.inner.as_raw_fd()).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> io::Result<()> {
        mio::unix::EventedFd(&self.inner.as_raw_fd()).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
        mio::unix::EventedFd(&self.inner.as_raw_fd()).deregister(poll)
    }
}

/// Tokio-aware EventFd implementation
pub struct EventFd {
    evented: PollEvented<EventFdInner>,
    accepted: Option<u64>,
}

impl fmt::Debug for EventFd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("EventFd").finish()
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.evented.get_ref().inner.as_raw_fd()
    }
}

impl EventFd {
    /// Create EventFd  with `init` permits.
    pub fn new(init: usize, semaphore: bool) -> Result<EventFd, EventFdError> {
        let flags = if semaphore {
            libc::O_CLOEXEC | libc::EFD_NONBLOCK as i32 | libc::EFD_SEMAPHORE as i32
        } else {
            libc::O_CLOEXEC | libc::EFD_NONBLOCK as i32
        };

        let fd = unsafe { eventfd(init as libc::c_uint, flags) };

        if fd < 0 {
            return Err(EventFdError::CreateError(io::Error::last_os_error()));
        }

        Ok(EventFd {
            evented: PollEvented::new(EventFdInner {
                inner: unsafe { File::from_raw_fd(fd) },
            })
            .map_err(EventFdError::PollError)?,
            accepted: None,
        })
    }
}

impl Stream for EventFd {
    type Item = Result<u64, EventFdError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let ready = Ready::readable();

        ready!(self.evented.poll_read_ready(cx, ready)).map_err(EventFdError::PollError)?;

        let mut result = 0u64;
        let result_ptr = &mut result as *mut u64 as *mut u8;

        match self
            .evented
            .get_mut()
            .inner
            .read(unsafe { slice::from_raw_parts_mut(result_ptr, 8) })
        {
            Ok(rc) => {
                if rc as usize != mem::size_of::<u64>() {
                    panic!(
                        "Reading from an eventfd should transfer exactly {} bytes",
                        mem::size_of::<u64>()
                    )
                }

                assert_ne!(result, 0);
                Poll::Ready(Some(Ok(result)))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.evented
                    .clear_read_ready(cx, ready)
                    .map_err(EventFdError::PollError)?;

                Poll::Pending
            }
            Err(e) => Poll::Ready(Some(Err(EventFdError::ReadError(e)))),
        }
    }
}

impl Sink<u64> for EventFd {
    type Error = EventFdError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.accepted.is_none() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: u64) -> Result<(), Self::Error> {
        assert!(self.accepted.is_none());
        self.accepted = Some(item);

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.evented.poll_write_ready(cx)).map_err(EventFdError::PollError)?;

        {
            let bytes: &mut [u8; 8] =
                unsafe { &mut *(self.accepted.as_mut().unwrap() as *mut u64 as *mut [u8; 8]) };

            match self.evented.get_mut().inner.write(bytes) {
                Ok(rc) => {
                    assert_eq!(8, rc);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.evented
                        .clear_write_ready(cx)
                        .map_err(EventFdError::PollError)?;

                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(EventFdError::ReadError(e))),
            }
        }

        self.accepted = None;

        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }
}

#[cfg(test)]
mod tests {
    use futures::{SinkExt, StreamExt};

    use super::*;

    #[tokio::test]
    async fn non_semaphore() {
        let init = 5;
        let increment: u64 = 10;

        let mut efd = EventFd::new(init, false).unwrap();

        assert_eq!(init as u64, efd.next().await.unwrap().unwrap());

        efd.send(increment).await.unwrap();
        assert_eq!(increment, efd.next().await.unwrap().unwrap());

        efd.send(increment).await.unwrap();
        efd.send(increment).await.unwrap();
        assert_eq!(2 * increment, efd.next().await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn semaphore() {
        let init = 2;
        let increment: u64 = 10;

        let mut efd = EventFd::new(init, true).unwrap();

        efd.send(increment).await.unwrap();
        for _ in 0..(increment as usize + init) {
            assert_eq!(1, efd.next().await.unwrap().unwrap());
        }

        efd.send(increment).await.unwrap();
        for _ in 0..(increment as usize) {
            assert_eq!(1, efd.next().await.unwrap().unwrap());
        }
    }
}
