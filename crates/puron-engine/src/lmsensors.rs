//! The lm-sensors source: sensors through libsensors.
//!
//! Each `read` initializes the library with the configuration, lists
//! chips and features in library order, reads one subfeature per
//! feature and releases the library before it returns, on every path:
//! the handle is a local value, dropped also on unwind. Nothing is
//! kept between reads. The adapter only reads.
//!
//! The device part of a row name is the exception: `start` finds it once
//! for each chip and keeps it until the next start.

use std::borrow::Cow;
use std::ffi::{CStr, OsStr};
use std::fs;
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use lm_sensors::errors::Listener;
use lm_sensors::feature::Kind as FeatureKind;
use lm_sensors::sub_feature::Flags;
use lm_sensors::value::Kind as SubKind;
use lm_sensors::{ChipRef, FeatureRef, Initializer};
use puron_core::rows::{Kind, Provider, RawReading};

use crate::diag::{self, lmsensors as ev};
use crate::registry::{ReadOutcome, Source, StartOutcome};

/// Chips that measure the CPU.
const CPU_CHIPS: [&str; 3] = ["coretemp", "k10temp", "zenpower"];
/// The file that holds the machine's name.
const MACHINE_NAME: &str = "/sys/class/dmi/id/product_name";
/// Text that firmware leaves in the machine's name when nobody filled it
/// in, compared without regard to ASCII case.
const MACHINE_PLACEHOLDERS: [&str; 11] = [
    "To be filled by O.E.M.",
    "System Product Name",
    "Default string",
    "Not Specified",
    "Not Applicable",
    "Product Name",
    "System Name",
    "O.E.M.",
    "None",
    "N/A",
    "Unknown",
];

/// The device name that `start` found for one chip.
#[derive(Debug)]
struct Device {
    chip: String,
    hwmon: PathBuf,
    name: String,
}

/// The lm-sensors adapter. `config` is the configuration file: `None` in
/// production, a file in tests. `cpuinfo` is the file that names the CPU.
#[derive(Debug)]
pub struct LmSensors {
    config: Option<PathBuf>,
    cpuinfo: PathBuf,
    devices: Vec<Device>,
}

impl LmSensors {
    pub fn new(config: Option<PathBuf>, cpuinfo: impl Into<PathBuf>) -> Self {
        LmSensors {
            config,
            cpuinfo: cpuinfo.into(),
            devices: Vec::new(),
        }
    }

    /// The configuration file, or `None` for the library's default.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// The file that names the CPU.
    pub fn cpuinfo(&self) -> &Path {
        &self.cpuinfo
    }

    /// The device names of the chips listed now. The configuration plays
    /// no part in which chips exist, so an empty one stands in for it: one
    /// that does not parse fails its read, not this list. A list that
    /// cannot be made is empty, which leaves every chip its chip name.
    fn find_devices(&self) -> Vec<Device> {
        let Ok(sensors) = Initializer::default().config_path("/dev/null").initialize() else {
            return Vec::new();
        };
        let cpu = fs::read(&self.cpuinfo)
            .ok()
            .and_then(|b| cpu_name(&String::from_utf8_lossy(&b)));
        let machine = machine_name();
        let mut found = Vec::new();
        for chip in sensors.chip_iter(None) {
            let (Ok(name), Some(hwmon), Some(prefix)) =
                (chip.raw_name(), chip.path(), chip.raw_prefix())
            else {
                continue;
            };
            if let Some(device) =
                own_device(hwmon, &text(prefix), cpu.as_deref(), machine.as_deref())
            {
                found.push(Device {
                    chip: text(&name).into_owned(),
                    hwmon: hwmon.to_path_buf(),
                    name: device,
                });
            }
        }
        found
    }
}

/// The trimmed text of a file; none when it cannot be read or holds
/// nothing else.
fn file_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The CPU's name from the first "model name" line, without the marks,
/// the word "CPU" and the frequency that say nothing about which CPU it is.
fn cpu_name(cpuinfo: &str) -> Option<String> {
    let line = cpuinfo.lines().find_map(|l| {
        let (key, value) = l.split_once(':')?;
        (key.trim() == "model name").then_some(value)
    })?;
    let line = line.replace("(R)", "").replace("(TM)", "");
    let mut words = line.split_whitespace().peekable();
    let mut kept = Vec::new();
    while let Some(word) = words.next() {
        if word == "@" && words.peek().is_some_and(|w| is_frequency(w)) {
            words.next();
        } else if word != "CPU" {
            kept.push(word);
        }
    }
    (!kept.is_empty()).then(|| kept.join(" "))
}

