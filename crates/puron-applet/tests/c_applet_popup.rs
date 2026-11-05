//! Tests of the popup, plus one panel test that clicks rows in the
//! popup. 5 s each. "Plan" tests read the popup plan and drive the
//! popup's own click handler; "Bounds" tests measure the widgets built
//! from the plan with the iced test simulator, headless, in a 360 × 1000
//! window, with no snapshot. Layouts come from `core.menu` with the
//! captured readings, the default settings with the three `group_*` off,
//! and pinned list P1, unless a test says otherwise.
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
use puron_applet::popup::{self, Clicked, Drawn, Event, Plan, Popup, Reply};
use puron_core::menu::{self, GroupKind, Key, Menu, Report, Update};
use puron_core::panel::State;
use puron_core::rows::{RawReading, ShownClasses};
use puron_core::settings::Settings;

const LIMIT: u64 = 5;
/// libcosmic's `popup_container` height limit.
const HEIGHT: f32 = 1000.0;

const FAN: &str = "puron-fan-symbolic";
const TEMP: &str = "puron-temperature-symbolic";
const GPU: &str = "puron-gpu-temperature-symbolic";
const DRIVE: &str = "drive-harddisk-system-symbolic";
const VOLT: &str = "puron-voltage-symbolic";

/// The captured temperatures in `core.order`'s order.
const TEMPS: [&str; 10] = [
    "Core 0", "Core 1", "Core 2", "Core 3", "CPU", "CPU temp", "WiFi", "Sensor 1", "Sensor 2",
    "SSD",
];

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

/// The default settings; groups off unless `groups`.
fn settings(groups: bool) -> Settings {
    let mut s = Settings::default();
    if !groups {
        s.group_temperature = false;
        s.group_rotation_rate = false;
        s.group_voltage = false;
    }
    s
}

