//! Tests of the popup view model, 2 s each. Fixtures are under
//! crates/puron-testkit/fixtures/raw/ (capture.json, summary.json,
//! menu.json, rebuild.json). Settings are the defaults, the lm-sensors
//! report is answered and `forced` is false unless a test says
//! otherwise.

use std::collections::BTreeMap;

use puron_core::format::{Text, Unit};
use puron_core::menu::{self, Entry, GroupKind, Item, Key, Menu, MessageKind, Report, Update};
use puron_core::rows::RawReading;
use puron_core::settings::{Settings, TemperatureUnit};

const LIMIT: u64 = 2;

fn capture() -> Vec<RawReading> {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", "capture.json"))
        .expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
}

fn sets(file: &str) -> BTreeMap<String, Vec<RawReading>> {
    puron_testkit::raw_sets(file, |r: &mut RawReading, v| r.value = Some(v))
}

fn menu_set(set: &str) -> Vec<RawReading> {
    sets("menu.json").remove(set).expect("X-raw-menu set")
}

/// A set of rebuild.json; its set S1 is capture.json.
fn snapshot(set: &str) -> Vec<RawReading> {
    let json = |name| -> serde_json::Value {
        let text =
            std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture");
        serde_json::from_str(&text).expect("parse fixture")
    };
    assert_eq!(
        json("rebuild.json")["S1"],
        json("capture.json"),
        "S1 is X-raw-capture"
    );
    sets("rebuild.json").remove(set).expect("X-raw-rebuild set")
}

/// The full call: shown rows, then the menu layout.
fn run(
    raw: &[RawReading],
    settings: &Settings,
    report: Report,
    last: Option<&Key>,
    forced: bool,
) -> Menu {
    menu::menu(&menu::shown(raw, settings), settings, report, last, forced)
}

fn lay(raw: &[RawReading]) -> Menu {
    run(raw, &Settings::default(), Report::Answered, None, false)
}

fn lay_with(raw: &[RawReading], settings: &Settings) -> Menu {
    run(raw, settings, Report::Answered, None, false)
}

fn after(raw: &[RawReading], last: &Key) -> Menu {
    run(
        raw,
        &Settings::default(),
        Report::Answered,
        Some(last),
        false,
    )
}

fn settings_with(change: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = Settings::default();
    change(&mut s);
    s
}

/// A value text as the popup would show it, unit suffix appended;
/// `N/A` for "no value".
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

fn item(i: &Item) -> String {
    format!("{} {}", i.name, text(&i.value))
}

/// The entries in a short text notation; the two links are `homepage`
/// and `settings`.
fn render(m: &Menu) -> Vec<String> {
    m.entries
        .iter()
        .map(|e| match e {
            Entry::Sensor(i) => item(i),
            Entry::Maximum(i) => format!("Max {}", text(&i.value)),
            Entry::Group {
                kind,
                header,
                members,
            } => {
                let kind = match kind {
                    GroupKind::Temperature => "temperature",
                    GroupKind::Fan => "fan",
                    GroupKind::Voltage => "voltage",
                };
                let header = header
                    .as_ref()
                    .map(|h| format!(", header {}", text(h)))
                    .unwrap_or_default();
                let members: Vec<String> = members.iter().map(item).collect();
                format!("[G {kind}{header}: {}]", members.join(", "))
            }
            Entry::Separator => "—".to_string(),
            Entry::Message {
                kind: MessageKind::SensorsDetect,
                ..
            } => "msg sensors-detect".to_string(),
            Entry::Message {
                kind: MessageKind::Install,
                ..
            } => "msg install".to_string(),
            Entry::HomepageLink { .. } => "homepage".to_string(),
            Entry::SettingsLink => "settings".to_string(),
        })
        .collect()
}

fn assert_entries(m: &Menu, want: &[&str]) {
    assert_eq!(render(m), want);
}

/// The identity of the reading named `name` (all fixtures here are
/// lm-sensors readings).
fn id(raw: &[RawReading], class: &str, name: &str) -> String {
    let r = raw
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .expect("reading by name");
    format!("{class}/lm-sensors/{}", r.key)
}

