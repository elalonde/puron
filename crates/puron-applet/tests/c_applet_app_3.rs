//! `Event::Stop` shuts the engine down once and exits. In the test
//! process, the first stop must call the engine's shutdown and give
//! `Exit`, and a second stop must do nothing. Then a fresh run of this
//! test binary runs `app::run` with a fake engine whose shutdown takes
//! 1 s, and receives SIGTERM: it must exit with status 0, not by the
//! signal, no sooner than 1 s later, after exactly one shutdown. The test
//! has its own binary because signal handlers are process-wide, and a
//! time limit of 10 s because it starts a process.
#![allow(non_snake_case)]

mod common;

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::*;
use puron_applet::app::{self, Command, Event, Output, Source};
use puron_testkit::cfg;
use puron_testkit::eng::HandleCall;

const CHILD: &str = "PURON_APP3_CHILD";

#[test]
fn c_applet_app_3() {
    puron_testkit::limit("C-applet.app-3", 10, || {
        let c = cfg::build("p5");
        let h = handle(0);
        let mut a = app(&c.root, &h);
        a.handle(Event::Start);
        let first = a.handle(Event::Stop);
        assert_eq!(first, [Output::Engine(Command::Shutdown), Output::Exit]);
        let second = a.handle(Event::Stop);
        assert_eq!(second, []);
        let n = h
            .calls()
            .iter()
            .filter(|c| **c == HandleCall::Shutdown)
            .count();
        assert_eq!(n, 1);

        let mut child = puron_testkit::rerun_self("c_applet_app_3__child")
            .env(CHILD, "1")
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn the child");
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "child ended before ready"
            );
            if line.trim_end().ends_with(" ready") || line.trim() == "ready" {
                break;
            }
        }
        let t0 = Instant::now();
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let mut rest = String::new();
        out.read_to_string(&mut rest).unwrap();
        let st = child.wait().unwrap();
        let took = t0.elapsed();
        assert_eq!(st.signal(), None, "ended by a signal: {st:?}; {rest}");
        assert_eq!(st.code(), Some(0), "{rest}");
        assert!(rest.contains("shutdowns=1"), "{rest}");
        assert!(
            took >= Duration::from_secs(1),
            "exit {took:?} after SIGTERM"
        );
    });
}

/// The child's side of `c_applet_app_3`; it does nothing unless it runs
/// in the child. It runs `app::run` with a fake engine whose shutdown
/// takes 1 s, prints "ready" once the loop has handled `Event::Start`,
/// and prints the number of shutdowns when the loop exits.
#[test]
fn c_applet_app_3__child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    puron_testkit::limit("C-applet.app-3", 10, || {
        let c = cfg::build("p5");
        let h = handle(1);
        let a = app(&c.root, &h);
        let (tx, rx) = mpsc::channel();
        let h2 = h.clone();
        std::mem::forget(c);
        app::run(a, tx, rx, move |s, _, out| {
            if s == Source::Start {
                println!("ready");
            }
            if out.contains(&Output::Exit) {
                let n = h2
                    .calls()
                    .iter()
                    .filter(|c| **c == HandleCall::Shutdown)
                    .count();
                println!("shutdowns={n}");
            }
        })
    });
}
