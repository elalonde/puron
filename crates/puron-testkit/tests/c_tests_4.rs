//! The limit helper's self-test.
//! Each case runs itself again as a child under `timeout 10`; the child
//! takes the case's body under a limit of 1 s.

#![allow(non_snake_case)] // case names: c_<id>__<case>

use std::process::Command;
use std::time::{Duration, Instant};

const MODE: &str = "PURON_C_TESTS_4_CHILD";

fn is_child() -> bool {
    std::env::var_os(MODE).is_some()
}

/// Runs `test_name` again under `timeout 10`: exit code, time, stderr.
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
