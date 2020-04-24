#![allow(missing_docs)]

use intrusive_collections::{
    intrusive_adapter, linked_list, Adapter, DefaultLinkOps, DefaultPointerOps,
};
use lock_api::RawMutex;

use crate::requests::Request;
use crate::{AtomicLink, NoopLock};

intrusive_adapter!(pub SyncRequestAdapter = Box<Request<parking_lot::RawMutex, AtomicLink>>: Request<parking_lot::RawMutex, AtomicLink> { link: AtomicLink });
intrusive_adapter!(pub LocalRequestAdapter = Box<Request<NoopLock, linked_list::Link>>: Request<NoopLock, linked_list::Link> { link: linked_list::Link });

/// Intrusive adapter suitable for storing `Request`
pub trait IntrusiveAdapter<M, L>:
    Adapter<PointerOps = DefaultPointerOps<Box<Request<M, L>>>>
where
    M: RawMutex,
    L: DefaultLinkOps,
{
    /// Create new intrusive adapter
    fn new() -> Self;
}

impl IntrusiveAdapter<parking_lot::RawMutex, AtomicLink> for SyncRequestAdapter {
    fn new() -> Self {
        Self::new()
    }
}

impl IntrusiveAdapter<NoopLock, linked_list::Link> for LocalRequestAdapter {
    fn new() -> Self {
        Self::new()
    }
}
