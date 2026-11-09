//! Tests of the panel button, each with a time limit of 5 s. Some check
//! the panel plan alone; others lay out the widgets built from the plan
//! in iced's headless test simulator, at panel size M, and measure them.
//! Unless a test says otherwise, the items come from
//! `puron_core::panel::panel` with the captured readings and the pinned
//! list `P1` of `pins.json`. The icon theme test runs in a fresh run of
//! this test binary whose `XDG_DATA_DIRS` holds the project's icon files
//! in a temporary hicolor tree.
#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cosmic::applet::cosmic_panel_config::PanelSize;
use cosmic::applet::{Context, Size as AppletSize};
use cosmic::iced::theme::Base;
use cosmic::iced::{Length, Rectangle, Size};
use cosmic::widget::{container, Id};
use cosmic::Theme;
use iced_test::{selector, Simulator};
use puron_applet::panel::{self, Element, Orientation, Plan};
use puron_core::menu;
use puron_core::panel::{Panel, State};
use puron_core::rows::RawReading;
use puron_core::settings::Settings;

const LIMIT: u64 = 5;
const CHILD: &str = "PURON_PANEL_CHILD";
/// The button's size across the panel in px: the height of a horizontal
/// button, the width of a vertical one.
const BUTTON: f32 = 48.0;
/// The simulator window's size along the panel in px.
const LONG: f32 = 600.0;
/// The scale factor at which a snapshot is rendered.
const SCALE: f32 = 2.0;

const FAN: &str = "puron-fan-symbolic";
const TEMP: &str = "puron-temperature-symbolic";
const GPU: &str = "puron-gpu-temperature-symbolic";
const DRIVE: &str = "drive-harddisk-system-symbolic";
const VOLT: &str = "puron-voltage-symbolic";

fn raw(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

fn capture() -> Vec<RawReading> {
    serde_json::from_str(&raw("capture.json")).expect("parse capture.json")
}

fn pinned(name: &str) -> Vec<String> {
    serde_json::from_str::<BTreeMap<String, Vec<String>>>(&raw("pins.json"))
        .expect("parse pins.json")
        .remove(name)
        .expect("X-raw-pins set")
}

fn settings(icons: bool) -> Settings {
    Settings {
        show_panel_icons: icons,
        ..Settings::default()
    }
}

fn core_panel(raw: &[RawReading], pins: &[String], icons: bool) -> Panel {
    let s = settings(icons);
    puron_core::panel::panel(&menu::shown(raw, &s), pins, &s, &State::default())
}

/// An applet context at panel size M. The size is set here, so the
/// environment cannot change it.
fn context() -> Context {
    Context {
        size: AppletSize::PanelSize(PanelSize::M),
        ..Context::default()
    }
}

/// The icon size in px that libcosmic gives for panel size M.
fn m_icon_size() -> f32 {
    PanelSize::M.get_applet_icon_size(true) as f32
}

fn plan_of(p: &Panel, o: Orientation) -> Plan {
    panel::plan(p, o, panel::icon_size(&context()))
}

fn p1_plan(icons: bool, o: Orientation) -> Plan {
    plan_of(&core_panel(&capture(), &pinned("P1"), icons), o)
}

/// The plan's elements, one string each: the item index, then
/// `icon <name>`, `slot` or `text <text>`.
fn kinds(plan: &Plan) -> Vec<String> {
    plan.elements
        .iter()
        .map(|e| match e {
            Element::Icon { item, name } => format!("{item} icon {name}"),
            Element::Slot { item } => format!("{item} slot"),
            Element::Text { item, text } => format!("{item} text {text}"),
        })
        .collect()
}

fn names(plan: &Plan) -> Vec<&'static str> {
    plan.elements
        .iter()
        .filter_map(|e| match e {
            Element::Icon { name, .. } => Some(*name),
            _ => None,
        })
        .collect()
}

type Sim = Simulator<'static, (), Theme, cosmic::Renderer>;

