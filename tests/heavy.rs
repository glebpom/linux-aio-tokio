use std::fs::OpenOptions;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use futures::{pin_mut, select, FutureExt};
use rand::{thread_rng, Rng};
use tempfile::tempdir;
use tokio::time::delay_for;

use linux_aio_tokio::AioOpenOptionsExt;
use linux_aio_tokio::{aio_context, LockedBuf, ReadFlags, WriteFlags};

const PAGE_SIZE: usize = 1024 * 1024;
const NUM_PAGES: usize = 256;

const NUM_READERS: usize = 256;
const NUM_WRITERS: usize = 4;
const NUM_AIO_THREADS: usize = 4;

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

                assert_eq!(
                    PAGE_SIZE,
                    file.read_at(
                        &aio_handle,
                        (page * PAGE_SIZE) as u64,
                        &mut buffer,
                        ReadFlags::empty()
                    )
                    .await
                    .unwrap() as usize
                );
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

                assert_eq!(
                    PAGE_SIZE,
                    file.write_at(
                        &aio_handle,
                        (page * PAGE_SIZE) as u64,
                        &mut buffer,
                        WriteFlags::DSYNC,
                    )
                    .await
                    .unwrap() as usize,
                );
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