/// A number with its unit, such as `2.80GHz`.
fn is_frequency(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_digit()) && word.ends_with("Hz")
}

/// The machine's name, unless firmware left a placeholder there.
fn machine_name() -> Option<String> {
    let name = file_text(Path::new(MACHINE_NAME))?;
    (!MACHINE_PLACEHOLDERS
        .iter()
        .any(|p| name.eq_ignore_ascii_case(p)))
    .then_some(name)
}

/// The device name of the chip in `hwmon`, from the first of its own
/// device's model or product, the CPU's name, the machine's name.
/// Only the device that the `device` link names counts: its parent is
/// often a bridge. A power supply's text is no name (often just "BAT").
fn own_device(
    hwmon: &Path,
    prefix: &str,
    cpu: Option<&str>,
    machine: Option<&str>,
) -> Option<String> {
    let device = hwmon.join("device");
    let subsystem = subsystem(hwmon);
    let subsystem = subsystem.as_deref();
    if subsystem != Some(OsStr::new("power_supply")) {
        let own = file_text(&device.join("model")).or_else(|| file_text(&device.join("product")));
        if own.is_some() {
            return own;
        }
    }
    if CPU_CHIPS.contains(&prefix) && cpu.is_some() {
        return cpu.map(str::to_string);
    }
    if subsystem == Some(OsStr::new("platform")) {
        return machine.map(str::to_string);
    }
    None
}

/// A chip's name without a final "_<digits>", which only numbers chips of
/// one driver.
fn chip_device(prefix: &str) -> &str {
    match prefix.rsplit_once('_') {
        Some((stem, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => prefix,
    }
}

/// What the library takes from feature `fname`'s label file: the file's
/// bytes without the last one, up to the first NUL. None when the file
/// cannot be read or is empty.
fn kernel_label(fname: &str, hwmon: Option<&Path>) -> Option<Vec<u8>> {
    let file = fs::read(hwmon?.join(format!("{fname}_label"))).ok()?;
    let (_, kernel) = file.split_last()?;
    Some(
        kernel
            .split(|&b| b == 0)
            .next()
            .unwrap_or_default()
            .to_vec(),
    )
}

/// True when the library's `label` for feature `fname` can only come from
/// the configuration: it is neither the raw name nor the kernel's label.
/// A configured label equal to one of those two cannot be told from it.
fn configured(label: &[u8], fname: &str, kernel: Option<&[u8]>) -> bool {
    label != fname.as_bytes() && kernel != Some(label)
}

/// The subsystem that the chip's own device belongs to.
fn subsystem(hwmon: &Path) -> Option<std::ffi::OsString> {
    let link = fs::read_link(hwmon.join("device/subsystem")).ok()?;
    link.file_name().map(OsStr::to_os_string)
}

/// The subfeature read and the kind of a feature type; `None` for every
/// other type.
fn selected(kind: Option<FeatureKind>) -> Option<(SubKind, Kind)> {
    match kind? {
        FeatureKind::Temperature => Some((SubKind::TemperatureInput, Kind::Temperature)),
        FeatureKind::Fan => Some((SubKind::FanInput, Kind::Fan)),
        FeatureKind::Voltage => Some((SubKind::VoltageInput, Kind::Voltage)),
        FeatureKind::Power => Some((SubKind::PowerAverage, Kind::Power)),
        _ => None,
    }
}

/// Text that is not valid UTF-8 gets U+FFFD per invalid sequence.
fn text(s: &CStr) -> Cow<'_, str> {
    s.to_string_lossy()
}

/// Gives the library's configuration errors to `diag`.
#[derive(Debug)]
struct Log;

impl Listener for Log {
    fn on_lm_sensors_config_error(&self, error: &str, file: Option<&Path>, line: usize) {
        if !diag::on() {
            return;
        }
        let file = file.map(|f| f.display().to_string()).unwrap_or_default();
        diag::event(ev::config_error(&file, &line.to_string(), error));
    }

    /// The library ends the process after this; nothing to add.
    fn on_lm_sensors_fatal_error(&self, _error: &str, _procedure: &str) {}
}

/// What the readings of one chip share.
struct ChipInfo<'a> {
    name: &'a str,
    device: &'a str,
    hwmon: Option<&'a Path>,
    platform: bool,
}

