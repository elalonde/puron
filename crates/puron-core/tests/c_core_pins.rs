//! Tests of the pin and unpin rules, 2 s each. Fixtures are under
//! crates/puron-testkit/fixtures/raw/ (capture.json, pins.json,
//! menuids.json). Menu identities are set M1 and all four classes are
//! shown unless a test says otherwise.

use std::collections::BTreeMap;

use puron_core::menu::{self, Device, Heading, LineWord, Shown, ShownLine};
use puron_core::pins;
use puron_core::rows::{Class, Kind, RawReading, ShownClasses};
use puron_core::settings::Settings;

const LIMIT: u64 = 2;

fn json<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
}

fn pinned(set: &str) -> Vec<String> {
    json::<BTreeMap<String, Vec<String>>>("pins.json")
        .remove(set)
        .expect("X-raw-pins set")
}

fn menu_ids(set: &str) -> Vec<String> {
    json::<BTreeMap<String, Vec<String>>>("menuids.json")
        .remove(set)
        .expect("X-raw-menuids set")
}

/// The identity of the capture.json row `name`; other texts stand for
/// themselves.
fn id(name: &str) -> String {
    let raw: Vec<RawReading> = json("capture.json");
    match raw.iter().find(|r| r.name.as_deref() == Some(name)) {
        Some(r) => {
            let class = match r.kind {
                Some(Kind::Fan) => "fan",
                Some(Kind::Voltage) => "voltage",
                Some(Kind::Power) => "power",
                _ => "temperature",
            };
            format!("{class}/lm-sensors/{}", r.key)
        }
        None => name.to_string(),
    }
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| id(n)).collect()
}

fn click(list: &[String], clicked: &str) -> Vec<String> {
    pins::click(list, &id(clicked), &menu_ids("M1"), &ShownClasses::ALL)
}

const FAN_HIDDEN: ShownClasses = ShownClasses {
    fan: false,
    ..ShownClasses::ALL
};
const TEMPERATURE_HIDDEN: ShownClasses = ShownClasses {
    temperature: false,
    ..ShownClasses::ALL
};

#[test]
fn c_core_pins_1() {
    puron_testkit::limit("C-core.pins-1", LIMIT, || {
        assert_eq!(click(&ids(&["CPU"]), "WiFi"), ids(&["CPU", "WiFi"]));
    });
}

#[test]
fn c_core_pins_2() {
    puron_testkit::limit("C-core.pins-2", LIMIT, || {
        assert_eq!(pinned("P1"), ids(&["CPU fan", "WiFi", "CPU", "SSD"]));
        assert_eq!(
            click(&pinned("P1"), "WiFi"),
            ids(&["CPU fan", "CPU", "SSD"])
        );
    });
}

#[test]
fn c_core_pins_3() {
    puron_testkit::limit("C-core.pins-3", LIMIT, || {
        assert_eq!(pinned("P4"), ids(&["gone", "CPU"]));
        assert_eq!(click(&pinned("P4"), "SSD"), ids(&["CPU", "SSD"]));
    });
}

#[test]
fn c_core_pins_4() {
    puron_testkit::limit("C-core.pins-4", LIMIT, || {
        let list = ids(&["@maximum", "@maximum/cpu", "@cpu", "CPU fan"]);
        let m2 = menu_ids("M2");
        // The older pins wait to move and stay; a Maximum line goes.
        assert_eq!(
            pins::click(&list, &id("CPU fan"), &m2, &ShownClasses::ALL),
            ids(&["@maximum", "@cpu"])
        );
        assert_eq!(
            pins::click(&list, &id("CPU fan"), &m2, &TEMPERATURE_HIDDEN),
            ids(&["@maximum", "@maximum/cpu", "@cpu"])
        );
    });
}

#[test]
fn c_core_pins_5() {
    puron_testkit::limit("C-core.pins-5", LIMIT, || {
        // M1 is what the popup shows of capture.json.
        let shown = menu::shown(
            &json::<Vec<RawReading>>("capture.json"),
            &Settings::default(),
        );
        let lines: Vec<String> = shown.lines().map(|l| l.identity.clone()).collect();
        assert_eq!(lines, menu_ids("M1"));
        assert_eq!(click(&[], "@maximum/cpu"), ids(&["@maximum/cpu"]));
    });
}

#[test]
fn c_core_pins_6() {
    puron_testkit::limit("C-core.pins-6", LIMIT, || {
        let m3 = menu_ids("M3");
        let none = Vec::<String>::new();
        assert_eq!(
            pins::click(&ids(&["x"]), "x", &m3, &ShownClasses::ALL),
            none
        );
        assert_eq!(pins::click(&[], "y", &m3, &ShownClasses::ALL), none);
    });
}