/// A simulator holding the button: the plan's content in a container
/// with the ID `root`, `BUTTON` px across the panel, in a window `LONG` px
/// along it.
fn simulate(plan: &Plan) -> Sim {
    let content = container(panel::view::<()>(plan)).id(Id::new("root"));
    let (button, window) = match plan.orientation {
        Orientation::Horizontal => (
            content.height(Length::Fixed(BUTTON)),
            Size::new(LONG, BUTTON),
        ),
        Orientation::Vertical => (
            content.width(Length::Fixed(BUTTON)),
            Size::new(BUTTON, LONG),
        ),
    };
    Simulator::with_size(Default::default(), window, button)
}

fn bounds(sim: &mut Sim, id: Id) -> Rectangle {
    sim.find(selector::id(id.clone()))
        .unwrap_or_else(|e| panic!("find {id:?}: {e:?}"))
        .bounds()
}

/// The laid-out rectangles of the plan's elements, in plan order.
fn rects(sim: &mut Sim, plan: &Plan) -> Vec<Rectangle> {
    (0..plan.elements.len())
        .map(|i| bounds(sim, panel::element_id(i)))
        .collect()
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn is_text(e: &Element) -> bool {
    matches!(e, Element::Text { .. })
}

/// A rendered snapshot's pixels, as RGBA rows at `SCALE`; `at` drops the
/// alpha.
struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Pixels {
    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    /// The pixels inside a layout rectangle, whose edges are scaled and
    /// rounded inward.
    fn inside(&self, r: &Rectangle) -> Vec<[u8; 3]> {
        let x0 = (r.x * SCALE).ceil() as u32;
        let y0 = (r.y * SCALE).ceil() as u32;
        let x1 = (((r.x + r.width) * SCALE).floor() as u32).min(self.width);
        let y1 = (((r.y + r.height) * SCALE).floor() as u32).min(self.height);
        let mut v = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                v.push(self.at(x, y));
            }
        }
        v
    }
}

fn rgb8(c: cosmic::iced::Color) -> [u8; 3] {
    let q = |v: f32| (v * 255.0).round() as u8;
    [q(c.r), q(c.g), q(c.b)]
}

/// Renders the button with `Theme::dark()` and returns its pixels with the
/// theme's background and text colours. It fails unless the pixel at
/// (0, 0) has the background colour.
fn snapshot(sim: &mut Sim, test: &str) -> (Pixels, [u8; 3], [u8; 3]) {
    let theme = Theme::dark();
    let snap = sim.snapshot(&theme).expect("snapshot");
    let dir = scratch(test);
    let stem = dir.join("snap.png");
    assert!(snap.matches_image(&stem).expect("write snapshot"));
    let file = std::fs::File::open(dir.join("snap-tiny-skia.png")).expect("snapshot png");
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .expect("png header");
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let _ = std::fs::remove_dir_all(&dir);
    let px = Pixels {
        width: info.width,
        height: info.height,
        rgba,
    };
    let base = theme.base();
    let (bg, fg) = (rgb8(base.background_color), rgb8(base.text_color));
    assert_eq!(px.at(0, 0), bg, "background pixel");
    (px, bg, fg)
}

fn scratch(test: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("puron-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch dir");
    d
}

#[test]
fn c_applet_panel_1() {
    puron_testkit::limit("C-applet.panel-1", LIMIT, || {
        let core = core_panel(&capture(), &pinned("P1"), true);
        let plan = plan_of(&core, Orientation::Horizontal);
        let texts: Vec<String> = core
            .items
            .iter()
            .map(|i| match i {
                puron_core::panel::Item::Pin { value, .. } => {
                    puron_applet::i18n::panel_value(value.as_ref().map(|v| v.as_ref()))
                }
                puron_core::panel::Item::EmptyState => panic!("pin item"),
            })
            .collect();
        let want: Vec<String> = [TEMP, DRIVE, TEMP, FAN]
            .iter()
            .zip(&texts)
            .enumerate()
            .flat_map(|(i, (n, t))| [format!("{i} icon {n}"), format!("{i} text {t}")])
            .collect();
        assert_eq!(kinds(&plan), want);
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        for i in 1..r.len() {
            assert!(
                r[i].x >= r[i - 1].x + r[i - 1].width,
                "element {i} left {} before {} right {}",
                r[i].x,
                i - 1,
                r[i - 1].x + r[i - 1].width
            );
        }
    });
}

