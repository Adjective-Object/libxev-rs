//! Extension trait that adds extended-API-specific callback shapes to
//! [`Completion`].
//!
//! The base [`Completion`] in the watcher module only knows about
//! `c_int`-result callbacks (timer/async). The extended C API adds
//! file I/O operations whose results are `isize` (bytes transferred,
//! or `-errno`). Rather than gating those bits inside the core type,
//! they live here so the core stays feature-agnostic.

use std::ffi::c_void;

use libxev_sys as sys;

use crate::watcher::{CbAction, Completion, CompletionRef, LoopRef};

type DynCbIsize = dyn FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, isize) -> CbAction + Send;

unsafe fn drop_dyn_cb_isize(ptr: *mut ()) {
    let _ = unsafe { Box::from_raw(ptr as *mut Box<DynCbIsize>) };
}

/// Trampoline for read/write style callbacks whose result is `isize`.
pub(crate) unsafe extern "C" fn trampoline_isize(
    l: *mut sys::xev_loop,
    c: *mut sys::xev_completion,
    result: isize,
    userdata: *mut c_void,
) -> sys::xev_cb_action {
    let cb = unsafe { &mut *(userdata as *mut Box<DynCbIsize>) };
    let mut lr = unsafe { LoopRef::from_raw(l) };
    let mut cr = unsafe { CompletionRef::from_raw(c) };
    cb(&mut lr, &mut cr, result).to_raw()
}

/// Extension methods on [`Completion`] enabled by the `extended-api` feature.
pub trait CompletionExt {
    /// Install an `isize`-result callback (used by File read/write ops
    /// where the value encodes either bytes transferred (>= 0) or
    /// `-errno` (< 0)). Drops any previously installed callback.
    /// Returns the thin userdata pointer to hand to libxev.
    fn install_callback_isize<F>(&mut self, cb: F) -> *mut c_void
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, isize) -> CbAction + Send + 'static;
}

impl CompletionExt for Completion {
    fn install_callback_isize<F>(&mut self, cb: F) -> *mut c_void
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, isize) -> CbAction + Send + 'static,
    {
        let inner: Box<DynCbIsize> = Box::new(cb);
        // Double-box so userdata is a thin pointer.
        let outer: Box<Box<DynCbIsize>> = Box::new(inner);
        let ptr = Box::into_raw(outer) as *mut ();
        self.set_callback_owner(ptr, drop_dyn_cb_isize);
        ptr as *mut c_void
    }
}
