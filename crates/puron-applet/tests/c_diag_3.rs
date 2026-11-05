//! An empty config; a delivery with `debug_logging` true, one `panic`; a
//! delivery with false, one `panic`: exactly one datagram. In its own
//! test binary, since diag's switch and socket are per process.

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_diag_3() {
    puron_testkit::limit("C-diag-3", 5, || {
        let j = Journal::new("c-diag-3");
        diag::set_socket(&j.path);
        let c = cfg::build("empty-none");
        let mut a = app(&c.root, &handle(0));
        a.handle(Event::Start);
        a.handle(delivery(&a, &["debug_logging"], |s| s.debug_logging = true));
        diag::event(diag::engine::panic("lm-sensors", "read", "first"));
        a.handle(delivery(&a, &["debug_logging"], |s| {
            s.debug_logging = false
        }));
        diag::event(diag::engine::panic("lm-sensors", "read", "second"));
        let got = j.wait(2, Duration::from_millis(500));
        assert_eq!(got.len(), 1, "{}", jrn::dump(&got));
        assert_eq!(jrn::field(&got[0], "PURON_TEXT"), Some("first"));
    });
}
