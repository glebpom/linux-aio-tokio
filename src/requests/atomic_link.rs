use std::cell::Cell;
use std::fmt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use intrusive_collections::linked_list::LinkedListOps;
use intrusive_collections::{DefaultLinkOps, LinkOps};

/// Link for intrusive collection, suitable for multi-threaded environments
pub struct AtomicLink {
    locked: AtomicBool,
    next: Cell<Option<NonNull<AtomicLink>>>,
    prev: Cell<Option<NonNull<AtomicLink>>>,
}

unsafe impl Sync for AtomicLink {}

const UNLINKED_MARKER: Option<NonNull<AtomicLink>> =
    unsafe { Some(NonNull::new_unchecked(1 as *mut AtomicLink)) };

impl AtomicLink {
    /// Creates a new `Link`.
    #[inline]
    pub const fn new() -> AtomicLink {
        AtomicLink {
            locked: AtomicBool::new(false),
            next: Cell::new(UNLINKED_MARKER),
            prev: Cell::new(UNLINKED_MARKER),
        }
    }
}

impl DefaultLinkOps for AtomicLink {
    type Ops = AtomicLinkOps;

    const NEW: Self::Ops = AtomicLinkOps;
}

// An object containing a link can be sent to another thread if it is unlinked.
unsafe impl Send for AtomicLink {}

// Provide an implementation of Clone which simply initializes the new link as
// unlinked. This allows structs containing a link to derive Clone.
impl Clone for AtomicLink {
    #[inline]
    fn clone(&self) -> AtomicLink {
        AtomicLink::new()
    }
}

// Same as above
impl Default for AtomicLink {
    #[inline]
    fn default() -> AtomicLink {
        AtomicLink::new()
    }
}

// Provide an implementation of Debug so that structs containing a link can
// still derive Debug.
impl fmt::Debug for AtomicLink {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // There isn't anything sensible to print here except whether the link
        // is currently in a list.
        if self.locked.load(Ordering::Relaxed) {
            write!(f, "linked")
        } else {
            write!(f, "unlinked")
        }
    }
}

/// LinkOps for intrusive collection, suitable for multi-threaded environments
#[derive(Clone, Copy, Default, Debug)]
pub struct AtomicLinkOps;

// https://github.com/Amanieu/intrusive-rs/issues/47
unsafe impl LinkOps for AtomicLinkOps {
    type LinkPtr = NonNull<AtomicLink>;

    #[inline]
    unsafe fn acquire_link(&mut self, ptr: Self::LinkPtr) -> bool {
        !ptr.as_ref().locked.swap(true, Ordering::Acquire)
    }

    #[inline]
    unsafe fn release_link(&mut self, ptr: Self::LinkPtr) {
        ptr.as_ref().locked.store(false, Ordering::Release)
    }
}

unsafe impl LinkedListOps for AtomicLinkOps {
    #[inline]
    unsafe fn next(&self, ptr: Self::LinkPtr) -> Option<Self::LinkPtr> {
        ptr.as_ref().next.get()
    }

    #[inline]
    unsafe fn prev(&self, ptr: Self::LinkPtr) -> Option<Self::LinkPtr> {
        ptr.as_ref().prev.get()
    }

    #[inline]
    unsafe fn set_next(&mut self, ptr: Self::LinkPtr, next: Option<Self::LinkPtr>) {
        ptr.as_ref().next.set(next);
    }

    #[inline]
    unsafe fn set_prev(&mut self, ptr: Self::LinkPtr, prev: Option<Self::LinkPtr>) {
        ptr.as_ref().prev.set(prev);
    }
}
