//! The settings link and the homepage link when neither program can be found.
//! In its own test binary: `waitpid(-1)` checks that no child is left,
//! and diag's socket is per process.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::{Event, Launched, Output, Programs};
use puron_engine::diag;
use puron_testkit::cfg;
use puron_testkit::jrn::{self, Journal};

fn echild() -> bool {
    let mut st = 0;
    let r = unsafe { libc::waitpid(-1, &mut st, libc::WNOHANG) };
    r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
}

#[test]
fn c_applet_app_16() {
    puron_testkit::limit("C-applet.app-16", 10, || {
        let j = Journal::new("c-applet-app-16");
        diag::set_socket(&j.path);
        let c = cfg::build("good");
        let noprog = puron_testkit::temp_dir("c-applet-app-16").join("noprog");
        std::fs::create_dir_all(&noprog).unwrap();
        let programs = Programs {
            settings_dir: noprog.clone(),
            search_path: noprog.clone().into_os_string(),
        };
        let mut a = app_with(&c.root, &handle(0), programs);
        a.handle(Event::Start);
        let out = a.handle(Event::SettingsLink);
        assert!(echild(), "a child left");
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Launch(_, Launched::Failed(2)))),
            "{out:?}"
        );
        let got = j.wait(1, Duration::from_secs(2));
        assert_eq!(got.len(), 1);
        assert_eq!(jrn::field(&got[0], "PURON_EVENT"), Some("launch-failed"));
        assert_eq!(jrn::field(&got[0], "PURON_ERRNO"), Some("2"));
        let out = a.handle(Event::HomepageLink);
        assert!(echild(), "a child left");
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Launch(_, Launched::Failed(e)) if *e == libc::ENOENT)),
            "{out:?}"
        );
    });
}
