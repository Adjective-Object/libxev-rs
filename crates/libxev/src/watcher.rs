//! Safe(-ish) wrappers around libxev watchers: [`Timer`] and [`Async`].
//!
//! Both watchers operate via [`Completion`]s, which carry the user's
//! Rust callback to the C side. A `Completion` owns the heap-allocated
//! callback for the duration of the operation; reassigning a new operation
//! to the same completion (or dropping it) frees the previous callback.

use std::ffi::c_void;
use std::io;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::os::raw::c_int;

use libxev_sys as sys;

use crate::Loop;

/// What to do after a watcher callback fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CbAction {
    /// Stop monitoring; the completion goes `Dead`.
    Disarm,
    /// Keep monitoring with the same parameters.
    Rearm,
}

impl CbAction {
    fn to_raw(self) -> sys::xev_cb_action {
        match self {
            CbAction::Disarm => sys::xev_cb_action_XEV_DISARM,
            CbAction::Rearm => sys::xev_cb_action_XEV_REARM,
        }
    }
}

/// Whether a completion is currently registered with a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionState {
    Dead,
    Active,
}

impl CompletionState {
    fn from_raw(v: sys::xev_completion_state_t) -> Self {
        if v == sys::xev_completion_state_t_XEV_COMPLETION_ACTIVE {
            CompletionState::Active
        } else {
            CompletionState::Dead
        }
    }
}

/// A non-owning reference to the [`Loop`] handed to watcher callbacks.
///
/// Inside a callback the loop is already mutably borrowed by [`Loop::run`],
/// so we expose a separate handle that lets you call back into a subset
/// of `Loop` operations (time queries, raw pointer escape hatch) without
/// reborrowing the owned `Loop`.
pub struct LoopRef<'a> {
    raw: *mut sys::xev_loop,
    _marker: PhantomData<&'a mut Loop>,
}

impl<'a> LoopRef<'a> {
    /// Cached monotonic time in milliseconds.
    pub fn now(&self) -> i64 {
        unsafe { sys::xev_loop_now(self.raw) }
    }

    /// Refresh the cached `now` value.
    pub fn update_now(&mut self) {
        unsafe { sys::xev_loop_update_now(self.raw) }
    }

    /// Raw pointer for FFI use.
    pub fn as_raw(&mut self) -> *mut sys::xev_loop {
        self.raw
    }
}

/// A non-owning reference to a [`Completion`] handed to watcher callbacks.
pub struct CompletionRef<'a> {
    raw: *mut sys::xev_completion,
    _marker: PhantomData<&'a mut Completion>,
}

impl<'a> CompletionRef<'a> {
    pub fn state(&mut self) -> CompletionState {
        CompletionState::from_raw(unsafe { sys::xev_completion_state(self.raw) })
    }

    pub fn as_raw(&mut self) -> *mut sys::xev_completion {
        self.raw
    }
}

/// Type-erased ownership handle for the boxed callback that backs an
/// active operation on a [`Completion`]. Dropping frees the callback.
struct CallbackOwner {
    ptr: *mut (),
    drop_fn: unsafe fn(*mut ()),
}

impl Drop for CallbackOwner {
    fn drop(&mut self) {
        unsafe { (self.drop_fn)(self.ptr) };
    }
}

/// A heap-allocated `xev_completion`. Required by every watcher operation.
///
/// A completion is *active* between the call that armed it (e.g.
/// [`Timer::run`]) and the moment its callback returns [`CbAction::Disarm`].
/// You must keep the `Completion` alive across that window; dropping it
/// while it is still registered with a [`Loop`] is undefined behavior
/// (this is unchecked).
pub struct Completion {
    raw: Box<sys::xev_completion>,
    callback: Option<CallbackOwner>,
}

// A Completion is tied to whichever loop owns it; it is safe to send
// across threads as long as the loop is not running, but Sync would be
// unsound. Mark `!Sync` implicitly by holding a raw pointer.
unsafe impl Send for Completion {}

impl Completion {
    /// Allocate a new, zeroed completion.
    pub fn new() -> Self {
        // Zero is a valid initial state for xev_completion (matches
        // xev_completion_zero behavior on a fresh allocation).
        let raw: Box<MaybeUninit<sys::xev_completion>> = Box::new(MaybeUninit::zeroed());
        let raw: Box<sys::xev_completion> = unsafe { Box::from_raw(Box::into_raw(raw).cast()) };
        Self {
            raw,
            callback: None,
        }
    }

    /// Reset the completion to a `Dead` state and drop any pending callback.
    pub fn zero(&mut self) {
        unsafe { sys::xev_completion_zero(&mut *self.raw) };
        self.callback = None;
    }

    pub fn state(&mut self) -> CompletionState {
        CompletionState::from_raw(unsafe { sys::xev_completion_state(&mut *self.raw) })
    }

    /// Raw pointer for FFI use.
    pub fn as_raw(&mut self) -> *mut sys::xev_completion {
        &mut *self.raw
    }

    /// Install a new callback, dropping any previously installed one.
    /// Returns the thin userdata pointer to hand to libxev.
    fn install_callback<F>(&mut self, cb: F) -> *mut c_void
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send + 'static,
    {
        type DynCb = dyn FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send;
        let inner: Box<DynCb> = Box::new(cb);
        // Double-box so userdata is a thin pointer.
        let outer: Box<Box<DynCb>> = Box::new(inner);
        let ptr = Box::into_raw(outer) as *mut ();
        self.callback = Some(CallbackOwner {
            ptr,
            drop_fn: drop_dyn_cb,
        });
        ptr as *mut c_void
    }
}

