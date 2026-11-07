//! Tests of the popup, plus one panel test that clicks rows in the
//! popup. 5 s each. "Plan" tests read the popup plan and drive the
//! popup's own click handler; "Bounds" tests measure the widgets built
//! from the plan with the iced test simulator, headless, in a 360 × 1000
//! window, with no snapshot. Layouts come from `core.menu` with the
//! captured readings, the default settings and pinned list P1, unless a test
//! says otherwise.
#![allow(non_snake_case)]

use std::collections::BTreeMap;

use cosmic::applet::cosmic_panel_config::PanelSize;
use cosmic::applet::{self, Context, Size as AppletSize};
use cosmic::iced::{Rectangle, Size};
use cosmic::widget::{container, Id};
use cosmic::Theme;
use iced_test::{selector, Simulator};
use puron_applet::i18n;
use puron_applet::panel;
use puron_applet::popup::{self, Clicked, Drawn, Event, Plan, Reply};
use puron_core::menu::{self, Key, Menu, Report, Update};
use puron_core::panel::State;
use puron_core::rows::{RawReading, ShownClasses};
use puron_core::settings::{Settings, TemperatureUnit};

const LIMIT: u64 = 5;
/// libcosmic's `popup_container` height limit.
const HEIGHT: f32 = 1000.0;

const FAN: &str = "puron-fan-symbolic";
const TEMP: &str = "puron-temperature-symbolic";
const DRIVE: &str = "drive-harddisk-system-symbolic";
const VOLT: &str = "puron-voltage-symbolic";

fn raw(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

fn capture() -> Vec<RawReading> {
    serde_json::from_str(&raw("capture.json")).expect("parse capture.json")
}

fn set(file: &str, name: &str) -> serde_json::Value {
    serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&raw(file))
        .expect("parse fixture")
        .remove(name)
        .expect("fixture set")
}

/// One of the rebuild snapshots.
fn rebuild(name: &str) -> Vec<RawReading> {
    serde_json::from_value(set("rebuild.json", name)).expect("X-raw-rebuild set")
}

fn pinned(name: &str) -> Vec<String> {
    serde_json::from_value(set("pins.json", name)).expect("X-raw-pins set")
}

/// `core.menu`'s layout; `last` is the previous layout's key.
fn lay(raw: &[RawReading], last: Option<&Key>) -> Menu {
    let s = Settings::default();
    menu::menu(&menu::shown(raw, &s), &s, Report::Answered, last, false)
}

/// The applet context at panel size M.
fn icon_size() -> u16 {
    panel::icon_size(&Context {
        size: AppletSize::PanelSize(PanelSize::M),
        ..Context::default()
    })
}

/// Texts from `i18n` after Start.
fn setup() {
    puron_applet::start();
}

fn plan_of(menu: &Menu, pins: &[String]) -> Plan {
    popup::plan(menu, pins, icon_size())
}

fn capture_plan(pins: &[String]) -> (Menu, Plan) {
    let m = lay(&capture(), None);
    let plan = plan_of(&m, pins);
    (m, plan)
}

/// What the plan draws: `[x] text value` for a line (`x` when marked), the
/// text of a heading, a device heading with two spaces first, `—`, `message`,
/// `homepage`, `settings`.
fn notation(plan: &Plan) -> Vec<String> {
    plan.entries
        .iter()
        .map(|e| match e {
            Drawn::Line(l) => format!(
                "[{}] {} {}",
                if l.pinned { "x" } else { " " },
                l.text,
                l.value,
            ),
            Drawn::Section { text, .. } => text.clone(),
            Drawn::Device { text } => format!("  {text}"),
            Drawn::Separator => "—".into(),
            Drawn::Message { .. } => "message".into(),
            Drawn::HomepageLink { .. } => "homepage".into(),
            Drawn::SettingsLink { .. } => "settings".into(),
        })
        .collect()
}

/// The index of the line with the text `text`.
fn line_index(plan: &Plan, text: &str) -> usize {
    plan.entries
        .iter()
        .position(|e| matches!(e, Drawn::Line(l) if l.text == text))
        .unwrap_or_else(|| panic!("line {text}: {:?}", notation(plan)))
}

