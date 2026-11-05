//! Start with an empty config (both kinds of root: no entry, and no ID
//! directory), then one of each event: no datagram in the 0.5 s quiet
//! window. In its own test binary, since diag's switch and socket are per
//! process.

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_diag_1() {
    puron_testkit::limit("C-diag-1", 5, || {
        let j = Journal::new("c-diag-1");
        diag::set_socket(&j.path);
        for name in ["empty-none", "empty-cosmic"] {
            let c = cfg::build(name);
            let mut a = app(&c.root, &handle(0));
            a.handle(Event::Start);
            jrn::one_of_each(diag::event);
        }
        let got = j.wait(1, Duration::from_millis(500));
        assert_eq!(got.len(), 0, "{}", jrn::dump(&got));
    });
}
