//! Tests of the look that the popup's pages share: the side padding of
//! every page, and the color roles of the About and Settings pages with
//! the one function that maps them to the theme's colors.
//!
//! The temperatures come from the captured readings with the default
//! settings and the pinned list `P1` of `pins.json`; the About page from
//! version 1.6.0, the Debian package and the source set `all` (lm-sensors
//! available with 12 readings, NVMe with 2, NVIDIA not available, udisks2
//! with 1, liquidctl not available); the Settings page from the `good`
//! settings fixture. The layout tests draw a page in iced's headless test
//! simulator at the narrowest and the widest popup width, find widgets
//! by the IDs that the views give them, and compare their rectangles in
//! logical pixels with a tolerance of half a pixel.
#![allow(non_snake_case)]

mod common;

use std::collections::BTreeMap;
use std::process::Command;

use cosmic::applet::cosmic_panel_config::PanelSize;
use cosmic::applet::{Context, Size as AppletSize};
use cosmic::iced::{Color, Rectangle, Size};
use cosmic::widget::{container, Id};
use cosmic::Theme;
use iced_test::selector::{self, Candidate, Selector};
use iced_test::Simulator;
use puron_applet::about::{self, Package, Place, Source};
use puron_applet::look::{self, Role, ROLES};
use puron_applet::panel;
use puron_applet::popup::{self, Drawn, PAGE_SIDE_PADDING};
use puron_applet::settings;
use puron_config::Config;
use puron_core::menu::{self, Menu, Report};
use puron_core::rows::RawReading;
use puron_core::settings::Settings;
use puron_testkit::cfg;

const LIMIT: u64 = 5;
/// The tolerance in px of the checks that compare edges.
const TOL: f32 = 0.5;
/// The popup's height limit.
const HEIGHT: f32 = 1000.0;

/// The icon size in px that libcosmic gives for panel size M.
fn icon_size() -> u16 {
    panel::icon_size(&Context {
        size: AppletSize::PanelSize(PanelSize::M),
        ..Context::default()
    })
}

/// The source set `all`, in the page's order.
const ALL: [Source; 5] = [
    Source::Available(12),
    Source::Available(2),
    Source::NotAvailable,
    Source::Available(1),
    Source::NotAvailable,
];

fn raw(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

/// The temperatures' layout of the captured readings with the default
/// settings and lm-sensors answered.
fn capture_menu() -> Menu {
    let raw: Vec<RawReading> = serde_json::from_str(&raw("capture.json")).expect("capture");
    let s = Settings::default();
    menu::menu(&menu::shown(&raw, &s), &s, Report::Answered, None, false)
}

/// The pinned list `P1` of `pins.json`.
fn p1() -> Vec<String> {
    let sets: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&raw("pins.json")).expect("parse pins.json");
    serde_json::from_value(sets["P1"].clone()).expect("P1")
}

fn readings_plan() -> popup::Plan {
    popup::plan(&capture_menu(), &p1(), icon_size())
}

fn about_plan() -> about::Plan {
    about::plan(&about::Input {
        version: "1.6.0".to_string(),
        package: Package::Deb,
        sources: ALL,
    })
}

/// The Settings input of `applied`, shown and applied, with no typed text,
/// no notice, the package kind `package`, the sources `sources` and the
/// number of drive readings `drives`.
fn settings_input(
    applied: Settings,
    package: Package,
    sources: [Source; 5],
    drives: usize,
) -> settings::Input {
    settings::Input {
        values: applied.clone(),
        applied,
        typed: None,
        notice: false,
        sources,
        drive_count: drives,
        package,
    }
}

/// The Settings plan of the `good` fixture on the Debian package with the
/// source set `all`.
fn settings_plan() -> settings::Plan {
    let fx = cfg::build("good");
    let values = Config::new(&fx.root).load().settings;
    settings::plan(&settings_input(values, Package::Deb, ALL, 0))
}

/// A simulator that draws `content` in a popup `width` px wide, with the
/// ID `root` on the whole of it.
fn sim<'a, M: Clone + 'a>(
    content: cosmic::Element<'a, M>,
    width: f32,
) -> Simulator<'a, M, Theme, cosmic::Renderer> {
    let root = container(content).id(Id::new("root"));
    Simulator::with_size(Default::default(), Size::new(width, HEIGHT), root)
}

