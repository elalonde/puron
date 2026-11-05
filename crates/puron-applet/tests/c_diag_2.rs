//! Start with a config where every key holds a non-default value, then
//! one of each event: 29 datagrams in the order of the event table. In
//! its own test binary, since diag's switch and socket are per process.

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_diag_2() {
    puron_testkit::limit("C-diag-2", 5, || {
        let j = Journal::new("c-diag-2");
        diag::set_socket(&j.path);
        let c = cfg::build("good");
        let mut a = app(&c.root, &handle(0));
        a.handle(Event::Start);
        let mut want = Vec::new();
        jrn::one_of_each(|e| want.push(e.id));
        jrn::one_of_each(diag::event);
        let got = j.wait(30, Duration::from_millis(500));
        let ids: Vec<_> = got
            .iter()
            .map(|e| jrn::field(e, "PURON_EVENT").unwrap_or("-"))
            .collect();
        assert_eq!(ids, want);
        assert_eq!(ids.len(), 29);
    });
}
