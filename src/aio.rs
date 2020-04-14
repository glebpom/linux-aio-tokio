#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/*
 * extracted from https://elixir.bootlin.com/linux/latest/source/include/uapi/linux/fs.h#L372
 * Flags for preadv2/pwritev2:
 */

/* per-IO O_DSYNC */
//#define RWF_DSYNC	((__force __kernel_rwf_t)0x00000002)
pub const RWF_DSYNC: u32 = 0x2;

/* per-IO O_SYNC */
//#define RWF_SYNC	((__force __kernel_rwf_t)0x00000004)
pub const RWF_SYNC: u32 = 0x4;

/* per-IO O_APPEND */
//# define RWF_APPEND    ((__force __kernel_rwf_t)0x00000010)
pub const RWF_APPEND: u32 = 0x10;

/* high priority request, poll if possible */
//# define RWF_HIPRI    ((__force __kernel_rwf_t)0x00000001)
pub const RWF_HIPRI: u32 = 0x1;

/* per-IO, return -EAGAIN if operation would block */
//# define RWF_NOWAIT    ((__force __kernel_rwf_t)0x00000008)
pub const RWF_NOWAIT: u32 = 0x8;

// -----------------------------------------------------------------------------------------------
// Inline functions that wrap the kernel calls for the entry points corresponding to Linux
// AIO functions
// -----------------------------------------------------------------------------------------------

// Initialize an AIO context for a given submission queue size within the kernel.
//
// See [io_setup(7)](http://man7.org/linux/man-pages/man2/io_setup.2.html) for details.
#[inline(always)]
pub unsafe fn io_setup(nr: libc::c_long, ctxp: *mut aio_context_t) -> libc::c_long {
    syscall(__NR_io_setup as libc::c_long, nr, ctxp)
}

// Destroy an AIO context.
//
// See [io_destroy(7)](http://man7.org/linux/man-pages/man2/io_destroy.2.html) for details.
#[inline(always)]
pub unsafe fn io_destroy(ctx: aio_context_t) -> libc::c_long {
    syscall(__NR_io_destroy as libc::c_long, ctx)
}

// Submit a batch of IO operations.
//
// See [io_sumit(7)](http://man7.org/linux/man-pages/man2/io_submit.2.html) for details.
#[inline(always)]
pub unsafe fn io_submit(
    ctx: aio_context_t,
    nr: libc::c_long,
    iocbpp: *mut *mut iocb,
) -> libc::c_long {
    syscall(__NR_io_submit as libc::c_long, ctx, nr, iocbpp)
}

// Retrieve completion events for previously submitted IO requests.
//
// See [io_getevents(7)](http://man7.org/linux/man-pages/man2/io_getevents.2.html) for details.
#[inline(always)]
pub unsafe fn io_getevents(
    ctx: aio_context_t,
    min_nr: libc::c_long,
    max_nr: libc::c_long,
    events: *mut io_event,
    timeout: *mut timespec,
) -> libc::c_long {
    syscall(
        __NR_io_getevents as libc::c_long,
        ctx,
        min_nr,
        max_nr,
        events,
        timeout,
    )
}
