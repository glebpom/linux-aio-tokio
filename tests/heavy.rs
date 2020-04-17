use std::fs::OpenOptions;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use futures::stream::FuturesUnordered;
use futures::{pin_mut, select, FutureExt, StreamExt};
use rand::{thread_rng, Rng};
use tempfile::tempdir;
use tokio::time::delay_for;

use helpers::*;
use linux_aio_tokio::AioOpenOptionsExt;
use linux_aio_tokio::{aio_context, LockedBuf, ReadFlags, WriteFlags};

const PAGE_SIZE: usize = 1024 * 1024;
const NUM_PAGES: usize = 256;

const NUM_READERS: usize = 256;
const NUM_WRITERS: usize = 4;
const NUM_AIO_THREADS: usize = 4;

pub mod helpers;

#[tokio::test(threaded_scheduler)]
async fn load_test() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tmp");

    let mut open_options = OpenOptions::new();
    open_options.write(true).create_new(true).read(true);

    let mut f = open_options.aio_open(path.clone(), false).await.unwrap();

    f.set_len((NUM_PAGES * PAGE_SIZE) as u64).await.unwrap();

    let file = Arc::new(f);

    let (_aio, aio_handle) = aio_context(NUM_AIO_THREADS).unwrap();

    let mut f = vec![];

    for _ in 0..NUM_READERS {
        let aio_handle = aio_handle.clone();
        let file = file.clone();

        f.push(tokio::spawn(async move {
            let mut buffer = LockedBuf::with_size(PAGE_SIZE).unwrap();
            let aio_handle = aio_handle.clone();
            let file = file.clone();

            loop {
                let page = thread_rng().gen_range(0, NUM_PAGES);

                let res = file
                    .read_at(
                        &aio_handle,
                        (page * PAGE_SIZE) as u64,
                        &mut buffer,
                        ReadFlags::empty(),
                    )
                    .await
                    .unwrap();

                assert_eq!(PAGE_SIZE, res as usize);
            }
        }));
    }

    for _ in 0..NUM_WRITERS {
        let aio_handle = aio_handle.clone();
        let file = file.clone();

        f.push(tokio::spawn(async move {
            let mut buffer = LockedBuf::with_size(PAGE_SIZE).unwrap();

            let aio_handle = aio_handle.clone();
            let file = file.clone();

            loop {
                let page = thread_rng().gen_range(0, NUM_PAGES);
                thread_rng().fill(buffer.as_mut());

                let res = file
                    .write_at(
                        &aio_handle,
                        (page * PAGE_SIZE) as u64,
                        &buffer,
                        WriteFlags::DSYNC,
                    )
                    .await
                    .unwrap();

                assert_eq!(PAGE_SIZE, res as usize);
            }
        }));
    }

    let stress = join_all(f).fuse();

    pin_mut!(stress);

    let mut timeout = delay_for(Duration::from_secs(30)).fuse();

    select! {
        _ = stress => {
            // never ends
            assert!(false);
        },
        _ = timeout => {
            assert!(true);
        },
    }

    dir.close().unwrap();
}

#[tokio::test(threaded_scheduler)]
async fn read_many_blocks_mt() {
    const FILE_SIZE: usize = 1024 * 512;
    const BUF_CAPACITY: usize = 8192;

    let (dir, path) = create_filled_tempfile(FILE_SIZE);

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
