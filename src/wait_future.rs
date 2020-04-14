use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::future::FusedFuture;
use futures::{ready, Future};

use crate::errors::AioCommandError;
use crate::requests::Request;
use crate::{AioContextInner, AioResult};

pub(crate) struct AioWaitFuture {
    rx: oneshot::Receiver<AioResult>,
    inner_context: Arc<AioContextInner>,
    request: Option<Box<Request>>,
    result: Option<AioResult>,
}

impl AioWaitFuture {
    fn return_request_to_pool(&mut self) {
        let req = self.request.take().unwrap();
        self.inner_context
            .requests
            .lock()
            .return_in_flight_to_ready(req);
        self.inner_context.capacity.add_permits(1);
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
            result: None,
        }
    }
}

impl FusedFuture for AioWaitFuture {
    fn is_terminated(&self) -> bool {
        self.result.is_some()
    }
}

impl Future for AioWaitFuture {
    type Output = Result<AioResult, AioCommandError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.request.is_some() {
            self.result = Some(
                ready!(Pin::new(&mut self.rx).poll(cx)).map_err(|_| AioCommandError::AioStopped)?,
            );

            self.return_request_to_pool();
        }

        Poll::Ready(Ok(self.result.unwrap()))
    }
}

impl Drop for AioWaitFuture {
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
