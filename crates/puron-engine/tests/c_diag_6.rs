//! Value forms (the binary form for a value with a newline) and the
//! 1024-byte cap at a character boundary, on a test journal socket.
//! Own test binary: diag's switch and socket are per process.

use std::time::Duration;

use puron_engine::diag::{self, lmsensors};
use puron_testkit::jrn::{self, Journal};

const NAMES: [&str; 8] = [
    "MESSAGE",
    "PRIORITY",
    "SYSLOG_IDENTIFIER",
    "PURON_SOURCE",
    "PURON_EVENT",
    "PURON_CHIP",
    "PURON_SUBFEATURE",
    "PURON_TEXT",
];

#[test]
fn c_diag_6() {
    puron_testkit::limit("C-diag-6", 5, || {
        let j = Journal::new("c-diag-6");
        diag::set_socket(&j.path);
        diag::switch(true);
        let long = "x".repeat(1023) + "é";
        let longer = "y".repeat(5000);
        for text in ["a\nb", long.as_str(), longer.as_str()] {
            diag::event(lmsensors::value_failed("chip0", "temp1_input", text));
        }
        let raw = j.wait_raw(3, Duration::from_secs(2));
        assert_eq!(raw.len(), 3);
        let e: Vec<jrn::Entry> = raw.iter().map(|d| jrn::parse(d).unwrap()).collect();
        for x in &e {
            let names: Vec<&str> = x.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, NAMES);
        }
        assert!(
            raw[0].windows(11).any(|w| w == b"PURON_TEXT\n"),
            "binary form"
        );
        assert_eq!(jrn::field(&e[0], "PURON_TEXT"), Some("a\nb"));
        assert_eq!(
            jrn::field(&e[1], "PURON_TEXT"),
            Some("x".repeat(1023).as_str())
        );
        assert_eq!(
            jrn::field(&e[2], "PURON_TEXT"),
            Some("y".repeat(1024).as_str())
        );
    });
}
