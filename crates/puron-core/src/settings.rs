//! Setting types, defaults and ranges.

use crate::rows::MAXIMUM_IDENTITY;

/// Default of `poll_interval_secs`.
pub const POLL_INTERVAL_DEFAULT: u32 = 5;
/// Range of `poll_interval_secs`; a value outside is clamped.
pub const POLL_INTERVAL_MIN: u32 = 1;
pub const POLL_INTERVAL_MAX: u32 = 60;

/// `temperature_unit`. Its stored form is the variant name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

/// The settings record: one effective value per key.
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
    pub show_temperature: bool,
    pub show_rotation_rate: bool,
    pub show_voltage: bool,
    pub show_power: bool,
    pub group_temperature: bool,
    pub group_rotation_rate: bool,
    pub group_voltage: bool,
    pub debug_logging: bool,
    pub pinned: Vec<String>,
}

/// The stored values: `None` is "not usable" (not stored,
/// unreadable or of the wrong type; `config` decides).
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
    pub show_temperature: Option<bool>,
    pub show_rotation_rate: Option<bool>,
    pub show_voltage: Option<bool>,
    pub show_power: Option<bool>,
    pub group_temperature: Option<bool>,
    pub group_rotation_rate: Option<bool>,
    pub group_voltage: Option<bool>,
    pub debug_logging: Option<bool>,
    pub pinned: Option<Vec<String>>,
}

/// The default pinned list: `@maximum` alone.
pub fn default_pinned() -> Vec<String> {
    vec![MAXIMUM_IDENTITY.to_string()]
}

/// The settings record of the stored values. Each key is taken on
/// its own.
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
        show_temperature: s.show_temperature.unwrap_or(true),
        show_rotation_rate: s.show_rotation_rate.unwrap_or(true),
        show_voltage: s.show_voltage.unwrap_or(true),
        show_power: s.show_power.unwrap_or(true),
        group_temperature: s.group_temperature.unwrap_or(true),
        group_rotation_rate: s.group_rotation_rate.unwrap_or(true),
        group_voltage: s.group_voltage.unwrap_or(true),
        debug_logging: s.debug_logging.unwrap_or(false),
        pinned: s.pinned.clone().unwrap_or_else(default_pinned),
    }
}

impl Default for Settings {
    /// The defaults: every key "not usable".
    fn default() -> Settings {
        settings(&Stored::default())
    }
}
