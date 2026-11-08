//! The liquidctl source: readings of liquid coolers, fan controllers and
//! the other devices that the external `liquidctl` program supports.
//!
//! `start` looks for the program on the search path, once per start.
//! Each `read` runs `liquidctl status --json` once (see `runner`), then
//! checks the output of a run that completed and turns it into readings.
//! This is the engine's only child-run provider: the run ends early when
//! the worker's stop signal is raised.

mod runner;

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::fd::BorrowedFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use puron_core::rows::{Provider, RawReading, LIQUIDCTL_UNITS};
use serde_json::{Map, Value};

use crate::diag::{self, liquidctl as ev};
use crate::registry::{ReadOutcome, Source, StartOutcome};
use runner::End;

/// The program's file name, looked up on the search path.
pub const PROGRAM: &str = "liquidctl";
/// The time limit of one run in production. A run that takes longer is
/// killed and the read fails.
pub const LIMIT: Duration = Duration::from_secs(10);
/// The most output, in bytes, that a run may write; a run that writes
/// more ends as `End::OverCap`, and the read fails.
pub const CAP: usize = 1_048_576;
/// The search path used when `PATH` is not set.
pub const DEFAULT_SEARCH_PATH: &str = "/bin:/usr/bin";

/// The liquidctl adapter.
#[derive(Debug)]
pub struct Liquidctl {
    /// Directories separated by ':', searched in order for `PROGRAM`.
    search_path: OsString,
    /// The time limit of one run.
    limit: Duration,
    /// The absolute path of the program that `start` found, or `None`
    /// before a start, after a stop, or when it was not found.
    program: Option<PathBuf>,
}

impl Liquidctl {
    pub fn new(search_path: impl Into<OsString>, limit: Duration) -> Self {
        Liquidctl {
            search_path: search_path.into(),
            limit,
            program: None,
        }
    }

    pub fn search_path(&self) -> &OsStr {
        &self.search_path
    }

    pub fn limit(&self) -> Duration {
        self.limit
    }

    pub fn cap(&self) -> usize {
        CAP
    }
}

impl Source for Liquidctl {
    fn start(&mut self) -> StartOutcome {
        self.program = find(&self.search_path);
        if self.program.is_some() {
            StartOutcome::Available
        } else {
            if diag::on() {
                diag::event(ev::not_found(&self.search_path.to_string_lossy()));
            }
            StartOutcome::Unavailable
        }
    }

    fn read(&mut self, stop: BorrowedFd<'_>) -> ReadOutcome {
        let Some(program) = &self.program else {
            return ReadOutcome::Failed;
        };
        let (output, status) = match runner::run(program, self.limit, CAP, stop) {
            End::Complete { output, status } => (output, status),
            End::TimeLimit => return failed("time-limit"),
            End::OverCap => return failed("over-cap"),
            // The stop signal ended the run, which happens only when the
            // provider is stopped or the engine shuts down. The engine
            // drops the results of reads from before a stop, so this one
            // is never used.
            End::Stopped => return ReadOutcome::Failed,
            End::NotStarted(e) => {
                if diag::on() {
                    diag::event(ev::not_started(&e.to_string()));
                }
                return ReadOutcome::Failed;
            }
            End::WaitError(e) => {
                if diag::on() {
                    diag::event(ev::run_failed("wait-error", &e.to_string(), ""));
                }
                return ReadOutcome::Failed;
            }
        };
        if !status.success() && diag::on() {
            let text = match status.code() {
                Some(code) => code.to_string(),
                None => status.to_string(),
            };
            diag::event(ev::exit_status(&text));
        }
        match readings(&output) {
            Ok(r) => ReadOutcome::Readings(r),
            Err(Check::Empty) => failed("empty"),
            Err(Check::Shape) => failed("other-shape"),
            Err(Check::NotJson(e)) => {
                if diag::on() {
                    let position = format!("line {} column {}", e.line(), e.column());
                    diag::event(ev::run_failed("not-json", &e.to_string(), &position));
                }
                ReadOutcome::Failed
            }
        }
    }

    fn stop(&mut self) {
        self.program = None;
    }
}

/// Logs a failed run with `reason` and no further detail, and returns
/// `Failed`.
fn failed(reason: &str) -> ReadOutcome {
    diag::event(ev::run_failed(reason, "", ""));
    ReadOutcome::Failed
}

