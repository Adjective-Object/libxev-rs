//! Tests for the File extension (extended-api).

#![cfg(feature = "extended-api")]

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, IntoRawFd};
use std::sync::Arc;
use std::sync::Mutex;

use libxev::extensions::File;
use libxev::{CbAction, Completion, Loop, RunMode};

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    // Make the name unique enough across parallel test runs.
    let pid = std::process::id();
    p.push(format!("libxev-rs-{pid}-{name}"));
    p
}

#[test]
fn file_write_then_pread_roundtrip() {
    let path = temp_path("write_pread.bin");
    let _ = std::fs::remove_file(&path);
    // Create the file empty, then take ownership of an O_RDWR fd.
    let fd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap()
        .into_raw_fd();

    let mut ev = Loop::new().unwrap();
    let mut file = File::new(fd).unwrap();
    let mut c_write = Completion::new();
    let mut c_read = Completion::new();

    let payload = b"hello libxev file".to_vec();
    let expected_len = payload.len();

    // Hold the write result so the read can run after.
    let write_result: Arc<Mutex<Option<std::io::Result<usize>>>> = Arc::new(Mutex::new(None));
    let read_result: Arc<Mutex<Option<std::io::Result<Vec<u8>>>>> = Arc::new(Mutex::new(None));

    {
        let write_result = Arc::clone(&write_result);
        file.write_owned(&mut ev, &mut c_write, payload, move |_lr, _cr, _buf, r| {
            *write_result.lock().unwrap() = Some(r);
            CbAction::Disarm
        });
    }

    ev.run(RunMode::UntilDone).unwrap();
    let n = write_result
        .lock()
        .unwrap()
        .take()
        .expect("write callback fired")
        .expect("write succeeded");
    assert_eq!(n, expected_len);

    // Now pread from offset 0 into a fresh buffer.
    {
        let read_result = Arc::clone(&read_result);
        let buf: Vec<u8> = Vec::with_capacity(expected_len);
        // pread via the raw API (no owned helper yet); reuse write_owned
        // style by inlining.
        let mut buf_holder = Some(buf);
        let read_result_inner = Arc::clone(&read_result);
        unsafe {
            let ptr = buf_holder.as_mut().unwrap().as_mut_ptr();
            let cap = buf_holder.as_ref().unwrap().capacity();
            file.pread_raw(&mut ev, &mut c_read, ptr, cap, 0, move |_lr, _cr, r| {
                let mut buf = buf_holder.take().unwrap();
                let r = r.map(|n| {
                    buf.set_len(n);
                    buf
                });
                *read_result_inner.lock().unwrap() = Some(r);
                CbAction::Disarm
            });
        }
    }

    ev.run(RunMode::UntilDone).unwrap();
    let got = read_result
        .lock()
        .unwrap()
        .take()
        .expect("read callback fired")
        .expect("read succeeded");
    assert_eq!(got, b"hello libxev file");

    // Clean up: drop the watcher (no-op on fd), then close fd via std.
    drop(file);
    // Reclaim the fd to drop it cleanly.
    let _ = unsafe { std::fs::File::from_raw_fd(fd) };
    let _ = std::fs::remove_file(&path);
}

#[test]
fn file_read_owned_returns_bytes() {
    let path = temp_path("read_owned.bin");
    let _ = std::fs::remove_file(&path);
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"abcdefgh").unwrap();
        f.sync_all().unwrap();
    }

    let fd = std::fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .unwrap()
        .into_raw_fd();

    let mut ev = Loop::new().unwrap();
    let mut file = File::new(fd).unwrap();
    let mut c = Completion::new();

    let got: Arc<Mutex<Option<std::io::Result<Vec<u8>>>>> = Arc::new(Mutex::new(None));
    {
        let got = Arc::clone(&got);
        let buf = Vec::with_capacity(16);
        file.read_owned(&mut ev, &mut c, buf, move |_lr, _cr, r| {
            *got.lock().unwrap() = Some(r);
            CbAction::Disarm
        });
    }

    ev.run(RunMode::UntilDone).unwrap();
    let bytes = got
        .lock()
        .unwrap()
        .take()
        .expect("callback fired")
        .expect("read ok");
    assert_eq!(&bytes, b"abcdefgh");

    drop(file);
    let _ = unsafe { std::fs::File::from_raw_fd(fd) };
    let _ = std::fs::remove_file(&path);
}

#[test]
fn file_pwrite_then_sync_read() {
    let path = temp_path("pwrite.bin");
    let _ = std::fs::remove_file(&path);
    let fd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap()
        .into_raw_fd();

    let mut ev = Loop::new().unwrap();
    let mut file = File::new(fd).unwrap();
    let mut c = Completion::new();

    let payload: &'static [u8] = b"OFFSET";
    let done: Arc<Mutex<Option<std::io::Result<usize>>>> = Arc::new(Mutex::new(None));
    {
        let done = Arc::clone(&done);
        // SAFETY: payload is 'static.
        unsafe {
            file.pwrite_raw(
                &mut ev,
                &mut c,
                payload.as_ptr(),
                payload.len(),
                4,
                move |_lr, _cr, r| {
                    *done.lock().unwrap() = Some(r);
                    CbAction::Disarm
                },
            );
        }
    }

    ev.run(RunMode::UntilDone).unwrap();
    let n = done
        .lock()
        .unwrap()
        .take()
        .expect("cb fired")
        .expect("pwrite ok");
    assert_eq!(n, payload.len());

    // Verify with a synchronous read of the underlying file.
    drop(file);
    // Reclaim the fd to read it synchronously.
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.seek(SeekFrom::Start(0)).unwrap();
    let mut data = Vec::new();
    f.read_to_end(&mut data).unwrap();
    // The file's first 4 bytes were never written, so they should be
    // zero-padded (since we created+truncated) up to offset 4, then
    // OFFSET.
    assert_eq!(data.len(), 4 + payload.len());
    assert_eq!(&data[0..4], &[0, 0, 0, 0]);
    assert_eq!(&data[4..], payload);

    let _ = std::fs::remove_file(&path);
}
