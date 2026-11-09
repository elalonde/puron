//! Tests of the temperature value rules: which values count, and the
//! Maximum line, the popup line with the highest temperature of a device.
//! Each test must finish within 2 seconds (`puron_testkit::limit`). The
//! fixtures are capture.json and summary.json under
//! crates/puron-testkit/fixtures/raw/; the sets of summary.json are named
//! by letters. Rows reach `summary` through `rows`, and Maximum lines are
//! read from `menu::shown`.

use std::collections::BTreeMap;

use puron_core::menu;
use puron_core::order;
use puron_core::rows::{self, Class, RawReading, Row, ShownClasses};
use puron_core::settings::Settings;
use puron_core::summary;

const LIMIT: u64 = 2;

fn capture() -> Vec<RawReading> {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", "capture.json"))
        .expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
}

/// The sets of summary.json, by name. JSON has no NaN or infinity, so the
/// testkit loader builds set G's non-finite values from strings.
fn sets() -> BTreeMap<String, Vec<RawReading>> {
    puron_testkit::raw_sets("summary.json", |r: &mut RawReading, v| r.value = Some(v))
}

fn set_rows(set: &str) -> Vec<Row> {
    rows::rows(&sets()[set], &ShownClasses::ALL)
}

fn values(rows: &[Row]) -> Vec<Option<f64>> {
    rows.iter().map(|r| r.value).collect()
}

/// The highest temperature of `rows` that counts.
fn highest(rows: &[Row]) -> Option<f64> {
    let temperatures = rows.iter().filter(|r| r.class == Class::Temperature);
    summary::highest(temperatures.map(|r| r.value))
}

/// The values of the Maximum lines that the popup shows for `raw`.
fn maximum_lines(raw: &[RawReading], settings: &Settings) -> Vec<Option<f64>> {
    menu::shown(raw, settings)
        .lines()
        .filter(|l| l.is_maximum())
        .map(|l| l.value)
        .collect()
}

fn set_maximum_lines(set: &str) -> Vec<Option<f64>> {
    maximum_lines(&sets()[set], &Settings::default())
}

/// Asserts that the rows are equal in every field but `device_chars`, with
/// values compared bit for bit, so that negative zero differs from zero.
fn assert_same_rows(got: &[Row], want: &[Row]) {
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert_eq!(
            (g.class, g.family, &g.name, g.provider, g.position, &g.key),
            (w.class, w.family, &w.name, w.provider, w.position, &w.key)
        );
        assert_eq!(
            g.value.map(f64::to_bits),
            w.value.map(f64::to_bits),
            "row {}",
            g.name
        );
    }
}

#[test]
fn c_core_summary_1() {
    puron_testkit::limit("C-core.summary-1", LIMIT, || {
        let input = order::order(rows::rows(&capture(), &ShownClasses::ALL));
        let out = summary::summary(input.clone());
        assert_eq!(highest(&out), Some(73.0));
        assert_same_rows(&out, &input);
        let shown = menu::shown(&capture(), &Settings::default());
        let cpu = shown.line("@maximum/cpu").expect("the CPU's Maximum line");
        assert_eq!(cpu.value, Some(73.0));
    });
}

#[test]
fn c_core_summary_2() {
    puron_testkit::limit("C-core.summary-2", LIMIT, || {
        let a = set_rows("A");
        let mut reversed = a.clone();
        reversed.reverse();
        for input in [a, reversed] {
            assert_eq!(highest(&summary::summary(input)), Some(70.0));
        }
    });
}

#[test]
fn c_core_summary_3() {
    puron_testkit::limit("C-core.summary-3", LIMIT, || {
        let out = summary::summary(set_rows("B"));
        assert_eq!(values(&out), [Some(10.0), None, Some(-1.5), Some(-2.0)]);
        assert_eq!(highest(&out), Some(10.0));
    });
}

#[test]
fn c_core_summary_4() {
    puron_testkit::limit("C-core.summary-4", LIMIT, || {
        let out = summary::summary(set_rows("C"));
        assert_eq!(values(&out), [None, None]);
        assert_eq!(highest(&out), None);
        // The device's Maximum line is still shown, with no value.
        assert_eq!(set_maximum_lines("C"), [None]);
    });
}

#[test]
fn c_core_summary_5() {
    puron_testkit::limit("C-core.summary-5", LIMIT, || {
        let d = summary::summary(set_rows("D"));
        assert_eq!(highest(&d), Some(0.0));
        assert_eq!(set_maximum_lines("D"), [Some(0.0)]);

        let e_in = set_rows("E");
        assert!(
            e_in[0].value.expect("E has a value").is_sign_negative(),
            "fixture E is negative zero"
        );
        let e = summary::summary(e_in.clone());
        assert_same_rows(&e, &e_in);
        let max = highest(&e).expect("a number");
        assert!(max == 0.0 && max.is_sign_negative(), "maximum {max:?}");

        // Set H holds negative zero and zero: in either order, the highest
        // is zero.
        let h = set_rows("H");
        assert!(
            h[0].value.expect("H has a value").is_sign_negative(),
            "fixture H starts with negative zero"
        );
        let mut reversed = h.clone();
        reversed.reverse();
        for input in [h, reversed] {
            let out = summary::summary(input);
            assert_eq!(
                highest(&out).map(f64::to_bits),
                Some(0.0f64.to_bits()),
                "maximum"
            );
        }
    });
}

#[test]
fn c_core_summary_6() {
    puron_testkit::limit("C-core.summary-6", LIMIT, || {
        for set in ["F", "F-empty"] {
            let input = set_rows(set);
            assert_eq!(input.len(), if set == "F" { 2 } else { 0 });
            assert_eq!(highest(&summary::summary(input)), None);
            assert_eq!(set_maximum_lines(set), []);
        }
    });
}

#[test]
fn c_core_summary_7() {
    puron_testkit::limit("C-core.summary-7", LIMIT, || {
        let no_temperature = ShownClasses {
            temperature: false,
            ..ShownClasses::ALL
        };
        let input = rows::rows(&capture(), &no_temperature);
        assert!(!input.is_empty());
        assert_eq!(highest(&summary::summary(input)), None);
        let hidden = Settings {
            show_temperature: false,
            ..Settings::default()
        };
        assert_eq!(maximum_lines(&capture(), &hidden), []);

        let no_fan = ShownClasses {
            fan: false,
            ..ShownClasses::ALL
        };
        assert_eq!(
            highest(&summary::summary(rows::rows(&capture(), &no_fan))),
            Some(73.0)
        );
    });
}

#[test]
fn c_core_summary_8() {
    puron_testkit::limit("C-core.summary-8", LIMIT, || {
        let g = summary::summary(set_rows("G"));
        assert_eq!(values(&g), [None, None, None]);
        assert_eq!(highest(&g), None);
        assert_eq!(set_maximum_lines("G"), [None]);

        let g20 = summary::summary(set_rows("G-20"));
        assert_eq!(values(&g20), [None, None, None, Some(20.0)]);
        assert_eq!(highest(&g20), Some(20.0));
        assert_eq!(set_maximum_lines("G-20"), [Some(20.0)]);
    });
}