/// Searches `search_path` for the program the way a shell does: the
/// entries in order, an empty entry meaning the current directory.
/// Returns the first `<entry>/liquidctl` that is a regular file (after
/// following symbolic links) and that this process may execute, made
/// absolute. `None` when no entry has one.
fn find(search_path: &OsStr) -> Option<PathBuf> {
    search_path
        .as_bytes()
        .split(|b| *b == b':')
        .map(|entry| match entry {
            b"" => PathBuf::from("."),
            dir => PathBuf::from(OsStr::from_bytes(dir)),
        })
        .map(|dir| dir.join(PROGRAM))
        .find(|candidate| runnable(candidate))
        .and_then(|found| std::path::absolute(found).ok())
}

/// Whether `path` is a regular file, after following symbolic links,
/// that this process may execute.
fn runnable(path: &Path) -> bool {
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Ok(c) = std::ffi::CString::new(path.as_os_str().to_owned().into_vec()) else {
        return false;
    };
    // SAFETY: c is a valid C string; access only checks the file.
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// Why the output of a completed run gives no readings.
enum Check {
    /// The run wrote nothing.
    Empty,
    /// The output is not valid JSON.
    NotJson(serde_json::Error),
    /// The output is JSON but not in the expected form (see `readings`).
    Shape,
}

/// One device in the output: its "description", its "status" list, and
/// the whole JSON object, from which "address" is taken when needed.
struct Device<'a> {
    description: &'a str,
    status: &'a [Value],
    object: &'a Map<String, Value>,
}

/// Turns a completed run's output into readings.
///
/// The output must be a JSON array of objects, each with a string
/// "description" and an array "status"; and every status element whose
/// "unit" is one of `LIQUIDCTL_UNITS` must have a string "key". Any
/// other form fails the whole output with `Check::Shape`.
///
/// Each status element with a string "unit" and a string "key" then
/// gives one reading; other elements are skipped. A reading's key is the
/// device's description and the element's key, joined by '/'. When
/// several devices share a description, the device's "address" is added
/// after the description to keep the keys apart.
fn readings(output: &[u8]) -> Result<Vec<RawReading>, Check> {
    if output.is_empty() {
        return Err(Check::Empty);
    }
    let top: Value = serde_json::from_slice(output).map_err(Check::NotJson)?;
    let devices = top
        .as_array()
        .ok_or(Check::Shape)?
        .iter()
        .map(|d| {
            let object = d.as_object().ok_or(Check::Shape)?;
            let description = object
                .get("description")
                .and_then(Value::as_str)
                .ok_or(Check::Shape)?;
            let status = object
                .get("status")
                .and_then(Value::as_array)
                .ok_or(Check::Shape)?;
            Ok(Device {
                description,
                status,
                object,
            })
        })
        .collect::<Result<Vec<_>, Check>>()?;
    for item in devices.iter().flat_map(|d| d.status) {
        let unit = item.get("unit").and_then(Value::as_str);
        let key = item.get("key").and_then(Value::as_str);
        if unit.is_some_and(|u| LIQUIDCTL_UNITS.contains(&u)) && key.is_none() {
            return Err(Check::Shape);
        }
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for d in &devices {
        *count.entry(d.description).or_default() += 1;
    }
    let mut out = Vec::new();
    for d in &devices {
        let prefix = if count[d.description] > 1 {
            let address = d.object.get("address").and_then(Value::as_str);
            format!("{}/{}", d.description, address.unwrap_or(""))
        } else {
            d.description.to_string()
        };
        for item in d.status {
            let unit = item.get("unit").and_then(Value::as_str);
            let key = item.get("key").and_then(Value::as_str);
            let (Some(unit), Some(key)) = (unit, key) else {
                continue;
            };
            out.push(RawReading {
                provider: Provider::Liquidctl,
                position: out.len() as u32,
                kind: None,
                chip: None,
                name: None,
                device: Some(d.description.to_string()),
                item: Some(key.to_string()),
                unit: Some(unit.to_string()),
                key: format!("{prefix}/{key}"),
                value: item.get("value").and_then(Value::as_f64),
                kernel_label: None,
                platform: false,
                device_chars: 0,
            });
        }
    }
    Ok(out)
}
