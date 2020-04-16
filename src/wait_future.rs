use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::{ready, Future};

use crate::errors::AioCommandError;
use crate::requests::Request;
use crate::{AioContextInner, AioResult, LockedBuf};

pub(crate) type WaitResult = (AioResult, Option<LockedBuf>);

pub(crate) struct AioWaitFuture {
    rx: oneshot::Receiver<AioResult>,
    inner_context: Arc<AioContextInner>,
    request: Option<Box<Request>>,
}

impl AioWaitFuture {
    fn return_request_to_pool_and_take_locked_buf(&mut self) -> Option<LockedBuf> {
        let req = self.request.take().unwrap();
        let locked_buf = req.inner.lock().take_locked_buf();
        self.inner_context
            .requests
            .lock()
            .return_in_flight_to_ready(req);
        self.inner_context.capacity.add_permits(1);
        locked_buf
    }

    pub fn new(
        inner_context: &Arc<AioContextInner>,
        rx: oneshot::Receiver<AioResult>,
        request: Box<Request>,
    ) -> Self {
        AioWaitFuture {
            rx,
            inner_context: inner_context.clone(),
            request: Some(request),
        }
    }
}

impl Future for AioWaitFuture {
    type Output = Result<WaitResult, AioCommandError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(Pin::new(&mut self.rx).poll(cx))
            .expect("AIO stopped while AioWaitFuture was not completed");
        let buf = self.return_request_to_pool_and_take_locked_buf();

        Poll::Ready(Ok((res, buf)))
    }
}

impl Drop for AioWaitFuture {
    fn drop(&mut self) {
        self.rx.close();

        if self.rx.try_recv().is_ok() {
            // the sender have successfully sent data to the channel, but we didn't accept it
            self.return_request_to_pool_and_take_locked_buf();
        }

        if let Some(in_flight) = self.request.take() {
            self.inner_context
                .requests
                .lock()
                .move_to_outstanding(in_flight)
        }
    }
}
