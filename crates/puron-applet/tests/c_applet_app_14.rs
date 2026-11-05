//! Start with a config whose entries hold values of the wrong type: each
//! problem that Load reports reaches diag, in order. In its own test
//! binary, since diag's socket is per process.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_applet_app_14() {
    puron_testkit::limit("C-applet.app-14", 5, || {
        let j = Journal::new("c-applet-app-14");
        diag::set_socket(&j.path);
        let c = cfg::build("wrong");
        let problems = puron_config::Config::new(&c.root).load().problems;
        assert!(!problems.is_empty());
        let mut a = app(&c.root, &handle(0));
        a.handle(Event::Start);
        let got = j.wait(problems.len(), Duration::from_secs(2));
        let keys: Vec<_> = got
            .iter()
            .map(|e| jrn::field(e, "PURON_KEY").unwrap_or("-"))
            .collect();
        let want: Vec<_> = problems.iter().map(|p| p.key.unwrap_or("")).collect();
        assert_eq!(keys, want);
        assert!(got
            .iter()
            .all(|e| jrn::field(e, "PURON_EVENT") == Some("config-problem")));
    });
}
