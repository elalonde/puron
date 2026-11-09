//! The homepage link starts the `xdg-open` found on the search path with
//! the homepage address as its only argument. The search path holds only
//! a fake `xdg-open` that writes its arguments to a file; the launch must
//! report `Started` and the file must hold the address. The test has its
//! own binary because it sets an environment variable, and a time limit
//! of 10 s because it starts a process.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::{Event, Launched, Output, Programs};
use puron_testkit::cfg;

#[test]
fn c_applet_app_17() {
    let file = puron_testkit::temp_dir("c-applet-app-17").join("args.txt");
    std::env::set_var("PURON_TEST_ARGFILE", &file);
    puron_testkit::limit("C-applet.app-17", 10, move || {
        let c = cfg::build("good");
        let open = puron_testkit::workspace_root().join("crates/puron-testkit/fixtures/prog/open");
        let programs = Programs {
            settings_dir: open.clone(),
            search_path: open.into_os_string(),
        };
        let mut a = app_with(&c.root, &handle(0), programs);
        a.handle(Event::Start);
        let out = a.handle(Event::HomepageLink);
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Launch(_, Launched::Started))),
            "{out:?}"
        );
        let got = loop {
            let s = std::fs::read_to_string(&file).unwrap_or_default();
            if s.ends_with('\n') {
                break s;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(got, "https://elalonde.dev/software/puron/\n");
    });
}