/// The group of c_core_menu_1: the 10 temperatures of capture.json in
/// display order, at the default settings.
const CAPTURE_GROUP: &str =
    "[G temperature, header 73°C: Core 0 65°C, Core 1 71°C, Core 2 64°C, Core 3 72°C, \
     CPU 73°C, CPU temp 67°C, WiFi 33°C, Sensor 1 38°C, Sensor 2 45°C, SSD 38°C]";
const CAPTURE_TEMPERATURES: [&str; 10] = [
    "Core 0", "Core 1", "Core 2", "Core 3", "CPU", "CPU temp", "WiFi", "Sensor 1", "Sensor 2",
    "SSD",
];

fn capture_identities(raw: &[RawReading]) -> Vec<String> {
    let mut ids: Vec<String> = CAPTURE_TEMPERATURES
        .iter()
        .map(|n| id(raw, "temperature", n))
        .collect();
    ids.push("@maximum".to_string());
    ids.push(id(raw, "fan", "CPU fan"));
    ids.push(id(raw, "voltage", "in0"));
    ids
}

#[test]
fn c_core_menu_1() {
    puron_testkit::limit("C-core.menu-1", LIMIT, || {
        let raw = capture();
        let m = lay(&raw);
        assert_entries(
            &m,
            &[
                CAPTURE_GROUP,
                "—",
                "Max 73°C",
                "—",
                "CPU fan 0rpm",
                "—",
                "in0 +8.49V",
                "—",
                "homepage",
                "settings",
            ],
        );
        assert_eq!(m.identities, capture_identities(&raw));
        assert_eq!(m.key, Key::Identities(capture_identities(&raw)));
    });
}

