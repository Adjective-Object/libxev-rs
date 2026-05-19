//! Safe wrappers for the `xev.File` watcher (libxev fork extended API).
//!
//! A [`File`] borrows an existing OS file descriptor; it never closes the
//! fd implicitly on drop. To close asynchronously through the loop, call
//! [`File::close`]. Dropping a `File` is a no-op on the fd (matches
//! `xev.File.deinit`).
//!
//! Read/write/pread/pwrite operations borrow the user buffer by raw
//! pointer for the duration of the operation. The caller must keep the
//! buffer alive until the callback fires; in safe usage this is achieved
//! by moving ownership of the buffer into the closure.

use std::io;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_void};

use libxev_sys as sys;

use crate::Loop;
use crate::watcher::{CbAction, Completion, CompletionRef, LoopRef, trampoline};

use super::completion::{CompletionExt, trampoline_isize};

/// A file watcher. Wraps `xev_watcher` configured for files.
///
/// The underlying file descriptor is borrowed; closing it (via
/// [`File::close`] or externally) is the caller's responsibility.
pub struct File {
    raw: Box<sys::xev_watcher>,
}

// xev.File holds only a file descriptor; cross-thread move is safe as
// long as the fd itself is used in a thread-safe manner.
unsafe impl Send for File {}

/// Decode the `isize` result libxev's File read/write callbacks produce:
/// `>= 0` is bytes transferred, `< 0` is `-errno`.
fn decode_rw(result: isize) -> io::Result<usize> {
    if result >= 0 {
        Ok(result as usize)
    } else {
        Err(io::Error::from_raw_os_error((-result) as i32))
    }
}

