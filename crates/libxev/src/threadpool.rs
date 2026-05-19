//! Safe-ish wrapper around `xev_threadpool`.
//!
//! Tasks are submitted as [`Task`] values via a [`Batch`]. Each task owns a
//! Rust closure that the threadpool worker invokes. The closure has access to
//! a user-supplied piece of `Send + 'static` state, which keeps the C-side
//! "embed your data after the task struct" pattern hidden from callers.
//!
//! ## Memory ownership
//!
//! Each [`Task`] is heap-allocated. Once scheduled, the pool takes ownership
//! and the task box is freed inside the worker callback after the closure
//! runs. Callers therefore never observe the task again after scheduling.

use std::ffi::c_void;
use std::ptr;

use libxev_sys as sys;

/// Configuration for a [`ThreadPool`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Config {
    /// Stack size for worker threads. `None` uses libxev's default.
    pub stack_size: Option<u32>,
    /// Maximum number of worker threads. `None` uses libxev's default.
    pub max_threads: Option<u32>,
}

/// A libxev thread pool. Wraps `xev_threadpool`.
///
/// On macOS this is the backing store libxev's higher-level `File` watcher
/// uses for `pread`/`pwrite`. Exposing it directly gives callers an
/// efficient way to parallelize blocking I/O without spawning OS threads
/// per work item.
pub struct ThreadPool {
    raw: Box<sys::xev_threadpool>,
}

unsafe impl Send for ThreadPool {}
unsafe impl Sync for ThreadPool {}

impl ThreadPool {
    /// Initialize a new thread pool with the given configuration.
    pub fn new(config: Config) -> std::io::Result<Self> {
        let mut raw: Box<std::mem::MaybeUninit<sys::xev_threadpool>> =
            Box::new(std::mem::MaybeUninit::zeroed());

        let mut cfg: sys::xev_threadpool_config =
            unsafe { std::mem::zeroed::<sys::xev_threadpool_config>() };
        unsafe { sys::xev_threadpool_config_init(&mut cfg) };
        if let Some(v) = config.stack_size {
            unsafe { sys::xev_threadpool_config_set_stack_size(&mut cfg, v) };
        }
        if let Some(v) = config.max_threads {
            unsafe { sys::xev_threadpool_config_set_max_threads(&mut cfg, v) };
        }

        let rc = unsafe { sys::xev_threadpool_init(raw.as_mut_ptr(), &mut cfg) };
        if rc != 0 {
            return Err(std::io::Error::from_raw_os_error(rc));
        }
        let raw: Box<sys::xev_threadpool> = unsafe { Box::from_raw(Box::into_raw(raw).cast()) };
        Ok(Self { raw })
    }

    /// Schedule a [`Batch`] of tasks onto the pool. After this call the
    /// batch is empty and can be reused or dropped.
    pub fn schedule(&self, batch: &mut Batch) {
        // SAFETY: `xev_threadpool_schedule` is thread-safe per libxev's
        // contract; the C signature only takes `*mut` for ABI reasons.
        let raw = &*self.raw as *const sys::xev_threadpool as *mut sys::xev_threadpool;
        unsafe { sys::xev_threadpool_schedule(raw, &mut batch.raw) };
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        unsafe {
            sys::xev_threadpool_shutdown(&mut *self.raw);
            sys::xev_threadpool_deinit(&mut *self.raw);
        }
    }
}

/// A unit of work submitted to a [`ThreadPool`]. Construct with
/// [`Task::new`] and add to a [`Batch`] before scheduling.
///
/// The task struct is heap-allocated and freed inside the worker callback
/// after `closure` runs.
pub struct Task {
    /// The boxed inner state. We hand a raw pointer to libxev, which calls
    /// our trampoline with a pointer to the `pool_task` field. The
    /// trampoline reconstructs the [`Box<Inner>`] via `container_of`-style
    /// pointer arithmetic and frees it after invoking the closure.
    inner: *mut Inner,
}

unsafe impl Send for Task {}

#[repr(C)]
struct Inner {
    /// MUST be the first field — the trampoline reconstructs an `Inner`
    /// pointer from a `pool_task` pointer by simple cast.
    pool_task: sys::xev_threadpool_task,
    closure: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl Task {
    /// Create a new task that runs `closure` on a worker thread.
    pub fn new<F>(closure: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        let boxed: Box<dyn FnOnce() + Send + 'static> = Box::new(closure);
        let inner = Box::new(Inner {
            pool_task: unsafe { std::mem::zeroed() },
            closure: Some(boxed),
        });
        let inner_ptr = Box::into_raw(inner);
        unsafe {
            sys::xev_threadpool_task_init(
                ptr::addr_of_mut!((*inner_ptr).pool_task),
                Some(trampoline),
            );
        }
        Task { inner: inner_ptr }
    }
}

unsafe extern "C" fn trampoline(t: *mut sys::xev_threadpool_task) {
    // `Inner` is `#[repr(C)]` with `pool_task` as its first field, so the
    // `Inner` pointer equals the `pool_task` pointer.
    let inner_ptr = t as *mut Inner;
    // SAFETY: trampoline runs exactly once per task, and the Inner pointer
    // came from `Box::into_raw`.
    let mut boxed = unsafe { Box::from_raw(inner_ptr) };
    if let Some(closure) = boxed.closure.take() {
        // Run the closure; the Box is freed when `boxed` drops.
        closure();
    }
}

/// A batch of [`Task`]s. Use [`Batch::push`] to add tasks, then submit via
/// [`ThreadPool::schedule`].
pub struct Batch {
    raw: sys::xev_threadpool_batch,
}

impl Batch {
    /// Create an empty batch.
    pub fn new() -> Self {
        let mut raw: sys::xev_threadpool_batch = unsafe { std::mem::zeroed() };
        unsafe { sys::xev_threadpool_batch_init(&mut raw) };
        Batch { raw }
    }

    /// Append a [`Task`] to this batch. The batch takes ownership of the
    /// task pointer; the task will be freed by the worker callback.
    pub fn push(&mut self, task: Task) {
        unsafe {
            sys::xev_threadpool_batch_push_task(
                &mut self.raw,
                ptr::addr_of_mut!((*task.inner).pool_task),
            );
        }
        // Prevent the Task destructor (if any) from running; the inner
        // allocation is now owned by the threadpool.
        let _ = task.inner;
        std::mem::forget(task);
    }
}

impl Default for Batch {
    fn default() -> Self {
        Self::new()
    }
}

// Silence unused-import on `c_void` if the trampoline ever changes shape.
#[allow(dead_code)]
fn _unused_cvoid(_: *mut c_void) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn run_tasks_to_completion() {
        let pool = ThreadPool::new(Config::default()).expect("init pool");
        let counter = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let mut batch = Batch::new();
        for _ in 0..32 {
            let counter = Arc::clone(&counter);
            let tx = tx.clone();
            batch.push(Task::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            }));
        }
        drop(tx);
        pool.schedule(&mut batch);

        let mut got = 0;
        while rx.recv().is_ok() {
            got += 1;
        }
        assert_eq!(got, 32);
        assert_eq!(counter.load(Ordering::SeqCst), 32);
    }
}
