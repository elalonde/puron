//! The lm-sensors source: sensors through libsensors.
//!
//! Each Read initializes the library with the configuration, lists
//! chips and features in library order, reads one subfeature per
//! feature and releases the library before it returns, on every path:
//! the handle is a local value, dropped also on unwind. Nothing is
//! kept between reads. The adapter only reads.

use std::borrow::Cow;
use std::ffi::CStr;
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

/// The lm-sensors adapter. `config` is the configuration file: `None` in
/// production, a file in tests.
#[derive(Debug)]
pub struct LmSensors {
    config: Option<PathBuf>,
}

impl LmSensors {
    pub fn new(config: Option<PathBuf>) -> Self {
        LmSensors { config }
    }

    /// The configuration file, or `None` for the library's default.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }
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

/// The reading of one feature, or none when its type is not read or it
/// lacks the selected subfeature.
fn reading(chip: &str, feature: FeatureRef<'_>, position: u32) -> Option<RawReading> {
    let (sub_kind, kind) = selected(feature.kind())?;
    let sub = feature.sub_feature_by_kind(sub_kind).ok()?;
    let fname = feature.raw_name().map(text).unwrap_or_default();
    // The library's label; one it cannot give is the raw name.
    let name = match feature.raw_label() {
        Ok(l) => text(&l).into_owned(),
        Err(_) => fname.to_string(),
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
    })
}

/// The readings of one chip, appended to `out`; a chip whose name the
/// library cannot give is left out.
fn chip_readings(chip: ChipRef<'_>, out: &mut Vec<RawReading>) {
    let Ok(name) = chip.raw_name() else {
        diag::event(ev::chip_name());
        return;
    };
    let name = text(&name).into_owned();
    for feature in chip.feature_iter() {
        let position = out.len() as u32;
        out.extend(reading(&name, feature, position));
    }
}

impl Source for LmSensors {
    /// Holds nothing; always available.
    fn start(&mut self) -> StartOutcome {
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
            chip_readings(chip, &mut out);
        }
        drop(sensors);
        ReadOutcome::Readings(out)
    }

    /// No library state exists between reads.
    fn stop(&mut self) {}
}