fn rect<M: Clone>(s: &mut Simulator<'_, M, Theme, cosmic::Renderer>, id: Id) -> Rectangle {
    s.find(selector::id(id.clone()))
        .unwrap_or_else(|e| panic!("find {id:?}: {e:?}"))
        .bounds()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOL
}

/// Asserts that `r` starts `PAGE_SIDE_PADDING` right of the popup's left
/// edge `root`.
fn at_left(what: &str, width: f32, r: Rectangle, root: Rectangle) {
    let side = f32::from(PAGE_SIDE_PADDING);
    assert!(
        near(r.x, root.x + side),
        "width {width}: {what} left edge {} against {}",
        r.x,
        root.x + side
    );
}

/// Asserts that `r` ends `PAGE_SIDE_PADDING` left of the popup's right
/// edge.
fn at_right(what: &str, width: f32, r: Rectangle, root: Rectangle) {
    let side = f32::from(PAGE_SIDE_PADDING);
    assert!(
        near(r.x + r.width, root.x + root.width - side),
        "width {width}: {what} right edge {} against {}",
        r.x + r.width,
        root.x + root.width - side
    );
}

/// The temperatures' plan, the About plan and the Settings plan each carry
/// the side padding, and it is the shared constant, 24 px.
#[test]
fn c_applet_popup_44__plans() {
    puron_testkit::limit("C-applet.popup-44", LIMIT, || {
        common::start_in_english();
        assert_eq!(PAGE_SIDE_PADDING, 24);
        assert_eq!(readings_plan().side_padding, PAGE_SIDE_PADDING, "readings");
        assert_eq!(about_plan().side_padding, PAGE_SIDE_PADDING, "About");
        assert_eq!(settings_plan().side_padding, PAGE_SIDE_PADDING, "Settings");
    });
}

/// The index in `plan` of the section heading "Temperature".
fn temperature_heading(plan: &popup::Plan) -> usize {
    plan.entries
        .iter()
        .position(|e| matches!(e, Drawn::Section { text, .. } if text == "Temperature"))
        .expect("a Temperature heading")
}

/// The temperatures drawn at the narrowest and the widest popup: the tile
/// of the section heading "Temperature" starts 24 px right of the popup's
/// left edge; each card reaches from 24 px right of the popup's left edge
/// to 24 px left of its right edge; and every line's value ends 38 px left
/// of the popup's right edge, the side padding and the 14 px of the line's
/// padding in its card.
#[test]
fn c_applet_popup_44__readings() {
    puron_testkit::limit("C-applet.popup-44", LIMIT, || {
        common::start_in_english();
        let plan = readings_plan();
        let heading = temperature_heading(&plan);
        for width in [popup::WIDTH, popup::MAX_WIDTH] {
            let mut s = sim(popup::view(&plan), width);
            let root = rect(&mut s, Id::new("root"));
            let tile = rect(&mut s, popup::element_id(heading, "tile"));
            at_left("Temperature tile", width, tile, root);
            let mut lines = 0;
            let mut cards = 0;
            for (i, e) in plan.entries.iter().enumerate() {
                if let Drawn::Line(l) = e {
                    let value = rect(&mut s, popup::element_id(i, "value"));
                    let edge = root.x + root.width - 38.0;
                    assert!(
                        near(value.x + value.width, edge),
                        "width {width}: value of {} ends at {} against {edge}",
                        l.text,
                        value.x + value.width
                    );
                    lines += 1;
                    cards = cards.max(l.card);
                }
            }
            assert!(lines > 0, "no line drawn");
            for n in 1..=cards {
                let card = rect(&mut s, popup::card_id(n));
                at_left(&format!("card {n}"), width, card, root);
                at_right(&format!("card {n}"), width, card, root);
            }
        }
    });
}

