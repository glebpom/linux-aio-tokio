#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

//! Tokio Bindings for Linux Kernel AIO
//!
//! This package provides an integration of Linux kernel-level asynchronous I/O to the
//! [Tokio platform](https://tokio.rs/).
//!
//! Linux kernel-level asynchronous I/O is different from the [Posix AIO library](http://man7.org/linux/man-pages/man7/aio.7.html).
//! Posix AIO is implemented using a pool of userland threads, which invoke regular, blocking system
//! calls to perform file I/O. [Linux kernel-level AIO](http://lse.sourceforge.net/io/aio.html), on the
//! other hand, provides kernel-level asynchronous scheduling of I/O operations to the underlying block device.

use std::convert::TryInto;
use std::os::unix::prelude::*;
use std::ptr;
use std::sync::{Arc, Weak};
use std::{fmt, io, mem};

use futures::channel::oneshot;
use futures::{pin_mut, select, FutureExt, StreamExt};
use futures_intrusive::sync::Semaphore;
use parking_lot::Mutex;
use tokio::task;

pub use commands::*;
pub use errors::{AioCommandError, AioContextError};
pub use eventfd::EventFd;
pub use flags::*;
pub use fs::{AioOpenOptionsExt, File};
pub use locked_buf::{LockedBuf, LockedBufError};
use requests::{Request, Requests};
use wait_future::AioWaitFuture;

mod aio;
mod commands;
mod errors;
mod eventfd;
mod flags;
mod fs;
mod locked_buf;
mod requests;
mod wait_future;

type AioResult = aio::__s64;

pub(crate) struct AioContextInner {
    context: aio::aio_context_t,
    eventfd: RawFd,
    num_slots: usize,
    capacity: Semaphore,
    requests: parking_lot::Mutex<Requests>,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl AioContextInner {
    fn new(
        eventfd: RawFd,
        nr: usize,
        stop_tx: oneshot::Sender<()>,
    ) -> Result<AioContextInner, AioContextError> {
        let mut context: aio::aio_context_t = 0;

        unsafe {
            if aio::io_setup(nr as libc::c_long, &mut context) != 0 {
                return Err(AioContextError::IoSetup(io::Error::last_os_error()));
            }
        };

        Ok(AioContextInner {
            context,
            requests: Mutex::new(Requests::new(nr)?),
            capacity: Semaphore::new(true, nr),
            eventfd,
            stop_tx: Mutex::new(Some(stop_tx)),
            num_slots: nr,
        })
    }
}

impl Drop for AioContextInner {
    fn drop(&mut self) {
        let result = unsafe { aio::io_destroy(self.context) };
        assert_eq!(0, result, "io_destroy returned bad code");
    }
}

/// Represents running AIO context. Must be kept while AIO is in use.
/// In order to close it, [`close`] should be called. It will wait
/// until all related futures are finished.
/// Otherwise, if it just dropped, the termination will be triggered,
/// but some running futures will continue running until they receive
/// the data.
///
/// [`close`]: struct.AioContext.html#method.close
pub struct AioContext {
    inner: Arc<AioContextInner>,
}

impl fmt::Debug for AioContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AioContext")
            .field("num_slots", &self.inner.num_slots)
            .finish()
    }
}

/// Cloneable handle to AIO context. Required for any AIO operations
#[derive(Clone)]
pub struct AioContextHandle {
    inner: Weak<AioContextInner>,
}

impl AioContextHandle {
    /// Number of available AIO slots left in the context
    pub fn available_slots(&self) -> Option<usize> {
        self.inner.upgrade().map(|i| i.capacity.permits())
    }

