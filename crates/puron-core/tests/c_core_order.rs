//! Tests of the row order, 2 s each. Fixtures are under
//! crates/puron-testkit/fixtures/raw/. Every input runs as given and
//! reversed: `order` takes its rows in any order.

// The cases of one test are named `<name>__<case>`.
#![allow(non_snake_case)]

use std::collections::BTreeMap;

use puron_core::order;
use puron_core::rows::{self, Class, Provider, RawReading, Row, ShownClasses};

const LIMIT: u64 = 2;

/// Set in the re-executed child of the locale test (c_core_order_10).
const LOCALE_CHILD: &str = "PURON_ORDER_LOCALE_CHILD";

fn text(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

fn load(name: &str) -> Vec<RawReading> {
    serde_json::from_str(&text(name)).expect("parse fixture")
}

fn load_set(name: &str, set: &str) -> Vec<RawReading> {
    let mut sets: BTreeMap<String, Vec<RawReading>> =
        serde_json::from_str(&text(name)).expect("parse fixture");
    sets.remove(set).expect("set in fixture")
}

/// `rows`, then `order`; once as given, once reversed.
fn ordered(raw: &[RawReading]) -> [Vec<Row>; 2] {
    let given = rows::rows(raw, &ShownClasses::ALL);
    let mut reversed = given.clone();
    reversed.reverse();
    [order::order(given), order::order(reversed)]
}

fn names(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| r.name.clone()).collect()
}

/// Asserts the names of both runs.
fn assert_names(raw: &[RawReading], want: &[&str]) {
    for out in ordered(raw) {
        assert_eq!(names(&out), want);
    }
}

/// Asserts name, provider and position of both runs.
fn assert_rows(raw: &[RawReading], want: &[(&str, Provider, u32)]) {
    for out in ordered(raw) {
        let got: Vec<(&str, Provider, u32)> = out
            .iter()
            .map(|r| (r.name.as_str(), r.provider, r.position))
            .collect();
        assert_eq!(got, want);
    }
}

const CAPTURE_ORDER: [&str; 12] = [
    "Core 0", "Core 1", "Core 2", "Core 3", "CPU", "CPU temp", "WiFi", "Sensor 1", "Sensor 2",
    "SSD", "CPU fan", "in0",
];

#[test]
fn c_core_order_1() {
    puron_testkit::limit("C-core.order-1", LIMIT, || {
        assert_names(&load("capture.json"), &CAPTURE_ORDER);
    });
}

#[test]
fn c_core_order_2() {
    puron_testkit::limit("C-core.order-2", LIMIT, || {
        assert_rows(
            &load_set("order.json", "gpu"),
            &[
                ("zeta", Provider::LmSensors, 0),
                ("alpha", Provider::LmSensors, 1),
                ("Beta", Provider::Nvidia, 0),
            ],
        );
    });
}

#[test]
fn c_core_order_3() {
    puron_testkit::limit("C-core.order-3", LIMIT, || {
        assert_names(&load_set("order.json", "families"), &["m", "z", "a"]);
    });
}

#[test]
fn c_core_order_4() {
    puron_testkit::limit("C-core.order-4", LIMIT, || {
        assert_names(&load_set("order.json", "fans"), &["A Fan", "b"]);
    });
}

#[test]
fn c_core_order_5() {
    puron_testkit::limit("C-core.order-5", LIMIT, || {
        assert_names(&load_set("order.json", "voltages"), &["A Pump", "b"]);
    });
}

#[test]
fn c_core_order_6() {
    puron_testkit::limit("C-core.order-6", LIMIT, || {
        assert_names(&load_set("order.json", "power"), &["a", "b"]);
    });
}

#[test]
fn c_core_order_7() {
    puron_testkit::limit("C-core.order-7", LIMIT, || {
        // One row per class, in reverse class order.
        let raw: Vec<RawReading> = serde_json::from_str(
            r#"[
            {"provider": "lm-sensors", "position": 0, "kind": "power", "chip": "amdgpu-pci-0300", "name": "p0", "key": "k0", "value": 10},
            {"provider": "lm-sensors", "position": 1, "kind": "voltage", "chip": "puron-isa-0000", "name": "v1", "key": "k1", "value": 12},
            {"provider": "lm-sensors", "position": 2, "kind": "fan", "chip": "puron-isa-0000", "name": "f2", "key": "k2", "value": 1000},
            {"provider": "lm-sensors", "position": 3, "kind": "temperature", "chip": "puron-isa-0000", "name": "t3", "key": "k3", "value": 40}
            ]"#,
        )
        .expect("parse readings");
        for out in ordered(&raw) {
            let classes: Vec<Class> = out.iter().map(|r| r.class).collect();
            assert_eq!(
                classes,
                [Class::Temperature, Class::Fan, Class::Voltage, Class::Power]
            );
        }
    });
}

