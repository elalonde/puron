//! Tests of value text. Each test must finish within 2 seconds
//! (`puron_testkit::limit`). Values are given inline, except in
//! c_core_format_1, which reads the fixture capture.json. Each result is
//! compared as one string: the number, then an English unit symbol, or
//! "no value".

use puron_core::format::{self, Unit};
use puron_core::rows::{self, Class, RawReading, ShownClasses};
use puron_core::settings::{Settings, TemperatureUnit};

const LIMIT: u64 = 2;

const T: Class = Class::Temperature;
const F: Class = Class::Fan;
const V: Class = Class::Voltage;
const P: Class = Class::Power;

/// An English symbol for each unit; empty when the unit is hidden.
fn suffix(unit: Option<Unit>) -> &'static str {
    match unit {
        None => "",
        Some(Unit::Celsius) => "°C",
        Some(Unit::Fahrenheit) => "°F",
        Some(Unit::Rpm) => " rpm",
        Some(Unit::Volt) => "V",
        Some(Unit::Watt) => "W",
    }
}

fn text(class: Class, value: Option<f64>, settings: &Settings) -> String {
    match format::format(class, value, settings) {
        None => "no value".to_string(),
        Some(t) => format!("{}{}", t.number, suffix(t.unit)),
    }
}

/// The texts of `values`, each formatted as a value of `class`.
fn texts(class: Class, values: &[f64], settings: &Settings) -> Vec<String> {
    values
        .iter()
        .map(|&v| text(class, Some(v), settings))
        .collect()
}

fn defaults() -> Settings {
    Settings::default()
}

fn decimals(on: bool) -> Settings {
    Settings {
        show_decimals: on,
        ..Settings::default()
    }
}

fn fahrenheit(decimals_on: bool) -> Settings {
    Settings {
        temperature_unit: TemperatureUnit::Fahrenheit,
        show_decimals: decimals_on,
        ..Settings::default()
    }
}

#[test]
fn c_core_format_1() {
    puron_testkit::limit("C-core.format-1", LIMIT, || {
        let text_of = std::fs::read_to_string(puron_testkit::fixture("raw", "capture.json"))
            .expect("read fixture");
        let raw: Vec<RawReading> = serde_json::from_str(&text_of).expect("parse fixture");
        let got: Vec<String> = rows::rows(&raw, &ShownClasses::ALL)
            .iter()
            .map(|r| text(r.class, r.value, &defaults()))
            .collect();
        // The rows in fixture order under the default settings: whole
        // degrees Celsius, whole rpm, and volts with a sign and two decimals.
        assert_eq!(
            got,
            [
                "33°C", "38°C", "38°C", "45°C", "73°C", "65°C", "71°C", "64°C", "72°C", "0 rpm",
                "67°C", "+8.49V"
            ]
        );
    });
}

#[test]
fn c_core_format_2() {
    puron_testkit::limit("C-core.format-2", LIMIT, || {
        assert_eq!(
            texts(T, &[36.5, 0.5, 2.5, 37.5], &decimals(false)),
            ["36°C", "0°C", "2°C", "38°C"]
        );
        assert_eq!(
            texts(T, &[37.25, 0.25], &decimals(true)),
            ["37.2°C", "0.2°C"]
        );
    });
}

#[test]
fn c_core_format_3() {
    puron_testkit::limit("C-core.format-3", LIMIT, || {
        assert_eq!(texts(T, &[-0.4, -0.5], &decimals(false)), ["-0°C", "-0°C"]);
        assert_eq!(texts(T, &[-0.04], &decimals(true)), ["-0.0°C"]);
        assert_eq!(text(T, Some(-0.0), &decimals(false)), "-0°C");
        assert_eq!(text(T, Some(-0.0), &decimals(true)), "-0.0°C");
    });
}

#[test]
fn c_core_format_4() {
    puron_testkit::limit("C-core.format-4", LIMIT, || {
        assert_eq!(text(T, Some(9.25), &fahrenheit(true)), "48.7°F");
        assert_eq!(text(T, Some(9.25), &fahrenheit(false)), "49°F");
        assert_eq!(text(T, Some(1.25), &fahrenheit(true)), "34.2°F");
        assert_eq!(text(T, Some(2.5), &fahrenheit(false)), "36°F");
        assert_eq!(text(T, Some(37.85), &fahrenheit(false)), "100°F");
    });
}

#[test]
fn c_core_format_5() {
    puron_testkit::limit("C-core.format-5", LIMIT, || {
        assert_eq!(
            texts(T, &[0.0, 100.0, -0.0], &fahrenheit(true)),
            ["32.0°F", "212.0°F", "32.0°F"]
        );
    });
}