impl File {
    /// Wrap an existing raw file descriptor. The fd is not duplicated
    /// and not closed on drop.
    pub fn new(fd: c_int) -> io::Result<Self> {
        let mut raw: Box<MaybeUninit<sys::xev_watcher>> = Box::new(MaybeUninit::zeroed());
        // The C ABI is `uintptr_t` so a Windows HANDLE can be passed
        // without truncation. Sign-extend the POSIX fd through isize so
        // sentinel values like -1 round-trip correctly.
        let fd_word = fd as isize as usize;
        let rc = unsafe { sys::xev_file_init(raw.as_mut_ptr(), fd_word) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        let raw: Box<sys::xev_watcher> = unsafe { Box::from_raw(Box::into_raw(raw).cast()) };
        Ok(Self { raw })
    }

    /// Raw pointer for FFI use.
    pub fn as_raw(&mut self) -> *mut sys::xev_watcher {
        &mut *self.raw
    }

    /// Asynchronously close the underlying file descriptor.
    pub fn close<F>(&mut self, ev: &mut Loop, c: &mut Completion, cb: F)
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<()>) -> CbAction
            + Send
            + 'static,
    {
        let mut cb = cb;
        let wrapped = move |lr: &mut LoopRef<'_>, cr: &mut CompletionRef<'_>, result: c_int| {
            let r = if result == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(result))
            };
            cb(lr, cr, r)
        };
        let userdata = c.install_callback(wrapped);
        unsafe {
            sys::xev_file_close(
                self.as_raw(),
                ev.as_raw(),
                c.as_raw(),
                userdata,
                Some(trampoline),
            );
        }
    }

    /// Read into `buf`. The buffer must outlive the operation; this is
    /// safe because `cb` takes ownership of any value that owns it via
    /// `move`.
    ///
    /// # Safety
    ///
    /// `buf` and `len` must point to a valid, unique, writable region of
    /// memory that lives until `cb` is invoked. In practice, the easiest
    /// way to satisfy this is to keep the buffer alive in a value that
    /// the closure captures by move (e.g. a `Vec<u8>`), then use
    /// [`File::read_owned`] which encapsulates that pattern.
    pub unsafe fn read_raw<F>(
        &mut self,
        ev: &mut Loop,
        c: &mut Completion,
        buf: *mut u8,
        len: usize,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<usize>) -> CbAction
            + Send
            + 'static,
    {
        let mut cb = cb;
        let wrapped = move |lr: &mut LoopRef<'_>, cr: &mut CompletionRef<'_>, result: isize| {
            cb(lr, cr, decode_rw(result))
        };
        let userdata = c.install_callback_isize(wrapped);
        unsafe {
            sys::xev_file_read(
                self.as_raw(),
                ev.as_raw(),
                c.as_raw(),
                buf as *mut c_void,
                len,
                userdata,
                Some(trampoline_isize),
            );
        }
    }

    /// Positional read at `offset`.
    ///
    /// # Safety
    ///
    /// See [`File::read_raw`].
    pub unsafe fn pread_raw<F>(
        &mut self,
        ev: &mut Loop,
        c: &mut Completion,
        buf: *mut u8,
        len: usize,
        offset: u64,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<usize>) -> CbAction
            + Send
            + 'static,
    {
        let mut cb = cb;
        let wrapped = move |lr: &mut LoopRef<'_>, cr: &mut CompletionRef<'_>, result: isize| {
            cb(lr, cr, decode_rw(result))
        };
        let userdata = c.install_callback_isize(wrapped);
        unsafe {
            sys::xev_file_pread(
                self.as_raw(),
                ev.as_raw(),
                c.as_raw(),
                buf as *mut c_void,
                len,
                offset,
                userdata,
                Some(trampoline_isize),
            );
        }
    }

    /// Write from `buf`.
    ///
    /// # Safety
    ///
    /// See [`File::read_raw`].
    pub unsafe fn write_raw<F>(
        &mut self,
        ev: &mut Loop,
        c: &mut Completion,
        buf: *const u8,
        len: usize,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<usize>) -> CbAction
            + Send
            + 'static,
    {
        let mut cb = cb;
        let wrapped = move |lr: &mut LoopRef<'_>, cr: &mut CompletionRef<'_>, result: isize| {
            cb(lr, cr, decode_rw(result))
        };
        let userdata = c.install_callback_isize(wrapped);
        unsafe {
            sys::xev_file_write(
                self.as_raw(),
                ev.as_raw(),
                c.as_raw(),
                buf as *const c_void,
                len,
                userdata,
                Some(trampoline_isize),
            );
        }
    }

    /// Positional write at `offset`.
    ///
    /// # Safety
    ///
    /// See [`File::read_raw`].
    pub unsafe fn pwrite_raw<F>(
        &mut self,
        ev: &mut Loop,
        c: &mut Completion,
        buf: *const u8,
        len: usize,
        offset: u64,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<usize>) -> CbAction
            + Send
            + 'static,
    {
        let mut cb = cb;
        let wrapped = move |lr: &mut LoopRef<'_>, cr: &mut CompletionRef<'_>, result: isize| {
            cb(lr, cr, decode_rw(result))
        };
        let userdata = c.install_callback_isize(wrapped);
        unsafe {
            sys::xev_file_pwrite(
                self.as_raw(),
                ev.as_raw(),
                c.as_raw(),
                buf as *const c_void,
                len,
                offset,
                userdata,
                Some(trampoline_isize),
            );
        }
    }

    /// Owned read helper: takes a `Vec<u8>` (using its `capacity()` as
    /// the maximum read length), performs an async read into it, and
    /// hands the buffer back to the callback with `len` set to the bytes
    /// actually read on success (unchanged on error).
    pub fn read_owned<F>(&mut self, ev: &mut Loop, c: &mut Completion, mut buf: Vec<u8>, cb: F)
    where
        F: FnOnce(&mut LoopRef<'_>, &mut CompletionRef<'_>, io::Result<Vec<u8>>) -> CbAction
            + Send
            + 'static,
    {
        let cap = buf.capacity();
        let ptr = buf.as_mut_ptr();
        // Move the buffer into the closure so it stays alive until the
        // callback fires.
        let mut buf_holder = Some(buf);
        let mut cb_holder = Some(cb);
        // SAFETY: `buf_holder` retains ownership of the allocation until
        // `wrapped` is invoked (exactly once); the pointer/len describe
        // that allocation. The closure is FnMut but will only run once
        // because we use `take()` on the inner FnOnce.
        unsafe {
            self.read_raw(ev, c, ptr, cap, move |lr, cr, r| {
                let cb = cb_holder.take().expect("read_owned callback fired twice");
                let mut buf = buf_holder.take().expect("read_owned buffer taken twice");
                let r = r.map(|n| {
                    // libxev wrote `n` initialized bytes into the
                    // first `n` bytes of `buf`'s allocation.
                    buf.set_len(n);
                    buf
                });
                cb(lr, cr, r)
            });
        }
    }

    /// Owned write helper: takes a `Vec<u8>` and hands it back to the
    /// callback alongside the result so the caller can recover the
    /// allocation.
    pub fn write_owned<F>(&mut self, ev: &mut Loop, c: &mut Completion, buf: Vec<u8>, cb: F)
    where
        F: FnOnce(&mut LoopRef<'_>, &mut CompletionRef<'_>, Vec<u8>, io::Result<usize>) -> CbAction
            + Send
            + 'static,
    {
        let ptr = buf.as_ptr();
        let len = buf.len();
        let mut buf_holder = Some(buf);
        let mut cb_holder = Some(cb);
        // SAFETY: `buf_holder` keeps the allocation alive until the
        // callback fires exactly once.
        unsafe {
            self.write_raw(ev, c, ptr, len, move |lr, cr, r| {
                let cb = cb_holder.take().expect("write_owned callback fired twice");
                let buf = buf_holder.take().expect("write_owned buffer taken twice");
                cb(lr, cr, buf, r)
            });
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe { sys::xev_file_deinit(&mut *self.raw) };
    }
}
