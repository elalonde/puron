//! Helpers and fixtures shared by the tests of every Puron crate.
//!
//! This module holds the general helpers: a time limit for test bodies,
//! the paths of the checked-in fixture files under `fixtures/`, checks of
//! what the test machine must have installed, and a reader of the JSON
//! reading fixtures. The other modules build fixtures at test time and
//! stand in for the system services that the providers read: `cfg`
//! (settings directories), `sys` (sysfs trees), `umd` (tests run under
//! umockdev), `dbus` (a private D-Bus bus and a fake udisks2), `nvml` (a
//! fake NVIDIA library), `jrn` (stand-ins for the systemd journal's
//! socket), `eng` (a virtual
//! clock and scripted providers for the engine), `loc` (locale
//! environments) and, with the `dump` feature, `d5` (the text dump of
//! the rows Puron shows).

use std::path::{Path, PathBuf};
use std::process::Command;

pub mod cfg;
#[cfg(feature = "dump")]
pub mod d5;
pub mod dbus;
pub mod eng;
pub mod jrn;
pub mod loc;
pub mod nvml;
pub mod sys;
pub mod umd;

/// Runs `body` under a time limit of `seconds` and returns its value.
/// Every test body goes through this, so that a hung test fails instead
/// of blocking the run.
///
/// The body runs on its own thread, named `id`. When `seconds` pass on
/// the real clock before it returns, the helper prints
/// `TIME LIMIT <id> <seconds>` to standard error and ends the whole test
/// process with exit code 124 (the code that `timeout(1)` uses); the
/// body is not stopped any other way. A panic in the body is passed on
/// to the caller, so the test fails as if the body had run on the
/// caller's thread.
pub fn limit<T, F>(id: &str, seconds: u64, body: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(id.to_string())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
            let _ = tx.send(result);
        })
        .expect("spawn the test body");
    match rx.recv_timeout(std::time::Duration::from_secs(seconds)) {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            eprintln!("TIME LIMIT {id} {seconds}");
            std::process::exit(124);
        }
    }
}

/// The workspace root: the directory of the top `Cargo.toml`, two
/// levels above this crate's directory.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

/// The path of the checked-in fixture file `name` of the kind `kind`:
/// `crates/puron-testkit/fixtures/<kind>/<name>`. The path is not checked
/// to exist.
pub fn fixture(kind: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(kind)
        .join(name)
}

/// Creates a fresh, empty directory under the system temporary directory
/// and returns its path. The name holds `tag`, the process ID and a
/// counter, so tests in the same or in other processes never share one.
/// A leftover directory of the same name is removed first. Nothing
/// removes the new directory; the fixture builders remove theirs when
/// they are dropped.
pub fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "puron-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Checks that the test machine has something a test needs, such as a
/// program or a library: runs `check` with `sh -c`, its output
/// discarded. When the command cannot run or exits non-zero, prints
/// `HOST NEED <need>` to standard error and ends the process with exit
/// code 1. A missing need therefore fails the test; it never skips it.
pub fn host_need(need: &str, check: &str) {
    let ok = Command::new("sh")
        .args(["-c", check])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("HOST NEED {need}");
        std::process::exit(1);
    }
}

/// Checks that the locale `name`, such as `eo.UTF-8`, is installed, for
/// the tests that run under a locale. Both must hold: `locale -a` lists
/// it (that list spells the codeset `utf8`, so `UTF-8` in `name` is
/// matched as `utf8`), and `newlocale` can load it. Otherwise prints
/// `HOST NEED <name>` to standard error and ends the process with exit
/// code 1, as `host_need` does.
pub fn host_locale(name: &str) {
    let listed = name.replace("UTF-8", "utf8").replace('.', "\\.");
    let in_list = Command::new("sh")
        .args(["-c", &format!("locale -a | grep -qx '{listed}'")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let c_name = std::ffi::CString::new(name).expect("locale name");
    // SAFETY: a valid C string, no base locale; the result is freed.
    let loads = unsafe {
        let l = libc::newlocale(libc::LC_ALL_MASK, c_name.as_ptr(), std::ptr::null_mut());
        if l.is_null() {
            false
        } else {
            libc::freelocale(l);
            true
        }
    };
    if !(in_list && loads) {
        eprintln!("HOST NEED {name}");
        std::process::exit(1);
    }
}

/// Returns a command that runs the test `test_name` of the current test
/// binary again, alone, in a new process: `--exact <name>
/// --test-threads=1 --nocapture`. A test uses it to run its body again
/// in a new process, for example with a different environment; the
/// child's output is not captured by the test harness.
pub fn rerun_self(test_name: &str) -> Command {
    let exe = std::env::current_exe().expect("current test binary");
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", test_name, "--test-threads=1", "--nocapture"]);
    cmd
}

/// Reads the reading fixture `fixtures/raw/<name>`: a JSON object that
/// maps each set name to an array of readings, each parsed as `T`.
/// Returns the sets by name.
///
/// JSON cannot hold NaN or the infinities, so a fixture writes them as
/// the strings `"nan"`, `"inf"` and `"-inf"` in the field `value`. Such
/// a reading is parsed with `value` null, and then `set_value` stores the
/// number into it. Any other string in `value` panics.
pub fn raw_sets<T: serde::de::DeserializeOwned>(
    name: &str,
    set_value: fn(&mut T, f64),
) -> std::collections::BTreeMap<String, Vec<T>> {
    use serde_json::Value;
    let text = std::fs::read_to_string(fixture("raw", name)).expect("read fixture");
    let sets: std::collections::BTreeMap<String, Vec<Value>> =
        serde_json::from_str(&text).expect("parse fixture");
    sets.into_iter()
        .map(|(set, readings)| {
            let readings = readings
                .into_iter()
                .map(|mut reading| {
                    let special = match reading.get("value") {
                        Some(Value::String(word)) => Some(match word.as_str() {
                            "nan" => f64::NAN,
                            "inf" => f64::INFINITY,
                            "-inf" => f64::NEG_INFINITY,
                            other => panic!("value {other:?} in {name} set {set}"),
                        }),
                        _ => None,
                    };
                    if special.is_some() {
                        reading["value"] = Value::Null;
                    }
                    let mut out: T = serde_json::from_value(reading).expect("parse reading");
                    if let Some(v) = special {
                        set_value(&mut out, v);
                    }
                    out
                })
                .collect();
            (set, readings)
        })
        .collect()
}
