//! With an empty config, diagnostic logging stays off. The applet starts
//! on each of the two empty configs (the settings directory with no
//! entry, and no settings directory at all), and every diagnostic event
//! is then sent once: the fake journal must receive nothing within 0.5 s.
//! The test has its own binary because the diagnostics switch and socket
//! are process-wide.

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
