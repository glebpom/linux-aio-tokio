use bitflags::bitflags;

use crate::aio;

bitflags! {
    /// AIO write flags. See [`io_submit`](http://man7.org/linux/man-pages/man2/io_submit.2.html)
    pub struct WriteFlags: isize {
        /// Append data to the end of the file.  See the description
        /// of the flag of the same name in [`pwritev2(2)`] as well as
        /// the description of O_APPEND in [`open(2)`].  The aio_offset
        /// field is ignored.  The file offset is not changed.
        ///
        /// [`pwritev2(2)`]: http://man7.org/linux/man-pages/man2/pwritev2.2.html
        /// [`open(2)`]: http://man7.org/linux/man-pages/man2/open.2.html
        const APPEND = aio::RWF_APPEND as isize;

        /// Write operation complete according to requirement of
        /// synchronized I/O data integrity.  See the description
        /// of the flag of the same name in [`pwritev2(2)`] as well the
        /// description of `O_DSYNC` in [`open(2)`].
        ///
        /// [`pwritev2(2)`]: http://man7.org/linux/man-pages/man2/pwritev2.2.html
        /// [`open(2)`]: http://man7.org/linux/man-pages/man2/open.2.html
        const DSYNC = aio::RWF_DSYNC as isize;

        /// High priority request, poll if possible
        const HIPRI = aio::RWF_HIPRI as isize;

        /// Don't wait if the I/O will block for operations such as
        /// file block allocations, dirty page flush, mutex locks,
        /// or a congested block device inside the kernel.  If any
        /// of these conditions are met, the control block is
        /// returned immediately with a return value of `-EAGAIN` in
        /// the res field of the io_event structure.
        const NOWAIT = aio::RWF_NOWAIT as isize;

        /// Write operation complete according to requirement of
        /// synchronized I/O file integrity.  See the description
        /// of the flag of the same name in [`pwritev2(2)`] as well the
        /// description of `O_SYNC` in [`open(2)`].
        ///
        /// [`pwritev2(2)`]: http://man7.org/linux/man-pages/man2/pwritev2.2.html
        /// [`open(2)`]: http://man7.org/linux/man-pages/man2/open.2.html
        const SYNC = aio::RWF_SYNC as isize;
    }
}

bitflags! {
    /// AIO read flags. See [`io_submit`](http://man7.org/linux/man-pages/man2/io_submit.2.html)
    pub struct ReadFlags: isize {
        /// High priority request, poll if possible
        const HIPRI = aio::RWF_HIPRI as isize;

        /// Don't wait if the I/O will block for operations such as
        /// file block allocations, dirty page flush, mutex locks,
        /// or a congested block device inside the kernel.  If any
        /// of these conditions are met, the control block is
        /// returned immediately with a return value of `-EAGAIN` in
        /// the res field of the io_event structure.
        const NOWAIT = aio::RWF_NOWAIT as isize;
    }
}
