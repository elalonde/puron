//! Stores Puron's settings, reads them back, and reports changes to them.
//!
//! The settings live in cosmic-config, one entry (file) per key, in the
//! user's directory `<root>/cosmic/<CONFIG_ID>/v<VERSION>`. `Config::load`
//! turns those entries into a `Settings` record, `Config::store` writes
//! one entry, and `Config::watch` calls back when an entry changes.
//!
//! Only the user's own entry of a key is ever read. cosmic-config can
//! also hold a system-wide default for each key, but this crate never
//! falls back to it, so a missing entry always takes the default that
//! `puron_core::settings` defines. An entry that cannot be used is
//! reported as a `Problem` in the result; this crate shows and logs
//! nothing itself.

use std::io::ErrorKind;
use std::path::PathBuf;

use cosmic_config::{ConfigGet, ConfigSet};
use puron_core::settings::{self, Settings, Stored, TemperatureUnit};

/// The cosmic-config ID under which the settings are stored. It is also
/// the name of the settings directory.
pub const CONFIG_ID: &str = "io.github.elalonde.Puron";
/// The version of the settings layout. cosmic-config keeps each version
/// in its own directory `v<VERSION>`; entries under another version are
/// not read.
pub const VERSION: u64 = 1;

/// The names of the keys. Each is the name of the `Settings` field it
/// stores.
pub const POLL_INTERVAL_SECS: &str = "poll_interval_secs";
pub const SHOW_PANEL_ICONS: &str = "show_panel_icons";
pub const TEMPERATURE_UNIT: &str = "temperature_unit";
pub const SHOW_TEMPERATURE_UNIT: &str = "show_temperature_unit";
pub const SHOW_ROTATION_RATE_UNIT: &str = "show_rotation_rate_unit";
pub const SHOW_VOLTAGE_UNIT: &str = "show_voltage_unit";
pub const SHOW_POWER_UNIT: &str = "show_power_unit";
pub const SHOW_DECIMALS: &str = "show_decimals";
pub const PROVIDER_LM_SENSORS: &str = "provider_lm_sensors";
pub const PROVIDER_LIQUIDCTL: &str = "provider_liquidctl";
pub const PROVIDER_NVIDIA: &str = "provider_nvidia";
pub const PROVIDER_UDISKS2: &str = "provider_udisks2";
pub const PROVIDER_NVME: &str = "provider_nvme";
pub const SHOW_TEMPERATURE: &str = "show_temperature";
pub const SHOW_ROTATION_RATE: &str = "show_rotation_rate";
pub const SHOW_VOLTAGE: &str = "show_voltage";
pub const SHOW_POWER: &str = "show_power";
pub const DEBUG_LOGGING: &str = "debug_logging";
pub const PINNED: &str = "pinned";
/// An entry left by development builds: a list of (row, sensor) pairs
/// that chose which sensor each panel row reads. It is not one of
/// `KEYS`. `load` reads it only to fill `Settings::old_cpu_source`, which
/// moves an old CPU pin to the matching sensor; nothing writes it.
const OLD_ROW_SOURCES: &str = "row_sources";
/// The only row of `OLD_ROW_SOURCES` whose choice is still read.
const OLD_CPU_ROW: &str = "@cpu";

/// Every key that `load` reads, `store` accepts and `watch` reports, in
/// the field order of `Settings`. Problems are reported in this order.
pub const KEYS: [&str; 19] = [
    POLL_INTERVAL_SECS,
    SHOW_PANEL_ICONS,
    TEMPERATURE_UNIT,
    SHOW_TEMPERATURE_UNIT,
    SHOW_ROTATION_RATE_UNIT,
    SHOW_VOLTAGE_UNIT,
    SHOW_POWER_UNIT,
    SHOW_DECIMALS,
    PROVIDER_LM_SENSORS,
    PROVIDER_LIQUIDCTL,
    PROVIDER_NVIDIA,
    PROVIDER_UDISKS2,
    PROVIDER_NVME,
    SHOW_TEMPERATURE,
    SHOW_ROTATION_RATE,
    SHOW_VOLTAGE,
    SHOW_POWER,
    DEBUG_LOGGING,
    PINNED,
];