/// The texts of the marked lines, in plan order.
fn marked(plan: &Plan) -> Vec<String> {
    plan.entries
        .iter()
        .filter_map(|e| match e {
            Drawn::Line(l) if l.pinned => Some(l.text.clone()),
            _ => None,
        })
        .collect()
}

type Sim = Simulator<'static, Clicked, Theme, cosmic::Renderer>;

/// The popup content in libcosmic's popup width.
fn simulate(plan: &Plan) -> Sim {
    simulate_at(plan, popup::WIDTH)
}

fn simulate_at(plan: &Plan, width: f32) -> Sim {
    let content = container(popup::view(plan)).id(Id::new("root"));
    Simulator::with_size(Default::default(), Size::new(width, HEIGHT), content)
}

fn bounds(sim: &mut Sim, id: Id) -> Rectangle {
    sim.find(selector::id(id.clone()))
        .unwrap_or_else(|e| panic!("find {id:?}: {e:?}"))
        .bounds()
}

fn element(sim: &mut Sim, i: usize, name: &str) -> Rectangle {
    bounds(sim, popup::element_id(i, name))
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn right(r: &Rectangle) -> f32 {
    r.x + r.width
}

const CPU_LINE: &str = "coretemp: CPU";

#[test]
fn c_applet_popup_1() {
    puron_testkit::limit("C-applet.popup-1", LIMIT, || {
        setup();
        let (m, plan) = capture_plan(&pinned("P1"));
        // One drawn entry per entry, in Entries order, nothing added.
        assert_eq!(plan.entries.len(), m.entries.len());
        for (e, d) in m.entries.iter().zip(&plan.entries) {
            match (e, d) {
                (menu::Entry::Line(it), Drawn::Line(l)) => assert_eq!(l.identity, it.identity),
                (menu::Entry::Section(_), Drawn::Section { .. })
                | (menu::Entry::Device(_), Drawn::Device { .. })
                | (menu::Entry::Separator, Drawn::Separator)
                | (menu::Entry::HomepageLink { .. }, Drawn::HomepageLink { .. })
                | (menu::Entry::SettingsLink, Drawn::SettingsLink { .. }) => {}
                (e, d) => panic!("entry {e:?} drawn as {d:?}"),
            }
        }
        // Bounds: each top edge at or below the previous bottom edge.
        let mut sim = simulate(&plan);
        let r: Vec<Rectangle> = (0..plan.entries.len())
            .map(|i| bounds(&mut sim, popup::entry_id(i)))
            .collect();
        for i in 1..r.len() {
            assert!(
                r[i].y >= r[i - 1].y + r[i - 1].height,
                "entry {i} top {} above {} bottom {}",
                r[i].y,
                i - 1,
                r[i - 1].y + r[i - 1].height
            );
        }
    });
}

#[test]
fn c_applet_popup_2() {
    puron_testkit::limit("C-applet.popup-2", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let i = line_index(&plan, CPU_LINE);
        let mut sim = simulate(&plan);
        let r: Vec<Rectangle> = ["check", "name", "value"]
            .iter()
            .map(|e| element(&mut sim, i, e))
            .collect();
        for k in 1..r.len() {
            assert!(
                r[k].x >= right(&r[k - 1]),
                "element {k} left {} before {} right {}",
                r[k].x,
                k - 1,
                right(&r[k - 1])
            );
        }
    });
}

#[test]
fn c_applet_popup_3() {
    puron_testkit::limit("C-applet.popup-3", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let (cpu, core0) = (
            line_index(&plan, CPU_LINE),
            line_index(&plan, "coretemp: Core 0"),
        );
        assert!(matches!(&plan.entries[cpu], Drawn::Line(l) if l.pinned));
        assert!(matches!(&plan.entries[core0], Drawn::Line(l) if !l.pinned));
        let mut sim = simulate(&plan);
        let (a, b) = (
            element(&mut sim, cpu, "name"),
            element(&mut sim, core0, "name"),
        );
        assert!(near(a.x, b.x, 0.5), "name left {} and {}", a.x, b.x);
    });
}

