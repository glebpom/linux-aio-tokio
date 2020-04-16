use crate::{aio, LockedBuf};

/// Represents AIO operation. See [`io_submit`](https://manpages.debian.org/testing/manpages-dev/io_submit.2.en.html)
#[derive(Copy, Clone, Debug)]
pub enum Opcode {
    /// Read
    Pread,

    /// Write
    Pwrite,

    /// Sync data only
    Fdsync,

    /// Sync data and metadata
    Fsync,
}

impl Opcode {
    #[inline]
    pub(crate) fn aio_const(self) -> u32 {
        use Opcode::*;

        match self {
            Pread => aio::IOCB_CMD_PREAD,
            Pwrite => aio::IOCB_CMD_PWRITE,
            Fdsync => aio::IOCB_CMD_FDSYNC,
            Fsync => aio::IOCB_CMD_FSYNC,
        }
    }
}

/// Raw AIO command
#[derive(Debug)]
pub struct RawCommand {
    /// Operation
    pub opcode: Opcode,

    /// Offset in the file
    pub offset: u64,

    /// Optional buffer
    pub buf: Option<LockedBuf>,

    /// ReadFlags or WriteFlags, depending on the operation
    pub flags: u32,
}
