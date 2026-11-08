//! Tests of the popup view model, 2 s each. Fixtures are under
//! crates/puron-testkit/fixtures/raw/ (capture.json, summary.json,
//! menu.json, rebuild.json). Settings are the defaults, the lm-sensors
//! report is answered and `forced` is false unless a test says
//! otherwise.

use std::collections::BTreeMap;

use puron_core::format::{Text, Unit};
use puron_core::menu::{
    self, Entry, Heading, Item, Key, LineWord, Menu, MessageKind, Report, Section, Update,
};
use puron_core::rows::{Provider, RawReading};
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

/// A line as the popup would show it: name, word, default mark, value.
fn line(i: &Item) -> String {
    let word = match i.word {
        None => "",
        Some(LineWord::Composite) => "composite",
        Some(LineWord::Package) => "package",
        Some(LineWord::Temperature) => "temperature",
        Some(LineWord::Maximum) => "Maximum",
        Some(LineWord::Waiting) => "CPU: waiting for lm-sensors",
    };
    let parts = [
        i.name.as_str(),
        word,
        if i.default { "(default)" } else { "" },
        &text(&i.value),
    ];
    let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    parts.join(" ")
}

/// The entries in a short text notation: `# section`, `## device`, a line,
/// `—`; the two links are `homepage` and `settings`.
fn render(m: &Menu) -> Vec<String> {
    m.entries
        .iter()
        .map(|e| match e {
            Entry::Section(Section::Temperature) => "# temperature".to_string(),
            Entry::Section(Section::Fan) => "# fan".to_string(),
            Entry::Section(Section::Voltage) => "# voltage".to_string(),
            Entry::Section(Section::Power) => "# power".to_string(),
            Entry::Device(Heading::Cpu) => "## CPU".to_string(),
            Entry::Device(Heading::Name(name)) => format!("## {name}"),
            Entry::Line(i) => line(i),
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

/// What the capture shows at the default settings. It has no kernel labels.
const CAPTURE: [&str; 26] = [
    "# temperature",
    "## CPU",
    "coretemp: Core 0 65°C",
    "coretemp: Core 1 71°C",
    "coretemp: Core 2 64°C",
    "coretemp: Core 3 72°C",
    "coretemp: CPU 73°C",
    "Maximum (default) 73°C",
    "## nvme",
    "Sensor 1 (default) 38°C",
    "Sensor 2 45°C",
    "SSD 38°C",
    "Maximum 45°C",
    "## CPU temp",
    "example_acpi: CPU temp 67°C",
    "## WiFi",
    "iwlwifi: WiFi 33°C",
    "—",
    "# fan",
    "CPU fan 0rpm",
    "—",
    "# voltage",
    "in0 +8.49V",
    "—",
    "homepage",
    "settings",
];

fn capture_identities(raw: &[RawReading]) -> Vec<String> {
    let t = |n: &str| id(raw, "temperature", n);
    vec![
        t("Core 0"),
        t("Core 1"),
        t("Core 2"),
        t("Core 3"),
        t("CPU"),
        "@maximum/cpu".to_string(),
        t("Sensor 1"),
        t("Sensor 2"),
        t("SSD"),
        "@maximum/lm-sensors/nvme-pci-0100".to_string(),
        t("CPU temp"),
        t("WiFi"),
        id(raw, "fan", "CPU fan"),
        id(raw, "voltage", "in0"),
    ]
}

#[test]
fn c_core_menu_1() {
    puron_testkit::limit("C-core.menu-1", LIMIT, || {
        let raw = capture();
        let m = lay(&raw);
        assert_entries(&m, &CAPTURE);
        assert_eq!(m.identities, capture_identities(&raw));
        assert_eq!(m.key, Key::Identities(capture_identities(&raw)));
    });
}

#[test]
fn c_core_menu_3() {
    puron_testkit::limit("C-core.menu-3", LIMIT, || {
        assert_entries(
            &lay(&menu_set("C")),
            &[
                "# fan",
                "f 1000rpm",
                "—",
                "# voltage",
                "v +12.00V",
                "—",
                "homepage",
                "settings",
            ],
        );
        assert_entries(
            &lay(&menu_set("D")),
            &[
                "# fan",
                "f1 1000rpm",
                "f2 1000rpm",
                "—",
                "# voltage",
                "v1 +12.00V",
                "v2 +12.00V",
                "—",
                "# power",
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
                "# temperature",
                "## t",
                "k0: t 40°C",
                "—",
                "# power",
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
        assert!(!render(&c).contains(&"# temperature".to_string()));
        assert_entries(
            &lay(&menu_set("G")),
            &["# fan", "f 1000rpm", "—", "homepage", "settings"],
        );
    });
}

#[test]
fn c_core_menu_9() {
    puron_testkit::limit("C-core.menu-9", LIMIT, || {
        assert_entries(
            &lay(&menu_set("H")),
            &[
                "# power",
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
        assert_entries(&m, &["# power", "p +10.00W", "—", "homepage", "settings"]);
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
                "# fan",
                "CPU fan 0rpm",
                "—",
                "# voltage",
                "in0 +8.49V",
                "—",
                "homepage",
                "settings",
            ],
        );
        assert!(!m.identities.iter().any(|i| i.starts_with("@maximum")));
        // Each other class hides its whole section, and its separator.
        let no_fans = lay_with(&raw, &settings_with(|s| s.show_rotation_rate = false));
        assert!(!render(&no_fans).contains(&"# fan".to_string()));
        assert_eq!(render(&no_fans).len(), CAPTURE.len() - 3);
        let no_volts = lay_with(&raw, &settings_with(|s| s.show_voltage = false));
        assert!(!render(&no_volts).contains(&"# voltage".to_string()));
        assert_eq!(render(&no_volts).len(), CAPTURE.len() - 3);
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
        let rendered = render(&second);
        for want in [
            "coretemp: Core 0 66°C",
            "coretemp: CPU 74°C",
            "Maximum (default) 74°C",
            "iwlwifi: WiFi 34°C",
            "Maximum 46°C",
            "CPU fan 1rpm",
            "in0 +9.49V",
        ] {
            assert!(rendered.contains(&want.to_string()), "{want}: {rendered:?}");
        }
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
        assert_eq!(m4.identities.len(), 14);
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
            render(&m).contains(&"Maximum (default) 163.4°F".to_string()),
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
        let lines: Vec<(&str, Option<f64>)> = shown
            .lines()
            .filter(|l| !l.is_maximum())
            .map(|l| (l.name.as_str(), l.value))
            .collect();
        assert_eq!(
            lines,
            [
                ("t0", Some(10.0)),
                ("t1", None),
                ("v2", Some(-1.5)),
                ("p3", Some(-2.0))
            ]
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
        let names: Vec<&str> = shown.others.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["in0"]);
        assert_eq!(shown.lines().count(), 13);
    });
}

#[test]
fn c_core_menu_25() {
    puron_testkit::limit("C-core.menu-25", LIMIT, || {
        let (s5, s6) = (snapshot("S5"), snapshot("S6"));
        let maximum = "@maximum/lm-sensors/amdgpu-pci-0300".to_string();
        let k5 = lay(&s5).key;
        assert_eq!(
            k5,
            Key::Identities(vec![
                id(&s5, "temperature", "edge"),
                id(&s5, "temperature", "junction"),
                maximum.clone()
            ])
        );
        let m = after(&s6, &k5);
        let want = [
            "# temperature",
            "## amdgpu",
            "junction (default) 91°C",
            "edge 41°C",
            "Maximum 91°C",
            "—",
            "homepage",
            "settings",
        ];
        let ids = vec![
            id(&s6, "temperature", "junction"),
            id(&s6, "temperature", "edge"),
            maximum,
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
            let lines = menu.entries.iter().filter_map(|e| match e {
                Entry::Line(i) => Some(i),
                _ => None,
            });
            let mut lines = lines.enumerate();
            lines
                .find(|(_, i)| i.identity == wifi)
                .map(|(p, i)| (p, i.clone()))
        };
        let (p1, _) = place(&first).expect("WiFi in S1");
        let (p7, it) = place(&m).expect("WiFi's identity in S7");
        assert_eq!(p7, p1);
        assert_eq!(line(&it), "iwlwifi: WLAN 33°C");
    });
}

/// Set `name` of the CPU fixture.
fn cpu_set(name: &str) -> Vec<RawReading> {
    sets("cpu.json").remove(name).expect("X-raw-cpu set")
}

fn drive_set(name: &str) -> Vec<RawReading> {
    sets("drives.json").remove(name).expect("X-raw-drives set")
}

/// The lines under the heading `heading`.
fn under(m: &Menu, heading: &str) -> Vec<String> {
    let rendered = render(m);
    let start = rendered
        .iter()
        .position(|e| e == heading)
        .unwrap_or_else(|| panic!("{heading}: {rendered:?}"));
    let end = |e: &String| e.starts_with('#') || e == "—";
    let rest = rendered[start + 1..].iter().take_while(|e| !end(e));
    rest.cloned().collect()
}

/// Sections and device headings, the groups in order, CPU first. A waiting
/// pin's line ends the CPU group, ahead of the other device groups.
#[test]
fn c_core_menu_32() {
    puron_testkit::limit("C-core.menu-32", LIMIT, || {
        let raw = cpu_set("sections");
        let m = lay(&raw);
        let want = [
            "# temperature",
            "## CPU",
            "coretemp: Package id 0 (default) 40°C",
            "coretemp: Package id 1 39°C",
            "example_acpi: CPU temp 38°C",
            "coretemp: Core 0 41°C",
            "Maximum 41°C",
            "## GeForce RTX 4070",
            "temperature 42°C",
            "## Radeon RX 6600",
            "edge (default) 36°C",
            "junction 37°C",
            "Maximum 37°C",
            "## Alpha SSD",
            "composite 32°C",
            "## Barracuda",
            "temperature 44°C",
            "## WD Blue",
            "temperature 43°C",
            "## Zeta SSD",
            "composite (default) 30°C",
            "Sensor 1 31°C",
            "Maximum 31°C",
            "## acpitz",
            "temp1 (default) 34°C",
            "temp2 35°C",
            "Maximum 35°C",
            "## iwlwifi",
            "temp1 33°C",
            "## Kraken X",
            "Liquid 45°C",
            "—",
            "# fan",
            "Example Laptop CPU fan 0rpm",
            "—",
            "# voltage",
            "BAT0 in0 +8.68V",
            "—",
            "# power",
            "Radeon RX 6600 PPT +15.00W",
            "—",
            "homepage",
            "settings",
        ];
        assert_entries(&m, &want);
        // A section heading and a device heading are labels: no identity.
        assert!(!m.identities.iter().any(|i| i.is_empty()));
        let settings = Settings::default();
        let shown = menu::shown(&raw, &settings);
        assert_eq!(shown.lines().count(), m.identities.len());

        // The same readings with a pin that waits for lm-sensors: its line
        // is the last of the CPU group, which stays the only CPU group, and
        // no other group gets it.
        let pin = "@maximum".to_string();
        let w = menu::menu_with(
            &shown,
            &settings,
            Report::Answered,
            None,
            false,
            std::slice::from_ref(&pin),
        );
        let waiting = "CPU: waiting for lm-sensors N/A";
        let mut want_w: Vec<&str> = want.to_vec();
        let gpu = want.iter().position(|e| *e == "## GeForce RTX 4070");
        want_w.insert(gpu.expect("the first group after CPU"), waiting);
        assert_entries(&w, &want_w);
        assert_eq!(
            under(&w, "## CPU").last().map(String::as_str),
            Some(waiting)
        );
        let rendered = render(&w);
        assert_eq!(rendered.iter().filter(|e| *e == "## CPU").count(), 1);
        assert_eq!(rendered.iter().filter(|e| *e == waiting).count(), 1);
        // The line counts as a line: its identity follows the CPU group's.
        let cpu_lines = under(&m, "## CPU").len();
        let mut want_ids = m.identities.clone();
        want_ids.insert(cpu_lines, pin.clone());
        assert_eq!(w.identities, want_ids);
    });
}

/// The default lines, and what a new install checks.
#[test]
fn c_core_menu_33() {
    puron_testkit::limit("C-core.menu-33", LIMIT, || {
        let defaults = |raw: &[RawReading]| -> Vec<String> {
            let m = lay(raw);
            let lines = m.entries.iter().filter_map(|e| match e {
                Entry::Line(i) if i.default => Some(line(i)),
                _ => None,
            });
            lines.collect()
        };
        let cases = [
            ("two-packages", "coretemp: Package id 0 (default) 70°C"),
            ("package-silent", "coretemp: Package id 0 (default) N/A"),
            ("k10temp-tdie", "k10temp: Tdie (default) 50°C"),
            ("k10temp-tctl", "k10temp: Tctl (default) 60°C"),
            ("zenpower-tdie", "zenpower: Tdie (default) 52°C"),
            ("monolith", "Maximum (default) 38°C"),
        ];
        for (set, want) in cases {
            assert_eq!(defaults(&cpu_set(set)), [want], "{set}");
        }
        // No package: the board controller's reading goes first.
        let mut raw = cpu_set("example");
        raw.retain(|r| r.key != "coretemp-isa-0000/temp1");
        assert_eq!(
            under(&lay(&raw), "## CPU")[..2],
            [
                "example_acpi: CPU temp (default) 46°C",
                "coretemp: Core 0 44°C"
            ]
        );
        // The board controller's reading alone is a group of one line.
        assert_eq!(
            under(&lay(&cpu_set("board")), "## CPU"),
            ["example_acpi: CPU temp 46°C"]
        );
        // Tdie goes first, Tctl beside it stays a line, Tccd is under CPU.
        let m = lay(&cpu_set("k10temp-tdie"));
        assert_eq!(
            under(&m, "## CPU"),
            [
                "k10temp: Tdie (default) 50°C",
                "k10temp: Tctl 60°C",
                "Maximum 60°C"
            ]
        );
        let m = lay(&cpu_set("k10temp-tctl"));
        assert_eq!(
            under(&m, "## CPU"),
            [
                "k10temp: Tctl (default) 60°C",
                "k10temp: Tccd1 55°C",
                "Maximum 60°C"
            ]
        );
        // Maximum stays last when it is the default.
        let m = lay(&cpu_set("monolith"));
        let lines = under(&m, "## CPU");
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Maximum (default) 38°C")
        );
        assert_eq!(lines.len(), 5);
        // A chip that names no CPU has no place in its group.
        let m = lay(&cpu_set("board-label-not-platform"));
        assert!(
            !render(&m).contains(&"## CPU".to_string()),
            "{:?}",
            render(&m)
        );
        // Each drive has its Composite.
        let raw = drive_set("two-drives");
        let d = Settings::default();
        let shown = menu::shown(&raw, &d);
        let pins = shown.new_install_pins();
        for key in ["nvme-pci-0100/temp1", "nvme-pci-0200/temp1"] {
            assert!(
                pins.contains(&format!("temperature/lm-sensors/{key}")),
                "{pins:?}"
            );
        }
        // No Composite: the first reading.
        let m = lay(&drive_set("no-composite"));
        assert!(
            render(&m)
                .iter()
                .filter(|e| e.contains("(default)"))
                .count()
                >= 1
        );
        // A group of one line: no mark, and a new install does not check it,
        // unless no default line exists at all.
        let m = lay(&drive_set("one-sensor"));
        assert_eq!(
            render(&m)
                .iter()
                .filter(|e| e.starts_with("composite"))
                .count(),
            1
        );
        assert!(
            render(&m).contains(&"composite 38°C".to_string()),
            "{:?}",
            render(&m)
        );
        let mut raw = cpu_set("two-packages");
        raw.extend(drive_set("one-sensor"));
        let shown = menu::shown(&raw, &d);
        assert!(!shown
            .new_install_pins()
            .contains(&"temperature/lm-sensors/nvme-pci-0100/temp1".to_string()));
        let raw = cpu_set("no-cpu");
        let shown = menu::shown(&raw, &d);
        assert!(shown.lines().all(|l| !l.default));
        assert_eq!(
            shown.new_install_pins(),
            ["temperature/lm-sensors/iwlwifi_1-virtual-0/temp1"]
        );
        // Fans and voltages never have one, and none temperature means none.
        let shown = menu::shown(&menu_set("D"), &d);
        assert!(shown.new_install_pins().is_empty());
        assert!(shown.lines().all(|l| !l.default));
    });
}

/// A group's Maximum follows the higher reading at each poll.
#[test]
fn c_core_menu_34() {
    puron_testkit::limit("C-core.menu-34", LIMIT, || {
        let d = Settings::default();
        let maximum = |raw: &[RawReading], identity: &str| {
            let shown = menu::shown(raw, &d);
            shown.line(identity).expect(identity).value
        };
        let set_value = |raw: &mut Vec<RawReading>, key: &str, value: Option<f64>| {
            let reading = raw.iter_mut().find(|r| r.key == key).expect(key);
            reading.value = value;
        };
        let cpu = "@maximum/cpu";
        let mut raw = cpu_set("example");
        assert_eq!(maximum(&raw, cpu), Some(48.0));
        set_value(&mut raw, "example_acpi-isa-0000/temp1", Some(60.0));
        assert_eq!(maximum(&raw, cpu), Some(60.0));
        set_value(&mut raw, "example_acpi-isa-0000/temp1", Some(40.0));
        assert_eq!(maximum(&raw, cpu), Some(48.0));
        // A reading that does not count is left out; with none left: no value.
        for key in ["temp1", "temp2", "temp3", "temp4", "temp5"] {
            let key = format!("coretemp-isa-0000/{key}");
            set_value(&mut raw, &key, Some(-5.0));
        }
        assert_eq!(maximum(&raw, cpu), Some(40.0));
        set_value(&mut raw, "example_acpi-isa-0000/temp1", None);
        assert_eq!(maximum(&raw, cpu), None);
        let m = lay(&raw);
        assert!(under(&m, "## CPU").contains(&"Maximum N/A".to_string()));
        // Its own group's readings only, and one reading has no such line.
        let raw = drive_set("example");
        let drive = "@maximum/lm-sensors/nvme-pci-0100";
        assert_eq!(maximum(&raw, drive), Some(43.85));
        assert_eq!(maximum(&raw, cpu), Some(48.0));
        let shown = menu::shown(&cpu_set("no-cpu"), &d);
        assert!(shown.lines().all(|l| !l.is_maximum()));
        // The identity does not depend on the rows' values or order.
        let raw = cpu_set("two-packages");
        assert_eq!(
            menu::shown(&raw, &d).line(cpu).map(|l| l.is_maximum()),
            Some(true)
        );
    });
}

/// The set's nvme-cli readings, with lm-sensors off.
fn nvme_cli_only(set: &str) -> Vec<RawReading> {
    let mut raw = drive_set(set);
    raw.retain(|r| r.provider == Provider::NvmeCli);
    raw
}

/// An nvme-cli line has the word when its name has no device part, and the
/// reading's label when it has one.
#[test]
fn nvme_cli_line_names() {
    puron_testkit::limit("nvme_cli_line_names", LIMIT, || {
        let lines = |set: &str| {
            let mut all = render(&lay(&nvme_cli_only(set)));
            all.retain(|l| {
                !l.starts_with('#') && !["—", "homepage", "settings"].contains(&l.as_str())
            });
            all
        };
        assert_eq!(lines("one-sensor"), ["temperature 38°C"]);
        assert_eq!(
            lines("lm-sensors-off"),
            ["S1 (default) 38°C", "S2 44°C", "Maximum 44°C"]
        );
    });
}