/// `core.menu`'s layout; `last` is the previous layout's key.
fn lay(raw: &[RawReading], groups: bool, last: Option<&Key>) -> Menu {
    let s = settings(groups);
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

/// A popup that has drawn `menu`: the layout passed, then the plan.
fn shown(popup: &mut Popup, menu: &Menu, pins: &[String]) -> Plan {
    popup.layout(menu);
    popup.plan(menu, pins, icon_size())
}

fn capture_plan(groups: bool, pins: &[String]) -> (Popup, Menu, Plan) {
    let mut p = Popup::default();
    let m = lay(&capture(), groups, None);
    let plan = shown(&mut p, &m, pins);
    (p, m, plan)
}

/// What the plan draws: `[x] name value` for a row (`x` when marked,
/// two spaces before a member), `G text value open|closed`, `—`,
/// `message`, `homepage`, `settings`.
fn notation(plan: &Plan) -> Vec<String> {
    plan.entries
        .iter()
        .map(|e| match e {
            Drawn::Row(r) => format!(
                "{}[{}] {} {}",
                if r.member { "  " } else { "" },
                if r.pinned { "x" } else { " " },
                r.name,
                r.value
            ),
            Drawn::Header {
                text, value, open, ..
            } => format!(
                "G {text} {} {}",
                value.as_deref().unwrap_or("-"),
                if *open { "open" } else { "closed" }
            ),
            Drawn::Separator => "—".into(),
            Drawn::Message { .. } => "message".into(),
            Drawn::HomepageLink { .. } => "homepage".into(),
            Drawn::SettingsLink { .. } => "settings".into(),
        })
        .collect()
}

/// The index of the row named `name` (the Maximum row by its `i18n` name).
fn row_index(plan: &Plan, name: &str) -> usize {
    plan.entries
        .iter()
        .position(|e| matches!(e, Drawn::Row(r) if r.name == name))
        .unwrap_or_else(|| panic!("row {name}"))
}

fn header_index(plan: &Plan, kind: GroupKind) -> usize {
    plan.entries
        .iter()
        .position(|e| matches!(e, Drawn::Header { kind: k, .. } if *k == kind))
        .expect("group header")
}

/// The names of the marked rows, in plan order.
fn marked(plan: &Plan) -> Vec<String> {
    plan.entries
        .iter()
        .filter_map(|e| match e {
            Drawn::Row(r) if r.pinned => Some(r.name.clone()),
            _ => None,
        })
        .collect()
}

fn members(plan: &Plan) -> Vec<String> {
    plan.entries
        .iter()
        .filter_map(|e| match e {
            Drawn::Row(r) if r.member => Some(r.name.clone()),
            _ => None,
        })
        .collect()
}

/// A click on drawn entry `i` of `plan`, through the popup's handler.
fn click(popup: &mut Popup, plan: &Plan, i: usize) -> Reply {
    popup.click(&plan.entries[i])
}

type Sim = Simulator<'static, Clicked, Theme, cosmic::Renderer>;

/// The popup content in libcosmic's popup width.
fn simulate(plan: &Plan) -> Sim {
    let content = container(popup::view(plan)).id(Id::new("root"));
    Simulator::with_size(Default::default(), Size::new(popup::WIDTH, HEIGHT), content)
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

#[test]
fn c_applet_popup_1() {
    puron_testkit::limit("C-applet.popup-1", LIMIT, || {
        setup();
        let (_, m, plan) = capture_plan(false, &pinned("P1"));
        let max = i18n::text("row-maximum");
        let want: Vec<String> = TEMPS
            .iter()
            .map(|s| s.to_string())
            .chain(
                [
                    "—", &max, "—", "CPU fan", "—", "in0", "—", "homepage", "settings",
                ]
                .map(String::from),
            )
            .collect();
        let got: Vec<String> = plan
            .entries
            .iter()
            .zip(notation(&plan))
            .map(|(e, n)| match e {
                Drawn::Row(r) => r.name.clone(),
                _ => n,
            })
            .collect();
        assert_eq!(got, want);
        // One drawn entry per entry, in Entries order, nothing added.
        assert_eq!(plan.entries.len(), m.entries.len());
        for (e, d) in m.entries.iter().zip(&plan.entries) {
            match (e, d) {
                (menu::Entry::Sensor(it) | menu::Entry::Maximum(it), Drawn::Row(r)) => {
                    assert_eq!(r.identity, it.identity)
                }
                (menu::Entry::Separator, Drawn::Separator)
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
        let (_, _, plan) = capture_plan(false, &pinned("P1"));
        let i = row_index(&plan, "CPU");
        let mut sim = simulate(&plan);
        let r: Vec<Rectangle> = ["check", "icon", "name", "value"]
            .iter()
            .map(|e| element(&mut sim, i, e))
            .collect();
        for k in 1..r.len() {
            assert!(
                r[k].x > r[k - 1].x,
                "element {k} left {} not after {}",
                r[k].x,
                r[k - 1].x
            );
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
        let (_, _, plan) = capture_plan(false, &pinned("P1"));
        let (cpu, core0) = (row_index(&plan, "CPU"), row_index(&plan, "Core 0"));
        assert!(matches!(&plan.entries[cpu], Drawn::Row(r) if r.pinned));
        assert!(matches!(&plan.entries[core0], Drawn::Row(r) if !r.pinned));
        let mut sim = simulate(&plan);
        let (a, b) = (
            element(&mut sim, cpu, "icon"),
            element(&mut sim, core0, "icon"),
        );
        assert!(near(a.x, b.x, 0.5), "icon left {} and {}", a.x, b.x);
    });
}

#[test]
fn c_applet_popup_4() {
    puron_testkit::limit("C-applet.popup-4", LIMIT, || {
        setup();
        let (_, _, plan) = capture_plan(false, &pinned("P1"));
        let i = row_index(&plan, "CPU");
        let mut sim = simulate(&plan);
        let (icon, name, value) = (
            element(&mut sim, i, "icon"),
            element(&mut sim, i, "name"),
            element(&mut sim, i, "value"),
        );
        let gap = (right(&icon) + value.x) / 2.0;
        let centre = name.x + name.width / 2.0;
        assert!(
            near(centre, gap, 1.0),
            "name centre {centre}, gap centre {gap}"
        );
    });
}

#[test]
fn c_applet_popup_5() {
    puron_testkit::limit("C-applet.popup-5", LIMIT, || {
        setup();
        let (_, _, plan) = capture_plan(false, &pinned("P1"));
        let i = row_index(&plan, "CPU");
        let mut sim = simulate(&plan);
        let row = bounds(&mut sim, popup::entry_id(i));
        let value = element(&mut sim, i, "value");
        // The button's right edge less libcosmic's padding.
        let edge = right(&row) - applet::menu_control_padding().right;
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
        let (_, _, p1) = capture_plan(false, &pinned("P1"));
        assert_eq!(marked(&p1), ["CPU", "WiFi", "SSD", "CPU fan"]);
        let (_, _, p3) = capture_plan(false, &pinned("P3"));
        assert_eq!(marked(&p3), ["WiFi".to_string(), i18n::text("row-maximum")]);
    });
}

#[test]
fn c_applet_popup_7() {
    puron_testkit::limit("C-applet.popup-7", LIMIT, || {
        setup();
        let (mut p, m, plan) = capture_plan(true, &pinned("P1"));
        let h = header_index(&plan, GroupKind::Temperature);
        click(&mut p, &plan, h);
        let plan = p.plan(&m, &pinned("P1"), icon_size());
        let rows: Vec<usize> = (0..plan.entries.len())
            .filter(|&i| matches!(&plan.entries[i], Drawn::Row(r) if r.member))
            .collect();
        assert_eq!(rows.len(), TEMPS.len());
        let mut sim = simulate(&plan);
        let header = bounds(&mut sim, popup::entry_id(h));
        for i in rows {
            let r = bounds(&mut sim, popup::entry_id(i));
            assert!(
                near(r.x, header.x + 16.0, 0.5),
                "member {i} left {}, header left {}",
                r.x,
                header.x
            );
        }
    });
}

#[test]
fn c_applet_popup_8() {
    puron_testkit::limit("C-applet.popup-8", LIMIT, || {
        setup();
        let pins = pinned("P1");
        let (mut p, m, plan) = capture_plan(true, &pins);
        assert!(members(&plan).is_empty());
        let h = header_index(&plan, GroupKind::Temperature);
        let first = click(&mut p, &plan, h);
        assert_eq!(
            first,
            Reply {
                event: None,
                close: false
            }
        );
        let open = p.plan(&m, &pins, icon_size());
        assert_eq!(open.open, [GroupKind::Temperature]);
        assert_eq!(members(&open), TEMPS);
        let members_at: Vec<usize> = (h + 1..h + 1 + TEMPS.len()).collect();
        assert!(members_at
            .iter()
            .all(|&i| matches!(&open.entries[i], Drawn::Row(r) if r.member)));
        let second = click(&mut p, &open, h);
        assert_eq!(
            second,
            Reply {
                event: None,
                close: false
            }
        );
        let closed = p.plan(&m, &pins, icon_size());
        assert!(closed.open.is_empty());
        assert_eq!(notation(&closed), notation(&plan));
    });
}

#[test]
fn c_applet_popup_9() {
    puron_testkit::limit("C-applet.popup-9", LIMIT, || {
        setup();
        let pins = pinned("P1");
        let mut p = Popup::default();
        let s1 = rebuild("S1");
        let m1 = lay(&s1, true, None);
        let plan = shown(&mut p, &m1, &pins);
        click(&mut p, &plan, header_index(&plan, GroupKind::Temperature));
        assert!(p.is_open(GroupKind::Temperature));
        // S1 again, in place.
        let m2 = lay(&s1, true, Some(&m1.key));
        assert_eq!(m2.update, Update::InPlace);
        let plan2 = shown(&mut p, &m2, &pins);
        assert_eq!(plan2.open, [GroupKind::Temperature]);
        let want: Vec<String> = m2
            .entries
            .iter()
            .flat_map(|e| match e {
                menu::Entry::Group { members, .. } => members
                    .iter()
                    .map(|it| i18n::value_text(it.value.as_ref()))
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        let got: Vec<String> = plan2
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Row(r) if r.member => Some(r.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(got, want, "the second layout's values");
        // S3, a rebuild.
        let m3 = lay(&rebuild("S3"), true, Some(&m1.key));
        assert_eq!(m3.update, Update::Rebuild);
        let plan3 = shown(&mut p, &m3, &pins);
        assert!(
            plan3.open.is_empty(),
            "open after rebuild: {:?}",
            plan3.open
        );
        assert!(members(&plan3).is_empty());
    });
}

#[test]
fn c_applet_popup_10() {
    puron_testkit::limit("C-applet.popup-10", LIMIT, || {
        setup();
        let pins = pinned("P1");
        let (mut p, m, plan) = capture_plan(true, &pins);
        click(&mut p, &plan, header_index(&plan, GroupKind::Temperature));
        assert!(p.is_open(GroupKind::Temperature));
        // The popup closes (nothing reaches the popup), then Opened.
        p.opened();
        let plan = p.plan(&m, &pins, icon_size());
        assert!(plan.open.is_empty(), "open after Opened: {:?}", plan.open);
        assert!(members(&plan).is_empty());
    });
}

#[test]
fn c_applet_popup_12() {
    puron_testkit::limit("C-applet.popup-12", LIMIT, || {
        setup();
        let (mut p, _, plan) = capture_plan(false, &pinned("P1"));
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
        assert_eq!(click(&mut p, &plan, w), homepage);
        // No raw readings: the message entry.
        let mut p = Popup::default();
        let m = lay(&[], false, None);
        let plan = shown(&mut p, &m, &pinned("P1"));
        let i = plan
            .entries
            .iter()
            .position(|e| matches!(e, Drawn::Message { .. }))
            .expect("message entry");
        assert_eq!(click(&mut p, &plan, i), homepage);
    });
}

#[test]
fn c_applet_popup_13() {
    puron_testkit::limit("C-applet.popup-13", LIMIT, || {
        setup();
        let (mut p, _, plan) = capture_plan(false, &pinned("P1"));
        let i = plan
            .entries
            .iter()
            .position(|e| matches!(e, Drawn::SettingsLink { .. }))
            .expect("settings link");
        assert_eq!(
            click(&mut p, &plan, i),
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
        let mut p = Popup::default();
        let m = lay(&raw, false, None);
        let plan = shown(&mut p, &m, &[]);
        let names: BTreeMap<String, &str> = plan
            .entries
            .iter()
            .filter_map(|e| match e {
                Drawn::Row(r) => Some((r.identity.clone(), r.icon)),
                _ => None,
            })
            .collect();
        let want: BTreeMap<String, &str> = [
            ("temperature/lm-sensors/k0", GPU),
            ("temperature/lm-sensors/k1", TEMP),
            ("temperature/lm-sensors/k2", DRIVE),
            ("fan/lm-sensors/k3", FAN),
            ("voltage/lm-sensors/k4", VOLT),
            ("power/lm-sensors/k5", VOLT),
            ("@maximum", TEMP),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        assert_eq!(names, want);
        // Each icon comes from the one map, `panel::icon_name`.
        for e in &m.entries {
            if let menu::Entry::Sensor(it) | menu::Entry::Maximum(it) = e {
                assert_eq!(names[&it.identity], panel::icon_name(it.icon));
            }
        }
        // Groups on: the temperature header.
        let (_, _, plan) = capture_plan(true, &pinned("P1"));
        let h = header_index(&plan, GroupKind::Temperature);
        let Drawn::Header { icon, .. } = &plan.entries[h] else {
            unreachable!()
        };
        assert_eq!(*icon, TEMP);
        assert_eq!(*icon, panel::group_icon_name(GroupKind::Temperature));
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
        // The panel items follow the pinned list.
        let on_panel: Vec<(usize, &str)> = plan
            .elements
            .iter()
            .filter_map(|e| match e {
                panel::Element::Icon { item, name } => Some((*item, *name)),
                _ => None,
            })
            .collect();
        assert_eq!(on_panel, [(0, FAN), (1, TEMP), (2, TEMP), (3, DRIVE)]);
        let (_, _, popup) = capture_plan(false, &pins);
        for (item, name) in on_panel {
            let row = popup
                .entries
                .iter()
                .find_map(|e| match e {
                    Drawn::Row(r) if r.identity == pins[item] => Some(r),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("popup row {}", pins[item]));
            assert_eq!(row.icon, name, "identity {}", pins[item]);
        }
    });
}

#[test]
fn c_applet_popup_15() {
    puron_testkit::limit("C-applet.popup-15", LIMIT, || {
        setup();
        let mut pins = pinned("P1");
        let (mut p, m, plan) = capture_plan(true, &pins);
        click(&mut p, &plan, header_index(&plan, GroupKind::Temperature));
        let before = p.plan(&m, &pins, icon_size());
        assert_eq!(marked(&before), ["CPU", "WiFi", "SSD", "CPU fan"]);
        let max = i18n::text("row-maximum");
        let steps = [
            (
                "SSD",
                "temperature/lm-sensors/nvme-pci-0100/temp1",
                vec!["CPU", "WiFi", "CPU fan"],
            ),
            ("@maximum", "@maximum", vec!["CPU", "WiFi", &max, "CPU fan"]),
        ];
        let mut plan = before;
        for (name, identity, want) in steps {
            let at = if name == "@maximum" {
                max.as_str()
            } else {
                name
            };
            let i = row_index(&plan, at);
            let reply = click(&mut p, &plan, i);
            assert_eq!(
                reply,
                Reply {
                    event: Some(Event::PinClick(identity.to_string())),
                    close: false
                }
            );
            // `app` passes the click to `core.pins`; the
            // default settings show every class.
            pins = puron_core::pins::click(&pins, identity, &m.identities, &ShownClasses::ALL);
            let next = p.plan(&m, &pins, icon_size());
            assert_eq!(next.open, [GroupKind::Temperature], "after {name}");
            assert_eq!(marked(&next), want, "after {name}");
            // No row moves; only the clicked row's mark changes.
            let unmarked = |pl: &Plan| -> Vec<String> {
                notation(pl)
                    .iter()
                    .map(|n| n.replace("[x]", "[ ]"))
                    .collect()
            };
            assert_eq!(unmarked(&next), unmarked(&plan), "after {name}");
            plan = next;
        }
    });
}

#[test]
fn c_applet_popup_16() {
    puron_testkit::limit("C-applet.popup-16", LIMIT, || {
        setup();
        let mut p = Popup::default();
        let m5 = lay(&rebuild("S5"), true, None);
        let plan = shown(&mut p, &m5, &[]);
        let h = header_index(&plan, GroupKind::Temperature);
        click(&mut p, &plan, h);
        assert_eq!(
            members(&p.plan(&m5, &[], icon_size())),
            ["edge", "junction"]
        );
        let m6 = lay(&rebuild("S6"), true, Some(&m5.key));
        assert_eq!(m6.update, Update::Rebuild);
        let plan6 = shown(&mut p, &m6, &[]);
        assert!(plan6.open.is_empty(), "open after S6: {:?}", plan6.open);
        assert!(members(&plan6).is_empty());
        // The new order, once the group is opened again.
        click(&mut p, &plan6, header_index(&plan6, GroupKind::Temperature));
        assert_eq!(
            members(&p.plan(&m6, &[], icon_size())),
            ["junction", "edge"]
        );
    });
}
