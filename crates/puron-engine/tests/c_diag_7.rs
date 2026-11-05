//! Sending never blocks: with a journal socket whose buffer is full,
//! 10 000 `panic` events from one thread return within 1 s in total.
//! Own test binary: diag's switch and socket are per process.

use std::time::{Duration, Instant};

use puron_engine::diag::{self, engine};
use puron_testkit::jrn;

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
