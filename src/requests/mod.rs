#![allow(clippy::unneeded_field_pattern)]

use std::os::unix::prelude::*;
use std::{io, mem};

use futures::channel::oneshot;
use intrusive_collections::{intrusive_adapter, LinkedList};

use crate::requests::atomic_link::AtomicLink;
use crate::{aio, LockedBuf};
use crate::{AioResult, RawCommand};
use parking_lot::Mutex;

mod atomic_link;

#[derive(Debug)]
pub(crate) struct RequestInner {
    pub aio_req: aio::iocb,
    pub completed_tx: Option<oneshot::Sender<AioResult>>,
    pub locked_buf: Option<LockedBuf>,
}

impl RequestInner {
    pub(crate) fn take_locked_buf(&mut self) -> Option<LockedBuf> {
        self.locked_buf.take()
    }
}

#[derive(Debug)]
pub struct Request {
    link: AtomicLink,
    pub(crate) inner: Mutex<RequestInner>,
}

impl Request {
    pub fn aio_addr(&self) -> u64 {
        (unsafe { mem::transmute::<_, usize>(self as *const Request) }) as u64
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

        let (ptr, len) = command
            .buf
            .as_ref()
            .map(|buf| buf.aio_addr_and_len())
            .unwrap_or((0, 0));

        inner.aio_req.aio_data = request_addr;
        inner.aio_req.aio_resfd = eventfd as u32;
        inner.aio_req.aio_flags = aio::IOCB_FLAG_RESFD | command.flags;
        inner.aio_req.aio_fildes = fd as u32;
        inner.aio_req.aio_offset = command.offset as i64;
        inner.aio_req.aio_buf = ptr;
        inner.aio_req.aio_nbytes = len;
        inner.aio_req.aio_lio_opcode = command.opcode.aio_const() as u16;

        inner.locked_buf = command.buf.take();
        inner.completed_tx = Some(tx);

        request_ptr_array[0] = &mut inner.aio_req as *mut aio::iocb;
    }
}

intrusive_adapter!(RequestAdapter = Box<Request>: Request { link: AtomicLink });

pub struct Requests {
    ready_pool: LinkedList<RequestAdapter>,
    outstanding: LinkedList<RequestAdapter>,
}

impl Requests {
    pub fn new(nr: usize) -> Result<Requests, io::Error> {
        let outstanding = LinkedList::new(RequestAdapter::new());
        let mut ready_pool = LinkedList::new(RequestAdapter::new());

        for _ in 0..nr {
            ready_pool.push_back(Box::new(Request {
                link: Default::default(),
                inner: Mutex::new(RequestInner {
                    aio_req: unsafe { mem::zeroed() },
                    completed_tx: None,
                    locked_buf: None,
                }),
            }));
        }

        Ok(Requests {
            ready_pool,
            outstanding,
        })
    }

    pub fn move_to_outstanding(&mut self, ptr: Box<Request>) {
        self.outstanding.push_back(ptr);
    }

    pub fn return_outstanding_to_ready(&mut self, request: *const Request) {
        let mut cursor = unsafe { self.outstanding.cursor_mut_from_ptr(request) };

        self.ready_pool.push_back(
            cursor.remove().expect(
                "Could not find item in outstanding list while trying to move to ready_pool",
            ),
        );
    }

    pub fn return_in_flight_to_ready(&mut self, req: Box<Request>) {
        self.ready_pool.push_back(req);
    }

    pub fn take(&mut self) -> Box<Request> {
        self.ready_pool.pop_front().expect(
            "could not retrieve request from ready_pool after successful acquire from semaphore",
        )
    }
}