/// The About page drawn at the narrowest and the widest popup: Back's
/// icon, the two headings, the three cards and the "Copy details" button
/// start 24 px right of the popup's left edge, and the cards and the
/// button end 24 px left of its right edge.
#[test]
fn c_applet_popup_44__about() {
    puron_testkit::limit("C-applet.popup-44", LIMIT, || {
        common::start_in_english();
        let plan = about_plan();
        let headings: Vec<usize> = plan
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, about::Entry::Heading(_)))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(headings.len(), 2, "headings");
        let copy = plan.entries.len() - 1;
        assert!(matches!(plan.entries[copy], about::Entry::Copy { .. }));
        for width in [popup::WIDTH, popup::MAX_WIDTH] {
            let mut s = sim(about::view(&plan), width);
            let root = rect(&mut s, Id::new("root"));
            let back = rect(&mut s, popup::element_id(0, "icon"));
            at_left("Back icon", width, back, root);
            for &h in &headings {
                let r = rect(&mut s, popup::entry_id(h));
                at_left(&format!("heading {h}"), width, r, root);
            }
            for place in [Place::CardFacts, Place::CardSources, Place::CardLinks] {
                let r = rect(&mut s, about::card_id(place));
                at_left(&format!("{place:?}"), width, r, root);
                at_right(&format!("{place:?}"), width, r, root);
            }
            let r = rect(&mut s, popup::element_id(copy, "button"));
            at_left("Copy details", width, r, root);
            at_right("Copy details", width, r, root);
        }
    });
}

/// The left edge of the "Temperature" heading's tile on the temperatures
/// drawn `width` px wide, relative to the popup's left edge.
fn temperature_tile_x(width: f32) -> f32 {
    let plan = readings_plan();
    let i = temperature_heading(&plan);
    let mut s = sim(popup::view(&plan), width);
    let root = rect(&mut s, Id::new("root"));
    rect(&mut s, popup::element_id(i, "tile")).x - root.x
}

/// The Settings page drawn at the narrowest and the widest popup: Back's
/// icon and each section's icon tile start 24 px right of the popup's
/// left edge, each tile where the temperatures' "Temperature" tile starts
/// at the same width, and each section's card reaches from 24 px right of
/// the popup's left edge to 24 px left of its right edge.
#[test]
fn c_applet_popup_44__settings() {
    puron_testkit::limit("C-applet.popup-44", LIMIT, || {
        common::start_in_english();
        let plan = settings_plan();
        for width in [popup::WIDTH, popup::MAX_WIDTH] {
            let readings = temperature_tile_x(width);
            let mut s = sim(settings::view(&plan), width);
            let root = rect(&mut s, Id::new("root"));
            let back = rect(&mut s, settings::id("back-icon"));
            at_left("Back icon", width, back, root);
            for n in 1..=5 {
                let tile = rect(&mut s, settings::id(format!("tile-{n}")));
                at_left(&format!("tile {n}"), width, tile, root);
                assert!(
                    near(tile.x - root.x, readings),
                    "width {width}: tile {n} at {} against the Temperature tile at {readings}",
                    tile.x - root.x
                );
                let card = rect(&mut s, settings::id(format!("card-{n}")));
                at_left(&format!("card {n}"), width, card, root);
                at_right(&format!("card {n}"), width, card, root);
            }
        }
    });
}

/// The Settings plan of the `good` fixture: the five section headings in
/// order, each with its tile's icon and color role, and each icon name is
/// a line of the icon list of the test fixtures, so that it is one the
/// applet is known to find.
#[test]
fn c_applet_popup_45() {
    puron_testkit::limit("C-applet.popup-45", LIMIT, || {
        common::start_in_english();
        let plan = settings_plan();
        let got: Vec<(String, &str, Role)> = plan
            .sections()
            .iter()
            .map(|s| (s.heading.clone(), s.icon, s.role))
            .collect();
        let want = [
            ("Display Options", "video-display-symbolic", Role::Accent),
            (
                "Generic Sensor Providers",
                "puron-cpu-temperature-symbolic",
                Role::Orange,
            ),
            (
                "GPU Sensor Providers",
                "puron-gpu-temperature-symbolic",
                Role::Purple,
            ),
            (
                "Drive Sensor Providers",
                "drive-harddisk-system-symbolic",
                Role::Indigo,
            ),
            ("Show Sensors", "puron-temperature-symbolic", Role::Green),
        ];
        let want: Vec<(String, &str, Role)> = want
            .iter()
            .map(|(h, i, r)| (h.to_string(), *i, *r))
            .collect();
        assert_eq!(got, want);
        let names = std::fs::read_to_string(puron_testkit::fixture("icons", "names.tsv"))
            .expect("read names.tsv");
        for (_, icon, _) in &got {
            assert!(
                names.lines().any(|l| l.split('\t').nth(2) == Some(icon)),
                "{icon} is not in names.tsv"
            );
        }
    });
}

/// The source set `flatpak-off`: as `all`, with liquidctl off.
const FLATPAK_OFF: [Source; 5] = [
    Source::Available(12),
    Source::Available(2),
    Source::NotAvailable,
    Source::Available(1),
    Source::Off,
];