/// Why an entry could not be used as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The entry exists but cannot be read: for example its permissions
    /// forbid it, or it is not valid UTF-8. The setting takes its default.
    Unreadable,
    /// The entry was read but does not parse as a value of the key's
    /// type. The setting takes its default.
    WrongType,
    /// `poll_interval_secs` is outside the allowed range; the setting
    /// takes the nearest value inside it.
    Clamped,
}

/// One entry that `load` could not use as stored. `key` is `None` only
/// when the settings directory itself exists but cannot be opened; then
/// every setting takes its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub key: Option<&'static str>,
    pub kind: ProblemKind,
}

/// What `Config::load` returns: the settings in effect and the problems
/// found on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// At most one per key, in the order of [`KEYS`]; empty when every
    /// entry was usable or missing.
    pub problems: Vec<Problem>,
}

/// A value to store with `Config::store`. The variant must match the
/// key's type: `U32` for `poll_interval_secs`, `TemperatureUnit` for
/// `temperature_unit`, `List` for `pinned`, and `Bool` for every other
/// key.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    U32(u32),
    TemperatureUnit(TemperatureUnit),
    List(Vec<String>),
}

/// Whether `Config::store` wrote the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreResult {
    Stored,
    Failed,
}

/// What a watch passes to its callback for one change: the keys whose
/// entries changed, in the order of [`KEYS`], and the result of a fresh
/// `load` made at that moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Delivery {
    pub keys: Vec<&'static str>,
    pub loaded: Loaded,
}

/// A running watch, returned by `Config::watch`. Dropping it stops the
/// watch.
pub struct Watch {
    _watcher: Box<dyn std::any::Any + Send>,
}

/// The settings under one configuration root. Cloning it is cheap; it
/// holds only the path.
#[derive(Clone, Debug)]
pub struct Config {
    root: PathBuf,
}

impl Config {
    /// The settings under `root`. The application passes [`user_root`];
    /// tests pass a temporary directory.
    pub fn new(root: impl Into<PathBuf>) -> Config {
        Config { root: root.into() }
    }

    /// Reads every key and returns the settings in effect. A missing
    /// entry, or a missing settings directory, takes the default and is
    /// not a problem. Never writes an entry or creates a directory.
    pub fn load(&self) -> Loaded {
        let Some(config) = self.open() else {
            return Loaded {
                settings: Settings::default(),
                problems: vec![Problem {
                    key: None,
                    kind: ProblemKind::Unreadable,
                }],
            };
        };
        let Some(config) = config else {
            return Loaded {
                settings: Settings::default(),
                problems: Vec::new(),
            };
        };
        let mut r = Reader {
            config,
            problems: Vec::new(),
        };
        // Each read records its own problem, so the fields are read in
        // the order of `KEYS` to keep the problems in that order.
        let stored = Stored {
            poll_interval_secs: r.poll_interval(),
            show_panel_icons: r.bool(SHOW_PANEL_ICONS),
            temperature_unit: r.unit(TEMPERATURE_UNIT),
            show_temperature_unit: r.bool(SHOW_TEMPERATURE_UNIT),
            show_rotation_rate_unit: r.bool(SHOW_ROTATION_RATE_UNIT),
            show_voltage_unit: r.bool(SHOW_VOLTAGE_UNIT),
            show_power_unit: r.bool(SHOW_POWER_UNIT),
            show_decimals: r.bool(SHOW_DECIMALS),
            provider_lm_sensors: r.bool(PROVIDER_LM_SENSORS),
            provider_liquidctl: r.bool(PROVIDER_LIQUIDCTL),
            provider_nvidia: r.bool(PROVIDER_NVIDIA),
            provider_udisks2: r.bool(PROVIDER_UDISKS2),
            provider_nvme: r.bool(PROVIDER_NVME),
            show_temperature: r.bool(SHOW_TEMPERATURE),
            show_rotation_rate: r.bool(SHOW_ROTATION_RATE),
            show_voltage: r.bool(SHOW_VOLTAGE),
            show_power: r.bool(SHOW_POWER),
            debug_logging: r.bool(DEBUG_LOGGING),
            pinned: r.list(PINNED),
            old_cpu_source: r.old_cpu_source(),
        };
        Loaded {
            settings: settings::settings(&stored),
            problems: r.problems,
        }
    }

