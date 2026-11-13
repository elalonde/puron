//! The lm-sensors source: temperatures, fans, voltages and power, read
//! through libsensors.
//!
//! Each `read` initializes the library with its configuration, lists the
//! chips and their features in the library's order, reads one value for
//! each temperature, fan, voltage and power feature, and releases the
//! library before it returns. The library handle is a local value, so it
//! is released on every path, also when a panic unwinds. The adapter
//! never writes to a sensor.
//!
//! The only thing kept between reads is each chip's device name: a
//! readable name for the hardware the chip belongs to, such as a drive's
//! model or the CPU's name, which starts each reading's name. `start`
//! finds it once for each chip and keeps it until the next start.
//!
//! The settings can switch the chips of drives off. The adapter then
//! skips such a chip before it reads any value of it, because a read
//! makes the kernel ask the drive for its temperature, and that can keep
//! a hard disk from spinning down.

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
use puron_core::rows::{self, Kind, Provider, RawReading};
use puron_core::settings::Settings;

use crate::diag::{self, lmsensors as ev};
use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The chip names (prefixes) of drivers that measure the CPU. Their
/// readings are named after the CPU when the chip's own device gives no
/// name.
const CPU_CHIPS: [&str; 3] = ["coretemp", "k10temp", "zenpower"];
/// The file that holds the machine's product name, from the firmware.
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

/// The device name that `start` found for one chip. A chip is matched
/// by both its name and its hwmon directory.
#[derive(Debug)]
struct Device {
    chip: String,
    hwmon: PathBuf,
    name: String,
}

/// The lm-sensors adapter. `config` is the libsensors configuration
/// file: `None` in production, for the library's default files, and a
/// file of the test's own in tests. `cpuinfo` is the file that names the
/// CPU, `/proc/cpuinfo` in production.
#[derive(Debug)]
pub struct LmSensors {
    config: Option<PathBuf>,
    cpuinfo: PathBuf,
    devices: Vec<Device>,
    /// Whether the chips of drives are read (see `rows::drive_chip`).
    /// `configure` sets it from the settings before each start.
    drives: bool,
}

impl LmSensors {
    pub fn new(config: Option<PathBuf>, cpuinfo: impl Into<PathBuf>) -> Self {
        LmSensors {
            config,
            cpuinfo: cpuinfo.into(),
            devices: Vec::new(),
            drives: true,
        }
    }

    /// The configuration file, or `None` for the library's default files.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// The file the CPU's name is read from.
    pub fn cpuinfo(&self) -> &Path {
        &self.cpuinfo
    }

    /// Whether the chips of drives are read. True until `configure`
    /// gets settings that switch them off.
    pub fn drives(&self) -> bool {
        self.drives
    }

    /// Finds the device name of every chip present now, for the chips
    /// that have one. The library is initialized here with an empty
    /// configuration (`/dev/null`) instead of the real one, because the
    /// configuration does not change which chips exist; so a
    /// configuration that does not parse fails each read, but not this
    /// list. If the library cannot be initialized, the list is empty and
    /// every chip goes by the short name that `chip_device` gives.
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

/// The content of a file with surrounding whitespace removed, or `None`
/// when the file cannot be read or holds only whitespace.
fn file_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The CPU's name, from the first "model name" line of `cpuinfo`. The
/// "(R)" and "(TM)" marks, the word "CPU", and an "@" followed by a
/// frequency are removed, because they say nothing about which CPU it
/// is. `None` if no such line exists or nothing is left.
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

/// Whether `word` looks like a frequency with its unit, such as
/// `2.80GHz`: it starts with a digit and ends with "Hz".
fn is_frequency(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_digit()) && word.ends_with("Hz")
}

/// The machine's product name, or `None` when it cannot be read or the
/// firmware left one of `MACHINE_PLACEHOLDERS` there.
fn machine_name() -> Option<String> {
    let name = file_text(Path::new(MACHINE_NAME))?;
    (!MACHINE_PLACEHOLDERS
        .iter()
        .any(|p| name.eq_ignore_ascii_case(p)))
    .then_some(name)
}

/// The device name of the chip in `hwmon`, or `None` if it has none. The
/// first that applies wins:
/// - the `model` or `product` file of the chip's own device (the target
///   of hwmon's `device` link), unless the device is a power supply;
/// - the CPU's name, for a CPU chip (see `CPU_CHIPS`);
/// - the machine's name, for a chip on the platform bus.
///
/// Only the device that the `device` link names counts, not its parents,
/// since a parent is often a bridge. A power supply's model is not used
/// because it is often just "BAT".
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

