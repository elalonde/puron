//! One entry per event: 8 threads each send 500 `read-deadline` events
//! to a test journal socket. Every datagram received is one whole entry
//! of one event, and each thread's entries arrive in order. Entries may
//! be dropped on a full buffer, so neither the count nor an entry per
//! thread is checked; the receive must not be empty, or no datagram is
//! checked. Own test binary: diag's switch and socket are per process.

use std::time::Duration;

use puron_engine::diag::{self, engine};
use puron_testkit::jrn::{self, Journal};

const NAMES: [&str; 6] = [
    "MESSAGE",
    "PRIORITY",
    "SYSLOG_IDENTIFIER",
    "PURON_SOURCE",
    "PURON_EVENT",
    "PURON_PROVIDER",
];

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
        // Every send is done; wait until all 4000 or the reader is idle.
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
