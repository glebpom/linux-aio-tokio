use std::fs::{OpenOptions, Permissions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use futures::{select_biased, FutureExt, StreamExt};
use tempfile::{tempdir, TempDir};
use tokio::time::delay_for;

use assert_matches::assert_matches;
use linux_aio_tokio::{aio_context, AioCommandError, LockedBuf, ReadFlags, WriteFlags};
use linux_aio_tokio::{AioOpenOptionsExt, File};

const FILE_SIZE: usize = 1024 * 512;
const BUF_CAPACITY: usize = 8192;

fn fill_temp_file(file: &mut std::fs::File) {
    let mut data = [0u8; FILE_SIZE];

    for index in 0..data.len() {
        data[index] = index as u8;
    }

    file.write(&data).unwrap();
    file.sync_all().unwrap();
}

fn create_filled_tempfile() -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tmp");
    let mut temp_file = std::fs::File::create(dir.path().join("tmp")).unwrap();
    fill_temp_file(&mut temp_file);
    (dir, path)
}

#[tokio::test]
async fn aio_close() {
    let (dir, path) = create_filled_tempfile();

    let (aio, aio_handle) = aio_context(10).unwrap();
    let file = File::open(&path, false).await.unwrap();

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    aio.close().await;

    assert_matches!(
        file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
            .await
            .err()
            .unwrap(),
        AioCommandError::AioStopped
    );

    dir.close().unwrap();
}

#[tokio::test]
async fn file_open_and_meta() {
    let (dir, path) = create_filled_tempfile();

    let file = File::open(&path, false).await.unwrap();

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    let (_aio, aio_handle) = aio_context(10).unwrap();

    file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
        .await
        .unwrap();
    assert!(validate_block(buffer.as_ref()));

    assert!(file
        .write_at(&aio_handle, 0, &mut buffer, WriteFlags::empty())
        .await
        .is_err());

    file.metadata().await.unwrap();
    file.set_permissions(Permissions::from_mode(0o644))
        .await
        .unwrap();

    dir.close().unwrap();
}