/// The state line of each item of the Settings plan for `input`, as
/// `(item, text, role)`, for the items that have one.
fn lines(input: &settings::Input) -> Vec<(usize, String, Role)> {
    settings::plan(input)
        .items()
        .iter()
        .enumerate()
        .filter_map(|(i, it)| it.state.as_ref().map(|s| (i + 1, s.text.clone(), s.role)))
        .collect()
}

/// The state line of item `n` for `input`. Every plan must have one line
/// for each of items 11 to 16 and none for any other item.
fn line_of(input: &settings::Input, n: usize) -> (String, Role) {
    let all = lines(input);
    let items: Vec<usize> = all.iter().map(|l| l.0).collect();
    assert_eq!(
        items,
        (11..=16).collect::<Vec<_>>(),
        "items with a state line"
    );
    let (_, text, role) = all.into_iter().find(|l| l.0 == n).expect("a line");
    (text, role)
}

fn want(text: &str, role: Role) -> (String, Role) {
    (text.to_string(), role)
}

/// lm-sensors on (as by default) with the source set `all`: its line
/// reads "available · 12" in green.
#[test]
fn c_applet_popup_46__available() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let s = Settings::default();
        assert!(s.provider_lm_sensors);
        let input = settings_input(s, Package::Deb, ALL, 0);
        assert_eq!(line_of(&input, 11), want("available · 12", Role::Success));
    });
}

/// Udisks2 off (as by default) with the source set `all`, where udisks2
/// is available: its line reads "off" in grey, since the line follows
/// the switch first.
#[test]
fn c_applet_popup_46__off() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let s = Settings::default();
        assert!(!s.provider_udisks2);
        let input = settings_input(s, Package::Deb, ALL, 0);
        assert_eq!(line_of(&input, 15), want("off", Role::Muted));
    });
}

/// Nvidia on with the source set `all`, where NVIDIA is not available:
/// its line reads "not available" in grey.
#[test]
fn c_applet_popup_46__unavailable() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let s = Settings {
            provider_nvidia: true,
            ..Settings::default()
        };
        let input = settings_input(s, Package::Deb, ALL, 0);
        assert_eq!(line_of(&input, 13), want("not available", Role::Muted));
    });
}

/// liquidctl in the Flatpak: switched on with `flatpak-on`, its line reads
/// "not available in the Flatpak" in the warning color with no count;
/// switched off with `flatpak-off`, "off" in grey; switched on in the
/// Debian package with `all`, "not available" in grey.
#[test]
fn c_applet_popup_46__flatpak() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let on = Settings {
            provider_liquidctl: true,
            ..Settings::default()
        };
        let off = Settings {
            provider_liquidctl: false,
            ..on.clone()
        };
        assert_eq!(
            line_of(&settings_input(on.clone(), Package::Flatpak, ALL, 0), 12),
            want("not available in the Flatpak", Role::Warning)
        );
        assert_eq!(
            line_of(&settings_input(off, Package::Flatpak, FLATPAK_OFF, 0), 12),
            want("off", Role::Muted)
        );
        assert_eq!(
            line_of(&settings_input(on, Package::Deb, ALL, 0), 12),
            want("not available", Role::Muted)
        );
    });
}

/// The drives switch with 3 drive readings and the source set `all`:
/// with lm-sensors and the drives switch on it reads "available · 3" in
/// green, the drive count and not lm-sensors' 12; with the drives switch
/// off, "off"; with lm-sensors off and the drives switch on, "off" too,
/// since lm-sensors then reads no drive either.
#[test]
fn c_applet_popup_46__drives() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let s = Settings {
            provider_lm_sensors: true,
            provider_lm_sensors_drives: true,
            ..Settings::default()
        };
        assert_eq!(
            line_of(&settings_input(s.clone(), Package::Deb, ALL, 3), 14),
            want("available · 3", Role::Success)
        );
        let drives_off = Settings {
            provider_lm_sensors_drives: false,
            ..s.clone()
        };
        assert_eq!(
            line_of(&settings_input(drives_off, Package::Deb, ALL, 3), 14),
            want("off", Role::Muted)
        );
        let lm_off = Settings {
            provider_lm_sensors: false,
            ..s
        };
        assert_eq!(
            line_of(&settings_input(lm_off, Package::Deb, ALL, 3), 14),
            want("off", Role::Muted)
        );
    });
}