#[test]
fn c_core_menu_2() {
    puron_testkit::limit("C-core.menu-2", LIMIT, || {
        let m = lay(&menu_set("A"));
        assert_entries(
            &m,
            &[
                "[G temperature, header 40°C: t 40°C]",
                "—",
                "Max 40°C",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_3() {
    puron_testkit::limit("C-core.menu-3", LIMIT, || {
        assert_entries(
            &lay(&menu_set("C")),
            &["f 1000rpm", "—", "v +12.00V", "—", "homepage", "settings"],
        );
        assert_entries(
            &lay(&menu_set("D")),
            &[
                "[G fan: f1 1000rpm, f2 1000rpm]",
                "—",
                "[G voltage: v1 +12.00V, v2 +12.00V]",
                "p +10.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_4() {
    puron_testkit::limit("C-core.menu-4", LIMIT, || {
        let d = lay(&menu_set("D"));
        let groups: Vec<_> = d
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Group { kind, header, .. } => Some((*kind, header.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(groups, [(GroupKind::Fan, None), (GroupKind::Voltage, None)]);
        let c = lay(&capture());
        let header = c.entries.iter().find_map(|e| match e {
            Entry::Group {
                kind: GroupKind::Temperature,
                header,
                ..
            } => Some(header.clone()),
            _ => None,
        });
        let max = c.entries.iter().find_map(|e| match e {
            Entry::Maximum(i) => Some(i.value.clone()),
            _ => None,
        });
        assert_eq!(
            header,
            Some(Some(Some(Text {
                number: "73".to_string(),
                unit: Some(Unit::Celsius)
            })))
        );
        assert_eq!(header.flatten(), max);
    });
}

#[test]
fn c_core_menu_5() {
    puron_testkit::limit("C-core.menu-5", LIMIT, || {
        let m = lay_with(&capture(), &settings_with(|s| s.group_temperature = false));
        let rows = [
            "Core 0 65°C",
            "Core 1 71°C",
            "Core 2 64°C",
            "Core 3 72°C",
            "CPU 73°C",
            "CPU temp 67°C",
            "WiFi 33°C",
            "Sensor 1 38°C",
            "Sensor 2 45°C",
            "SSD 38°C",
        ];
        let mut want: Vec<&str> = rows.to_vec();
        want.extend([
            "—",
            "Max 73°C",
            "—",
            "CPU fan 0rpm",
            "—",
            "in0 +8.49V",
            "—",
            "homepage",
            "settings",
        ]);
        assert_entries(&m, &want);
    });
}

#[test]
fn c_core_menu_6() {
    puron_testkit::limit("C-core.menu-6", LIMIT, || {
        let d = menu_set("D");
        assert_entries(
            &lay_with(&d, &settings_with(|s| s.group_rotation_rate = false)),
            &[
                "f1 1000rpm",
                "f2 1000rpm",
                "—",
                "[G voltage: v1 +12.00V, v2 +12.00V]",
                "p +10.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
        assert_entries(
            &lay_with(&d, &settings_with(|s| s.group_voltage = false)),
            &[
                "[G fan: f1 1000rpm, f2 1000rpm]",
                "—",
                "v1 +12.00V",
                "v2 +12.00V",
                "p +10.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_7() {
    puron_testkit::limit("C-core.menu-7", LIMIT, || {
        assert_entries(
            &lay(&menu_set("B")),
            &[
                "[G temperature, header 40°C: t 40°C]",
                "—",
                "Max 40°C",
                "p +10.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_8() {
    puron_testkit::limit("C-core.menu-8", LIMIT, || {
        let c = lay(&menu_set("C"));
        assert_entries(
            &c,
            &["f 1000rpm", "—", "v +12.00V", "—", "homepage", "settings"],
        );
        assert!(!c.identities.contains(&"@maximum".to_string()));
        assert_entries(
            &lay(&menu_set("G")),
            &["f 1000rpm", "—", "homepage", "settings"],
        );
    });
}

#[test]
fn c_core_menu_9() {
    puron_testkit::limit("C-core.menu-9", LIMIT, || {
        let all_on = settings_with(|s| {
            s.group_temperature = true;
            s.group_rotation_rate = true;
            s.group_voltage = true;
        });
        assert_entries(
            &lay_with(&menu_set("H"), &all_on),
            &[
                "p1 +10.00W",
                "p2 +10.00W",
                "p3 +10.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_10() {
    puron_testkit::limit("C-core.menu-10", LIMIT, || {
        let m = lay(&menu_set("E"));
        assert_entries(&m, &["p +10.00W", "—", "homepage", "settings"]);
        assert!(matches!(m.key, Key::Identities(_)));
    });
}

#[test]
fn c_core_menu_11() {
    puron_testkit::limit("C-core.menu-11", LIMIT, || {
        let f = menu_set("F");
        let d = Settings::default();
        let off = settings_with(|s| s.provider_lm_sensors = false);
        let cases = [
            (
                &d,
                Report::Answered,
                MessageKind::SensorsDetect,
                "msg sensors-detect",
            ),
            (&d, Report::Unavailable, MessageKind::Install, "msg install"),
            (&off, Report::Answered, MessageKind::Install, "msg install"),
        ];
        for (settings, report, kind, msg) in cases {
            let m = run(&f, settings, report, None, false);
            assert_entries(&m, &[msg, "—", "homepage", "settings"]);
            assert_eq!(m.key, Key::Message(kind), "{report:?}");
        }
    });
}

#[test]
fn c_core_menu_12() {
    puron_testkit::limit("C-core.menu-12", LIMIT, || {
        let raw = capture();
        let only_power = settings_with(|s| {
            s.show_temperature = false;
            s.show_rotation_rate = false;
            s.show_voltage = false;
        });
        assert_entries(
            &lay_with(&raw, &only_power),
            &["msg sensors-detect", "—", "homepage", "settings"],
        );
        let m = lay_with(&raw, &settings_with(|s| s.show_temperature = false));
        assert_entries(
            &m,
            &[
                "CPU fan 0rpm",
                "—",
                "in0 +8.49V",
                "—",
                "homepage",
                "settings",
            ],
        );
        assert!(!m.identities.contains(&"@maximum".to_string()));
    });
}

#[test]
fn c_core_menu_13() {
    puron_testkit::limit("C-core.menu-13", LIMIT, || {
        let url = "https://elalonde.dev/software/puron/";
        assert_eq!(menu::HOMEPAGE_URL, url);
        let mut urls = Vec::new();
        for set in ["F", "A"] {
            for e in lay(&menu_set(set)).entries {
                match e {
                    Entry::Message { url, .. } | Entry::HomepageLink { url } => urls.push(url),
                    _ => {}
                }
            }
        }
        // F: the message and the homepage link; A: the homepage link.
        assert_eq!(urls, [url, url, url]);
    });
}

#[test]
fn c_core_menu_14() {
    puron_testkit::limit("C-core.menu-14", LIMIT, || {
        let first = lay(&snapshot("S1"));
        assert_eq!(first.update, Update::Rebuild);
        let second = after(&snapshot("S2"), &first.key);
        assert_eq!(second.update, Update::InPlace);
        assert_eq!(second.identities, first.identities);
        assert_eq!(second.key, first.key);
        let groups =
            "[G temperature, header 74°C: Core 0 66°C, Core 1 72°C, Core 2 65°C, Core 3 73°C, \
             CPU 74°C, CPU temp 68°C, WiFi 34°C, Sensor 1 39°C, Sensor 2 46°C, SSD 39°C]";
        assert_entries(
            &second,
            &[
                groups,
                "—",
                "Max 74°C",
                "—",
                "CPU fan 1rpm",
                "—",
                "in0 +9.49V",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_15() {
    puron_testkit::limit("C-core.menu-15", LIMIT, || {
        let (s1, s3) = (snapshot("S1"), snapshot("S3"));
        let wifi = id(&s1, "temperature", "WiFi");
        let k1 = lay(&s1).key;
        let m3 = after(&s3, &k1);
        assert_eq!(m3.update, Update::Rebuild);
        assert!(!m3.identities.contains(&wifi));
        let back = after(&s1, &m3.key);
        assert_eq!(back.update, Update::Rebuild);
        assert!(back.identities.contains(&wifi));
    });
}

#[test]
fn c_core_menu_16() {
    puron_testkit::limit("C-core.menu-16", LIMIT, || {
        let k1 = lay(&snapshot("S1")).key;
        let m4 = after(&snapshot("S4"), &k1);
        assert_eq!(m4.identities.len(), 13);
        assert_eq!(m4.update, Update::Rebuild);
    });
}

#[test]
fn c_core_menu_17() {
    puron_testkit::limit("C-core.menu-17", LIMIT, || {
        let s1 = snapshot("S1");
        let k1 = lay(&s1).key;
        let m = run(&s1, &Settings::default(), Report::Answered, Some(&k1), true);
        assert_eq!(m.key, k1);
        assert_eq!(m.update, Update::Rebuild);
    });
}

#[test]
fn c_core_menu_18() {
    puron_testkit::limit("C-core.menu-18", LIMIT, || {
        let s1 = snapshot("S1");
        let k1 = lay(&s1).key;
        let f = settings_with(|s| {
            s.temperature_unit = TemperatureUnit::Fahrenheit;
            s.show_decimals = true;
        });
        let m = run(&s1, &f, Report::Answered, Some(&k1), false);
        assert_eq!(m.update, Update::InPlace);
        assert!(
            render(&m).contains(&"Max 163.4°F".to_string()),
            "{:?}",
            render(&m)
        );
    });
}

#[test]
fn c_core_menu_20() {
    puron_testkit::limit("C-core.menu-20", LIMIT, || {
        let d = Settings::default();
        let last = Key::Message(MessageKind::SensorsDetect);
        let first = run(&menu_set("F"), &d, Report::Answered, Some(&last), false);
        assert_eq!(
            (first.key.clone(), first.update),
            (last.clone(), Update::InPlace)
        );
        let second = run(
            &menu_set("F"),
            &d,
            Report::Unavailable,
            Some(&first.key),
            false,
        );
        assert_eq!(
            (second.key.clone(), second.update),
            (Key::Message(MessageKind::Install), Update::Rebuild)
        );
        let third = run(
            &menu_set("A"),
            &d,
            Report::Answered,
            Some(&second.key),
            false,
        );
        assert_eq!(third.update, Update::Rebuild);
    });
}

#[test]
fn c_core_menu_21() {
    puron_testkit::limit("C-core.menu-21", LIMIT, || {
        let raw = sets("summary.json").remove("B").expect("X-raw-summary B");
        let d = Settings::default();
        let shown = menu::shown(&raw, &d);
        let rows: Vec<(&str, Option<f64>)> = shown
            .rows
            .iter()
            .map(|r| (r.name.as_str(), r.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("t0", Some(10.0)),
                ("t1", None),
                ("v2", Some(-1.5)),
                ("p3", Some(-2.0))
            ]
        );
        let m = menu::menu(&shown, &d, Report::Answered, None, false);
        assert_entries(
            &m,
            &[
                "[G temperature, header 10°C: t0 10°C, t1 N/A]",
                "—",
                "Max 10°C",
                "—",
                "v2 -1.50V",
                "p3 -2.00W",
                "—",
                "homepage",
                "settings",
            ],
        );
    });
}

#[test]
fn c_core_menu_22() {
    puron_testkit::limit("C-core.menu-22", LIMIT, || {
        let m = run(&[], &Settings::default(), Report::NoneYet, None, false);
        assert_entries(&m, &["msg sensors-detect", "—", "homepage", "settings"]);
        assert_eq!(m.key, Key::Message(MessageKind::SensorsDetect));
        assert_eq!(m.update, Update::Rebuild);
    });
}

#[test]
fn c_core_menu_23() {
    puron_testkit::limit("C-core.menu-23", LIMIT, || {
        let shown = menu::shown(&capture(), &settings_with(|s| s.show_rotation_rate = false));
        let names: Vec<&str> = shown.rows.iter().map(|r| r.name.as_str()).collect();
        let mut want = CAPTURE_TEMPERATURES.to_vec();
        want.push("in0");
        assert_eq!(names, want);
    });
}

#[test]
fn c_core_menu_25() {
    puron_testkit::limit("C-core.menu-25", LIMIT, || {
        let (s5, s6) = (snapshot("S5"), snapshot("S6"));
        let k5 = lay(&s5).key;
        assert_eq!(
            k5,
            Key::Identities(vec![
                id(&s5, "temperature", "edge"),
                id(&s5, "temperature", "junction"),
                "@maximum".to_string()
            ])
        );
        let m = after(&s6, &k5);
        let want = [
            "[G temperature, header 91°C: junction 91°C, edge 41°C]",
            "—",
            "Max 91°C",
            "—",
            "homepage",
            "settings",
        ];
        let ids = vec![
            id(&s6, "temperature", "junction"),
            id(&s6, "temperature", "edge"),
            "@maximum".to_string(),
        ];
        assert_eq!(m.update, Update::Rebuild);
        assert_entries(&m, &want);
        assert_eq!(m.identities, ids);
        assert_eq!(m.key, Key::Identities(ids.clone()));
        let again = after(&s6, &m.key);
        assert_eq!(again.update, Update::InPlace);
        assert_entries(&again, &want);
        assert_eq!(again.key, Key::Identities(ids));
    });
}

#[test]
fn c_core_menu_26() {
    puron_testkit::limit("C-core.menu-26", LIMIT, || {
        let (s1, s7) = (snapshot("S1"), snapshot("S7"));
        let first = lay(&s1);
        let m = after(&s7, &first.key);
        assert_eq!(m.update, Update::InPlace);
        assert_eq!(m.identities, first.identities);
        let wifi = id(&s1, "temperature", "WiFi");
        let place = |menu: &Menu| -> Option<(usize, Item)> {
            menu.entries.iter().find_map(|e| match e {
                Entry::Group { members, .. } => members
                    .iter()
                    .position(|i| i.identity == wifi)
                    .map(|p| (p, members[p].clone())),
                _ => None,
            })
        };
        let (p1, _) = place(&first).expect("WiFi in S1");
        let (p7, it) = place(&m).expect("WiFi's identity in S7");
        assert_eq!(p7, p1);
        assert_eq!(item(&it), "WLAN 33°C");
    });
}