#[test]
fn c_core_format_6() {
    puron_testkit::limit("C-core.format-6", LIMIT, || {
        let celsius = Settings {
            show_temperature_unit: false,
            ..Settings::default()
        };
        assert_eq!(text(T, Some(37.85), &celsius), "38");
        let f = Settings {
            show_temperature_unit: false,
            ..fahrenheit(false)
        };
        assert_eq!(text(T, Some(37.85), &f), "100");
    });
}

#[test]
fn c_core_format_7() {
    puron_testkit::limit("C-core.format-7", LIMIT, || {
        let cases = [
            (F, 0.0, "0", "0 rpm"),
            (V, 8.494, "+8.49", "+8.49V"),
            (P, 1.5, "+1.50", "+1.50W"),
        ];
        let off = |class: Class| {
            let mut s = Settings::default();
            match class {
                Class::Fan => s.show_rotation_rate_unit = false,
                Class::Voltage => s.show_voltage_unit = false,
                Class::Power => s.show_power_unit = false,
                Class::Temperature => s.show_temperature_unit = false,
            }
            s
        };
        for (class, value, own_off, with_unit) in cases {
            assert_eq!(
                text(class, Some(value), &off(class)),
                own_off,
                "{class:?}, own switch off"
            );
            for other in [T, F, V, P] {
                if other != class {
                    assert_eq!(
                        text(class, Some(value), &off(other)),
                        with_unit,
                        "{class:?}, {other:?} off"
                    );
                }
            }
        }
    });
}

#[test]
fn c_core_format_8() {
    puron_testkit::limit("C-core.format-8", LIMIT, || {
        let on = decimals(true);
        assert_eq!(text(T, Some(37.85), &on), "37.9°C");
        assert_eq!(text(V, Some(8.494), &on), "+8.49V");
        assert_eq!(text(P, Some(1.5), &on), "+1.50W");
        assert_eq!(text(F, Some(1234.9), &on), "1234 rpm");
    });
}

#[test]
fn c_core_format_9() {
    puron_testkit::limit("C-core.format-9", LIMIT, || {
        let fans = [
            1234.9,
            -1.5,
            -0.5,
            -0.0,
            0.0,
            9.99e-7,
            1.5e21,
            2f64.powi(60),
        ];
        assert_eq!(
            texts(F, &fans, &defaults()),
            [
                "1234 rpm",
                "-1 rpm",
                "-0 rpm",
                "-0 rpm",
                "0 rpm",
                "0 rpm",
                "1500000000000000000000 rpm",
                "1152921504606846976 rpm"
            ]
        );
    });
}

#[test]
fn c_core_format_10() {
    puron_testkit::limit("C-core.format-10", LIMIT, || {
        let values = [0.0, -0.0, -0.001, 0.125, 0.375, -1.5];
        let want = ["+0.00", "-0.00", "-0.00", "+0.12", "+0.38", "-1.50"];
        let with = |unit: &str| {
            want.iter()
                .map(|w| format!("{w}{unit}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(V, &values, &defaults()), with("V"));
        assert_eq!(texts(P, &values, &defaults()), with("W"));
    });
}

#[test]
fn c_core_format_11() {
    puron_testkit::limit("C-core.format-11", LIMIT, || {
        assert_eq!(
            text(T, Some(1e21), &decimals(false)),
            "1000000000000000000000°C"
        );
        assert_eq!(
            text(T, Some(1.5e21), &decimals(true)),
            "1500000000000000000000.0°C"
        );
        assert_eq!(
            text(T, Some(999999999999999868928.0), &decimals(false)),
            "999999999999999868928°C"
        );
        assert_eq!(
            texts(V, &[1e21, -1.25e21], &defaults()),
            ["+1000000000000000000000.00V", "-1250000000000000000000.00V"]
        );
    });
}

#[test]
fn c_core_format_12() {
    puron_testkit::limit("C-core.format-12", LIMIT, || {
        for class in [T, F, V, P] {
            for settings in [defaults(), decimals(true), fahrenheit(true)] {
                assert_eq!(text(class, None, &settings), "no value", "{class:?}");
                for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    assert_eq!(text(class, Some(v), &settings), "no value", "{class:?} {v}");
                }
            }
        }
    });
}

#[test]
fn c_core_format_13() {
    puron_testkit::limit("C-core.format-13", LIMIT, || {
        assert_eq!(
            texts(T, &[0.35, 0.45, 45.05, 113.45], &decimals(true)),
            ["0.3°C", "0.5°C", "45.0°C", "113.5°C"]
        );
        assert_eq!(text(V, Some(2.675), &decimals(true)), "+2.67V");
    });
}