impl Default for Completion {
    fn default() -> Self {
        Self::new()
    }
}

type DynCb = dyn FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send;

unsafe fn drop_dyn_cb(ptr: *mut ()) {
    let _ = unsafe { Box::from_raw(ptr as *mut Box<DynCb>) };
}

/// Common trampoline used by both timer and async callbacks.
unsafe extern "C" fn trampoline(
    l: *mut sys::xev_loop,
    c: *mut sys::xev_completion,
    result: c_int,
    userdata: *mut c_void,
) -> sys::xev_cb_action {
    let cb = unsafe { &mut *(userdata as *mut Box<DynCb>) };
    let mut lr = LoopRef {
        raw: l,
        _marker: PhantomData,
    };
    let mut cr = CompletionRef {
        raw: c,
        _marker: PhantomData,
    };
    cb(&mut lr, &mut cr, result).to_raw()
}

// ----------------------------------------------------------------------
// Timer
// ----------------------------------------------------------------------

/// A timer watcher. Wraps `xev_watcher` configured for timers.
pub struct Timer {
    raw: Box<sys::xev_watcher>,
}

impl Timer {
    pub fn new() -> io::Result<Self> {
        let mut raw: Box<MaybeUninit<sys::xev_watcher>> = Box::new(MaybeUninit::zeroed());
        let rc = unsafe { sys::xev_timer_init(raw.as_mut_ptr()) };
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

    /// Arm the timer to fire `next_ms` milliseconds from now (relative).
    pub fn run<F>(&mut self, ev: &mut Loop, c: &mut Completion, next_ms: u64, cb: F)
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send + 'static,
    {
        let userdata = c.install_callback(cb);
        unsafe {
            sys::xev_timer_run(
                &mut *self.raw,
                ev.as_raw(),
                &mut *c.raw,
                next_ms,
                userdata,
                Some(trampoline),
            );
        }
    }

    /// Reset the timer. `c` is the live timer completion (must currently
    /// be `Active`); `c_cancel` is a separate completion used to cancel
    /// the in-flight operation.
    pub fn reset<F>(
        &mut self,
        ev: &mut Loop,
        c: &mut Completion,
        c_cancel: &mut Completion,
        next_ms: u64,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send + 'static,
    {
        let userdata = c.install_callback(cb);
        unsafe {
            sys::xev_timer_reset(
                &mut *self.raw,
                ev.as_raw(),
                &mut *c.raw,
                &mut *c_cancel.raw,
                next_ms,
                userdata,
                Some(trampoline),
            );
        }
    }

    /// Cancel the timer associated with `c_timer`. `c_cancel` is the
    /// completion that will fire to acknowledge the cancellation; `cb`
    /// is invoked on that cancellation.
    pub fn cancel<F>(
        &mut self,
        ev: &mut Loop,
        c_timer: &mut Completion,
        c_cancel: &mut Completion,
        cb: F,
    ) where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send + 'static,
    {
        let userdata = c_cancel.install_callback(cb);
        unsafe {
            sys::xev_timer_cancel(
                &mut *self.raw,
                ev.as_raw(),
                &mut *c_timer.raw,
                &mut *c_cancel.raw,
                userdata,
                Some(trampoline),
            );
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        unsafe { sys::xev_timer_deinit(&mut *self.raw) };
    }
}

// ----------------------------------------------------------------------
// Async
// ----------------------------------------------------------------------

/// An async notifier. Wraps `xev_watcher` configured for async.
///
/// `notify()` is safe to call from any thread to wake a [`Loop`] that is
/// blocked on a corresponding `wait()` registration.
pub struct Async {
    raw: Box<sys::xev_watcher>,
}

// `xev_async_notify` is documented to be safe from any thread.
unsafe impl Send for Async {}
unsafe impl Sync for Async {}

impl Async {
    pub fn new() -> io::Result<Self> {
        let mut raw: Box<MaybeUninit<sys::xev_watcher>> = Box::new(MaybeUninit::zeroed());
        let rc = unsafe { sys::xev_async_init(raw.as_mut_ptr()) };
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

    /// Wake any pending `wait()` on this notifier.
    pub fn notify(&self) -> io::Result<()> {
        // Cast away the const of &self for FFI; xev_async_notify is
        // documented to be thread-safe.
        let p = (&*self.raw as *const sys::xev_watcher) as *mut sys::xev_watcher;
        let rc = unsafe { sys::xev_async_notify(p) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        Ok(())
    }

    /// Register interest in async notifications.
    pub fn wait<F>(&mut self, ev: &mut Loop, c: &mut Completion, cb: F)
    where
        F: FnMut(&mut LoopRef<'_>, &mut CompletionRef<'_>, c_int) -> CbAction + Send + 'static,
    {
        let userdata = c.install_callback(cb);
        unsafe {
            sys::xev_async_wait(
                &mut *self.raw,
                ev.as_raw(),
                &mut *c.raw,
                userdata,
                Some(trampoline),
            );
        }
    }
}

impl Drop for Async {
    fn drop(&mut self) {
        unsafe { sys::xev_async_deinit(&mut *self.raw) };
    }
}
