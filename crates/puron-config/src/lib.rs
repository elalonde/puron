//! config: settings storage and change notification.
//!
//! Stores the settings per user through cosmic-config, reads them back
//! as a `Settings` record, and watches the per-user entries. Only the
//! per-user entry of a key is read; cosmic-config's system default for
//! the ID never is, so every default is Puron's own. Problems are
//! returned to the caller; `config` shows and logs nothing.

use std::io::ErrorKind;
use std::path::PathBuf;

use cosmic_config::{ConfigGet, ConfigSet};
use puron_core::settings::{self, Settings, Stored, TemperatureUnit};

/// The config ID.
pub const CONFIG_ID: &str = "io.github.elalonde.Puron";
/// The config version.
pub const VERSION: u64 = 1;

/// The key names, each the field of `Settings` of that name.
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
/// A development build's choices of what a row reads, as pairs of a row and a
/// sensor key. No key of its own: read once, for a stored CPU pin that has yet
/// to move, and never written.
const OLD_ROW_SOURCES: &str = "row_sources";
/// The row in those pairs whose choice still matters.
const OLD_CPU_ROW: &str = "@cpu";

/// The 19 keys, in the field order of `Settings`.
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

/// The kind of a Problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The entry exists but cannot be read.
    Unreadable,
    /// The content is not a value of the key's type.
    WrongType,
    /// `poll_interval_secs` stored out of range.
    Clamped,
}

/// One Problem of a load. `key` is `None` only for a per-user
/// directory that cannot be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub key: Option<&'static str>,
    pub kind: ProblemKind,
}

/// The result of a load.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// In the key order of [`KEYS`].
    pub problems: Vec<Problem>,
}

/// A value of a key's type, for `Config::store`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    U32(u32),
    TemperatureUnit(TemperatureUnit),
    List(Vec<String>),
}

/// The result of a store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreResult {
    Stored,
    Failed,
}

/// One delivery of the watch: the keys that changed, and the
/// settings and Problems as a load at that moment returns them.
#[derive(Clone, Debug, PartialEq)]
pub struct Delivery {
    pub keys: Vec<&'static str>,
    pub loaded: Loaded,
}

/// A running watch; dropping it stops the watch.
pub struct Watch {
    _watcher: Box<dyn std::any::Any + Send>,
}

/// A handle on the settings under one configuration root.
#[derive(Clone, Debug)]
pub struct Config {
    root: PathBuf,
}

impl Config {
    /// A handle on the configuration root `root` (production:
    /// [`user_root`]; tests: a temporary directory).
    pub fn new(root: impl Into<PathBuf>) -> Config {
        Config { root: root.into() }
    }

    /// Load. Writes no entry and creates no directory.
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
        // Fields in key order, so that Problems are in key order.
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

    /// Store: replaces the entry of `key` as a whole. *Failed*
    /// for a key that is not one of [`KEYS`], a value of another type,
    /// or a write that fails; nothing is retried.
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
        // One atomic replace of the entry (atomicwrites).
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

    /// Watch: `deliver` is called once for each change of an
    /// entry of [`KEYS`] in the per-user directory. `None` when the
    /// per-user directory cannot be opened.
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
        // cosmic-config watches the per-user directory only, and passes
        // the entry names of each change (atomicwrites temporaries left
        // out).
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

    /// `<root>/cosmic/<ID>/v<VERSION>`.
    fn user_dir(&self) -> PathBuf {
        self.root
            .join("cosmic")
            .join(CONFIG_ID)
            .join(format!("v{VERSION}"))
    }

    /// The cosmic-config handle on the per-user directory: `Some(None)`
    /// when the directory does not exist (no entry, no Problem), `None`
    /// when it cannot be opened. Creates nothing: the directory exists
    /// before `with_custom_path` is called. The handle has no system
    /// path, so no read falls back to a system default.
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

/// Reads per-user entries, collecting Problems in call order.
struct Reader {
    config: cosmic_config::Config,
    problems: Vec<Problem>,
}

impl Reader {
    // The per-user value of a key, or `None` when it is not usable. Only
    // `get_local` is called: `get` would fall back to the system default.

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

    /// What a development build stored for its CPU row, the last one chosen.
    /// An entry that cannot be read is no problem: it counts as absent.
    fn old_cpu_source(&self) -> Option<String> {
        let read = self
            .config
            .get_local::<Vec<(String, String)>>(OLD_ROW_SOURCES);
        let pairs = read.unwrap_or_default();
        let last = pairs.into_iter().rfind(|(row, _)| row == OLD_CPU_ROW);
        last.map(|(_, source)| source)
    }

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

    /// `poll_interval_secs`; a value out of range is passed on (core
    /// clamps it) and reported as clamped.
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

/// cosmic-config's per-user configuration root: in Flatpak
/// `HOST_XDG_CONFIG_HOME` or `$HOME/.config`; else `XDG_CONFIG_HOME`
/// when it is absolute, or `$HOME/.config`.
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