    /// Submit command to the AIO context
    pub async fn submit_request(
        &self,
        fd: &impl AsRawFd,
        mut command: RawCommand<'_>,
    ) -> Result<u64, AioCommandError> {
        let inner_context = self
            .inner
            .upgrade()
            .ok_or_else(|| AioCommandError::AioStopped)?
            .clone();

        inner_context.capacity.acquire(1).await.disarm();

        let mut request = inner_context.requests.lock().take();

        let request_addr = request.aio_addr();

        let (tx, rx) = oneshot::channel();

        let result = {
            let mut request_ptr_array: [*mut aio::iocb; 1] = [ptr::null_mut(); 1];

            request.set_payload(
                &mut request_ptr_array,
                request_addr,
                inner_context.eventfd,
                fd.as_raw_fd(),
                &mut command,
                tx,
            );

            unsafe {
                aio::io_submit(
                    inner_context.context,
                    1,
                    request_ptr_array.as_mut_ptr() as *mut *mut aio::iocb,
                )
            }
        };

        if result != 1 {
            mem::drop(request.inner.lock().take_buf_lifetime_extender());
            inner_context
                .requests
                .lock()
                .return_in_flight_to_ready(request);
            inner_context.capacity.release(1);

            return Err(AioCommandError::IoSubmit(io::Error::last_os_error()));
        }

        let base = AioWaitFuture::new(&inner_context, rx, request);

        let code = base.await?;

        if code < 0 {
            Err(AioCommandError::BadResult(io::Error::from_raw_os_error(
                -code as _,
            )))
        } else {
            Ok(code.try_into().unwrap())
        }
    }
}

impl fmt::Debug for AioContextHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AioContextHandle").finish()
    }
}

/// Create new AIO context
pub fn aio_context(nr: usize) -> Result<(AioContext, AioContextHandle), AioContextError> {
    let mut eventfd = EventFd::new(0, false)?;
    let (stop_tx, stop_rx) = oneshot::channel();

    let inner = Arc::new(AioContextInner::new(eventfd.as_raw_fd(), nr, stop_tx)?);

    let context = inner.context;

    let poll_future = {
        let inner = inner.clone();

        async move {
            let mut events = Vec::with_capacity(nr);

            while let Some(Ok(available)) = eventfd.next().await {
                assert!(available > 0, "kernel reported zero ready events");
                assert!(
                    available <= nr as u64,
                    "kernel reported more events than number of maximum tasks"
                );

                unsafe {
                    let num_received = aio::io_getevents(
                        context,
                        available as libc::c_long,
                        available as libc::c_long,
                        events.as_mut_ptr(),
                        ptr::null_mut::<aio::timespec>(),
                    );

                    if num_received < 0 {
                        return Err(io::Error::last_os_error());
                    }

                    assert!(
                        num_received == available as _,
                        "io_getevents received events num not equal to reported through eventfd"
                    );
                    events.set_len(available as usize);
                };

                for event in &events {
                    let request_ptr = event.data as usize as *mut Request;

                    let sent_succeeded = unsafe { &*request_ptr }.send_to_waiter(event.res);

                    if !sent_succeeded {
                        mem::drop(
                            unsafe { &*request_ptr }
                                .inner
                                .lock()
                                .take_buf_lifetime_extender(),
                        );
                        inner
                            .requests
                            .lock()
                            .return_outstanding_to_ready(request_ptr);
                        inner.capacity.release(1)
                    }
                }
            }

            Ok(())
        }
    }
    .fuse();

    tokio::spawn(async move {
        pin_mut!(poll_future);

        select! {
            _ = poll_future => {},
            _ = stop_rx.fuse() => {},
        }
    });

    let handle = AioContextHandle {
        inner: Arc::downgrade(&inner),
    };

    Ok((AioContext { inner }, handle))
}

impl AioContext {
    /// Number of available AIO slots left in the context
    pub fn available_slots(&self) -> usize {
        self.inner.capacity.permits()
    }

    /// Close the AIO context and wait for all related running futures to complete.
    pub async fn close(self) {
        self.inner.stop_tx.lock().take().unwrap().send(()).unwrap();
        while Arc::strong_count(&self.inner) != 1 {
            task::yield_now().await;
        }
    }
}
