#![allow(clippy::unneeded_field_pattern)]

use std::marker::PhantomData;
use std::os::unix::prelude::*;
use std::{io, mem};

use futures::channel::oneshot;
use intrusive_collections::linked_list::LinkedListOps;
use intrusive_collections::{DefaultLinkOps, LinkedList};
use lock_api::{Mutex, RawMutex};

use crate::locked_buf::LifetimeExtender;
pub use crate::requests::atomic_link::AtomicLink;
use crate::{aio, AioResult, RawCommand};

pub use self::intrusive_adapter::{IntrusiveAdapter, LocalRequestAdapter, SyncRequestAdapter};

mod atomic_link;
mod intrusive_adapter;

#[derive(Debug)]
pub(crate) struct RequestInner {
    pub aio_req: aio::iocb,
    pub completed_tx: Option<oneshot::Sender<AioResult>>,
    pub buf_lifetime_extender: Option<LifetimeExtender>,
}

impl RequestInner {
    pub(crate) fn take_buf_lifetime_extender(&mut self) -> Option<LifetimeExtender> {
        self.buf_lifetime_extender.take()
    }
}

#[derive(Debug)]
pub struct Request<M: RawMutex, L: DefaultLinkOps + Default> {
    link: L,
    pub(crate) inner: Mutex<M, RequestInner>,
}

impl<M: RawMutex, L: DefaultLinkOps + Default> Default for Request<M, L> {
    fn default() -> Self {
        Request {
            link: Default::default(),
            inner: Mutex::new(RequestInner {
                aio_req: unsafe { mem::zeroed() },
                completed_tx: None,
                buf_lifetime_extender: None,
            }),
        }
    }
}
impl<M: RawMutex, L: DefaultLinkOps + Default> Request<M, L> {
    pub fn aio_addr(&self) -> u64 {
        (unsafe { mem::transmute::<_, usize>(self as *const Self) }) as u64
    }

    pub fn send_to_waiter(&self, data: AioResult) -> bool {
        self.inner
            .lock()
            .completed_tx
            .take()
            .expect("no completed_tx in received AIO request")
            .send(data)
            .is_ok()
    }

    pub fn set_payload(
        &mut self,
        request_ptr_array: &mut [*mut aio::iocb; 1],
        request_addr: u64,
        eventfd: RawFd,
        fd: RawFd,
        command: &mut RawCommand,
        tx: oneshot::Sender<AioResult>,
    ) {
        let inner = &mut *self.inner.lock();

        let (addr, buf_len) = command.buffer_addr().unwrap_or((0, 0));
        let len = command.len().unwrap_or(0);

        assert!(len <= buf_len as u64, "len should be <= buffer.size()");

        inner.aio_req.aio_data = request_addr;
        inner.aio_req.aio_resfd = eventfd as u32;
        inner.aio_req.aio_flags = aio::IOCB_FLAG_RESFD | command.flags().unwrap_or(0);
        inner.aio_req.aio_fildes = fd as u32;
        inner.aio_req.aio_offset = command.offset().unwrap_or(0) as i64;
        inner.aio_req.aio_buf = addr;
        inner.aio_req.aio_nbytes = len;
        inner.aio_req.aio_lio_opcode = command.opcode() as u16;

        inner.buf_lifetime_extender = command.buffer_lifetime_extender();
        inner.completed_tx = Some(tx);

        request_ptr_array[0] = &mut inner.aio_req as *mut aio::iocb;
    }
}

pub struct Requests<
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
> where
    A::LinkOps: LinkedListOps + Default,
{
    ready_pool: LinkedList<A>,
    outstanding: LinkedList<A>,
    _request_mutex: PhantomData<M>,
    _link_ops: PhantomData<L>,
}

impl<M, A, L> Requests<M, A, L>
where
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    A::LinkOps: LinkedListOps + Default,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
{
    pub fn new(nr: usize) -> Result<Self, io::Error> {
        let outstanding = LinkedList::new(A::new());
        let mut ready_pool = LinkedList::new(A::new());

        for _ in 0..nr {
            ready_pool.push_back(Box::new(Request::default()));
        }

        Ok(Requests {
            ready_pool,
            outstanding,
            _request_mutex: Default::default(),
            _link_ops: Default::default(),
        })
    }

    pub fn move_to_outstanding(&mut self, ptr: Box<Request<M, L>>) {
        self.outstanding.push_back(ptr);
    }

    pub fn return_outstanding_to_ready(&mut self, request: *const Request<M, L>) {
        let mut cursor = unsafe { self.outstanding.cursor_mut_from_ptr(request) };

        self.ready_pool.push_back(
            cursor.remove().expect(
                "Could not find item in outstanding list while trying to move to ready_pool",
            ),
        );
    }

    pub fn return_in_flight_to_ready(&mut self, req: Box<Request<M, L>>) {
        self.ready_pool.push_back(req);
    }

    pub fn take(&mut self) -> Option<Box<Request<M, L>>> {
        self.ready_pool.pop_front()
    }
}
