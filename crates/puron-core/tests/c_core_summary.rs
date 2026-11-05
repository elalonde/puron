//! Tests of the Maximum and invalid temperatures, 2 s each. Fixtures are
//! under crates/puron-testkit/fixtures/raw/ (capture.json,
//! summary.json). Rows reach `summary` through `rows`.

use std::collections::BTreeMap;

use puron_core::order;
use puron_core::rows::{self, RawReading, Row, ShownClasses};
use puron_core::summary::{self, Summary};

const LIMIT: u64 = 2;

fn capture() -> Vec<RawReading> {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", "capture.json"))
        .expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
}

/// The sets of summary.json; set G's non-finite values come from the
/// testkit loader, since JSON has no NaN or infinity.
fn sets() -> BTreeMap<String, Vec<RawReading>> {
    puron_testkit::raw_sets("summary.json", |r: &mut RawReading, v| r.value = Some(v))
}

fn set_rows(set: &str) -> Vec<Row> {
    rows::rows(&sets()[set], &ShownClasses::ALL)
}

fn values(rows: &[Row]) -> Vec<Option<f64>> {
    rows.iter().map(|r| r.value).collect()
}

/// Rows equal in all fields; values compared by bits (negative zero, NaN).
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

/// Asserts Maximum and header are both `want`.
fn assert_max(out: &Summary, want: Option<Option<f64>>) {
    assert_eq!(out.maximum, want, "maximum");
    assert_eq!(out.header, want, "header");
}

#[test]
fn c_core_summary_1() {
    puron_testkit::limit("C-core.summary-1", LIMIT, || {
        let input = order::order(rows::rows(&capture(), &ShownClasses::ALL));
        let out = summary::summary(input.clone());
        assert_eq!(out.maximum, Some(Some(73.0)));
        assert_same_rows(&out.rows, &input);
    });
}

#[test]
fn c_core_summary_2() {
    puron_testkit::limit("C-core.summary-2", LIMIT, || {
        let a = set_rows("A");
        let mut reversed = a.clone();
        reversed.reverse();
        for input in [a, reversed] {
            assert_max(&summary::summary(input), Some(Some(70.0)));
        }
    });
}

#[test]
fn c_core_summary_3() {
    puron_testkit::limit("C-core.summary-3", LIMIT, || {
        let out = summary::summary(set_rows("B"));
        assert_eq!(
            values(&out.rows),
            [Some(10.0), None, Some(-1.5), Some(-2.0)]
        );
        assert_eq!(out.maximum, Some(Some(10.0)));
    });
}

#[test]
fn c_core_summary_4() {
    puron_testkit::limit("C-core.summary-4", LIMIT, || {
        let out = summary::summary(set_rows("C"));
        assert_eq!(values(&out.rows), [None, None]);
        assert_max(&out, Some(None));
    });
}

#[test]
fn c_core_summary_5() {
    puron_testkit::limit("C-core.summary-5", LIMIT, || {
        let d = summary::summary(set_rows("D"));
        assert_max(&d, Some(Some(0.0)));

        let e_in = set_rows("E");
        assert!(
            e_in[0].value.expect("E has a value").is_sign_negative(),
            "fixture E is negative zero"
        );
        let e = summary::summary(e_in.clone());
        assert_same_rows(&e.rows, &e_in);
        let max = e.maximum.expect("present").expect("a number");
        assert!(max == 0.0 && max.is_sign_negative(), "maximum {max:?}");
        assert_eq!(
            e.header.map(|h| h.map(f64::to_bits)),
            Some(Some(max.to_bits()))
        );

        // H: 0 is above negative zero in any order.
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
                out.maximum.map(|m| m.map(f64::to_bits)),
                Some(Some(0.0f64.to_bits())),
                "maximum"
            );
            assert_eq!(
                out.header.map(|m| m.map(f64::to_bits)),
                Some(Some(0.0f64.to_bits())),
                "header"
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
            assert_max(&summary::summary(input), None);
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
        assert_max(&summary::summary(input), None);

        let no_fan = ShownClasses {
            fan: false,
            ..ShownClasses::ALL
        };
        assert_max(
            &summary::summary(rows::rows(&capture(), &no_fan)),
            Some(Some(73.0)),
        );
    });
}

#[test]
fn c_core_summary_8() {
    puron_testkit::limit("C-core.summary-8", LIMIT, || {
        let g = summary::summary(set_rows("G"));
        assert_eq!(values(&g.rows), [None, None, None]);
        assert_max(&g, Some(None));

        let g20 = summary::summary(set_rows("G-20"));
        assert_eq!(values(&g20.rows), [None, None, None, Some(20.0)]);
        assert_max(&g20, Some(Some(20.0)));
    });
}
