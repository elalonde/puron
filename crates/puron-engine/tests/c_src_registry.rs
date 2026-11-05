//! Tests of the provider registry, 5 s each. `c_src_registry_6` is run
//! by `tools/read-only-run.sh`; one more registry check is the script
//! `tools/c_src_registry_7.sh`.

use puron_core::order::PROVIDER_RANK;
use puron_core::rows::Provider::{self, *};
use puron_core::settings::Settings;
use puron_engine::registry::{self, Action, Entry, ProviderKind, PROVIDERS};

const LIMIT: u64 = 5;

#[test]
fn c_src_registry_1() {
    puron_testkit::limit("C-src.registry-1", LIMIT, || {
        let entry = |provider, key, kind| Entry {
            provider,
            key,
            kind,
        };
        // The five entries of the provider table, written out.
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

/// `s` with every key that is not a switch changed from its default.
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
        group_temperature: !d.group_temperature,
        group_rotation_rate: !d.group_rotation_rate,
        group_voltage: !d.group_voltage,
        debug_logging: !d.debug_logging,
        pinned: vec!["x".to_string()],
        ..s.clone()
    }
}

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

/// The provider table has five entries and the table of
/// `tools/read-only-run.sh` names each provider once. The script runs
/// this test after the five adapters' read-only tests and exits 0 only
/// when all six pass.
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
