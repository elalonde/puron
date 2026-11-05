//! The diag events `menu-rebuilt` and `display-updated`, with fixture
//! `p5-debug` (the defaults with pinned list P5 and `debug_logging`
//! true). In its own test binary for diag's socket.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_applet_app_20() {
    puron_testkit::limit("C-applet.app-20", 5, || {
        let j = Journal::new("c-applet-app-20");
        diag::set_socket(&j.path);
        let c = cfg::build("p5-debug");
        let mut a = app(&c.root, &handle(0));
        a.handle(Event::Start);
        assert!(a.settings().debug_logging);
        a.handle(Event::Snapshot(rebuild("S1")));
        a.handle(Event::Snapshot(rebuild("S1")));
        // A panel rebuild that changes only icons writes no event.
        a.handle(delivery(&a, &["show_panel_icons"], |s| {
            s.show_panel_icons = false
        }));
        a.handle(delivery(&a, &["show_temperature"], |s| {
            s.show_temperature = false
        }));
        a.handle(Event::Snapshot(rebuild("S1")));
        let got = j.wait(6, Duration::from_millis(500));
        let ids: Vec<_> = got
            .iter()
            .map(|e| jrn::field(e, "PURON_EVENT").unwrap_or("-"))
            .collect();
        assert_eq!(
            ids,
            [
                "menu-rebuilt",
                "display-updated",
                "display-updated",
                "menu-rebuilt",
                "display-updated"
            ]
        );
    });
}
