use tempfile::tempdir;

use linux_aio_tokio::{aio_context, AioOpenOptionsExt, LockedBuf, ReadFlags, WriteFlags};
use std::fs::OpenOptions;

#[tokio::main]
async fn main() {
    let (aio, aio_handle) = aio_context(8).unwrap();

    let dir = tempdir().unwrap();

    let mut open_options = OpenOptions::new();
    open_options
        .read(true)
        .create_new(true)
        .append(true)
        .write(true);

    let file = open_options
        .aio_open(dir.path().join("tmp"), false)
        .await
        .unwrap();

    let mut write_buf = LockedBuf::with_size(1024).unwrap();

    for i in 0..write_buf.size() {
        write_buf.as_mut()[i] = (i % 0xff) as u8;
    }

    let (_, write_buf) = file
        .write_at(&aio_handle, 0, write_buf, WriteFlags::APPEND)
        .await
        .unwrap();

    let read_buf = LockedBuf::with_size(1024).unwrap();

    let (_, read_buf) = file
        .read_at(&aio_handle, 0, read_buf, ReadFlags::empty())
        .await
        .unwrap();

    assert_eq!(read_buf.as_ref(), write_buf.as_ref());

    aio.close().await;

    println!("all good!");
}
