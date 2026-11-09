//! Diagnostics: how a field value is written into a journal entry.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use std::time::Duration;

use puron_engine::diag::{self, lmsensors};
use puron_testkit::jrn::{self, Journal};

/// The fields of an lm-sensors `value-failed` entry, in the order they
/// are written.
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

/// Sends three lm-sensors `value-failed` events whose texts are `a\nb`,
/// 1023 `x` followed by `é` (1025 bytes), and 5000 `y`. Expects three
/// entries, each with the fields of `NAMES` in that order. The first
/// entry writes its text in the binary form (`PURON_TEXT\n`, then the
/// length) because the text holds a newline, and the text reads back
/// unchanged. Longer texts are cut to at most 1024 bytes at a character
/// boundary: the `é`, which would cross the limit, is dropped whole, and
/// the `y` text keeps exactly 1024 bytes.
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
