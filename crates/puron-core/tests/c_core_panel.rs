//! Tests of the panel view model, 2 s each. Fixtures are under
//! crates/puron-testkit/fixtures/raw/ (capture.json, summary.json,
//! dupes.json, rebuild.json, pins.json, kinds.json). Settings are the
//! defaults and the panel state is the start state unless a test says
//! otherwise.

use std::collections::BTreeMap;

use puron_core::format::{Text, Unit};
use puron_core::menu::{self, Entry, IconKind, Report};
use puron_core::panel::{self, Icon, Item, Panel, State};
use puron_core::rows::{Kind, RawReading};
use puron_core::settings::{Settings, TemperatureUnit};

const LIMIT: u64 = 2;

fn json<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
}

fn capture() -> Vec<RawReading> {
    json("capture.json")
}

fn set(file: &str, name: &str) -> Vec<RawReading> {
    puron_testkit::raw_sets(file, |r: &mut RawReading, v| r.value = Some(v))
        .remove(name)
        .expect("fixture set")
}

/// A set of rebuild.json; its set S1 is capture.json (checked in
/// c_core_menu.rs).
fn snapshot(name: &str) -> Vec<RawReading> {
    set("rebuild.json", name)
}

fn pinned(name: &str) -> Vec<String> {
    json::<BTreeMap<String, Vec<String>>>("pins.json")
        .remove(name)
        .expect("X-raw-pins set")
}

/// The identity of the row `name` of `raw` (lm-sensors readings only);
/// other texts stand for themselves.
fn id(raw: &[RawReading], name: &str) -> String {
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

fn ids(raw: &[RawReading], names: &[&str]) -> Vec<String> {
    names.iter().map(|n| id(raw, n)).collect()
}

fn settings_with(change: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = Settings::default();
    change(&mut s);
    s
}

fn run(raw: &[RawReading], pinned: &[String], settings: &Settings, state: &State) -> Panel {
    panel::panel(&menu::shown(raw, settings), pinned, settings, state)
}

fn lay(raw: &[RawReading], pinned: &[String]) -> Panel {
    run(raw, pinned, &Settings::default(), &State::default())
}

/// A value text with its unit suffix; `N/A` for "no value".
fn text(value: &Option<Text>) -> String {
    match value {
        None => "N/A".to_string(),
        Some(t) => {
            let unit = match t.unit {
                None => "",
                Some(Unit::Celsius) => "°C",
                Some(Unit::Fahrenheit) => "°F",
                Some(Unit::Rpm) => "rpm",
                Some(Unit::Volt) => "V",
                Some(Unit::Watt) => "W",
            };
            format!("{}{unit}", t.number)
        }
    }
}

fn kind(k: IconKind) -> &'static str {
    match k {
        IconKind::GeneralTemperature => "general",
        IconKind::GpuTemperature => "gpu",
        IconKind::DriveTemperature => "drive",
        IconKind::Fan => "fan",
        IconKind::Voltage => "voltage",
        IconKind::Power => "power",
        IconKind::Maximum => "Maximum",
    }
}

/// The items in a short text notation: icon kind and text, `∅` the empty
/// slot, `⚠` missing; no icon word with icons off; `empty` the
/// empty-state item.
fn render(p: &Panel) -> Vec<String> {
    p.items
        .iter()
        .map(|i| match i {
            Item::EmptyState => "empty".to_string(),
            Item::Pin { icon, value } => {
                let value = value.as_ref().map_or("⚠".to_string(), text);
                match icon {
                    Icon::None => value,
                    Icon::EmptySlot => format!("∅ {value}"),
                    Icon::Kind(k) => format!("{} {value}", kind(*k)),
                }
            }
        })
        .collect()
}

fn assert_items(p: &Panel, want: &[&str]) {
    assert_eq!(render(p), want);
}

#[test]
fn c_core_panel_1() {
    puron_testkit::limit("C-core.panel-1", LIMIT, || {
        let raw = capture();
        assert_eq!(pinned("P1"), ids(&raw, &["CPU fan", "WiFi", "CPU", "SSD"]));
        let s = settings_with(|s| s.temperature_unit = TemperatureUnit::Fahrenheit);
        let p = run(&raw, &pinned("P1"), &s, &State::default());
        assert_items(
            &p,
            &["fan 0rpm", "general 91°F", "general 163°F", "drive 100°F"],
        );
    });
}

/// The value of `identity` in the menu layout (walking group members).
fn menu_value(raw: &[RawReading], settings: &Settings, identity: &str) -> Option<Text> {
    let m = menu::menu(
        &menu::shown(raw, settings),
        settings,
        Report::Answered,
        None,
        false,
    );
    let mut items = Vec::new();
    for e in &m.entries {
        match e {
            Entry::Sensor(i) | Entry::Maximum(i) => items.push(i.clone()),
            Entry::Group { members, .. } => items.extend(members.iter().cloned()),
            _ => {}
        }
    }
    items
        .into_iter()
        .find(|i| i.identity == identity)
        .expect("identity in the menu")
        .value
}