#[test]
fn c_applet_panel_2() {
    puron_testkit::limit("C-applet.panel-2", LIMIT, || {
        let plan = p1_plan(true, Orientation::Horizontal);
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        let mut n = 0;
        for i in 1..r.len() {
            if is_text(&plan.elements[i - 1]) && !is_text(&plan.elements[i]) {
                let gap = r[i].x - (r[i - 1].x + r[i - 1].width);
                assert!(near(gap, 9.0, 0.5), "text {} to icon {i}: {gap}", i - 1);
                n += 1;
            }
        }
        assert_eq!(n, 3, "three text-to-icon gaps in P1");
    });
}

#[test]
fn c_applet_panel_3() {
    puron_testkit::limit("C-applet.panel-3", LIMIT, || {
        let on = p1_plan(true, Orientation::Horizontal);
        assert_eq!(on.padding, 0);
        assert_eq!(on.icon_padding, 9.0);
        let mut sim = simulate(&on);
        let r = rects(&mut sim, &on);
        for i in (0..r.len()).step_by(2) {
            let gap = r[i + 1].x - (r[i].x + r[i].width);
            assert!(near(gap, 9.0, 0.5), "icon {i} to its text: {gap}");
        }
        let off = p1_plan(false, Orientation::Horizontal);
        assert_eq!(off.padding, 4);
        assert!(off.elements.iter().all(is_text));
        let mut sim = simulate(&off);
        let root = bounds(&mut sim, Id::new("root"));
        let r = rects(&mut sim, &off);
        let first = r[0].x - root.x;
        assert!(
            near(first, 4.0, 0.5),
            "first text from the content's left: {first}"
        );
        for i in 1..r.len() {
            let gap = r[i].x - (r[i - 1].x + r[i - 1].width);
            assert!(near(gap, 8.0, 0.5), "texts {} and {i}: {gap}", i - 1);
        }
    });
}

#[test]
fn c_applet_panel_4() {
    puron_testkit::limit("C-applet.panel-4", LIMIT, || {
        let plan = p1_plan(true, Orientation::Horizontal);
        let mut sim = simulate(&plan);
        let root = bounds(&mut sim, Id::new("root"));
        assert_eq!(root.height, BUTTON);
        let centre = root.y + root.height / 2.0;
        let r = rects(&mut sim, &plan);
        let mut n = 0;
        for (i, e) in plan.elements.iter().enumerate() {
            if is_text(e) {
                let c = r[i].y + r[i].height / 2.0;
                assert!(near(c, centre, 1.0), "text {i} centre {c}, button {centre}");
                n += 1;
            }
        }
        assert_eq!(n, 4, "four texts in P1");
    });
}

#[test]
fn c_applet_panel_5() {
    puron_testkit::limit("C-applet.panel-5", LIMIT, || {
        let h = p1_plan(true, Orientation::Horizontal);
        let v = p1_plan(true, Orientation::Vertical);
        assert_eq!(v.orientation, Orientation::Vertical);
        assert_eq!(kinds(&v), kinds(&h));
        assert_eq!(v.elements.len(), 8);
        let mut sim = simulate(&v);
        let r = rects(&mut sim, &v);
        for i in 1..r.len() {
            assert!(
                r[i].y >= r[i - 1].y + r[i - 1].height,
                "element {i} top {} above {} bottom {}",
                r[i].y,
                i - 1,
                r[i - 1].y + r[i - 1].height
            );
        }
    });
}