    /// Replaces the entry of `key` with `value`. Returns `Failed`, and
    /// writes nothing, for a key that is not one of [`KEYS`] or a value
    /// of the wrong type (see [`Value`]); returns `Failed` also when the
    /// write fails. A failed write is not retried.
    pub fn store(&self, key: &str, value: &Value) -> StoreResult {
        let Some(&key) = KEYS.iter().find(|&&k| k == key) else {
            return StoreResult::Failed;
        };
        let typed = match (key, value) {
            (POLL_INTERVAL_SECS, Value::U32(_))
            | (TEMPERATURE_UNIT, Value::TemperatureUnit(_))
            | (PINNED, Value::List(_)) => true,
            (POLL_INTERVAL_SECS | TEMPERATURE_UNIT | PINNED, _) => false,
            (_, value) => matches!(value, Value::Bool(_)),
        };
        if !typed {
            return StoreResult::Failed;
        }
        // Creates the per-user directory if it does not exist.
        let Ok(config) =
            cosmic_config::Config::with_custom_path(CONFIG_ID, VERSION, self.root.clone())
        else {
            return StoreResult::Failed;
        };
        // cosmic-config writes the entry to a temporary file and renames
        // it over the old one, so a reader never sees half an entry.
        let result = match value {
            Value::Bool(v) => config.set(key, v),
            Value::U32(v) => config.set(key, v),
            Value::TemperatureUnit(v) => config.set(key, v),
            Value::List(v) => config.set(key, v),
        };
        match result {
            Ok(()) => StoreResult::Stored,
            Err(_) => StoreResult::Failed,
        }
    }

    /// Starts watching the settings directory. `deliver` is called on
    /// cosmic-config's watcher thread for each change that touches at
    /// least one entry of [`KEYS`]; a change to any other file calls
    /// nothing. Returns `None` when the settings directory cannot be
    /// created or opened, or the watch cannot start.
    pub fn watch<F>(&self, deliver: F) -> Option<Watch>
    where
        F: Fn(Delivery) + Send + Sync + 'static,
    {
        // Creates the per-user directory (no entry) if it does not exist,
        // so that a watch started on a fresh install sees the first store.
        let config =
            cosmic_config::Config::with_custom_path(CONFIG_ID, VERSION, self.root.clone()).ok()?;
        std::fs::read_dir(self.user_dir()).ok()?;
        let this = self.clone();
        // cosmic-config watches only the user's directory, never the
        // system defaults, and passes the names of the changed entries;
        // the temporary files of its own writes are not among them.
        let watcher = config
            .watch(move |_, changed| {
                let keys: Vec<&'static str> = KEYS
                    .iter()
                    .copied()
                    .filter(|k| changed.iter().any(|c| c == k))
                    .collect();
                if keys.is_empty() {
                    return;
                }
                deliver(Delivery {
                    keys,
                    loaded: this.load(),
                });
            })
            .ok()?;
        Some(Watch {
            _watcher: Box::new(watcher),
        })
    }

    /// The settings directory: `<root>/cosmic/<CONFIG_ID>/v<VERSION>`.
    fn user_dir(&self) -> PathBuf {
        self.root
            .join("cosmic")
            .join(CONFIG_ID)
            .join(format!("v{VERSION}"))
    }

    /// Opens the settings directory for reading. Returns `Some(None)`
    /// when it does not exist, which `load` treats as "nothing stored",
    /// and `None` when it exists but cannot be opened. It creates
    /// nothing: `with_custom_path` would create the directory, so it is
    /// called only after the directory is known to exist. The handle has
    /// no system path, so no read can fall back to a system default.
    fn open(&self) -> Option<Option<cosmic_config::Config>> {
        match std::fs::read_dir(self.user_dir()) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return Some(None),
            Err(_) => return None,
        }
        cosmic_config::Config::with_custom_path(CONFIG_ID, VERSION, self.root.clone())
            .ok()
            .map(Some)
    }
}