/// nvme-cli switched on with all five sources off, as in a snapshot taken
/// before the source started: its line reads "available · 0" in green,
/// the state of a started source that has not reported yet.
#[test]
fn c_applet_popup_46__started() {
    puron_testkit::limit("C-applet.popup-46", LIMIT, || {
        common::start_in_english();
        let s = Settings {
            provider_nvme: true,
            ..Settings::default()
        };
        let input = settings_input(s, Package::Deb, [Source::Off; 5], 0);
        assert_eq!(line_of(&input, 16), want("available · 0", Role::Success));
    });
}

/// Selects the first text widget whose content is the given text. iced's
/// own text selector is not used because it also asks text inputs for
/// their text, which libcosmic's text input does not support at the
/// revision this workspace pins.
struct TextIs(String);

impl Selector for TextIs {
    type Output = Rectangle;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<Rectangle> {
        match candidate {
            Candidate::Text {
                bounds, content, ..
            } if content == self.0 => Some(bounds),
            _ => None,
        }
    }

    fn description(&self) -> String {
        format!("text == {:?}", self.0)
    }
}

/// The rectangle of the first text that reads `content`.
fn text_rect<M: Clone>(
    s: &mut Simulator<'_, M, Theme, cosmic::Renderer>,
    content: &str,
) -> Rectangle {
    s.find(TextIs(content.to_string()))
        .unwrap_or_else(|e| panic!("text {content}: {e:?}"))
}

/// Whether `r` lies within `area`, give or take `TOL`.
fn inside(r: Rectangle, area: Rectangle) -> bool {
    r.x >= area.x - TOL
        && r.y >= area.y - TOL
        && r.x + r.width <= area.x + area.width + TOL
        && r.y + r.height <= area.y + area.height + TOL
}

/// The Settings page of the `good` fixture at the widest popup with the
/// source set `all`. Each section's tile is 28 × 28 px with its 16 × 16
/// icon centered within 1 px, and the heading text starts 10 px after
/// the tile. All the items of a section lie inside one card, which starts
/// where the tile starts and below it; each label starts 14 px inside the
/// card. Each state line of items 11 to 16 is below its label, with a
/// 6 × 6 dot.
#[test]
fn c_applet_popup_48() {
    puron_testkit::limit("C-applet.popup-48", LIMIT, || {
        common::start_in_english();
        let plan = settings_plan();
        let mut s = sim(settings::view(&plan), popup::MAX_WIDTH);
        let mut n = 0;
        for (k, sec) in plan.sections().iter().enumerate() {
            let k = k + 1;
            let tile = rect(&mut s, settings::id(format!("tile-{k}")));
            let icon = rect(&mut s, settings::id(format!("tile-icon-{k}")));
            let heading = rect(&mut s, settings::id(format!("heading-{k}")));
            let card = rect(&mut s, settings::id(format!("card-{k}")));
            assert!(
                near(tile.width, 28.0) && near(tile.height, 28.0),
                "tile {k} {tile:?}"
            );
            assert!(
                near(icon.width, 16.0) && near(icon.height, 16.0),
                "tile icon {k} {icon:?}"
            );
            let (tc, ic) = (tile.center(), icon.center());
            assert!(
                (tc.x - ic.x).abs() <= 1.0 && (tc.y - ic.y).abs() <= 1.0,
                "tile {k}: icon {icon:?} not centered in {tile:?}"
            );
            assert!(
                near(heading.x, tile.x + tile.width + 10.0),
                "section {k}: heading starts at {} after tile {tile:?}",
                heading.x
            );
            assert!(
                near(card.x, tile.x),
                "section {k}: card {card:?} tile {tile:?}"
            );
            assert!(
                card.y >= tile.y + tile.height - TOL,
                "section {k}: card {card:?} above tile end {tile:?}"
            );
            for item in &sec.items {
                n += 1;
                let r = rect(&mut s, settings::id(format!("item-{n}")));
                assert!(inside(r, card), "item {n} {r:?} outside card {card:?}");
                let label = if item.state.is_some() {
                    rect(&mut s, settings::id(format!("label-{n}")))
                } else {
                    text_rect(&mut s, &item.label)
                };
                assert!(
                    near(label.x, card.x + 14.0),
                    "item {n}: label starts at {} in card {card:?}",
                    label.x
                );
                if item.state.is_some() {
                    let state = rect(&mut s, settings::id(format!("state-{n}")));
                    let dot = rect(&mut s, settings::id(format!("dot-{n}")));
                    let text = text_rect(&mut s, &item.label);
                    assert!(
                        state.y >= text.y + text.height - TOL,
                        "item {n}: state line {state:?} over label {text:?}"
                    );
                    assert!(
                        near(dot.width, 6.0) && near(dot.height, 6.0),
                        "item {n}: dot {dot:?}"
                    );
                }
            }
        }
        assert_eq!(n, 20, "items");
    });
}

