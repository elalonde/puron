//! Tests of the provider registry (`puron_engine::registry`): the table
//! of providers and the start and stop actions a change of settings
//! asks for. The engine tests of those actions are in `c_engine.rs`.
//!
//! `c_src_registry_6` is also run by `tools/read-only-run.sh`, and one
//! more registry test is the script `tools/c_src_registry_7.sh`.

use puron_core::order::PROVIDER_RANK;
use puron_core::rows::Provider::{self, *};
use puron_core::settings::Settings;
use puron_engine::registry::{self, Action, Entry, ProviderKind, PROVIDERS};

/// The time limit of each test here, in seconds.
const LIMIT: u64 = 5;

/// Expects the provider table to hold exactly these five entries, each
/// with its settings key and kind, and its order to be the provider
/// order of `PROVIDER_RANK`.
#[test]
fn c_src_registry_1() {
    puron_testkit::limit("C-src.registry-1", LIMIT, || {
        let entry = |provider, key, kind| Entry {
            provider,
            key,
            kind,
        };
        // The provider table as it must be, written out in full.
        let expected = [
            entry(LmSensors, "provider_lm_sensors", ProviderKind::InProcess),
            entry(Liquidctl, "provider_liquidctl", ProviderKind::ChildRun),
            entry(Nvidia, "provider_nvidia", ProviderKind::InProcess),
            entry(Udisks2, "provider_udisks2", ProviderKind::InProcess),
            entry(NvmeCli, "provider_nvme", ProviderKind::InProcess),
        ];
        assert_eq!(PROVIDERS, expected);
        let order: Vec<Provider> = PROVIDERS.iter().map(|e| e.provider).collect();
        assert_eq!(order, PROVIDER_RANK, "table order is not the provider rank");
    });
}

/// The default settings with every provider switched on.
fn all_on() -> Settings {
    Settings {
        provider_lm_sensors: true,
        provider_liquidctl: true,
        provider_nvidia: true,
        provider_udisks2: true,
        provider_nvme: true,
        ..Settings::default()
    }
}

/// `s` with every setting that is not a provider switch changed to a
/// value other than its default; the switches stay as in `s`.
fn other_keys_changed(s: &Settings) -> Settings {
    let d = Settings::default();
    Settings {
        poll_interval_secs: 9,
        show_panel_icons: !d.show_panel_icons,
        temperature_unit: puron_core::settings::TemperatureUnit::Fahrenheit,
        show_temperature_unit: !d.show_temperature_unit,
        show_rotation_rate_unit: !d.show_rotation_rate_unit,
        show_voltage_unit: !d.show_voltage_unit,
        show_power_unit: !d.show_power_unit,
        show_decimals: !d.show_decimals,
        show_temperature: !d.show_temperature,
        show_rotation_rate: !d.show_rotation_rate,
        show_voltage: !d.show_voltage,
        show_power: !d.show_power,
        debug_logging: !d.debug_logging,
        pinned: Some(vec!["x".to_string()]),
        old_cpu_source: Some("x".to_string()),
        ..s.clone()
    }
}

/// The actions for a change of settings. Expects, at engine start with
/// the default settings, a start of lm-sensors only; from the defaults
/// to every provider on, a start of the other four in provider order;
/// no action when only settings other than the switches change; and
/// from every provider on back to the defaults, a stop of the four that
/// the defaults switch off.
#[test]
fn c_src_registry_2() {
    puron_testkit::limit("C-src.registry-2", LIMIT, || {
        let d = Settings::default();
        let on = all_on();
        assert_eq!(registry::actions(None, &d), [Action::Start(LmSensors)]);
        assert_eq!(
            registry::actions(Some(&d), &on),
            [
                Action::Start(Liquidctl),
                Action::Start(Nvidia),
                Action::Start(Udisks2),
                Action::Start(NvmeCli),
            ]
        );
        let other = other_keys_changed(&on);
        assert_eq!(registry::actions(Some(&on), &other), []);
        assert_eq!(
            registry::actions(Some(&on), &d),
            [
                Action::Stop(Liquidctl),
                Action::Stop(Nvidia),
                Action::Stop(Udisks2),
                Action::Stop(NvmeCli),
            ]
        );
    });
}

/// Keeps `tools/read-only-run.sh` in step with the provider table. The
/// script runs the read-only test of each adapter, listed in its own
/// `# provider=<key> test=<name>` lines, then this test, and exits 0
/// only when all six pass. Expects five entries in the provider table,
/// five such lines in the script, and each entry's settings key named
/// by exactly one of them.
#[test]
fn c_src_registry_6() {
    puron_testkit::limit("C-src.registry-6", LIMIT, || {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/read-only-run.sh");
        let script = std::fs::read_to_string(path).expect("tools/read-only-run.sh");
        let lines: Vec<&str> = script
            .lines()
            .filter(|l| l.starts_with("# provider="))
            .collect();
        assert_eq!(PROVIDERS.len(), 5, "the provider table has not five rows");
        assert_eq!(
            lines.len(),
            5,
            "the script's table has not five lines: {lines:?}"
        );
        for entry in PROVIDERS {
            let tag = format!("# provider={} test=c_src_", entry.key);
            let n = lines.iter().filter(|l| l.starts_with(&tag)).count();
            assert_eq!(n, 1, "{} named {n} times in the script's table", entry.key);
        }
    });
}

/// A provider that stays on is stopped and then started when a choice
/// of its source changes. Only lm-sensors has one: whether it reads the
/// chips of drives (`provider_lm_sensors_drives`). Expects, for a change
/// of that setting in either direction while lm-sensors is on, a stop
/// and then a start of lm-sensors and no action for any other provider;
/// at engine start, a start only; and while lm-sensors is off, or goes
/// on or off in the same change, only what its own switch gives.
#[test]
fn c_src_registry_8() {
    puron_testkit::limit("C-src.registry-8", LIMIT, || {
        let no_drives = |s: &Settings| Settings {
            provider_lm_sensors_drives: false,
            ..s.clone()
        };
        let restart = [Action::Stop(LmSensors), Action::Start(LmSensors)];
        let d = Settings::default();
        assert_eq!(registry::actions(Some(&d), &no_drives(&d)), restart);
        assert_eq!(registry::actions(Some(&no_drives(&d)), &d), restart);
        assert_eq!(registry::actions(Some(&no_drives(&d)), &no_drives(&d)), []);
        assert_eq!(
            registry::actions(None, &no_drives(&d)),
            [Action::Start(LmSensors)]
        );
        // Every provider on: only lm-sensors is restarted.
        let on = all_on();
        assert_eq!(registry::actions(Some(&on), &no_drives(&on)), restart);
        for e in PROVIDERS {
            let p = e.provider;
            assert_eq!(
                registry::restarts(&on, &no_drives(&on), p),
                p == LmSensors,
                "{p:?}"
            );
            assert!(!registry::restarts(&on, &on, p), "{p:?}");
            assert!(
                !registry::restarts(&on, &other_keys_changed(&on), p),
                "{p:?}"
            );
        }
        // lm-sensors off: its switch alone decides.
        let lm_off = Settings {
            provider_lm_sensors: false,
            ..d.clone()
        };
        assert_eq!(registry::actions(Some(&lm_off), &no_drives(&lm_off)), []);
        assert_eq!(
            registry::actions(Some(&d), &no_drives(&lm_off)),
            [Action::Stop(LmSensors)]
        );
        assert_eq!(
            registry::actions(Some(&no_drives(&lm_off)), &d),
            [Action::Start(LmSensors)]
        );
    });
}