#[test]
fn c_core_panel_2() {
    puron_testkit::limit("C-core.panel-2", LIMIT, || {
        let raw = capture();
        let p1 = pinned("P1");
        let cases: [fn(&mut Settings); 7] = [
            |_| {},
            |s| s.temperature_unit = TemperatureUnit::Fahrenheit,
            |s| s.show_decimals = true,
            |s| s.show_temperature_unit = false,
            |s| s.show_rotation_rate_unit = false,
            |s| s.show_voltage_unit = false,
            |s| s.show_power_unit = false,
        ];
        for change in cases {
            let s = settings_with(change);
            let p = run(&raw, &p1, &s, &State::default());
            assert_eq!(p.items.len(), p1.len());
            for (item, identity) in p.items.iter().zip(&p1) {
                let Item::Pin { value, .. } = item else {
                    panic!("pin item")
                };
                assert_eq!(value, &Some(menu_value(&raw, &s, identity)), "{identity}");
            }
        }
    });
}

#[test]
fn c_core_panel_3() {
    puron_testkit::limit("C-core.panel-3", LIMIT, || {
        let raw = capture();
        let on = run(&raw, &pinned("P1"), &Settings::default(), &State::default());
        assert_eq!(on.padding, 0);
        let off = settings_with(|s| s.show_panel_icons = false);
        let off = run(&raw, &pinned("P1"), &off, &State::default());
        assert_eq!(off.padding, 4);
    });
}

#[test]
fn c_core_panel_4() {
    puron_testkit::limit("C-core.panel-4", LIMIT, || {
        let raw = capture();
        assert!(pinned("P2").is_empty());
        assert_items(&lay(&raw, &pinned("P2")), &["empty"]);
        let off = settings_with(|s| s.show_panel_icons = false);
        assert_items(
            &run(&raw, &pinned("P2"), &off, &State::default()),
            &["empty"],
        );
    });
}

#[test]
fn c_core_panel_5() {
    puron_testkit::limit("C-core.panel-5", LIMIT, || {
        let raw = capture();
        assert_eq!(pinned("P3"), ids(&raw, &["WiFi", "@maximum"]));
        assert_items(&lay(&raw, &pinned("P3")), &["general 33°C", "Maximum 73°C"]);
    });
}

#[test]
fn c_core_panel_6() {
    puron_testkit::limit("C-core.panel-6", LIMIT, || {
        assert_items(&lay(&[], &pinned("P3")), &["∅ ⚠", "∅ ⚠"]);
    });
}

#[test]
fn c_core_panel_7() {
    puron_testkit::limit("C-core.panel-7", LIMIT, || {
        let p5 = pinned("P5");
        assert_eq!(p5, ids(&capture(), &["WiFi", "CPU"]));
        let s = Settings::default();
        let first = run(&snapshot("S1"), &p5, &s, &State::default());
        assert_items(&first, &["general 33°C", "general 73°C"]);
        let second = run(&snapshot("S3"), &p5, &s, &first.state);
        assert_items(&second, &["general ⚠", "general 73°C"]);
        let third = run(&snapshot("S1"), &p5, &s, &second.state);
        assert_items(&third, &["general 33°C", "general 73°C"]);
    });
}

#[test]
fn c_core_panel_8() {
    puron_testkit::limit("C-core.panel-8", LIMIT, || {
        let p5 = pinned("P5");
        let s = Settings::default();
        let first = run(&snapshot("S3"), &p5, &s, &State::default());
        assert_items(&first, &["∅ ⚠", "general 73°C"]);
        let second = run(&snapshot("S1"), &p5, &s, &first.state);
        assert_items(&second, &["general 33°C", "general 73°C"]);
    });
}

#[test]
fn c_core_panel_10() {
    puron_testkit::limit("C-core.panel-10", LIMIT, || {
        let p5 = pinned("P5");
        let on = Settings::default();
        let off = settings_with(|s| s.show_panel_icons = false);
        let first = run(&snapshot("S1"), &p5, &on, &State::default());
        let second = run(&snapshot("S3"), &p5, &off, &first.state);
        assert_items(&second, &["⚠", "73°C"]);
        let third = run(&snapshot("S3"), &p5, &on, &second.state);
        assert_items(&third, &["∅ ⚠", "general 73°C"]);
    });
}

#[test]
fn c_core_panel_11() {
    puron_testkit::limit("C-core.panel-11", LIMIT, || {
        let p3 = pinned("P3");
        let s = Settings::default();
        let maximum = |p: &Panel| render(p)[1].clone();
        let first = run(&capture(), &p3, &s, &State::default());
        assert_eq!(maximum(&first), "Maximum 73°C");
        let second = run(&set("summary.json", "C"), &p3, &s, &first.state);
        assert_eq!(maximum(&second), "Maximum N/A");
        let third = run(&set("summary.json", "F"), &p3, &s, &second.state);
        assert_eq!(maximum(&third), "Maximum ⚠");
        let hidden = settings_with(|s| s.show_temperature = false);
        let fourth = run(&capture(), &p3, &hidden, &third.state);
        assert_items(&fourth, &["empty"]);
    });
}