/// No page of the popup takes its side padding from libcosmic's applet
/// helpers, whose padding follows the theme's density: a search of the
/// applet's source for their names finds nothing.
#[test]
fn c_applet_popup_44__source() {
    puron_testkit::limit("C-applet.popup-44", LIMIT, || {
        let out = Command::new("grep")
            .args([
                "-rnE",
                "menu_control_padding|padded_control",
                "crates/puron-applet/src",
            ])
            .current_dir(puron_testkit::workspace_root())
            .output()
            .expect("run grep");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "",
            "the applet still names libcosmic's padding"
        );
        assert_eq!(out.status.code(), Some(1), "grep exit");
    });
}

/// The color that the theme gives `role`, read from the theme's own fields
/// as the table of roles names them.
fn expected(role: Role, theme: &Theme) -> Option<Color> {
    let t = theme.cosmic();
    let p = &t.palette;
    let c = match role {
        Role::Default => return None,
        Role::Muted => {
            let mut c: Color = p.neutral_9.into();
            c.a = 0.7;
            return Some(c);
        }
        Role::Success => t.success_color(),
        Role::Warning => t.warning_color(),
        Role::Destructive => t.destructive_color(),
        Role::Accent => t.accent_text_color(),
        Role::Orange => p.accent_orange,
        Role::Purple => p.accent_purple,
        Role::Indigo => p.accent_indigo,
        Role::Green => p.accent_green,
    };
    Some(c.into())
}

/// The icon lookup that the popup's tiles, headings and buttons draw
/// through has no fallback, for every icon name that Puron uses. With
/// libcosmic's default fallback, a theme that lacks a name would look it
/// up again cut at each "-" from the end, so a missing
/// `puron-cpu-temperature-symbolic` could draw the icon of `puron-cpu` or
/// `puron`; with none, a missing name draws an empty square, as the panel
/// does.
#[test]
fn c_applet_popup_57() {
    puron_testkit::limit("C-applet.popup-57", LIMIT, || {
        let names = std::fs::read_to_string(puron_testkit::fixture("icons", "names.tsv"))
            .expect("read names.tsv");
        let names: Vec<&str> = names.lines().filter_map(|l| l.split('\t').nth(2)).collect();
        assert!(!names.is_empty(), "names.tsv lists no icon");
        for name in names {
            let lookup = look::icon_lookup(name, look::TILE_ICON);
            assert!(
                lookup.fallback.is_none(),
                "{name} has the fallback {:?}",
                lookup.fallback
            );
        }
    });
}

/// Under libcosmic's built-in dark and light themes, each of the ten
/// roles gives the color of the theme that the table of roles names, and
/// the default role gives none; every other role has a different color in
/// the dark theme than in the light one, so no role is a fixed color; and
/// the red of a hot temperature differs from the color of a warm one.
#[test]
fn c_applet_popup_47() {
    puron_testkit::limit("C-applet.popup-47", LIMIT, || {
        let dark = Theme::dark();
        let light = Theme::light();
        assert_eq!(
            ROLES,
            [
                Role::Default,
                Role::Muted,
                Role::Success,
                Role::Warning,
                Role::Destructive,
                Role::Accent,
                Role::Orange,
                Role::Purple,
                Role::Indigo,
                Role::Green,
            ]
        );
        for theme in [&dark, &light] {
            assert_ne!(
                look::color(Role::Destructive, theme),
                look::color(Role::Warning, theme),
                "hot and warm alike"
            );
        }
        for role in ROLES {
            for (name, theme) in [("dark", &dark), ("light", &light)] {
                assert_eq!(
                    look::color(role, theme),
                    expected(role, theme),
                    "{role:?} in the {name} theme"
                );
            }
            if role != Role::Default {
                assert_ne!(
                    look::color(role, &dark),
                    look::color(role, &light),
                    "{role:?} is the same in both themes"
                );
            }
        }
        assert_eq!(look::color(Role::Default, &dark), None);
    });
}
