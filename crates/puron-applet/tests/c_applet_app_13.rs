//! A failed store of `pinned` on a read-only config root that holds the
//! defaults with pinned list P5 (fixture `p5-readonly`; the test runs as
//! a user who is not root). In its own test binary for diag's socket.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::{Event, Output};
use puron_config::StoreResult;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_applet_app_13() {
    puron_testkit::limit("C-applet.app-13", 5, || {
        puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
        let j = Journal::new("c-applet-app-13");
        diag::set_socket(&j.path);
        let c = cfg::build("p5-readonly");
        let h = handle(0);
        let mut a = app(&c.root, &h);
        a.handle(Event::Start);
        a.handle(Event::Snapshot(capture()));
        // The default config keeps debug logging off; the test turns diag
        // on to read the one event (diag decides what is written).
        diag::switch(true);
        let ids = a.menu().unwrap().identities.clone();
        let fan = ids
            .iter()
            .find(|i| i.starts_with("fan/") && i.contains("fan1"))
            .cloned()
            .unwrap();
        let wifi = pins("P5")[0].clone();
        let out = a.handle(Event::PinClick(fan.clone()));
        let mut three = pins("P5");
        three.push(fan.clone());
        assert!(
            out.contains(&Output::Store(three, StoreResult::Failed)),
            "{out:?}"
        );
        assert_eq!(panel_of(&out).unwrap().items.len(), 3);
        let got = j.wait(1, Duration::from_secs(2));
        assert_eq!(got.len(), 1);
        assert_eq!(
            jrn::field(&got[0], "PURON_EVENT"),
            Some("pinned-store-failed")
        );
        let out = a.handle(Event::PinClick(wifi));
        let stores: Vec<_> = out
            .iter()
            .filter(|o| matches!(o, Output::Store(..)))
            .collect();
        assert_eq!(stores.len(), 1, "{out:?}");
        assert!(
            matches!(stores[0], Output::Store(l, _) if *l == vec![pins("P5")[1].clone(), fan.clone()]),
            "{out:?}"
        );
    });
}