#[test]
fn c_core_panel_12() {
    puron_testkit::limit("C-core.panel-12", LIMIT, || {
        let raw = set("summary.json", "B");
        let t1 = raw
            .iter()
            .find(|r| r.name.as_deref() == Some("t1"))
            .expect("t1");
        assert_eq!(t1.value, Some(-5.0));
        assert_items(&lay(&raw, &ids(&raw, &["t1"])), &["general N/A"]);
    });
}

#[test]
fn c_core_panel_13() {
    puron_testkit::limit("C-core.panel-13", LIMIT, || {
        let raw = capture();
        assert_eq!(pinned("P4"), ids(&raw, &["gone", "CPU"]));
        assert_items(&lay(&raw, &pinned("P4")), &["∅ ⚠", "general 73°C"]);
    });
}

#[test]
fn c_core_panel_14() {
    puron_testkit::limit("C-core.panel-14", LIMIT, || {
        let raw = capture();
        let p1 = pinned("P1");
        let first = run(&raw, &p1, &Settings::default(), &State::default());
        let fan_hidden = settings_with(|s| s.show_rotation_rate = false);
        let second = run(&raw, &p1, &fan_hidden, &first.state);
        assert_items(&second, &["general 33°C", "general 73°C", "drive 38°C"]);
        let third = run(&raw, &p1, &Settings::default(), &second.state);
        assert_items(
            &third,
            &["fan 0rpm", "general 33°C", "general 73°C", "drive 38°C"],
        );
        let temperature_hidden = settings_with(|s| s.show_temperature = false);
        let fourth = run(&raw, &pinned("P4"), &temperature_hidden, &third.state);
        assert_items(&fourth, &["∅ ⚠"]);
    });
}

#[test]
fn c_core_panel_15() {
    puron_testkit::limit("C-core.panel-15", LIMIT, || {
        let raw = snapshot("S1");
        let s = Settings::default();
        let first = run(&raw, &pinned("P5"), &s, &State::default());
        let cpu = ids(&raw, &["CPU"]);
        let second = run(&raw, &cpu, &s, &first.state);
        let held: Vec<&String> = second.state.icons.keys().collect();
        assert_eq!(held, [&cpu[0]]);
    });
}

#[test]
fn c_core_panel_16() {
    puron_testkit::limit("C-core.panel-16", LIMIT, || {
        #[derive(serde::Deserialize)]
        struct Kinds {
            #[serde(rename = "R")]
            readings: Vec<RawReading>,
            #[serde(rename = "P")]
            pinned: Vec<String>,
        }
        let k: Kinds = json("kinds.json");
        let p = lay(&k.readings, &k.pinned);
        let icons: Vec<Icon> = p
            .items
            .iter()
            .map(|i| match i {
                Item::Pin { icon, .. } => *icon,
                Item::EmptyState => panic!("pin item"),
            })
            .collect();
        assert_eq!(
            icons,
            [
                IconKind::GpuTemperature,
                IconKind::GeneralTemperature,
                IconKind::DriveTemperature,
                IconKind::Fan,
                IconKind::Voltage,
                IconKind::Power,
                IconKind::Maximum,
            ]
            .map(Icon::Kind)
        );
    });
}

#[test]
fn c_core_panel_17() {
    puron_testkit::limit("C-core.panel-17", LIMIT, || {
        let s1 = snapshot("S1");
        let s3 = snapshot("S3");
        let cpu = ids(&s1, &["CPU"]);
        let both = ids(&s1, &["CPU", "WiFi"]);
        let s = Settings::default();
        let first = run(&s1, &cpu, &s, &State::default());
        let second = run(&s1, &both, &s, &first.state);
        assert_items(&second, &["general 73°C", "general 33°C"]);
        let third = run(&s3, &cpu, &s, &second.state);
        let fourth = run(&s3, &both, &s, &third.state);
        assert_items(&fourth, &["general 73°C", "∅ ⚠"]);
    });
}

#[test]
fn c_core_panel_18() {
    puron_testkit::limit("C-core.panel-18", LIMIT, || {
        // A copy of set D of dupes.json, with values 50 and 60.
        let mut raw = set("dupes.json", "D");
        assert_eq!(raw.len(), 2);
        raw[0].value = Some(50.0);
        raw[1].value = Some(60.0);
        assert_eq!(raw[0].name.as_deref(), Some("a"));
        let p = lay(&raw, &["temperature/lm-sensors/k".to_string()]);
        assert_items(&p, &["general 50°C"]);
    });
}
