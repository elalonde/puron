//! Diagnostics: sending an event never blocks the caller.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use std::time::{Duration, Instant};

use puron_engine::diag::{self, engine};
use puron_testkit::jrn;

/// Points diagnostics at a socket with the smallest receive buffer and
/// no reader, which is full after the first few events, and sends 10 000
/// `panic` events from one thread. Expects all the sends together to
/// take less than 1 s: once the buffer is full, each send drops its
/// event instead of waiting.
#[test]
fn c_diag_7() {
    puron_testkit::limit("C-diag-7", 5, || {
        let (path, _sock) = jrn::full("c-diag-7");
        diag::set_socket(&path);
        diag::switch(true);
        let t = Instant::now();
        for _ in 0..10_000 {
            diag::event(engine::panic("lm-sensors", "read", "boom"));
        }
        let took = t.elapsed();
        assert!(took < Duration::from_secs(1), "took {took:?}");
    });
}
