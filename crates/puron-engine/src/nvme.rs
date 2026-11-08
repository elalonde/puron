//! The NVMe source: the temperatures of NVMe drives, read from the
//! kernel's hwmon files in sysfs. The provider is called `NvmeCli`, but
//! this adapter does not run the `nvme` program.
//!
//! `start` lists the controllers under sysfs `class/nvme/`, with each
//! one's model, address, hwmon directory and sensors. The list stays
//! fixed until the next start, so a drive added later appears only after
//! the provider is started again. Each `read` reads the
//! temperature files of the listed controllers. Every file is opened for
//! reading only.

use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use puron_core::rows::{Kind, Provider, RawReading};

use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The NVMe controller directory, relative to the sysfs root.
const CONTROLLERS: &str = "class/nvme";

/// A controller found at start that has everything a reading needs.
#[derive(Debug)]
struct Controller {
    /// The drive's model, which names its readings.
    model: String,
    /// The controller's `address` attribute (its PCI address), which
    /// keys its readings.
    address: String,
    /// The controller's first hwmon directory, which holds the
    /// temperature files.
    hwmon: PathBuf,
    /// Whether `temp3_input` existed at start. If it did, the controller
    /// gives two readings, "S1" from `temp2` and "S2" from `temp3`;
    /// otherwise one, from `temp1`.
    pair: bool,
}

/// The NVMe adapter. `root` is the sysfs root, `/sys` in production.
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

    /// The sysfs root the adapter was created with.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The number in a directory entry name made of `prefix` followed by
/// decimal digits, such as `hwmon3`. `None` for any other name.
fn number(name: &std::ffi::OsStr, prefix: &str) -> Option<u64> {
    let digits = name.to_str()?.strip_prefix(prefix)?;
    // Check for digits first, because `parse` alone would also accept a
    // leading "+". An empty string fails to parse, so it needs no check.
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The entries of `dir` whose names are `prefix` followed by a number,
/// sorted by that number. An unreadable directory gives none.
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

/// Removes one final newline and then every trailing space, and decodes
/// the rest as UTF-8, with U+FFFD for each invalid sequence. Leading
/// spaces are kept.
fn trim(mut bytes: &[u8]) -> String {
    if let [rest @ .., b'\n'] = bytes {
        bytes = rest;
    }
    while let [rest @ .., b' '] = bytes {
        bytes = rest;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// The content of an attribute file, trimmed by `trim`, or `None` when
/// the file cannot be read.
fn attribute(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| trim(&b))
}

/// The controller in `dir`, or `None` to skip it: it is kept only if its
/// model and address are readable and it has a hwmon directory. The
/// lowest-numbered hwmon directory is used.
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

/// The temperature in a hwmon input file, which holds millidegrees
/// Celsius: a decimal integer with an optional leading "-" and one
/// optional final newline, divided by 1000. Any other content, or a file
/// that cannot be read, gives no value.
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
    /// Lists the controllers, and returns `Unavailable` when none is
    /// kept.
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

    /// Returns one reading per sensor of each listed controller, in
    /// controller order. A file that gives no value leaves only its own
    /// reading without a value; the read itself never fails.
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

    /// Forgets the controllers listed at start.
    fn stop(&mut self) {
        self.controllers.clear();
    }
}
