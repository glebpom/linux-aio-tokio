# linux-aio-tokio

[![Version](https://img.shields.io/crates/v/linux-aio-tokio.svg)](https://crates.io/crates/linux-aio-tokio)
[![License](https://img.shields.io/crates/l/linux-aio-tokio.svg)](https://github.com/glebpom/linux-aio-tokio/blob/master/LICENSE)
[![Docs](https://docs.rs/linux-aio-tokio/badge.svg)](https://docs.rs/linux-aio-tokio/)
[![Build Status](https://travis-ci.org/glebpom/linux-aio-tokio.svg?branch=master)](https://travis-ci.org/glebpom/linux-aio-tokio)

This package provides an integration of Linux kernel-level asynchronous I/O to the [Tokio platform](https://tokio.rs/).

Linux kernel-level asynchronous I/O is different from the [Posix AIO library](http://man7.org/linux/man-pages/man7/aio.7.html). 
Posix AIO is implemented using a pool of userland threads, which invoke regular, blocking system calls to perform file I/O.
 [Linux kernel-level AIO](http://lse.sourceforge.net/io/aio.html), on the other hand, provides kernel-level asynchronous 
 scheduling of I/O operations to the underlying block device.

## Usage

Add this to your `Cargo.toml`:

    [dependencies]
    linux-aio-tokio = "0.2"

## Examples

```rust
use std::fs::OpenOptions;

use tempfile::tempdir;

use linux_aio_tokio::{aio_context, AioOpenOptionsExt, LockedBuf, ReadFlags, WriteFlags};

#[tokio::main]
async fn main() {
    let (aio, aio_handle) = aio_context(8, true).unwrap();

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

    file.write_at(&aio_handle, 0, &write_buf, 1024, WriteFlags::APPEND)
        .await
        .unwrap();

    let mut read_buf = LockedBuf::with_size(1024).unwrap();

    file.read_at(&aio_handle, 0, &mut read_buf, 1024, ReadFlags::empty())
        .await
        .unwrap();

    assert_eq!(read_buf.as_ref(), write_buf.as_ref());

    aio.close().await;

    println!("all good!");
}
```

## License

This code is licensed under the [MIT license](https://github.com/glebpom/linux-aio-tokio/blob/master/LICENSE).

## Credits

The current implementation is based on the code created by Hans-Martin Will, available at
[GitHub repository](https://github.com/hmwill/linux-aio-tokio).