#[test]
fn c_core_pins_7() {
    puron_testkit::limit("C-core.pins-7", LIMIT, || {
        assert_eq!(
            click(&ids(&["CPU", "SSD", "CPU"]), "WiFi"),
            ids(&["CPU", "SSD", "WiFi"])
        );
    });
}

#[test]
fn c_core_pins_8() {
    puron_testkit::limit("C-core.pins-8", LIMIT, || {
        assert_eq!(pins::start(&pinned("P4")), ids(&["gone", "CPU"]));
    });
}

#[test]
fn c_core_pins_9() {
    puron_testkit::limit("C-core.pins-9", LIMIT, || {
        let list = ids(&["CPU"]);
        let capitals = "TEMPERATURE/LM-SENSORS/CORETEMP-ISA-0000/TEMP1";
        assert_eq!(id("CPU").to_uppercase(), capitals);
        assert!(pins::is_pinned(&list, &id("CPU")));
        assert!(!pins::is_pinned(&list, capitals));
        assert!(!pins::is_pinned(&list, &id("Core 0")));
    });
}

#[test]
fn c_core_pins_10() {
    puron_testkit::limit("C-core.pins-10", LIMIT, || {
        let start = pins::start(&ids(&["CPU", "SSD", "CPU"]));
        assert_eq!(start, ids(&["CPU", "SSD"]));
        assert_eq!(click(&start, "CPU"), ids(&["SSD"]));
    });
}

#[test]
fn c_core_pins_11() {
    puron_testkit::limit("C-core.pins-11", LIMIT, || {
        let m4 = menu_ids("M4");
        assert_eq!(
            pins::click(&pinned("P1"), &id("WiFi"), &m4, &FAN_HIDDEN),
            ids(&["CPU fan", "CPU", "SSD"])
        );
        assert_eq!(
            pins::click(&pinned("P4"), &id("SSD"), &m4, &FAN_HIDDEN),
            ids(&["CPU", "SSD"])
        );
    });
}

/// A line of the temperature group in `pins_moved`.
fn line(identity: &str, word: Option<LineWord>, default: bool) -> ShownLine {
    ShownLine {
        class: Class::Temperature,
        family: None,
        identity: identity.to_string(),
        name: String::new(),
        word,
        value: Some(50.0),
        default,
    }
}

#[test]
fn pins_moved() {
    puron_testkit::limit("pins_moved", LIMIT, || {
        const PACKAGE: &str = "temperature/lm-sensors/coretemp-isa-0000/temp1";
        const CORE: &str = "temperature/lm-sensors/coretemp-isa-0000/temp2";
        const MAXIMUM: &str = "@maximum/cpu";
        let shown = Shown {
            devices: vec![Device {
                heading: Heading::Cpu,
                lines: vec![
                    line(PACKAGE, Some(LineWord::Package), true),
                    line(CORE, None, false),
                    line(MAXIMUM, Some(LineWord::Maximum), false),
                ],
            }],
            others: Vec::new(),
            repeats: vec![("a".to_string(), "b".to_string())],
        };
        let list = |l: &[&str]| l.iter().map(|e| e.to_string()).collect::<Vec<_>>();
        let moved = |l: &[&str], source| pins::moved(&list(l), &shown, source, true);
        // A repeat moves in its place, once; other pins and the order stay.
        assert_eq!(moved(&["x", "a", "y", "b"], None), list(&["x", "b", "y"]));
        assert_eq!(moved(&["b", "a"], None), list(&["b"]));
        assert_eq!(moved(&["x"], None), list(&["x"]));
        // The old Maximum pin moves to the default line, in its place.
        assert_eq!(moved(&["x", "@maximum"], None), list(&["x", PACKAGE]));
        assert_eq!(moved(&["@maximum", PACKAGE], None), list(&[PACKAGE]));
        // The old CPU pin moves to what its row read, else to the default line.
        let key = Some("coretemp-isa-0000/temp2");
        assert_eq!(moved(&["@cpu"], key), list(&[CORE]));
        assert_eq!(moved(&["@cpu"], Some("@maximum")), list(&[MAXIMUM]));
        assert_eq!(moved(&["@cpu"], Some("gone/temp1")), list(&[PACKAGE]));
        assert_eq!(moved(&["@cpu"], None), list(&[PACKAGE]));
        // Before the sensors are ready, only repeats move.
        let old = list(&["@maximum", "@cpu", "a"]);
        assert_eq!(
            pins::moved(&old, &shown, None, false),
            list(&["@maximum", "@cpu", "b"])
        );
        // With no temperature line there is nowhere to move to.
        let none = Shown {
            devices: Vec::new(),
            ..shown.clone()
        };
        assert_eq!(
            pins::moved(&old, &none, None, true),
            list(&["@maximum", "@cpu", "b"])
        );
    });
}