#[test]
fn c_applet_panel_6() {
    puron_testkit::limit("C-applet.panel-6", LIMIT, || {
        let plan = p1_plan(true, Orientation::Vertical);
        let mut sim = simulate(&plan);
        let root = bounds(&mut sim, Id::new("root"));
        assert_eq!(root.width, BUTTON);
        let centre = root.x + root.width / 2.0;
        let r = rects(&mut sim, &plan);
        assert_eq!(r.len(), 8, "eight elements in P1");
        for (i, r) in r.iter().enumerate() {
            let c = r.x + r.width / 2.0;
            assert!(
                near(c, centre, 1.0),
                "element {i} centre {c}, button {centre}"
            );
        }
    });
}

#[test]
fn c_applet_panel_7() {
    puron_testkit::limit("C-applet.panel-7", LIMIT, || {
        let plan = plan_of(
            &core_panel(&[], &pinned("P5"), true),
            Orientation::Horizontal,
        );
        assert_eq!(kinds(&plan), ["0 slot", "0 text ⚠", "1 slot", "1 text ⚠"]);
        assert_eq!(puron_applet::i18n::text("missing-value"), "⚠");
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        let size = m_icon_size();
        let (px, bg, _) = snapshot(&mut sim, "c_applet_panel_7");
        for i in [0, 2] {
            assert!(near(r[i].width, size, 0.5), "slot {i} width {}", r[i].width);
            assert!(
                near(r[i].height, size, 0.5),
                "slot {i} height {}",
                r[i].height
            );
            let p = px.inside(&r[i]);
            assert!(!p.is_empty());
            let other = p.iter().filter(|c| **c != bg).count();
            assert_eq!(other, 0, "slot {i}: pixels that are not background");
        }
    });
}

#[test]
fn c_applet_panel_8() {
    puron_testkit::limit("C-applet.panel-8", LIMIT, || {
        let plan = p1_plan(true, Orientation::Horizontal);
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        let size = m_icon_size();
        let mut n = 0;
        for (i, e) in plan.elements.iter().enumerate() {
            if let Element::Icon { .. } = e {
                assert!(near(r[i].width, size, 0.5), "icon {i} width {}", r[i].width);
                assert!(
                    near(r[i].height, size, 0.5),
                    "icon {i} height {}",
                    r[i].height
                );
                n += 1;
            }
        }
        assert_eq!(n, 4);
    });
}

#[test]
fn c_applet_panel_9() {
    puron_testkit::limit("C-applet.panel-9", LIMIT, || {
        let k: serde_json::Value = serde_json::from_str(&raw("kinds.json")).expect("kinds.json");
        let readings: Vec<RawReading> = serde_json::from_value(k["R"].clone()).expect("R");
        let pins: Vec<String> = serde_json::from_value(k["P"].clone()).expect("P");
        let plan = plan_of(&core_panel(&readings, &pins, true), Orientation::Horizontal);
        assert_eq!(names(&plan), [GPU, DRIVE, TEMP, FAN, VOLT, VOLT]);
    });
}

/// Copies the project's four icon files into a hicolor tree under `share`
/// and writes a minimal `index.theme` for it, since the system may have
/// none for hicolor.
fn icon_tree(share: &Path) {
    let status = share.join("icons/hicolor/scalable/status");
    std::fs::create_dir_all(&status).expect("icon dir");
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/icons/hicolor/scalable/status");
    let mut n = 0;
    for f in std::fs::read_dir(&src).expect("data/icons") {
        let f = f.expect("entry").path();
        if f.extension().is_some_and(|x| x == "svg") {
            std::fs::copy(&f, status.join(f.file_name().unwrap())).expect("copy icon");
            n += 1;
        }
    }
    assert_eq!(n, 4, "Puron's four icon files");
    std::fs::write(
        share.join("icons/hicolor/index.theme"),
        "[Icon Theme]\nName=hicolor\nComment=Puron test\nDirectories=scalable/status\n\n\
         [scalable/status]\nSize=16\nMinSize=8\nMaxSize=512\nType=Scalable\nContext=Status\n",
    )
    .expect("index.theme");
}