/// The mark and the slot do not follow the panel's icon size.
#[test]
fn c_applet_popup_23() {
    puron_testkit::limit("C-applet.popup-23", LIMIT, || {
        setup();
        let m = lay(&capture(), None);
        for size in [16, 20, 32, 64] {
            let plan = popup::plan(&m, &pinned("P1"), size);
            let (cpu, core0) = (
                line_index(&plan, CPU_LINE),
                line_index(&plan, "coretemp: Core 0"),
            );
            assert!(matches!(&plan.entries[cpu], Drawn::Line(l) if l.pinned));
            assert!(matches!(&plan.entries[core0], Drawn::Line(l) if !l.pinned));
            let mut sim = simulate(&plan);
            let mark = element(&mut sim, cpu, "mark");
            assert!(
                near(mark.width, 14.0, 0.5) && near(mark.height, 14.0, 0.5),
                "icon {size}: mark {mark:?}"
            );
            let name = element(&mut sim, cpu, "name");
            assert!(
                near(name.x - right(&mark), 6.0, 0.5),
                "icon {size}: mark right {} name left {}",
                right(&mark),
                name.x
            );
            for line in [cpu, core0] {
                let slot = element(&mut sim, line, "check");
                assert!(near(slot.width, 20.0, 0.5), "icon {size}: slot {slot:?}");
                let text_left = element(&mut sim, line, "name").x;
                assert!(
                    near(text_left, name.x, 0.5),
                    "icon {size}: text left {text_left} and {}",
                    name.x
                );
            }
            // The width rule counts the same slot.
            let measure = |t: &str| t.chars().count() as f32 * 7.0;
            assert_eq!(
                popup::fit_width(&m, size, 48.0, &measure),
                popup::fit_width(&m, 20, 48.0, &measure),
                "icon {size}"
            );
        }
    });
}

/// A section heading is in libcosmic's heading style, 21 px of line height
/// against a line's smaller one, and its icon stays at the icon size.
#[test]
fn section_heading_style_and_icon() {
    puron_testkit::limit("section-heading", LIMIT, || {
        setup();
        let m = lay(&capture(), None);
        for size in [16, 32] {
            let plan = popup::plan(&m, &[], size);
            let section = plan
                .entries
                .iter()
                .position(|e| matches!(e, Drawn::Section { .. }))
                .expect("a section");
            let line = line_index(&plan, CPU_LINE);
            let mut sim = simulate(&plan);
            let heading = element(&mut sim, section, "heading");
            assert!(near(heading.height, 21.0, 0.1), "heading {heading:?}");
            let name = element(&mut sim, line, "name");
            assert!(name.height < 21.0, "line text {name:?}");
            let icon = element(&mut sim, section, "icon");
            assert!(
                near(icon.width, f32::from(size), 0.5) && near(icon.height, f32::from(size), 0.5),
                "icon {size}: {icon:?}"
            );
        }
    });
}

#[test]
fn c_applet_popup_5() {
    puron_testkit::limit("C-applet.popup-5", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let i = line_index(&plan, CPU_LINE);
        let mut sim = simulate(&plan);
        let line = bounds(&mut sim, popup::entry_id(i));
        let value = element(&mut sim, i, "value");
        // The button's right edge less libcosmic's padding.
        let edge = right(&line) - applet::menu_control_padding().right;
        assert!(
            near(right(&value), edge, 0.5),
            "value right {}, content edge {edge}",
            right(&value)
        );
    });
}

#[test]
fn c_applet_popup_6() {
    puron_testkit::limit("C-applet.popup-6", LIMIT, || {
        setup();
        let (_, p1) = capture_plan(&pinned("P1"));
        assert_eq!(marked(&p1), [CPU_LINE, "SSD", "iwlwifi: WiFi", "CPU fan"]);
        let (_, p3) = capture_plan(&pinned("P3"));
        assert_eq!(marked(&p3), ["iwlwifi: WiFi"]);
    });
}

