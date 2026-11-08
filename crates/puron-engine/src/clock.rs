//! The clock that every engine thread waits on.
//!
//! Every wait in the engine (the poll timer, read deadlines, the shutdown
//! wait, and each wait for a command, a worker result or a snapshot) is a
//! call to `Clock::wait_until`. Production uses `SystemClock`. Tests use a
//! virtual clock instead, which advances only when every engine thread is
//! waiting on it, so that a test controls time exactly.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A monotonic clock, and the only way an engine thread blocks.
pub trait Clock: Send + Sync {
    /// The time elapsed since the clock was created.
    fn now(&self) -> Duration;
    /// Blocks until `ready` returns true, and then returns true; or until
    /// the clock reaches `deadline`, and then returns false. With no
    /// deadline it waits only for `ready`.
    ///
    /// `ready` runs while the clock's own lock is held. Between checks the
    /// thread sleeps until a `wake` or the deadline, so any thread that
    /// changes what `ready` looks at must call `wake` afterwards, or the
    /// change may go unnoticed. The caller must not hold a lock that
    /// `ready` takes, or the two locks could deadlock.
    fn wait_until(&self, deadline: Option<Duration>, ready: &mut dyn FnMut() -> bool) -> bool;
    /// Makes every thread blocked in `wait_until` check its `ready` again.
    fn wake(&self);
    /// Called by `spawn` on the parent thread, before the new engine
    /// thread starts. These three hooks let a test clock count the engine
    /// threads, so that it knows when all of them are waiting. They do
    /// nothing in `SystemClock`.
    fn spawning(&self) {}
    /// Called by `spawn` as the first thing on the new thread.
    fn entered(&self) {}
    /// Called by `spawn` as the last thing on the new thread, after its
    /// body has returned.
    fn exited(&self) {}
}

/// Starts an engine thread named `name` that runs `body`, and calls the
/// clock's `spawning`, `entered` and `exited` hooks around it. Every
/// engine thread must be started this way. It panics if the thread
/// cannot be created.
pub fn spawn<F>(clock: &Arc<dyn Clock>, name: String, body: F)
where
    F: FnOnce() + Send + 'static,
{
    clock.spawning();
    let c = Arc::clone(clock);
    std::thread::Builder::new()
        .name(name)
        .spawn(move || {
            c.entered();
            body();
            c.exited();
        })
        .expect("spawn an engine thread");
}

/// The clock used in production: the system's monotonic clock, with a
/// condition variable for the waits.
pub struct SystemClock {
    origin: Instant,
    lock: Mutex<()>,
    cv: Condvar,
}

impl SystemClock {
    // Every user holds the clock as a shared `dyn Clock`, so `new` returns
    // one instead of `Self`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Arc<dyn Clock> {
        Arc::new(SystemClock {
            origin: Instant::now(),
            lock: Mutex::new(()),
            cv: Condvar::new(),
        })
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn wait_until(&self, deadline: Option<Duration>, ready: &mut dyn FnMut() -> bool) -> bool {
        let mut g = self.lock.lock().unwrap();
        loop {
            if ready() {
                return true;
            }
            match deadline {
                None => g = self.cv.wait(g).unwrap(),
                Some(d) => {
                    let now = self.now();
                    if now >= d {
                        return false;
                    }
                    g = self.cv.wait_timeout(g, d - now).unwrap().0;
                }
            }
        }
    }

    fn wake(&self) {
        let _g = self.lock.lock().unwrap();
        self.cv.notify_all();
    }
}
