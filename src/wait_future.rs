use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::{ready, Future};
use intrusive_collections::DefaultLinkOps;
use lock_api::RawMutex;

use crate::errors::AioCommandError;
use crate::requests::Request;
use crate::{AioResult, GenericAioContextInner};
use intrusive_collections::linked_list::LinkedListOps;

pub(crate) struct AioWaitFuture<
    M: RawMutex,
    A: crate::IntrusiveAdapter<M, L>,
    L: DefaultLinkOps<Ops = A::LinkOps> + Default,
> where
    A::LinkOps: LinkedListOps + Default,
{
    rx: oneshot::Receiver<AioResult>,
    inner_context: Arc<GenericAioContextInner<M, A, L>>,
    request: Option<Box<Request<M, L>>>,
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > AioWaitFuture<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn return_request_to_pool(&mut self) {
        let req = self.request.take().unwrap();
        mem::drop(req.inner.lock().take_buf_lifetime_extender());
        self.inner_context
            .requests
            .lock()
            .return_in_flight_to_ready(req);
        self.inner_context.capacity.release(1);
    }

    pub fn new(
        inner_context: &Arc<GenericAioContextInner<M, A, L>>,
        rx: oneshot::Receiver<AioResult>,
        request: Box<Request<M, L>>,
    ) -> Self {
        AioWaitFuture {
            rx,
            inner_context: inner_context.clone(),
            request: Some(request),
        }
    }
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > Future for AioWaitFuture<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    type Output = Result<AioResult, AioCommandError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(Pin::new(&mut self.rx).poll(cx))
            .expect("AIO stopped while AioWaitFuture was not completed");
        self.return_request_to_pool();

        Poll::Ready(Ok(res))
    }
}

impl<
        M: RawMutex,
        A: crate::IntrusiveAdapter<M, L>,
        L: DefaultLinkOps<Ops = A::LinkOps> + Default,
    > Drop for AioWaitFuture<M, A, L>
where
    A::LinkOps: LinkedListOps + Default,
{
    fn drop(&mut self) {
        self.rx.close();

        if self.rx.try_recv().is_ok() {
            // the sender have successfully sent data to the channel, but we didn't accept it
            self.return_request_to_pool();
        }

        if let Some(in_flight) = self.request.take() {
            self.inner_context
                .requests
                .lock()
                .move_to_outstanding(in_flight)
        }
    }
}