#[test]
fn c_applet_popup_7() {
    puron_testkit::limit("C-applet.popup-7", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let at = |want: fn(&Drawn) -> bool| -> Vec<usize> {
            let found = plan.entries.iter().enumerate();
            found.filter(|(_, e)| want(e)).map(|(i, _)| i).collect()
        };
        let sections = at(|e| matches!(e, Drawn::Section { .. }));
        let devices = at(|e| matches!(e, Drawn::Device { .. }));
        let lines = at(|e| matches!(e, Drawn::Line(_)));
        let mut sim = simulate(&plan);
        let section = bounds(&mut sim, popup::entry_id(sections[0]));
        for i in devices {
            let r = bounds(&mut sim, popup::entry_id(i));
            assert!(near(r.x, section.x + popup::DEVICE_INDENT, 0.5), "{i}");
        }
        // Fan and voltage lines have no device heading, and the same column.
        for i in lines {
            let r = bounds(&mut sim, popup::entry_id(i));
            assert!(near(r.x, section.x + popup::LINE_INDENT, 0.5), "{i}");
        }
    });
}

#[test]
fn c_applet_popup_9() {
    puron_testkit::limit("C-applet.popup-9", LIMIT, || {
        setup();
        let pins = pinned("P1");
        let s1 = rebuild("S1");
        let m1 = lay(&s1, None);
        // The first layout is drawn, so a stale value would have a source.
        plan_of(&m1, &pins);
        // A second reading with new values: in place, and the plan shows them.
        let m2 = lay(&rebuild("S2"), Some(&m1.key));
        assert_eq!(m2.update, Update::InPlace);
        let want: Vec<String> = m2
            .entries
            .iter()
            .filter_map(|e| match e {
                menu::Entry::Line(it) => Some(i18n::value_text(it.value.as_ref())),
                _ => None,
            })
            .collect();
        let got: Vec<String> = plan_of(&m2, &pins)
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Line(l) => Some(l.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(got, want, "the second layout's values");
        // A rebuild changes nothing but the lines.
        let m3 = lay(&rebuild("S3"), Some(&m1.key));
        assert_eq!(m3.update, Update::Rebuild);
    });
}

#[test]
fn c_applet_popup_12() {
    puron_testkit::limit("C-applet.popup-12", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let w = plan
            .entries
            .iter()
            .position(|e| matches!(e, Drawn::HomepageLink { .. }))
            .expect("homepage link");
        assert_eq!(
            plan.entries[w],
            Drawn::HomepageLink {
                text: i18n::text("link-homepage")
            }
        );
        let homepage = Reply {
            event: Some(Event::HomepageLink),
            close: true,
        };
        assert_eq!(popup::click(&plan.entries[w]), homepage);
        // No raw readings: the message entry.
        let m = lay(&[], None);
        let plan = plan_of(&m, &pinned("P1"));
        let i = plan
            .entries
            .iter()
            .position(|e| matches!(e, Drawn::Message { .. }))
            .expect("message entry");
        assert_eq!(popup::click(&plan.entries[i]), homepage);
    });
}

#[test]
fn c_applet_popup_13() {
    puron_testkit::limit("C-applet.popup-13", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let i = plan
            .entries
            .iter()
            .position(|e| matches!(e, Drawn::SettingsLink { .. }))
            .expect("settings link");
        assert_eq!(
            popup::click(&plan.entries[i]),
            Reply {
                event: Some(Event::SettingsLink),
                close: true
            }
        );
    });
}

#[test]
fn c_applet_popup_14() {
    puron_testkit::limit("C-applet.popup-14", LIMIT, || {
        setup();
        let raw: Vec<RawReading> =
            serde_json::from_value(set("kinds.json", "R")).expect("X-raw-kinds rows");
        let plan = plan_of(&lay(&raw, None), &[]);
        let icons: Vec<(&str, &str)> = plan
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Section { text, icon } => Some((text.as_str(), *icon)),
                _ => None,
            })
            .collect();
        assert_eq!(
            icons,
            [
                ("Temperature", TEMP),
                ("Fan", FAN),
                ("Voltage", VOLT),
                ("Power", VOLT),
            ]
        );
    });
}

#[test]
fn c_applet_panel_14() {
    puron_testkit::limit("C-applet.panel-14", LIMIT, || {
        setup();
        let pins = pinned("P1");
        let s = Settings {
            show_panel_icons: true,
            ..Settings::default()
        };
        let items =
            puron_core::panel::panel(&menu::shown(&capture(), &s), &pins, &s, &State::default());
        let plan = panel::plan(&items, panel::Orientation::Horizontal, icon_size());
        // The panel items follow the popup, whatever the list's order.
        let on_panel: Vec<(usize, &str)> = plan
            .elements
            .iter()
            .filter_map(|e| match e {
                panel::Element::Icon { item, name } => Some((*item, *name)),
                _ => None,
            })
            .collect();
        assert_eq!(on_panel, [(0, TEMP), (1, DRIVE), (2, TEMP), (3, FAN)]);
    });
}