/// The name a chip goes by when it has no device name: its prefix
/// without a final "_<digits>", which only numbers the chips of one
/// driver.
fn chip_device(prefix: &str) -> &str {
    match prefix.rsplit_once('_') {
        Some((stem, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => prefix,
    }
}

/// The kernel's label for feature `fname`, taken from its `_label` file
/// the way libsensors takes it: the file's bytes without the last one,
/// up to the first NUL. `None` when there is no hwmon directory, or the
/// file cannot be read or is empty.
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

/// Whether the library's `label` for feature `fname` must come from the
/// configuration file: it is neither the feature's raw name nor the
/// kernel's label. A configured label equal to one of those two cannot
/// be told apart from it, and counts as not configured.
fn configured(label: &[u8], fname: &str, kernel: Option<&[u8]>) -> bool {
    label != fname.as_bytes() && kernel != Some(label)
}

/// The name of the subsystem (such as `platform` or `power_supply`) that
/// the chip's own device belongs to, from the `device/subsystem` link.
fn subsystem(hwmon: &Path) -> Option<std::ffi::OsString> {
    let link = fs::read_link(hwmon.join("device/subsystem")).ok()?;
    link.file_name().map(OsStr::to_os_string)
}

/// For a feature type that is read, the subfeature whose value is used
/// and the kind of the reading: the input of a temperature, fan or
/// voltage, and the average of a power feature. `None` for every other
/// type, which gives no reading.
fn selected(kind: Option<FeatureKind>) -> Option<(SubKind, Kind)> {
    match kind? {
        FeatureKind::Temperature => Some((SubKind::TemperatureInput, Kind::Temperature)),
        FeatureKind::Fan => Some((SubKind::FanInput, Kind::Fan)),
        FeatureKind::Voltage => Some((SubKind::VoltageInput, Kind::Voltage)),
        FeatureKind::Power => Some((SubKind::PowerAverage, Kind::Power)),
        _ => None,
    }
}

/// A library string as text, with U+FFFD for each invalid UTF-8
/// sequence.
fn text(s: &CStr) -> Cow<'_, str> {
    s.to_string_lossy()
}

/// Logs the library's configuration errors as diagnostic events.
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

    /// Does nothing: the library ends the process after a fatal error,
    /// so there is nothing useful to do here.
    fn on_lm_sensors_fatal_error(&self, _error: &str, _procedure: &str) {}
}

/// What every reading of one chip shares. `device` is the chip's device
/// name, or the name from `chip_device` when it has none; `platform` is
/// whether the chip's device is on the platform bus.
struct ChipInfo<'a> {
    name: &'a str,
    device: &'a str,
    hwmon: Option<&'a Path>,
    platform: bool,
}

/// The reading of one feature at `position`, or `None` when its type is
/// not read (see `selected`) or it lacks the subfeature to read.
fn reading(info: &ChipInfo<'_>, feature: FeatureRef<'_>, position: u32) -> Option<RawReading> {
    let chip = info.name;
    let (sub_kind, kind) = selected(feature.kind())?;
    let sub = feature.sub_feature_by_kind(sub_kind).ok()?;
    let fname = feature.raw_name().map(text).unwrap_or_default();
    // A label from the configuration file is the whole name. Any other
    // name is the device name followed by the label, or by the raw
    // feature name when the library gives no label.
    let kernel = kernel_label(&fname, info.hwmon);
    let device_chars = info.device.chars().count();
    let (name, device_chars) = match feature.raw_label() {
        Ok(l) if configured(l.as_bytes(), &fname, kernel.as_deref()) => (text(&l).into_owned(), 0),
        Ok(l) => (format!("{} {}", info.device, text(&l)), device_chars),
        Err(_) => (format!("{} {fname}", info.device), device_chars),
    };
    // A subfeature that is not readable, or whose read fails, leaves this
    // reading without a value; the other readings are not affected.
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

/// Appends the readings of one chip to `out`. A chip whose name the
/// library cannot give is left out and logged. With `drives` false, the
/// chip of a drive is left out before any of its values is read. A chip
/// that `start` did not find, such as one that appeared later, has no
/// device name yet and goes by the short name that `chip_device` gives.
fn chip_readings(chip: ChipRef<'_>, devices: &[Device], drives: bool, out: &mut Vec<RawReading>) {
    let Ok(name) = chip.raw_name() else {
        diag::event(ev::chip_name());
        return;
    };
    let name = text(&name).into_owned();
    if !drives && rows::drive_chip(&name) {
        return;
    }
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
    /// Takes from the settings whether the chips of drives are read.
    fn configure(&mut self, settings: &Settings) {
        self.drives = settings.provider_lm_sensors_drives;
    }

    /// Finds the chips' device names, and always returns `Available`.
    fn start(&mut self) -> StartOutcome {
        self.devices = self.find_devices();
        StartOutcome::Available
    }

    fn read(&mut self, _stop: BorrowedFd<'_>) -> ReadOutcome {
        let mut init = Initializer::default().error_listener(Box::new(Log));
        if let Some(c) = &self.config {
            init = init.config_path(c);
        }
        // The library is initialized at every read, so a failed init (for
        // example a configuration that does not parse) fails this read only;
        // a later read tries again.
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
            chip_readings(chip, &self.devices, self.drives, &mut out);
        }
        drop(sensors);
        ReadOutcome::Readings(out)
    }

    /// Forgets the device names. The library holds nothing between
    /// reads, so there is nothing else to release.
    fn stop(&mut self) {
        self.devices.clear();
    }
}