#[tokio::test]
async fn file_create_and_set_len() {
    let (dir, path) = create_filled_tempfile();

    let mut file = File::create(&path, false).await.unwrap();

    file.set_len(BUF_CAPACITY as u64).await.unwrap();

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    let (_aio, aio_handle) = aio_context(10).unwrap();

    file.write_at(&aio_handle, 0, &mut buffer, WriteFlags::empty())
        .await
        .unwrap();
    assert!(file
        .read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
        .await
        .is_err());

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn read_block_mt() {
    let (dir, path) = create_filled_tempfile();

    let mut open_options = OpenOptions::new();
    open_options.read(true).write(true);

    let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    let (aio, aio_handle) = aio_context(10).unwrap();

    file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
        .await
        .unwrap();

    assert!(validate_block(buffer.as_ref()));

    assert_eq!(10, aio.available_slots());

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn write_block_mt() {
    let (dir, path) = create_filled_tempfile();

    {
        let mut open_options = OpenOptions::new();
        open_options.read(true).write(true);

        let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

        let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();
        fill_pattern(65u8, buffer.as_mut());

        let (_aio, aio_handle) = aio_context(10).unwrap();

        file.write_at(&aio_handle, 16384, &buffer, WriteFlags::DSYNC)
            .await
            .unwrap();
    }

    let mut file = std::fs::File::open(&path).unwrap();

    file.seek(SeekFrom::Start(16384)).unwrap();

    let mut read_buffer: [u8; BUF_CAPACITY] = [0u8; BUF_CAPACITY];
    file.read(&mut read_buffer).unwrap();

    assert!(validate_pattern(65u8, &read_buffer));

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn write_block_sync_mt() {
    let (dir, path) = create_filled_tempfile();

    {
        let mut open_options = OpenOptions::new();
        open_options.read(true).write(true);

        let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

        let (_aio, aio_handle) = aio_context(2).unwrap();

        {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();
            fill_pattern(65u8, buffer.as_mut());
            file.write_at(&aio_handle, 16384, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();
        }

        {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();
            fill_pattern(66u8, buffer.as_mut());
            file.write_at(&aio_handle, 32768, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();
        }

        {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();
            fill_pattern(67u8, buffer.as_mut());
            file.write_at(&aio_handle, 49152, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();
        }
    }

    let mut file = std::fs::File::open(&path).unwrap();

    let mut read_buffer: [u8; BUF_CAPACITY] = [0u8; BUF_CAPACITY];

    file.seek(SeekFrom::Start(16384)).unwrap();
    file.read(&mut read_buffer).unwrap();
    assert!(validate_pattern(65u8, &read_buffer));

    file.seek(SeekFrom::Start(32768)).unwrap();
    file.read(&mut read_buffer).unwrap();
    assert!(validate_pattern(66u8, &read_buffer));

    file.seek(SeekFrom::Start(49152)).unwrap();
    file.read(&mut read_buffer).unwrap();
    assert!(validate_pattern(67u8, &read_buffer));

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn invalid_offset() {
    let (dir, path) = create_filled_tempfile();

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    let mut open_options = OpenOptions::new();
    open_options.read(true).write(true);

    let file = Arc::new(open_options.aio_open(path.clone(), false).await.unwrap());

    let (_aio, aio_handle) = aio_context(10).unwrap();
    let res = file
        .read_at(&aio_handle, 1000000, &mut buffer, ReadFlags::empty())
        .await;

    assert!(res.is_err());

    dir.close().unwrap();
}

#[tokio::test(basic_scheduler)]
async fn future_cancellation() {
    let (dir, path) = create_filled_tempfile();

    let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

    let mut open_options = OpenOptions::new();
    open_options.read(true).write(true);

    let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

    let num_slots = 10;

    let (aio, aio_handle) = aio_context(num_slots).unwrap();
    let mut read = Box::pin(
        file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
            .fuse(),
    );

    let (_, immediate) = oneshot::channel::<()>();

    let mut immediate = immediate.fuse();

    select_biased! {
        _ = read => {
            assert!(false);
        },
        _ = immediate => {},
    }

    mem::drop(read);

    while aio.available_slots() != num_slots {
        delay_for(Duration::from_millis(50)).await;
    }

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn read_many_blocks_mt() {
    let (dir, path) = create_filled_tempfile();

    let mut open_options = OpenOptions::new();
    open_options.read(true).write(true);

    let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

    let num_slots = 7;
    let (aio, aio_handle) = aio_context(num_slots).unwrap();

    // 50 waves of requests just going above the limit

    // Waves start here
    for _wave in 0u64..50 {
        let f = FuturesUnordered::new();
        let aio_handle = aio_handle.clone();
        let file = file.clone();

        // Each wave makes 100 I/O requests
        for index in 0u64..100 {
            let file = file.clone();
            let aio_handle = aio_handle.clone();

            f.push(async move {
                // let aio_handle = aio_handle.clone();

                let offset = (index * BUF_CAPACITY as u64) % FILE_SIZE as u64;
                let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

                file.read_at(&aio_handle, offset, &mut buffer, ReadFlags::empty())
                    .await
                    .unwrap();

                assert!(validate_block(buffer.as_ref()));
            });
        }

        let _ = f.collect::<Vec<_>>().await;

        // all slots have been returned
        assert_eq!(num_slots, aio.available_slots());
    }

    dir.close().unwrap();
}

// A test with a mixed read/write workload
#[tokio::test(threaded_scheduler)]
async fn mixed_read_write_at() {
    let (dir, path) = create_filled_tempfile();

    let mut open_options = OpenOptions::new();
    open_options.read(true).write(true);

    let file = Arc::new(open_options.aio_open(path.clone(), true).await.unwrap());

    let (_aio, aio_handle) = aio_context(7).unwrap();

    let seq1 = {
        let file = file.clone();
        let aio_handle = aio_handle.clone();

        async move {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

            file.read_at(&aio_handle, 8192, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(0u8, buffer.as_mut());
            file.write_at(&aio_handle, 8192, &mut buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(1u8, buffer.as_mut());
            file.write_at(&aio_handle, 0, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 8192, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(0u8, buffer.as_ref()));

            file.read_at(&aio_handle, 0, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(1u8, buffer.as_ref()));
        }
    };

    let seq2 = {
        let file = file.clone();
        let aio_handle = aio_handle.clone();

        async move {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

            file.read_at(&aio_handle, 16384, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(2u8, buffer.as_mut());
            file.write_at(&aio_handle, 16384, &mut buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 24576, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(3, buffer.as_mut());
            file.write_at(&aio_handle, 24576, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 16384, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(2, buffer.as_ref()));

            file.read_at(&aio_handle, 24576, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(3u8, buffer.as_ref()));
        }
    };

    let seq3 = {
        async move {
            let mut buffer = LockedBuf::with_size(BUF_CAPACITY).unwrap();

            file.read_at(&aio_handle, 40960, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(5u8, buffer.as_mut());
            file.write_at(&aio_handle, 40960, &mut buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 32768, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_block(buffer.as_ref()));

            fill_pattern(6, buffer.as_mut());
            file.write_at(&aio_handle, 32768, &buffer, WriteFlags::DSYNC)
                .await
                .unwrap();

            file.read_at(&aio_handle, 40960, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(5, buffer.as_ref()));

            file.read_at(&aio_handle, 32768, &mut buffer, ReadFlags::empty())
                .await
                .unwrap();
            assert!(validate_pattern(6, buffer.as_ref()));
        }
    };

    join_all(vec![
        tokio::spawn(seq1),
        tokio::spawn(seq2),
        tokio::spawn(seq3),
    ])
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

    dir.close().unwrap();
}

fn fill_pattern(key: u8, buffer: &mut [u8]) {
    assert_eq!(buffer.len() % 2, 0);

    for index in 0..buffer.len() / 2 {
        buffer[index * 2] = key;
        buffer[index * 2 + 1] = index as u8;
    }
}

fn validate_pattern(key: u8, buffer: &[u8]) -> bool {
    assert_eq!(buffer.len() % 2, 0);

    for index in 0..buffer.len() / 2 {
        if (buffer[index * 2] != key) || (buffer[index * 2 + 1] != (index as u8)) {
            return false;
        }
    }

    return true;
}

fn validate_block(data: &[u8]) -> bool {
    for index in 0..data.len() {
        if data[index] != index as u8 {
            return false;
        }
    }

    true
}
