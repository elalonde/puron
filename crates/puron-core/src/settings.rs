//! The settings: what each one holds, its default, and how a stored value
//! becomes the value in effect. Reading and writing the settings store is
//! done in the puron-config crate.

use crate::rows::ShownClasses;

/// How often the sensors are read, in seconds, when no usable interval
/// is stored.
pub const POLL_INTERVAL_DEFAULT: u32 = 5;
/// The shortest poll interval in seconds; a shorter stored value is
/// raised to it.
pub const POLL_INTERVAL_MIN: u32 = 1;
/// The longest poll interval in seconds; a longer stored value is lowered
/// to it.
pub const POLL_INTERVAL_MAX: u32 = 60;

/// The unit of temperatures. It is stored as the variant name, `Celsius`
/// or `Fahrenheit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

/// The settings in effect: one value per setting, after defaults and the
/// interval range have been applied to the stored values (see
/// [`settings`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub poll_interval_secs: u32,
    pub show_panel_icons: bool,
    pub temperature_unit: TemperatureUnit,
    pub show_temperature_unit: bool,
    pub show_rotation_rate_unit: bool,
    pub show_voltage_unit: bool,
    pub show_power_unit: bool,
    pub show_decimals: bool,
    pub provider_lm_sensors: bool,
    pub provider_liquidctl: bool,
    pub provider_nvidia: bool,
    pub provider_udisks2: bool,
    pub provider_nvme: bool,
    /// Whether lm-sensors reads the chips of drives. When it is off,
    /// lm-sensors reads no value of such a chip, so the drive is not
    /// asked for its temperature; the other chips are read as before.
    /// It has no effect while `provider_lm_sensors` is off.
    pub provider_lm_sensors_drives: bool,
    pub show_temperature: bool,
    pub show_rotation_rate: bool,
    pub show_voltage: bool,
    pub show_power: bool,
    pub debug_logging: bool,
    /// The pinned list as stored. `None` when nothing was ever stored, as
    /// on a new install, which starts with the popup's default lines
    /// pinned (see `menu::Shown::new_install_pins`).
    pub pinned: Option<Vec<String>>,
    /// The sensor that the single CPU row of development builds was last
    /// set to read: an lm-sensors key, or `@maximum` for the highest CPU
    /// temperature. It is used only to move a stored `@cpu` pin to the
    /// matching line (see `pins::moved`).
    pub old_cpu_source: Option<String>,
}

/// The values read from the settings store, one per setting. `None`
/// means there is no usable value: it is missing, unreadable or of the
/// wrong type. The crate that reads the store decides which.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stored {
    pub poll_interval_secs: Option<u32>,
    pub show_panel_icons: Option<bool>,
    pub temperature_unit: Option<TemperatureUnit>,
    pub show_temperature_unit: Option<bool>,
    pub show_rotation_rate_unit: Option<bool>,
    pub show_voltage_unit: Option<bool>,
    pub show_power_unit: Option<bool>,
    pub show_decimals: Option<bool>,
    pub provider_lm_sensors: Option<bool>,
    pub provider_liquidctl: Option<bool>,
    pub provider_nvidia: Option<bool>,
    pub provider_udisks2: Option<bool>,
    pub provider_nvme: Option<bool>,
    pub provider_lm_sensors_drives: Option<bool>,
    pub show_temperature: Option<bool>,
    pub show_rotation_rate: Option<bool>,
    pub show_voltage: Option<bool>,
    pub show_power: Option<bool>,
    pub debug_logging: Option<bool>,
    pub pinned: Option<Vec<String>>,
    pub old_cpu_source: Option<String>,
}

/// The settings in effect for `stored`. Each setting is decided on its
/// own: a missing value takes its default, and a poll interval outside
/// `POLL_INTERVAL_MIN..=POLL_INTERVAL_MAX` is clamped to that range.
pub fn settings(stored: &Stored) -> Settings {
    let s = stored;
    Settings {
        poll_interval_secs: s.poll_interval_secs.map_or(POLL_INTERVAL_DEFAULT, |v| {
            v.clamp(POLL_INTERVAL_MIN, POLL_INTERVAL_MAX)
        }),
        show_panel_icons: s.show_panel_icons.unwrap_or(true),
        temperature_unit: s.temperature_unit.unwrap_or(TemperatureUnit::Celsius),
        show_temperature_unit: s.show_temperature_unit.unwrap_or(true),
        show_rotation_rate_unit: s.show_rotation_rate_unit.unwrap_or(true),
        show_voltage_unit: s.show_voltage_unit.unwrap_or(true),
        show_power_unit: s.show_power_unit.unwrap_or(true),
        show_decimals: s.show_decimals.unwrap_or(false),
        provider_lm_sensors: s.provider_lm_sensors.unwrap_or(true),
        provider_liquidctl: s.provider_liquidctl.unwrap_or(false),
        provider_nvidia: s.provider_nvidia.unwrap_or(false),
        provider_udisks2: s.provider_udisks2.unwrap_or(false),
        provider_nvme: s.provider_nvme.unwrap_or(false),
        provider_lm_sensors_drives: s.provider_lm_sensors_drives.unwrap_or(true),
        show_temperature: s.show_temperature.unwrap_or(true),
        show_rotation_rate: s.show_rotation_rate.unwrap_or(true),
        show_voltage: s.show_voltage.unwrap_or(true),
        show_power: s.show_power.unwrap_or(true),
        debug_logging: s.debug_logging.unwrap_or(false),
        pinned: s.pinned.clone(),
        old_cpu_source: s.old_cpu_source.clone(),
    }
}

impl Settings {
    /// What these settings let the popup and the panel show.
    pub fn shown_classes(&self) -> ShownClasses {
        ShownClasses {
            temperature: self.show_temperature,
            fan: self.show_rotation_rate,
            voltage: self.show_voltage,
            power: self.show_power,
            lm_sensors_drives: self.provider_lm_sensors_drives,
        }
    }
}

impl Default for Settings {
    /// The default settings: those of a store with no usable value.
    fn default() -> Settings {
        settings(&Stored::default())
    }
}