#[test]
fn c_applet_popup_15() {
    puron_testkit::limit("C-applet.popup-15", LIMIT, || {
        setup();
        let mut pins = pinned("P1");
        let (m, mut plan) = capture_plan(&pins);
        assert_eq!(marked(&plan), [CPU_LINE, "SSD", "iwlwifi: WiFi", "CPU fan"]);
        let steps = [
            (
                "SSD",
                "temperature/lm-sensors/nvme-pci-0100/temp1",
                vec![CPU_LINE, "iwlwifi: WiFi", "CPU fan"],
            ),
            (
                "Maximum",
                "@maximum/lm-sensors/nvme-pci-0100",
                vec![CPU_LINE, "Maximum", "iwlwifi: WiFi", "CPU fan"],
            ),
        ];
        for (text, identity, want) in steps {
            let i = line_index(&plan, text);
            assert_eq!(
                popup::click(&plan.entries[i]),
                Reply {
                    event: Some(Event::PinClick(identity.to_string())),
                    close: false
                }
            );
            // `app` passes the click to `core.pins`; the
            // default settings show every class.
            pins = puron_core::pins::click(&pins, identity, &m.identities, &ShownClasses::ALL);
            let next = plan_of(&m, &pins);
            assert_eq!(marked(&next), want, "after {text}");
            // No line moves; only the clicked line's mark changes.
            let unmarked = |pl: &Plan| -> Vec<String> {
                notation(pl)
                    .iter()
                    .map(|n| n.replace("[x]", "[ ]"))
                    .collect()
            };
            assert_eq!(unmarked(&next), unmarked(&plan), "after {text}");
            plan = next;
        }
    });
}

#[test]
fn c_applet_popup_16() {
    puron_testkit::limit("C-applet.popup-16", LIMIT, || {
        setup();
        let m5 = lay(&rebuild("S5"), None);
        let m6 = lay(&rebuild("S6"), Some(&m5.key));
        assert_eq!(m6.update, Update::Rebuild);
        let texts: Vec<String> = plan_of(&m6, &[])
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Line(l) => Some(l.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["junction (default)", "edge", "Maximum"]);
    });
}

fn cpu_set(name: &str) -> Vec<RawReading> {
    serde_json::from_value(set("cpu.json", name)).expect("X-raw-cpu set")
}

fn drive_set(name: &str) -> Vec<RawReading> {
    serde_json::from_value(set("drives.json", name)).expect("X-raw-drives set")
}

