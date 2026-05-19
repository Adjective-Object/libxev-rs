//! High-level, safe-ish Rust wrapper around [libxev](https://github.com/mitchellh/libxev).
//!
//! This is a scaffold; only a thin `Loop` type is implemented so far. The
//! generated low-level FFI is available via [`sys`].

pub use libxev_sys as sys;

mod threadpool;
pub use threadpool::{Batch, Config as ThreadPoolConfig, Task, ThreadPool};

mod watcher;
pub use watcher::{Async, CbAction, Completion, CompletionRef, CompletionState, LoopRef, Timer};

use std::io;
use std::mem::MaybeUninit;

/// An event loop. Wraps `xev_loop`.
pub struct Loop {
    raw: Box<sys::xev_loop>,
}

/// How [`Loop::run`] should iterate the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    NoWait,
    Once,
    UntilDone,
}

impl RunMode {
    fn to_raw(self) -> sys::xev_run_mode_t {
        match self {
            RunMode::NoWait => sys::xev_run_mode_t_XEV_RUN_NO_WAIT,
            RunMode::Once => sys::xev_run_mode_t_XEV_RUN_ONCE,
            RunMode::UntilDone => sys::xev_run_mode_t_XEV_RUN_UNTIL_DONE,
        }
    }
}

impl Loop {
    /// Initialize a new event loop.
    pub fn new() -> io::Result<Self> {
        // Allocate zeroed storage for the opaque loop struct.
        let mut raw: Box<MaybeUninit<sys::xev_loop>> = Box::new(MaybeUninit::zeroed());
        let rc = unsafe { sys::xev_loop_init(raw.as_mut_ptr()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        // Safety: xev_loop_init succeeded, the storage is now initialized.
        let raw: Box<sys::xev_loop> = unsafe { Box::from_raw(Box::into_raw(raw).cast()) };
        Ok(Self { raw })
    }

    /// Run the loop with the given mode.
    pub fn run(&mut self, mode: RunMode) -> io::Result<()> {
        let rc = unsafe { sys::xev_loop_run(&mut *self.raw, mode.to_raw()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        Ok(())
    }

    /// Cached monotonic time in milliseconds.
    pub fn now(&mut self) -> i64 {
        unsafe { sys::xev_loop_now(&mut *self.raw) }
    }

    /// Refresh the cached `now()` value from the OS clock.
    pub fn update_now(&mut self) {
        unsafe { sys::xev_loop_update_now(&mut *self.raw) };
    }

    /// Access the raw underlying `xev_loop` pointer.
    pub fn as_raw(&mut self) -> *mut sys::xev_loop {
        &mut *self.raw
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        unsafe { sys::xev_loop_deinit(&mut *self.raw) };
    }
}
