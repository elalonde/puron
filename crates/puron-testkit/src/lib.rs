//! puron-testkit: helpers and fixtures shared by Puron's tests.

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

/// Runs `body` under a time limit of `seconds` and returns its
/// value. The body runs on its own thread; when `seconds` pass on the real
/// clock, the helper prints `TIME LIMIT <id> <seconds>` to standard error
/// and ends the test process with exit code 124. A panic in the body is
/// passed on to the caller.
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

/// The workspace root (the directory of the top `Cargo.toml`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

/// `crates/puron-testkit/fixtures/<kind>/<name>`.
pub fn fixture(kind: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(kind)
        .join(name)
}

/// A fresh, empty directory under the system temporary directory.
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

/// Runs the check command of a host need with `sh -c`. When it fails,
/// prints `HOST NEED <need>` and ends the process with exit code 1: a
/// missing need fails the test, it never skips it.
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

/// The locale precondition of the locale tests: `locale -a` lists
/// `name` (codeset written `utf8`) and `newlocale` succeeds for it.
/// Otherwise prints `HOST NEED <name>` and ends the process with exit
/// code 1.
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

/// A command that runs one test of the current test binary again
/// (`--exact <name> --test-threads=1 --nocapture`).
pub fn rerun_self(test_name: &str) -> Command {
    let exe = std::env::current_exe().expect("current test binary");
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", test_name, "--test-threads=1", "--nocapture"]);
    cmd
}

/// The sets of a raw fixture with sets, each set an array of `T`. JSON
/// cannot hold NaN or the infinities, so a string `value` of `nan`,
/// `inf` or `-inf` is built here: the reading is read with `value` null,
/// then `set_value` gives it the number.
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