/// The reading of one feature, or none when its type is not read or it
/// lacks the selected subfeature.
fn reading(info: &ChipInfo<'_>, feature: FeatureRef<'_>, position: u32) -> Option<RawReading> {
    let chip = info.name;
    let (sub_kind, kind) = selected(feature.kind())?;
    let sub = feature.sub_feature_by_kind(sub_kind).ok()?;
    let fname = feature.raw_name().map(text).unwrap_or_default();
    // A configured label stands alone; every other name follows the
    // device. A label the library cannot give is the raw name.
    let kernel = kernel_label(&fname, info.hwmon);
    let device_chars = info.device.chars().count();
    let (name, device_chars) = match feature.raw_label() {
        Ok(l) if configured(l.as_bytes(), &fname, kernel.as_deref()) => (text(&l).into_owned(), 0),
        Ok(l) => (format!("{} {}", info.device, text(&l)), device_chars),
        Err(_) => (format!("{} {fname}", info.device), device_chars),
    };
    // A failed or not readable value is "no value" for this reading
    // only.
    let value = if sub.flags().is_some_and(|f| f.contains(Flags::READABLE)) {
        match sub.raw_value() {
            Ok(v) => Some(v),
            Err(e) => {
                if diag::on() {
                    let sname = sub.raw_name().map(text).unwrap_or_default();
                    diag::event(ev::value_failed(chip, &sname, &e.to_string()));
                }
                None
            }
        }
    } else {
        None
    };
    Some(RawReading {
        provider: Provider::LmSensors,
        position,
        kind: Some(kind),
        chip: Some(chip.to_string()),
        name: Some(name),
        device: None,
        item: None,
        unit: None,
        key: format!("{chip}/{fname}"),
        value,
        kernel_label: kernel.map(|k| String::from_utf8_lossy(&k).into_owned()),
        platform: info.platform,
        device_chars,
    })
}

/// The readings of one chip, appended to `out`; a chip whose name the
/// library cannot give is left out. A chip that `start` did not find goes by
/// its chip name, as one that appeared later has no device name yet.
fn chip_readings(chip: ChipRef<'_>, devices: &[Device], out: &mut Vec<RawReading>) {
    let Ok(name) = chip.raw_name() else {
        diag::event(ev::chip_name());
        return;
    };
    let name = text(&name).into_owned();
    let hwmon = chip.path();
    let prefix = chip.raw_prefix().map(text).unwrap_or_default();
    let device = devices
        .iter()
        .find(|d| d.chip == name && Some(d.hwmon.as_path()) == hwmon)
        .map_or_else(|| chip_device(&prefix), |d| d.name.as_str());
    let info = ChipInfo {
        name: &name,
        device,
        hwmon,
        platform: hwmon
            .and_then(subsystem)
            .is_some_and(|s| s == OsStr::new("platform")),
    };
    for feature in chip.feature_iter() {
        let position = out.len() as u32;
        out.extend(reading(&info, feature, position));
    }
}

impl Source for LmSensors {
    /// Finds the device names; always available.
    fn start(&mut self) -> StartOutcome {
        self.devices = self.find_devices();
        StartOutcome::Available
    }

    fn read(&mut self, _stop: BorrowedFd<'_>) -> ReadOutcome {
        let mut init = Initializer::default().error_listener(Box::new(Log));
        if let Some(c) = &self.config {
            init = init.config_path(c);
        }
        // Initialized at every read, so a failure affects this read only.
        let sensors = match init.initialize() {
            Ok(s) => s,
            Err(e) => {
                if diag::on() {
                    diag::event(ev::init_failed(&e.to_string()));
                }
                return ReadOutcome::Unavailable;
            }
        };
        let mut out = Vec::new();
        for chip in sensors.chip_iter(None) {
            chip_readings(chip, &self.devices, &mut out);
        }
        drop(sensors);
        ReadOutcome::Readings(out)
    }

    /// No library state exists between reads.
    fn stop(&mut self) {
        self.devices.clear();
    }
}
