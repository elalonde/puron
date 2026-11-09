//! Tests of `puron_testkit::limit`, which must end a hung test.
//!
//! Each case runs its own test again in a child process, under
//! `timeout 10` as a backstop, and the child runs the case's body under a
//! limit of 1 s. The parent then checks how the child ended.

#![allow(non_snake_case)] // case names: c_<id>__<case>

use std::process::Command;
use std::time::{Duration, Instant};

/// Set in the child, which then runs the case's body under the limit.
const MODE: &str = "PURON_C_TESTS_4_CHILD";

fn is_child() -> bool {
    std::env::var_os(MODE).is_some()
}

/// Runs the test `test_name` of this binary again in a child process,
/// under `timeout 10`, and returns its exit code (`None` if a signal ended
/// it), the time it took and its standard error.
fn child(test_name: &str) -> (Option<i32>, Duration, String) {
    let exe = std::env::current_exe().expect("current test binary");
    let start = Instant::now();
    let out = Command::new("timeout")
        .arg("10")
        .arg(exe)
        .args(["--exact", test_name, "--test-threads=1", "--nocapture"])
        .env(MODE, "1")
        .output()
        .expect("run child");
    (
        out.status.code(),
        start.elapsed(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A body that sleeps 60 s under a limit of 1 s: the child must exit
/// with code 124 within 1.5 s and print the time-limit line with its ID
/// and the limit to standard error.
#[test]
fn c_tests_4__timeout() {
    if is_child() {
        puron_testkit::limit("C-tests-4", 1, || {
            std::thread::sleep(Duration::from_secs(60))
        });
        return;
    }
    puron_testkit::limit("C-tests-4", 15, || {
        let (code, took, err) = child("c_tests_4__timeout");
        assert_eq!(code, Some(124), "exit code; stderr: {err}");
        assert!(took <= Duration::from_millis(1500), "took {took:?}");
        assert!(err.contains("TIME LIMIT C-tests-4 1\n"), "stderr: {err}");
    });
}

/// A body that returns at once under a limit of 1 s: the child must exit
/// with code 0.
#[test]
fn c_tests_4__quick() {
    if is_child() {
        puron_testkit::limit("C-tests-4", 1, || ());
        return;
    }
    puron_testkit::limit("C-tests-4", 15, || {
        let (code, _, err) = child("c_tests_4__quick");
        assert_eq!(code, Some(0), "exit code; stderr: {err}");
    });
}