#[test]
fn c_applet_panel_11() {
    puron_testkit::limit("C-applet.panel-11", LIMIT, || {
        if std::env::var_os(CHILD).is_none() {
            let dir = scratch("c_applet_panel_11-share");
            icon_tree(&dir);
            let out = puron_testkit::rerun_self("c_applet_panel_11")
                .env("XDG_DATA_DIRS", format!("{}:/usr/share", dir.display()))
                .env(CHILD, "1")
                .output()
                .expect("run child");
            let _ = std::fs::remove_dir_all(&dir);
            assert!(
                out.status.success(),
                "child: {}\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
        let plan = p1_plan(true, Orientation::Horizontal);
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        let (px, bg, fg) = snapshot(&mut sim, "c_applet_panel_11");
        let within = |a: u8, b: f32| (f32::from(a) - b).abs() <= 2.0;
        let mut n = 0;
        for (i, e) in plan.elements.iter().enumerate() {
            let Element::Icon { name, .. } = e else {
                continue;
            };
            let mut at_fg = 0;
            for p in px.inside(&r[i]) {
                if p == bg {
                    continue;
                }
                // How far the pixel lies from the background towards the
                // foreground, as t in [0, 1]: its projection onto the line
                // between the two colours. The pixel must then be within 2
                // of that blend in each channel.
                let d: Vec<f32> = (0..3)
                    .map(|c| f32::from(fg[c]) - f32::from(bg[c]))
                    .collect();
                let q: Vec<f32> = (0..3).map(|c| f32::from(p[c]) - f32::from(bg[c])).collect();
                let dd: f32 = d.iter().map(|x| x * x).sum();
                let t = ((0..3).map(|c| d[c] * q[c]).sum::<f32>() / dd).clamp(0.0, 1.0);
                let ok = (0..3).all(|c| within(p[c], f32::from(bg[c]) + t * d[c]));
                assert!(
                    ok,
                    "icon {i} {name}: pixel {p:?} off the segment {bg:?}..{fg:?}"
                );
                if (0..3).all(|c| within(p[c], f32::from(fg[c]))) {
                    at_fg += 1;
                }
            }
            assert!(
                at_fg > 0,
                "icon {i} {name}: no pixel at the foreground {fg:?}"
            );
            n += 1;
        }
        assert_eq!(n, 4);
    });
}

#[test]
fn c_applet_panel_12() {
    puron_testkit::limit("C-applet.panel-12", LIMIT, || {
        let p2 = pinned("P2");
        assert!(p2.is_empty());
        for icons in [true, false] {
            let plan = plan_of(&core_panel(&capture(), &p2, icons), Orientation::Horizontal);
            assert_eq!(kinds(&plan), [format!("0 icon {GPU}")], "icons {icons}");
        }
    });
}

#[test]
fn c_applet_panel_13() {
    puron_testkit::limit("C-applet.panel-13", LIMIT, || {
        let plan = p1_plan(false, Orientation::Horizontal);
        assert_eq!(plan.elements.len(), 4);
        assert!(plan.elements.iter().all(is_text), "{:?}", kinds(&plan));
    });
}

#[test]
fn c_applet_panel_16() {
    puron_testkit::limit("C-applet.panel-16", LIMIT, || {
        let plan = p1_plan(true, Orientation::Vertical);
        let mut sim = simulate(&plan);
        let r = rects(&mut sim, &plan);
        let (mut own, mut next) = (0, 0);
        for i in 1..r.len() {
            let gap = r[i].y - (r[i - 1].y + r[i - 1].height);
            match (is_text(&plan.elements[i - 1]), is_text(&plan.elements[i])) {
                (false, true) => {
                    assert!(near(gap, 5.0, 0.5), "icon {} to its text: {gap}", i - 1);
                    own += 1;
                }
                (true, false) => {
                    assert!(near(gap, 5.0, 0.5), "text {} to icon {i}: {gap}", i - 1);
                    next += 1;
                }
                _ => panic!("elements {} and {i}: not icon and text", i - 1),
            }
        }
        assert_eq!(
            (own, next),
            (4, 3),
            "P1's icon-to-text and text-to-icon gaps"
        );
    });
}
