//! Settings link with a fake settings program that records its PID,
//! parent PID and session ID. In its own test binary: `waitpid(-1)` reaps
//! every child of the process. Time limit 10 s, as it starts a process.
#![allow(non_snake_case)]

mod common;

use std::time::Duration;

use common::*;
use puron_applet::app::{Event, Launched, Output, Programs};
use puron_testkit::cfg;

fn echild(pid: i32) -> bool {
    let mut st = 0;
    let r = unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG) };
    r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
}

#[test]
fn c_applet_app_15() {
    let file = puron_testkit::temp_dir("c-applet-app-15").join("pid.txt");
    std::env::set_var("PURON_TEST_PIDFILE", &file);
    puron_testkit::limit("C-applet.app-15", 10, move || {
        let c = cfg::build("good");
        let dir =
            puron_testkit::workspace_root().join("crates/puron-testkit/fixtures/prog/settings");
        let programs = Programs {
            settings_dir: dir,
            search_path: Default::default(),
        };
        let mut a = app_with(&c.root, &handle(0), programs);
        a.handle(Event::Start);
        let out = a.handle(Event::SettingsLink);
        assert!(echild(-1), "waitpid(-1) after the launch");
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Launch(_, Launched::Started))),
            "{out:?}"
        );
        let line = loop {
            let s = std::fs::read_to_string(&file).unwrap_or_default();
            if s.ends_with('\n') {
                break s;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let f: Vec<i32> = line
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        let (pid, ppid, sid) = (f[0], f[1], f[2]);
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "program not alive");
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let state = stat
            .rsplit_once(") ")
            .map(|(_, r)| r.chars().next())
            .unwrap_or(None);
        assert!(state.is_some() && state != Some('Z'), "state {state:?}");
        assert_ne!(ppid, std::process::id() as i32);
        assert_ne!(sid, unsafe { libc::getsid(0) });
        assert!(echild(pid), "waitpid(pid)");
        assert!(echild(-1), "waitpid(-1)");
    });
}