#[test]
fn c_applet_popup_18() {
    puron_testkit::limit("C-applet.popup-18", LIMIT, || {
        setup();
        let s = Settings::default();
        let lay_with = |raw: &[RawReading]| {
            menu::menu(&menu::shown(raw, &s), &s, Report::Answered, None, false)
        };
        let m = lay_with(&cpu_set("long-names"));
        let plan = plan_of(&m, &[]);
        let names: Vec<&str> = plan
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Device { text } => Some(text.as_str()),
                Drawn::Line(l) if !l.identity.starts_with("@maximum") => Some(l.text.as_str()),
                _ => None,
            })
            .collect();
        // A heading is cut at its end; a line holds its label whole.
        assert_eq!(
            names,
            [
                "A configured label that goes on for a very…",
                "A configured label that goes on for a very…",
                "A device with a very long name indeed this…",
                "A label too long for any device to stay",
                "Exactly forty-four characters, not a one cut",
                "Exactly forty-four characters, not a one cut",
                "Example SSD 100 PRO With Heatsink 2TB Very…",
                "Sensor 2",
            ]
        );
        for n in &names {
            assert!(n.chars().count() <= 44, "{n}");
        }
        // Every line is one line, in the least width and in the popup's own.
        let height = |sim: &mut Sim, i: usize| bounds(sim, popup::entry_id(i)).height;
        let lines: Vec<usize> = plan
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| matches!(e, Drawn::Line(_)).then_some(i))
            .collect();
        for width in [popup::WIDTH, popup::width(&m, icon_size())] {
            let mut sim = simulate_at(&plan, width);
            let one = height(&mut sim, lines[0]);
            for &i in &lines {
                assert!(near(height(&mut sim, i), one, 0.5), "entry {i} at {width}");
            }
        }
        // The width follows the longest line, between the limits. At 7 px a
        // character: 48 of padding, 24 of indent, 20 of check, 8 of gap, 64
        // for the value.
        let measure = |t: &str| t.chars().count() as f32 * 7.0;
        let fit = |raw: &[RawReading]| popup::fit_width(&lay_with(raw), 20, 48.0, &measure);
        assert_eq!(fit(&cpu_set("no-cpu")), popup::WIDTH);
        assert_eq!(
            fit(&cpu_set("long-names")),
            48.0 + 24.0 + 20.0 + 44.0 * 7.0 + 8.0 + 64.0
        );
        assert_eq!(
            popup::MAX_WIDTH,
            48.0 + 24.0 + 20.0 + 44.0 * 7.0 + 8.0 + 64.0
        );
        // The 44 characters hold on a line, with the mark of its default.
        let mut raw = cpu_set("example");
        raw[0].name = Some("x".repeat(60));
        raw[0].device_chars = 0;
        let plan = plan_of(&lay_with(&raw), &[]);
        let line = plan.entries.iter().find_map(|e| match e {
            Drawn::Line(l) if l.text.starts_with("coretemp: x") => Some(l.text.clone()),
            _ => None,
        });
        let want = format!("coretemp: {}… (default)", "x".repeat(23));
        assert_eq!(line.as_deref(), Some(want.as_str()));
        assert_eq!(want.chars().count(), 44);
        // A heading has the same limit.
        let mut raw = cpu_set("no-cpu");
        raw[0].name = Some(format!("{} temp1", "d".repeat(60)));
        raw[0].device_chars = 60;
        let plan = plan_of(&lay_with(&raw), &[]);
        let heading = plan.entries.iter().find_map(|e| match e {
            Drawn::Device { text } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(heading, Some(format!("{}…", "d".repeat(43))));
        // Far beyond: the limit.
        let wide = |t: &str| t.chars().count() as f32 * 40.0;
        let m = lay_with(&cpu_set("long-names"));
        assert_eq!(popup::fit_width(&m, 20, 48.0, &wide), popup::MAX_WIDTH);
        // No rows: the least width.
        assert_eq!(fit(&[]), popup::WIDTH);
    });
}

/// The plan of `raw` in Fahrenheit, new install: what the default pins check.
fn new_install(raw: &[RawReading]) -> Vec<String> {
    let s = Settings {
        temperature_unit: TemperatureUnit::Fahrenheit,
        ..Settings::default()
    };
    let shown = menu::shown(raw, &s);
    let m = menu::menu(&shown, &s, Report::Answered, None, false);
    notation(&plan_of(&m, &shown.new_install_pins()))
}

const EXAMPLE_FOOT: [&str; 9] = [
    "—",
    "Fan",
    "[ ] Example Laptop CPU fan 0 rpm",
    "—",
    "Voltage",
    "[ ] BAT0 in0 +8.68V",
    "—",
    "homepage",
    "settings",
];