/// Reads the user's entries for `load` and collects a problem for each
/// entry it cannot use, in the order of the reads.
struct Reader {
    config: cosmic_config::Config,
    problems: Vec<Problem>,
}

impl Reader {
    // Each reader returns the user's value of a key, or `None` when the
    // entry is missing or not usable. They call only `get_local`: `get`
    // would fall back to the system default.

    fn bool(&mut self, key: &'static str) -> Option<bool> {
        let read = self.config.get_local::<bool>(key);
        self.usable(key, read)
    }

    fn unit(&mut self, key: &'static str) -> Option<TemperatureUnit> {
        let read = self.config.get_local::<TemperatureUnit>(key);
        self.usable(key, read)
    }

    fn list(&mut self, key: &'static str) -> Option<Vec<String>> {
        let read = self.config.get_local::<Vec<String>>(key);
        self.usable(key, read)
    }

    /// The sensor that development builds last chose for their CPU row,
    /// read from `OLD_ROW_SOURCES`. When the list has several choices for
    /// that row, the last one wins. An entry that cannot be read or
    /// parsed counts as absent and records no problem.
    fn old_cpu_source(&self) -> Option<String> {
        let read = self
            .config
            .get_local::<Vec<(String, String)>>(OLD_ROW_SOURCES);
        let pairs = read.unwrap_or_default();
        let last = pairs.into_iter().rfind(|(row, _)| row == OLD_CPU_ROW);
        last.map(|(_, source)| source)
    }

    /// Turns the result of reading `key` into a value. A missing entry
    /// gives `None` with no problem; content that does not parse as the
    /// key's type records `WrongType`; any other error records
    /// `Unreadable`.
    fn usable<T>(&mut self, key: &'static str, read: Result<T, cosmic_config::Error>) -> Option<T> {
        let kind = match read {
            Ok(value) => return Some(value),
            Err(cosmic_config::Error::NotFound) => return None,
            Err(cosmic_config::Error::Ron(_) | cosmic_config::Error::RonSpanned(_)) => {
                ProblemKind::WrongType
            }
            Err(_) => ProblemKind::Unreadable,
        };
        self.problems.push(Problem {
            key: Some(key),
            kind,
        });
        None
    }

    /// Reads `poll_interval_secs`. A value outside the allowed range is
    /// returned unchanged, for `puron_core::settings::settings` to clamp,
    /// and is recorded as `Clamped`.
    fn poll_interval(&mut self) -> Option<u32> {
        let read = self.config.get_local::<u32>(POLL_INTERVAL_SECS);
        let value = self.usable(POLL_INTERVAL_SECS, read)?;
        if !(settings::POLL_INTERVAL_MIN..=settings::POLL_INTERVAL_MAX).contains(&value) {
            self.problems.push(Problem {
                key: Some(POLL_INTERVAL_SECS),
                kind: ProblemKind::Clamped,
            });
        }
        Some(value)
    }
}

/// The user's configuration root, chosen the way cosmic-config chooses
/// it. Inside Flatpak (`FLATPAK_ID` set) it is `HOST_XDG_CONFIG_HOME`,
/// else `$HOME/.config`. Outside Flatpak, or inside it when neither is
/// set, it is `XDG_CONFIG_HOME` when that is an absolute path, else
/// `$HOME/.config`. Empty variables count as unset. `None` when none of
/// them gives a path.
pub fn user_root() -> Option<PathBuf> {
    let env = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = || env("HOME").map(|h| PathBuf::from(h).join(".config"));
    if env("FLATPAK_ID").is_some() {
        if let Some(root) = env("HOST_XDG_CONFIG_HOME").map(PathBuf::from).or_else(home) {
            return Some(root);
        }
    }
    env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(home)
}
