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
use std::future::Future;
use std::os::unix::prelude::*;
use std::ptr;
use std::sync::{Arc, Weak};
use std::{fmt, io, mem};

use futures::channel::oneshot;
use futures::{pin_mut, select, FutureExt, StreamExt};
use futures_intrusive::sync::GenericSemaphore;
use intrusive_collections::linked_list::LinkedListOps;
use intrusive_collections::{linked_list, DefaultLinkOps};
use parking_lot::lock_api::{Mutex, RawMutex};
use tokio::task;

pub use commands::*;
pub use errors::{AioCommandError, AioContextError};
pub use eventfd::EventFd;
pub use flags::*;
pub use fs::{AioOpenOptionsExt, File};
pub use locked_buf::{LockedBuf, LockedBufError};
pub use noop_lock::NoopLock;
use requests::{Request, Requests};
use wait_future::AioWaitFuture;

pub use crate::requests::AtomicLink;
pub use crate::requests::IntrusiveAdapter;
pub use crate::requests::{LocalRequestAdapter, SyncRequestAdapter};

mod aio;
mod commands;
mod errors;
mod eventfd;
mod flags;
mod fs;
mod locked_buf;
mod noop_lock;
mod requests;
mod wait_future;

type AioResult = aio::__s64;

pub(crate) struct GenericAioContextInner<
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
> where
    A::LinkOps: LinkedListOps + Default,
{
    context: aio::aio_context_t,
    eventfd: RawFd,
    num_slots: usize,
    capacity: Option<GenericSemaphore<M>>,
    requests: Mutex<M, Requests<M, A, L>>,
    stop_tx: Mutex<M, Option<oneshot::Sender<()>>>,
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > GenericAioContextInner<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn new(
        eventfd: RawFd,
        nr: usize,
        use_semaphore: bool,
        stop_tx: oneshot::Sender<()>,
    ) -> Result<GenericAioContextInner<M, A, L>, AioContextError> {
        let mut context: aio::aio_context_t = 0;

        unsafe {
            if aio::io_setup(nr as libc::c_long, &mut context) != 0 {
                return Err(AioContextError::IoSetup(io::Error::last_os_error()));
            }
        };

        Ok(GenericAioContextInner {
            context,
            requests: Mutex::new(Requests::new(nr)?),
            capacity: if use_semaphore {
                Some(GenericSemaphore::new(true, nr))
            } else {
                None
            },
            eventfd,
            stop_tx: Mutex::new(Some(stop_tx)),
            num_slots: nr,
        })
    }
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > Drop for GenericAioContextInner<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
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
/// [`close`]: struct.GenericAioContext.html#method.close
pub struct GenericAioContext<
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
> where
    A::LinkOps: LinkedListOps + Default,
{
    inner: Arc<GenericAioContextInner<M, A, L>>,
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > fmt::Debug for GenericAioContext<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AioContext")
            .field("num_slots", &self.inner.num_slots)
            .finish()
    }
}

/// Cloneable handle to AIO context. Required for any AIO operations
pub struct GenericAioContextHandle<
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
> where
    A::LinkOps: LinkedListOps + Default,
{
    inner: Weak<GenericAioContextInner<M, A, L>>,
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > Clone for GenericAioContextHandle<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn clone(&self) -> Self {
        GenericAioContextHandle {
            inner: self.inner.clone(),
        }
    }
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > GenericAioContextHandle<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    /// Number of available AIO slots left in the context
    ///
    /// Return None if AIO context stopped, or if `use_semaphore`
    /// was set to `false`
    pub fn available_slots(&self) -> Option<usize> {
        self.inner
            .upgrade()
            .and_then(|i| i.capacity.as_ref().map(|c| c.permits()))
    }

    /// Submit command to the AIO context
    ///
    /// If `use_semaphore` set to `false`, this function will return
    /// `CapacityExceeded` error if the user's code tries to exceed
    /// the allowed number of in-flight requests
    pub async fn submit_request(
        &self,
        fd: &impl AsRawFd,
        mut command: RawCommand<'_>,
    ) -> Result<u64, AioCommandError> {
        let inner_context = self
            .inner
            .upgrade()
            .ok_or(AioCommandError::AioStopped)?
            .clone();

        if let Some(cap) = &inner_context.capacity {
            cap.acquire(1).await.disarm();
        }

        let mut request = inner_context
            .requests
            .lock()
            .take()
            .ok_or(AioCommandError::CapacityExceeded)?;

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
            if let Some(c) = &inner_context.capacity {
                c.release(1)
            }

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

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > fmt::Debug for GenericAioContextHandle<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AioContextHandle").finish()
    }
}

