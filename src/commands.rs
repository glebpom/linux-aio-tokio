use crate::flags::{ReadFlags, WriteFlags};
use crate::locked_buf::LifetimeExtender;
use crate::{aio, LockedBuf};

/// Raw AIO command
#[derive(Debug)]
pub enum RawCommand<'a> {
    /// Read
    Pread {
        /// Offset
        offset: u64,
        /// Buffer
        buffer: &'a mut LockedBuf,
        /// Read flags
        flags: ReadFlags,
    },

    /// Write
    Pwrite {
        /// Offset
        offset: u64,
        /// Buffer
        buffer: &'a LockedBuf,

        /// Write flags
        flags: WriteFlags,
    },

    /// Sync data only
    Fdsync,

    /// Sync data and metadata
    Fsync,
}

impl<'a> RawCommand<'a> {
    pub(crate) fn opcode(&self) -> u32 {
        use RawCommand::*;

        match self {
            Pread { .. } => aio::IOCB_CMD_PREAD,
            Pwrite { .. } => aio::IOCB_CMD_PWRITE,
            Fdsync => aio::IOCB_CMD_FDSYNC,
            Fsync => aio::IOCB_CMD_FSYNC,
        }
    }

    pub(crate) fn offset(&self) -> Option<u64> {
        use RawCommand::*;

        match *self {
            Pread { offset, .. } => Some(offset),
            Pwrite { offset, .. } => Some(offset),
            Fdsync => None,
            Fsync => None,
        }
    }

    pub(crate) fn buffer_addr(&self) -> Option<(u64, u64)> {
        use RawCommand::*;

        match self {
            Pread { buffer, .. } => Some(buffer.aio_addr_and_len()),
            Pwrite { buffer, .. } => Some(buffer.aio_addr_and_len()),
            Fdsync => None,
            Fsync => None,
        }
    }

    pub(crate) fn flags(&self) -> Option<u32> {
        use RawCommand::*;

        match self {
            Pread { flags, .. } => Some(flags.bits() as _),
            Pwrite { flags, .. } => Some(flags.bits() as _),
            Fdsync => None,
            Fsync => None,
        }
    }

    pub(crate) fn buffer_lifetime_extender(&self) -> Option<LifetimeExtender> {
        use RawCommand::*;

        match self {
            Pread { buffer, .. } => Some(buffer.lifetime_extender()),
            Pwrite { buffer, .. } => Some(buffer.lifetime_extender()),
            Fdsync => None,
            Fsync => None,
        }
    }
}
