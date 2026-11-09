//! Runs a test under umockdev, which gives the test process a fake sysfs
//! tree (the testbed) in place of the machine's devices. The tests that
//! read sensors through libsensors use it.
//!
//! The test body cannot run in the `cargo test` process itself, because
//! umockdev works by preloading a library into a process it starts. So
//! a test calls [`run`] twice over: in the `cargo test` process, `run`
//! starts `umockdev-run -d <testbed> -- <current test binary> --exact
//! <test name> --test-threads=1 --nocapture` with `PURON_UMD_CHILD=1`
//! and the configuration path in `PURON_UMD_CONF`, and passes when that
//! child exits 0. In the child, the same call to `run` runs the body.
//!
//! The test wraps its call in `puron_testkit::limit`, in the parent as in
//! the child, so the time limit applies to both. The `umockdev-run`
//! process is killed when the parent process ends.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The variable that marks the child: set to `1` there.
pub const CHILD_ENV: &str = "PURON_UMD_CHILD";
/// The variable that passes the configuration file's path to the child.
pub const CONF_ENV: &str = "PURON_UMD_CONF";

/// True in the child that `run` started under `umockdev-run`.
pub fn is_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some_and(|v| v == "1")
}

/// The configuration file's path that the parent passed; only in the
/// child (panics elsewhere).
pub fn conf() -> PathBuf {
    std::env::var_os(CONF_ENV)
        .map(PathBuf::from)
        .expect("PURON_UMD_CONF set by the parent")
}

/// In the parent, runs the test `test_name` of the current binary again
/// under `umockdev-run` on the testbed file `testbed`, passes `conf` as
/// the configuration path, and panics unless that child exits 0. `body`
/// does not run in the parent. In the child, runs `body` and nothing else.
///
/// The parent first checks that libsensors, `sensors` and umockdev are
/// installed; a missing one ends the process (see `host_need`).
pub fn run<F: FnOnce()>(test_name: &str, testbed: &Path, conf: &Path, body: F) {
    if is_child() {
        body();
        return;
    }
    crate::host_need(
        "libsensors5 and sensors",
        "dpkg-query -W libsensors5 lm-sensors",
    );
    crate::host_need(
        "umockdev",
        "umockdev-run --version && test -e /usr/lib/x86_64-linux-gnu/libumockdev-preload.so.0",
    );
    let exe = std::env::current_exe().expect("current test binary");
    let mut cmd = Command::new("umockdev-run");
    cmd.arg("-d")
        .arg(testbed)
        .arg("--")
        .arg(exe)
        .args(["--exact", test_name, "--test-threads=1", "--nocapture"])
        .env(CHILD_ENV, "1")
        .env(CONF_ENV, conf);
    // SAFETY: prctl is async-signal-safe; it only sets a flag of the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let status = cmd.status().expect("run umockdev-run");
    assert!(status.success(), "child of {test_name}: {status}");
}

/// The directory of the fake sysfs tree, which `umockdev-run` sets in
/// `$UMOCKDEV_DIR`; only in the child.
pub fn dir() -> PathBuf {
    std::env::var_os("UMOCKDEV_DIR")
        .map(PathBuf::from)
        .expect("UMOCKDEV_DIR set by umockdev-run")
}

/// In the child, writes `bytes` to the file `rel` of the fake sysfs
/// tree, so that a test can change a sensor's value between two reads.
pub fn write(rel: &str, bytes: &[u8]) {
    let path = dir().join(rel);
    std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// A new eventfd that nothing writes, to pass as the stop descriptor of
/// an adapter's `read`: the read is never asked to stop.
pub fn stop_fd() -> std::os::fd::OwnedFd {
    use std::os::fd::FromRawFd;
    // SAFETY: eventfd returns a new descriptor or -1.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
    assert!(fd >= 0, "eventfd");
    // SAFETY: fd is a new descriptor that nothing else owns.
    unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }
}
