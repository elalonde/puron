//! The nvme source: NVMe drive temperatures from sysfs.
//!
//! Start lists the controllers of `class/nvme/` with their model,
//! address, hwmon directory and sensor set; the list stays fixed until
//! the next start. Each Read reads the input files of the listed
//! controllers. Every file is opened for reading only.

use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use puron_core::rows::{Kind, Provider, RawReading};

use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The controller directory.
const CONTROLLERS: &str = "class/nvme";

/// One kept controller.
#[derive(Debug)]
struct Controller {
    model: String,
    address: String,
    hwmon: PathBuf,
    /// `temp3_input` existed at start: the controller has "S1" and "S2".
    pair: bool,
}

/// The nvme-cli adapter. `root` is the sysfs root.
#[derive(Debug)]
pub struct Nvme {
    root: PathBuf,
    controllers: Vec<Controller>,
}

impl Nvme {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Nvme {
            root: root.into(),
            controllers: Vec::new(),
        }
    }

    /// The sysfs root parameter.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The number of an entry named `prefix` and decimal digits.
fn number(name: &std::ffi::OsStr, prefix: &str) -> Option<u64> {
    let digits = name.to_str()?.strip_prefix(prefix)?;
    // Digits only: `parse` alone would also take a leading "+"; an
    // empty string does not parse.
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The entries of `dir` named `prefix` and a number, by number.
fn numbered(dir: &Path, prefix: &str) -> Vec<(u64, PathBuf)> {
    let Ok(list) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, PathBuf)> = list
        .flatten()
        .filter_map(|e| Some((number(&e.file_name(), prefix)?, e.path())))
        .collect();
    out.sort();
    out
}

/// Trims one final 0x0A, then every final 0x20; decodes as UTF-8 with
/// U+FFFD for each invalid sequence.
fn trim(mut bytes: &[u8]) -> String {
    if let [rest @ .., b'\n'] = bytes {
        bytes = rest;
    }
    while let [rest @ .., b' '] = bytes {
        bytes = rest;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// A trimmed attribute file, or `None` when it cannot be read.
fn attribute(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| trim(&b))
}

/// The controller at `dir`, when it is kept: it has a readable model and
/// address and a hwmon directory.
fn controller(dir: &Path) -> Option<Controller> {
    let model = attribute(&dir.join("model"))?;
    let address = attribute(&dir.join("address"))?;
    let (_, hwmon) = numbered(dir, "hwmon")
        .into_iter()
        .find(|(_, p)| p.is_dir())?;
    let pair = hwmon.join("temp3_input").exists();
    Some(Controller {
        model,
        address,
        hwmon,
        pair,
    })
}

/// The value of an input file: a decimal integer with an
/// optional leading "-" and one optional final newline, divided by
/// 1000; anything else, or a file that cannot be read, is no value.
fn value(path: &Path) -> Option<f64> {
    let bytes = std::fs::read(path).ok()?;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: i64 = std::str::from_utf8(bytes).ok()?.parse().ok()?;
    Some(n as f64 / 1000.0)
}

impl Source for Nvme {
    /// Lists the controllers; *unavailable* when none is kept.
    fn start(&mut self) -> StartOutcome {
        self.controllers = numbered(&self.root.join(CONTROLLERS), "nvme")
            .into_iter()
            .filter_map(|(_, dir)| controller(&dir))
            .collect();
        if self.controllers.is_empty() {
            StartOutcome::Unavailable
        } else {
            StartOutcome::Available
        }
    }

    /// One reading per sensor of each listed controller, in controller
    /// order; a file that gives no value affects only its reading.
    fn read(&mut self, _stop: BorrowedFd<'_>) -> ReadOutcome {
        let mut out = Vec::new();
        for c in &self.controllers {
            let rows: &[(&str, &str)] = if c.pair {
                &[(" S1", "temp2"), (" S2", "temp3")]
            } else {
                &[("", "temp1")]
            };
            for (suffix, sensor) in rows {
                out.push(RawReading {
                    provider: Provider::NvmeCli,
                    position: out.len() as u32,
                    kind: Some(Kind::Temperature),
                    chip: None,
                    name: Some(format!("{}{suffix}", c.model)),
                    device: None,
                    item: None,
                    unit: None,
                    key: format!("{}/{sensor}", c.address),
                    value: value(&c.hwmon.join(format!("{sensor}_input"))),
                    kernel_label: None,
                    platform: false,
                    device_chars: if suffix.is_empty() {
                        0
                    } else {
                        c.model.chars().count()
                    },
                });
            }
        }
        ReadOutcome::Readings(out)
    }

    /// Drops the list.
    fn stop(&mut self) {
        self.controllers.clear();
    }
}
