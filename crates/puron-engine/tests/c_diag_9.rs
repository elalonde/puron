//! Diagnostics: each event is sent whole, as one datagram, while many
//! threads send at once.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use std::time::Duration;

use puron_engine::diag::{self, engine};
use puron_testkit::jrn::{self, Journal};

/// The fields of an engine `read-deadline` entry, in the order they are
/// written.
const NAMES: [&str; 6] = [
    "MESSAGE",
    "PRIORITY",
    "SYSLOG_IDENTIFIER",
    "PURON_SOURCE",
    "PURON_EVENT",
    "PURON_PROVIDER",
];

/// Starts 8 threads that each send 500 `read-deadline` events to a test
/// journal socket; the provider name of each event is `t<thread>-<n>`,
/// with `n` counting up from 0. Expects every datagram received to be
/// one whole entry with the fields of `NAMES` in order and a `MESSAGE`
/// that names its provider, and the entries of each thread to arrive in
/// the order they were sent. A full socket buffer may drop events, so
/// the test checks neither the total nor that every thread is seen; it
/// does require at least one datagram, since otherwise nothing would be
/// checked.
#[test]
fn c_diag_9() {
    puron_testkit::limit("C-diag-9", 5, || {
        let j = Journal::new("c-diag-9");
        diag::set_socket(&j.path);
        diag::switch(true);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    for n in 0..500 {
                        diag::event(engine::read_deadline(&format!("t{t}-{n}")));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        // Every thread has finished sending. Take what has arrived once
        // all 4000 are in, or after 1 s, since some may have been dropped.
        let raw = j.wait_raw(4000, Duration::from_secs(1));
        assert!(!raw.is_empty());
        let mut last = [-1i64; 8];
        for d in &raw {
            let e = jrn::parse(d).expect("one whole entry");
            let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, NAMES);
            assert_eq!(jrn::field(&e, "PURON_EVENT"), Some("read-deadline"));
            let p = jrn::field(&e, "PURON_PROVIDER").unwrap();
            let (t, n) = p[1..].split_once('-').unwrap();
            let (t, n): (usize, i64) = (t.parse().unwrap(), n.parse().unwrap());
            assert!(n > last[t], "thread {t}: {n} after {}", last[t]);
            last[t] = n;
            let m = jrn::field(&e, "MESSAGE").unwrap();
            assert_eq!(m, format!("engine: read deadline passed; provider {p}"));
        }
    });
}