#[test]
fn c_core_order_8() {
    puron_testkit::limit("C-core.order-8", LIMIT, || {
        assert_names(
            &load_set("names.json", "N1"),
            &["Core 1", "Core 10", "Core 2", "CPU", "CPU temp"],
        );
    });
}

#[test]
fn c_core_order_9() {
    puron_testkit::limit("C-core.order-9", LIMIT, || {
        assert_names(&load_set("names.json", "N2"), &["ssb", "ßa"]);
        assert_names(&load_set("names.json", "N3"), &["ſa", "sb"]);
        assert_names(&load_set("names.json", "N4"), &["a_", "aa"]);
    });
}

/// The c_core_order_10 checks under `LC_ALL=<locale>`: the parent re-runs
/// in a child with that setting; the child takes the locale for the
/// process (as a program that calls `setlocale` does) and checks.
fn locale_case(test: &str, locale: &str) {
    if std::env::var_os(LOCALE_CHILD).is_some() {
        let set = unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) };
        assert!(!set.is_null(), "setlocale for LC_ALL={locale}");
        assert_names(&load("capture.json"), &CAPTURE_ORDER);
        assert_names(&load_set("names.json", "N5"), &["i1", "I2"]);
        return;
    }
    if locale != "C" {
        puron_testkit::host_locale(locale);
    }
    let status = puron_testkit::rerun_self(test)
        .env(LOCALE_CHILD, "1")
        .env("LC_ALL", locale)
        .status()
        .expect("run child");
    assert!(status.success(), "child under LC_ALL={locale}: {status}");
}

#[test]
fn c_core_order_10__tr() {
    puron_testkit::limit("C-core.order-10", LIMIT, || {
        locale_case("c_core_order_10__tr", "tr_TR.UTF-8")
    });
}

#[test]
fn c_core_order_10__c() {
    puron_testkit::limit("C-core.order-10", LIMIT, || {
        locale_case("c_core_order_10__c", "C")
    });
}

#[test]
fn c_core_order_11__ssd() {
    puron_testkit::limit("C-core.order-11", LIMIT, || {
        assert_rows(
            &load_set("ties.json", "SSD"),
            &[
                ("SSD", Provider::LmSensors, 1),
                ("SSD", Provider::LmSensors, 3),
                ("SSD", Provider::Udisks2, 0),
                ("SSD", Provider::NvmeCli, 0),
            ],
        );
    });
}

#[test]
fn c_core_order_11__fan() {
    puron_testkit::limit("C-core.order-11", LIMIT, || {
        assert_rows(
            &load_set("ties.json", "A Fan"),
            &[
                ("A Fan", Provider::LmSensors, 0),
                ("A Fan", Provider::Liquidctl, 0),
            ],
        );
    });
}

#[test]
fn c_core_order_12() {
    puron_testkit::limit("C-core.order-12", LIMIT, || {
        assert_names(&load_set("ties.json", "CPU1"), &["cpu", "CPU"]);
        assert_names(&load_set("ties.json", "CPU2"), &["cpu", "CPU"]);
        assert_names(&load_set("ties.json", "Ab"), &["aB", "Ab"]);
        assert_names(&load_set("ties.json", "AB"), &["Ab", "AB"]);
    });
}

const KELVIN: &str = "\u{212A}";

#[test]
fn c_core_order_13() {
    puron_testkit::limit("C-core.order-13", LIMIT, || {
        let lm = Provider::LmSensors;
        assert_rows(
            &load_set("ties.json", "K1"),
            &[(KELVIN, lm, 0), ("K", lm, 1)],
        );
        assert_rows(
            &load_set("ties.json", "K2"),
            &[("K", lm, 0), (KELVIN, lm, 1)],
        );
    });
}

#[test]
fn c_core_order_14() {
    puron_testkit::limit("C-core.order-14", LIMIT, || {
        assert_names(&load_set("ties.json", "KA"), &["Ka", "\u{212A}A"]);
        assert_names(&load_set("ties.json", "Kk1"), &["k", KELVIN]);
        assert_names(&load_set("ties.json", "Kk2"), &["k", KELVIN]);
    });
}

#[test]
fn c_core_order_15() {
    puron_testkit::limit("C-core.order-15", LIMIT, || {
        assert_eq!(order::FOLD_UNICODE_VERSION, (17, 0, 0), "folding table");
        assert_eq!(
            char::UNICODE_VERSION,
            (17, 0, 0),
            "toolchain character data"
        );
    });
}
