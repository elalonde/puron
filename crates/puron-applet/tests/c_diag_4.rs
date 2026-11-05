//! Start with a config whose entries hold values of the wrong type: the
//! first four `config-problem` datagrams come in key order, each
//! `wrong-type`. In its own test binary, since diag's switch and socket
//! are per process.

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::Event;
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_diag_4() {
    puron_testkit::limit("C-diag-4", 5, || {
        let j = Journal::new("c-diag-4");
        diag::set_socket(&j.path);
        let c = cfg::build("wrong");
        let mut a = app(&c.root, &handle(0));
        a.handle(Event::Start);
        let got = j.wait(4, Duration::from_secs(2));
        let f = |n: &str| -> Vec<String> {
            got.iter()
                .map(|e| jrn::field(e, n).unwrap_or("-").to_string())
                .collect()
        };
        assert_eq!(f("PURON_EVENT"), ["config-problem"; 4]);
        assert_eq!(
            f("PURON_KEY"),
            [
                "poll_interval_secs",
                "temperature_unit",
                "show_power",
                "pinned"
            ]
        );
        assert_eq!(f("PURON_KIND"), ["wrong-type"; 4]);
    });
}
