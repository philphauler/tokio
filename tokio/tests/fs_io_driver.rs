#![warn(rust_2018_idioms)]
#![cfg(all(feature = "full", not(target_os = "wasi")))] // WASI does not support all fs operations

//! `tokio::fs` must work on a runtime with or without an IO driver. When the
//! io_uring path is compiled in (`tokio_unstable` + `io-uring`) it is only
//! probed on runtimes that have one; otherwise the blocking fallback is used.

use std::path::Path;
use tempfile::tempdir;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::AsyncReadExt;
use tokio::runtime::Builder;

// Every fs operation that probes the io_uring driver before falling back.
async fn exercise_fs(dir: &Path) {
    let path = dir.join("file");
    fs::write(&path, b"hello").await.unwrap();
    assert_eq!(fs::read(&path).await.unwrap(), b"hello");

    let mut buf = String::new();
    let mut file = OpenOptions::new().read(true).open(&path).await.unwrap();
    file.read_to_string(&mut buf).await.unwrap();
    assert_eq!(buf, "hello");

    buf.clear();
    let mut file = File::from_std(std::fs::File::open(&path).unwrap());
    file.read_to_string(&mut buf).await.unwrap();
    assert_eq!(buf, "hello");

    let renamed = dir.join("renamed");
    fs::rename(&path, &renamed).await.unwrap();
    assert!(!fs::try_exists(&path).await.unwrap());
    assert!(fs::try_exists(&renamed).await.unwrap());
}

fn run(configure: impl Fn(&mut Builder) -> &mut Builder) {
    for mut builder in [Builder::new_current_thread(), Builder::new_multi_thread()] {
        let rt = configure(&mut builder).build().unwrap();
        let dir = tempdir().unwrap();
        rt.block_on(exercise_fs(dir.path()));
    }
}

#[test]
fn io_disabled() {
    run(|builder| builder);
}

#[test]
fn io_enabled() {
    run(Builder::enable_io);
}

#[test]
#[cfg(all(tokio_unstable, feature = "io-uring", target_os = "linux"))]
fn io_uring_enabled() {
    run(Builder::enable_io_uring);

    // On a kernel with io_uring, `read` must still take the io_uring path:
    // the blocking fallback would spawn a blocking thread, the uring path does not.
    if io_uring_available() {
        let rt = Builder::new_current_thread()
            .enable_io_uring()
            .build()
            .unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"hello").unwrap();

        assert_eq!(rt.block_on(fs::read(&path)).unwrap(), b"hello");
        assert_eq!(rt.metrics().num_blocking_threads(), 0);
    }
}

#[cfg(all(tokio_unstable, feature = "io-uring", target_os = "linux"))]
fn io_uring_available() -> bool {
    match io_uring::IoUring::new(2) {
        Ok(_) => true,
        Err(_) => false,
    }
}