/// The rows, byte for byte: each shape of machine.
#[test]
fn c_applet_popup_21() {
    puron_testkit::limit("C-applet.popup-21", LIMIT, || {
        setup();
        let foot = &EXAMPLE_FOOT;
        let tree = |head: &[&str]| -> Vec<String> {
            head.iter()
                .chain(foot.iter())
                .map(|t| t.to_string())
                .collect()
        };
        assert_eq!(
            new_install(&drive_set("example")),
            tree(&[
                "Temperature",
                "  CPU",
                "[x] coretemp: Package id 0 (default) 118°F",
                "[ ] example_acpi: CPU temp 115°F",
                "[ ] coretemp: Core 0 111°F",
                "[ ] coretemp: Core 1 118°F",
                "[ ] coretemp: Core 2 113°F",
                "[ ] coretemp: Core 3 109°F",
                "[ ] Maximum 118°F",
                "  Example SSD 1TB",
                "[x] composite (default) 100°F",
                "[ ] Sensor 1 100°F",
                "[ ] Sensor 2 111°F",
                "[ ] Maximum 111°F",
                "  iwlwifi",
                "[ ] temp1 84°F",
            ]),
        );
        assert_eq!(
            new_install(&drive_set("example-configured")),
            tree(&[
                "Temperature",
                "  CPU",
                "[x] coretemp: CPU package (default) 118°F",
                "[ ] example_acpi: CPU temp 115°F",
                "[ ] coretemp: Core 0 111°F",
                "[ ] coretemp: Core 1 118°F",
                "[ ] coretemp: Core 2 113°F",
                "[ ] coretemp: Core 3 109°F",
                "[ ] Maximum 118°F",
                "  SSD",
                "[x] SSD composite (default) 100°F",
                "[ ] Sensor 1 100°F",
                "[ ] Sensor 2 111°F",
                "[ ] Maximum 111°F",
                "  WiFi",
                "[ ] iwlwifi: WiFi 84°F",
            ]),
        );
        let links = ["—", "homepage", "settings"];
        let plain = |head: &[&str]| -> Vec<String> {
            head.iter().chain(&links).map(|t| t.to_string()).collect()
        };
        assert_eq!(
            new_install(&cpu_set("monolith"))[..7],
            plain(&[
                "Temperature",
                "  CPU",
                "[ ] coretemp: Core 0 95°F",
                "[ ] coretemp: Core 1 100°F",
                "[ ] coretemp: Core 2 97°F",
                "[ ] coretemp: Core 3 93°F",
                "[x] Maximum (default) 100°F",
            ])[..7],
        );
        assert_eq!(
            new_install(&cpu_set("k10temp-tdie")),
            plain(&[
                "Temperature",
                "  CPU",
                "[x] k10temp: Tdie (default) 122°F",
                "[ ] k10temp: Tctl 140°F",
                "[ ] Maximum 140°F",
            ]),
        );
        let two = new_install(&drive_set("two-drives"));
        let drives = two.iter().position(|e| e == "  Example SSD 1TB");
        let drives = drives.expect("the first drive");
        assert_eq!(
            two[drives..drives + 9],
            [
                "  Example SSD 1TB",
                "[x] composite (default) 100°F",
                "[ ] Sensor 1 100°F",
                "[ ] Sensor 2 111°F",
                "[ ] Maximum 111°F",
                "  Example SSD 1TB",
                "[x] composite (default) 122°F",
                "[ ] Sensor 1 124°F",
                "[ ] Maximum 124°F",
            ]
        );
        assert_eq!(
            new_install(&cpu_set("no-cpu")),
            plain(&["Temperature", "  iwlwifi", "[x] temp1 84°F"]),
        );
    });
}

/// A click on a line toggles its check; nothing else answers.
#[test]
fn c_applet_popup_22() {
    puron_testkit::limit("C-applet.popup-22", LIMIT, || {
        setup();
        let (_, plan) = capture_plan(&pinned("P1"));
        let i = line_index(&plan, CPU_LINE);
        // Through the simulator, on each part of the line: one message each.
        for part in ["check", "name", "value"] {
            let mut sim = simulate(&plan);
            sim.click(selector::id(popup::element_id(i, part)))
                .unwrap_or_else(|e| panic!("click {part}: {e:?}"));
            let got: Vec<Clicked> = sim.into_messages().collect();
            assert_eq!(got, [Clicked(i)], "{part}");
        }
        // A heading: no message, and no reply.
        let none = Reply {
            event: None,
            close: false,
        };
        let labels: Vec<usize> = plan
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                matches!(
                    e,
                    Drawn::Section { .. } | Drawn::Device { .. } | Drawn::Separator
                )
            })
            .map(|(i, _)| i)
            .collect();
        assert!(labels.len() >= 6);
        for i in labels {
            assert_eq!(popup::clicked(&plan, Clicked(i)), none, "entry {i}");
            let mut sim = simulate(&plan);
            sim.click(selector::id(popup::entry_id(i)))
                .unwrap_or_else(|e| panic!("click {i}: {e:?}"));
            assert_eq!(sim.into_messages().count(), 0, "entry {i}");
        }
        assert_eq!(popup::clicked(&plan, Clicked(plan.entries.len())), none);
    });
}