/// Create new AIO context with `nr` number of threads
///
/// If `use_semaphore` is set to true, the request sending future
/// will wait until the kernel thread is freed. Otherwise, no wait
/// for available capacity occurs. It's the user's code
/// responsibility to ensure that number of in-flight queries
/// doesn't exceed the number of kernel threads.
#[allow(clippy::type_complexity)]
pub fn generic_aio_context<M, A, L>(
    nr: usize,
    use_semaphore: bool,
) -> Result<
    (
        GenericAioContext<M, A, L>,
        GenericAioContextHandle<M, A, L>,
        impl Future<Output = Result<(), io::Error>>,
    ),
    AioContextError,
>
where
    A: crate::IntrusiveAdapter<M, L>,
    A::LinkOps: LinkedListOps + Default,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    M: RawMutex,
{
    let mut eventfd = EventFd::new(0, false)?;
    let (stop_tx, stop_rx) = oneshot::channel();

    let inner = Arc::new(GenericAioContextInner::new(
        eventfd.as_raw_fd(),
        nr,
        use_semaphore,
        stop_tx,
    )?);

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
                    let request_ptr = event.data as usize as *mut Request<M, L>;

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

                        if let Some(c) = &inner.capacity {
                            c.release(1)
                        }
                    }
                }
            }

            Ok(())
        }
    }
    .fuse();

    let background = async move {
        pin_mut!(poll_future);

        select! {
            res = poll_future => res,
            _ = stop_rx.fuse() => Ok(()),
        }
    };

    let handle = GenericAioContextHandle {
        inner: Arc::downgrade(&inner),
    };

    Ok((GenericAioContext { inner }, handle, background))
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > GenericAioContext<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    /// Number of available AIO slots left in the context
    pub fn available_slots(&self) -> Option<usize> {
        self.inner.capacity.as_ref().map(|c| c.permits())
    }

    /// Close the AIO context and wait for all related running futures to complete.
    pub async fn close(self) {
        self.inner.stop_tx.lock().take().unwrap().send(()).unwrap();
        while Arc::strong_count(&self.inner) != 1 {
            task::yield_now().await;
        }
    }
}

/// Create new AIO context suitable for cross-threaded environment (tokio rt-threaded),
/// backed by parking_lot Mutex. Automatically spawn background task, which polls
/// eventfd with `tokio::spawn`.
///
/// See [`generic_aio_context`](fn.generic_aio_context.html) for more details
#[inline]
pub fn aio_context(
    nr: usize,
    use_semaphore: bool,
) -> Result<(AioContext, AioContextHandle), AioContextError> {
    let (aio_context, aio_handle, background) = generic_aio_context(nr, use_semaphore)?;
    tokio::spawn(background);

    Ok((aio_context, aio_handle))
}

/// AIO context suitable for cross-threaded environment (tokio rt-threaded),
/// backed by parking_lot Mutex
pub type AioContext = GenericAioContext<parking_lot::RawMutex, SyncRequestAdapter, AtomicLink>;

/// AIO context handle suitable for cross-threaded environment (tokio rt-threaded),
/// backed by parking_lot Mutex
pub type AioContextHandle =
    GenericAioContextHandle<parking_lot::RawMutex, SyncRequestAdapter, AtomicLink>;

/// Create new AIO context suitable for single-threaded environment (tokio rt-core)
///
/// See [`generic_aio_context`](fn.generic_aio_context.html) for more details.
#[inline]
pub fn local_aio_context(
    nr: usize,
    use_semaphore: bool,
) -> Result<
    (
        LocalAioContext,
        LocalAioContextHandle,
        impl Future<Output = Result<(), io::Error>>,
    ),
    AioContextError,
> {
    generic_aio_context(nr, use_semaphore)
}

/// AIO context suitable for cross-threaded environment (tokio rt-core)
pub type LocalAioContext = GenericAioContext<NoopLock, LocalRequestAdapter, linked_list::Link>;

/// AIO context handle suitable for single-threaded environment (tokio rt-core)
pub type LocalAioContextHandle =
    GenericAioContextHandle<NoopLock, LocalRequestAdapter, linked_list::Link>;
