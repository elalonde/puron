//! The engine's monotonic clock.
//!
//! Every wait of the engine (timer, read deadlines, shutdown wait, and
//! each wait for a command, report or snapshot) is a `wait_until` on
//! the clock, so that a virtual clock in tests moves only when every
//! thread of the engine waits on it.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A monotonic clock and the waits on it.
pub trait Clock: Send + Sync {
    /// Time since the clock's creation.
    fn now(&self) -> Duration;
    /// Blocks until `ready` returns true (it returns true) or the clock
    /// reaches `deadline` (it returns false). `ready` runs under the
    /// clock's lock; whoever changes what it reads calls `wake` after.
    /// The caller holds no lock that `ready` takes.
    fn wait_until(&self, deadline: Option<Duration>, ready: &mut dyn FnMut() -> bool) -> bool;
    /// Wakes every wait to check its `ready` again.
    fn wake(&self);
    /// On the spawning thread, before a thread of the engine starts.
    fn spawning(&self) {}
    /// On the new thread, first.
    fn entered(&self) {}
    /// On the thread, last.
    fn exited(&self) {}
}

/// Starts a thread of the engine, with the clock told of it.
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

/// The system monotonic clock (production).
pub struct SystemClock {
    origin: Instant,
    lock: Mutex<()>,
    cv: Condvar,
}

impl SystemClock {
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
