use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use libxev::{Async, CbAction, Completion, Loop, RunMode, Timer};

#[test]
fn loop_now_monotonic() {
    let mut ev = Loop::new().unwrap();
    let a = ev.now();
    ev.update_now();
    let b = ev.now();
    assert!(b >= a, "expected monotonic now(): {a} then {b}");
}

#[test]
fn timer_fires_once() {
    let mut ev = Loop::new().unwrap();
    let mut t = Timer::new().unwrap();
    let mut c = Completion::new();

    let count = Arc::new(AtomicUsize::new(0));
    let count_cb = Arc::clone(&count);
    t.run(&mut ev, &mut c, 10, move |_lr, _cr, result| {
        assert_eq!(result, 0, "timer callback got error {result}");
        count_cb.fetch_add(1, Ordering::SeqCst);
        CbAction::Disarm
    });

    let start = Instant::now();
    ev.run(RunMode::UntilDone).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn timer_rearm_counts() {
    let mut ev = Loop::new().unwrap();
    let mut t = Timer::new().unwrap();
    let mut c = Completion::new();

    let count = Arc::new(AtomicUsize::new(0));
    let count_cb = Arc::clone(&count);
    t.run(&mut ev, &mut c, 1, move |_lr, _cr, _result| {
        let n = count_cb.fetch_add(1, Ordering::SeqCst) + 1;
        if n < 3 {
            CbAction::Rearm
        } else {
            CbAction::Disarm
        }
    });

    ev.run(RunMode::UntilDone).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn async_notify_wakes_loop() {
    let mut ev = Loop::new().unwrap();
    let mut notifier = Async::new().unwrap();
    let mut c = Completion::new();

    let woken = Arc::new(AtomicUsize::new(0));
    let woken_cb = Arc::clone(&woken);

    // Capture a raw pointer to the underlying watcher so a worker thread
    // can call `xev_async_notify` while the main thread is blocked in
    // `ev.run`. We round-trip through `usize` so we don't need a Send
    // wrapper for `*mut`.
    // SAFETY: `notifier` lives until after `handle.join()` below.
    let ptr_addr = notifier.as_raw() as usize;

    notifier.wait(&mut ev, &mut c, move |_lr, _cr, _result| {
        woken_cb.fetch_add(1, Ordering::SeqCst);
        CbAction::Disarm
    });

    let handle = thread::spawn(move || {
        // Re-introduce the pointer inside the new thread by going through
        // an integer address, sidestepping `*mut` not being `Send`.
        let p = ptr_addr as *mut libxev::sys::xev_watcher;
        // Tiny sleep to ensure the loop is parked in `run`.
        thread::sleep(Duration::from_millis(50));
        let rc = unsafe { libxev::sys::xev_async_notify(p) };
        assert_eq!(rc, 0);
    });

    ev.run(RunMode::UntilDone).unwrap();
    handle.join().unwrap();
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}

#[test]
fn timer_cancel_fires_cancel_completion() {
    let mut ev = Loop::new().unwrap();
    let mut t = Timer::new().unwrap();
    let mut c_timer = Completion::new();
    let mut c_cancel = Completion::new();

    let timer_results = Arc::new(std::sync::Mutex::new(Vec::<i32>::new()));
    let cancel_fired = Arc::new(AtomicUsize::new(0));

    let tr = Arc::clone(&timer_results);
    t.run(&mut ev, &mut c_timer, 10_000, move |_lr, _cr, r| {
        tr.lock().unwrap().push(r);
        CbAction::Disarm
    });

    let cf = Arc::clone(&cancel_fired);
    t.cancel(&mut ev, &mut c_timer, &mut c_cancel, move |_lr, _cr, _r| {
        cf.fetch_add(1, Ordering::SeqCst);
        CbAction::Disarm
    });

    ev.run(RunMode::UntilDone).unwrap();
    // Cancellation invokes the original timer callback with an error result
    // (non-zero), and the cancel completion's callback fires exactly once.
    let results = timer_results.lock().unwrap().clone();
    assert_eq!(results.len(), 1, "timer cb should fire once with error");
    assert_ne!(
        results[0], 0,
        "timer cb result should be non-zero on cancel"
    );
    assert_eq!(cancel_fired.load(Ordering::SeqCst), 1);
}
