//! The re-executed child of the libsensors tests.
//!
//! Under plain `cargo test` a libsensors test calls [`run`], which runs
//! `umockdev-run -d <testbed> -- <current test binary> --exact <its name>
//! --test-threads=1 --nocapture` with `PURON_UMD_CHILD=1` and the
//! configuration path in `PURON_UMD_CONF`, and passes when the child exits
//! 0. In the child the same call runs the test body. The child calls the
//! same `puron_testkit::limit` as the parent, so the parent's limit applies
//! to it; the `umockdev-run` process is killed when the parent ends.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set to `1` in the child.
pub const CHILD_ENV: &str = "PURON_UMD_CHILD";
/// The configuration file passed to the child.
pub const CONF_ENV: &str = "PURON_UMD_CONF";

/// True in the re-executed child.
pub fn is_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some_and(|v| v == "1")
}

/// The configuration path, in the child.
pub fn conf() -> PathBuf {
    std::env::var_os(CONF_ENV)
        .map(PathBuf::from)
        .expect("PURON_UMD_CONF set by the parent")
}

/// Parent: runs test `test_name` of the current binary under
/// `umockdev-run` on `testbed`, with `conf` as the configuration, and
/// panics unless the child exits 0. Child: runs `body`.
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

/// The testbed directory `$UMOCKDEV_DIR`, in the child.
pub fn dir() -> PathBuf {
    std::env::var_os("UMOCKDEV_DIR")
        .map(PathBuf::from)
        .expect("UMOCKDEV_DIR set by umockdev-run")
}

/// Child: writes `bytes` to `rel` under `$UMOCKDEV_DIR`, to change the
/// fixture between reads.
pub fn write(rel: &str, bytes: &[u8]) {
    let path = dir().join(rel);
    std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// A stop descriptor that never fires, for an adapter's Read.
pub fn stop_fd() -> std::os::fd::OwnedFd {
    use std::os::fd::FromRawFd;
    // SAFETY: eventfd returns a new descriptor or -1.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
    assert!(fd >= 0, "eventfd");
    // SAFETY: fd is a new descriptor that nothing else owns.
    unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }
}
