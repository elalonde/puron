//! Tests of the setting defaults and ranges, 2 s each. Stored values are
//! given inline.

use puron_core::rows::MAXIMUM_IDENTITY;
use puron_core::settings::{self, Settings, Stored, TemperatureUnit};

const LIMIT: u64 = 2;

/// The default of each key, written out here rather than taken from the
/// code under test.
fn defaults() -> Settings {
    Settings {
        poll_interval_secs: 5,
        show_panel_icons: true,
        temperature_unit: TemperatureUnit::Celsius,
        show_temperature_unit: true,
        show_rotation_rate_unit: true,
        show_voltage_unit: true,
        show_power_unit: true,
        show_decimals: false,
        provider_lm_sensors: true,
        provider_liquidctl: false,
        provider_nvidia: false,
        provider_udisks2: false,
        provider_nvme: false,
        show_temperature: true,
        show_rotation_rate: true,
        show_voltage: true,
        show_power: true,
        group_temperature: true,
        group_rotation_rate: true,
        group_voltage: true,
        debug_logging: false,
        pinned: vec!["@maximum".to_string()],
    }
}

#[test]
fn c_core_settings_1() {
    puron_testkit::limit("C-core.settings-1", LIMIT, || {
        let s = settings::settings(&Stored::default());
        assert_eq!(s.pinned, ["@maximum"]);
        assert_eq!(s.pinned, [MAXIMUM_IDENTITY]);
        assert_eq!(settings::default_pinned(), ["@maximum"]);
    });
}

#[test]
fn c_core_settings_2() {
    puron_testkit::limit("C-core.settings-2", LIMIT, || {
        assert_eq!(settings::settings(&Stored::default()), defaults());
        assert_eq!(Settings::default(), defaults());
    });
}

#[test]
fn c_core_settings_3() {
    puron_testkit::limit("C-core.settings-3", LIMIT, || {
        for (stored, want) in [(0, 1), (1, 1), (60, 60), (61, 60), (4294967295, 60)] {
            let s = settings::settings(&Stored {
                poll_interval_secs: Some(stored),
                ..Stored::default()
            });
            assert_eq!(s.poll_interval_secs, want, "stored {stored}");
        }
    });
}

#[test]
fn c_core_settings_4() {
    puron_testkit::limit("C-core.settings-4", LIMIT, || {
        let s = settings::settings(&Stored {
            poll_interval_secs: None,
            ..Stored::default()
        });
        assert_eq!(s.poll_interval_secs, 5);
    });
}

#[test]
fn c_core_settings_5() {
    puron_testkit::limit("C-core.settings-5", LIMIT, || {
        let stored = Stored {
            poll_interval_secs: Some(7),
            show_panel_icons: Some(false),
            temperature_unit: Some(TemperatureUnit::Fahrenheit),
            show_temperature_unit: Some(false),
            show_rotation_rate_unit: Some(false),
            show_voltage_unit: Some(false),
            show_power_unit: Some(false),
            show_decimals: Some(true),
            provider_lm_sensors: Some(false),
            provider_liquidctl: Some(true),
            provider_nvidia: Some(true),
            provider_udisks2: Some(true),
            provider_nvme: Some(true),
            show_temperature: Some(false),
            show_rotation_rate: Some(false),
            show_voltage: Some(false),
            show_power: Some(false),
            group_temperature: Some(false),
            group_rotation_rate: Some(false),
            group_voltage: Some(false),
            debug_logging: Some(true),
            pinned: Some(vec!["a".to_string(), "a".to_string()]),
        };
        let want = Settings {
            poll_interval_secs: 7,
            show_panel_icons: false,
            temperature_unit: TemperatureUnit::Fahrenheit,
            show_temperature_unit: false,
            show_rotation_rate_unit: false,
            show_voltage_unit: false,
            show_power_unit: false,
            show_decimals: true,
            provider_lm_sensors: false,
            provider_liquidctl: true,
            provider_nvidia: true,
            provider_udisks2: true,
            provider_nvme: true,
            show_temperature: false,
            show_rotation_rate: false,
            show_voltage: false,
            show_power: false,
            group_temperature: false,
            group_rotation_rate: false,
            group_voltage: false,
            debug_logging: true,
            pinned: vec!["a".to_string(), "a".to_string()],
        };
        // Every value differs from its default, so a stored value ignored
        // for any one key shows.
        assert_ne!(want.poll_interval_secs, defaults().poll_interval_secs);
        assert_eq!(settings::settings(&stored), want);

        let one_unusable = Stored {
            show_power: None,
            ..stored
        };
        assert_eq!(
            settings::settings(&one_unusable),
            Settings {
                show_power: true,
                ..want
            }
        );
    });
}
