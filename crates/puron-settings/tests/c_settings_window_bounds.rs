//! Layout tests of the settings window, run without a display.
//!
//! Each test draws the window that `view::view` builds for the `good`
//! settings fixture in the iced test simulator, finds widgets by the ids
//! that `view` gives them, and checks their rectangles in logical pixels
//! at scale 1. The last test compares rendered pixels to check the
//! tooltip. Building the simulators counts toward each test's time limit.
#![allow(non_snake_case)]

use cosmic::iced::mouse::{self, ScrollDelta};
use cosmic::iced::{Event as UiEvent, Point, Rectangle, Size, Vector};
use cosmic::widget::Id;
use cosmic::Theme;
use iced_test::selector::{Candidate, Selector, Target};
use iced_test::Simulator;
use puron_config::Config;
use puron_settings::view;
use puron_settings::window::{self, Event, Plan, Window};
use puron_testkit::cfg;

/// The time limit of each test, in seconds.
const LIMIT: u64 = 5;
/// The tolerance, in logical pixels, of the checks that compare edges.
const TOL: f32 = 0.5;

type Sim<'a> = Simulator<'a, Event, Theme, cosmic::Renderer>;

/// The plan of a window opened on the `good` settings fixture.
fn plan() -> Plan {
    let fx = cfg::build("good");
    let mut w = Window::new(Config::new(&fx.root));
    w.handle(Event::Open);
    w.plan()
}

/// A simulator that draws `plan` in a window `width` wide and 720 high.
fn sim(plan: &Plan, width: f32) -> Sim<'_> {
    Simulator::with_size(
        Default::default(),
        Size::new(width, 720.0),
        view::view(plan),
    )
}

/// Selects the widget with a given id. A scroll area is visited twice:
/// first as a `PreOperation` candidate before its children, then as its
/// real target after them (at the iced revision this workspace pins). The
/// selector ignores every `PreOperation`, so that `scroll` gets the real
/// target with the scroll translation.
struct ById(Id);

impl Selector for ById {
    type Output = Target;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<Target> {
        match candidate {
            Candidate::PreOperation { .. } => None,
            c if c.id() == Some(&self.0) => Some(Target::from(c)),
            _ => None,
        }
    }

    fn description(&self) -> String {
        format!("id == {:?}", self.0)
    }
}

/// The widget with id `name`; the test fails if there is none.
fn target(sim: &mut Sim, name: &str) -> Target {
    sim.find(ById(Id::new(name.to_string())))
        .unwrap_or_else(|e| panic!("find {name}: {e:?}"))
}

/// The rectangle of the widget with id `name`.
fn rect(sim: &mut Sim, name: &str) -> Rectangle {
    target(sim, name).bounds()
}

/// The scroll area's rectangle and its current scroll translation.
fn scroll(sim: &mut Sim) -> (Rectangle, Vector) {
    match target(sim, "scroll") {
        Target::Scrollable {
            bounds,
            translation,
            ..
        } => (bounds, translation),
        t => panic!("scroll is not a scroll area: {t:?}"),
    }
}

/// Scrolls to the end with one wheel event far larger than the content;
/// the scroll area stops at its last position. Returns the translation
/// there.
fn scroll_to_end(sim: &mut Sim) -> Vector {
    let (area, _) = scroll(sim);
    sim.point_at(area.center());
    sim.simulate([UiEvent::Mouse(mouse::Event::WheelScrolled {
        delta: ScrollDelta::Pixels { x: 0.0, y: -1.0e6 },
    })]);
    scroll(sim).1
}

/// Selects the text widget whose content is `text`, after skipping the
/// first `skip` such widgets in depth-first order; the same label can
/// occur twice. It is used instead of iced's `&str` selector because that
/// one also asks text inputs for their text, and libcosmic's
/// `TextInput::text` panics (`todo!()`) at the revision this workspace
/// pins.
struct Nth {
    text: String,
    skip: usize,
}

impl Selector for Nth {
    type Output = Rectangle;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<Rectangle> {
        match candidate {
            Candidate::Text {
                bounds, content, ..
            } if content == self.text => {
                if self.skip == 0 {
                    Some(bounds)
                } else {
                    self.skip -= 1;
                    None
                }
            }
            _ => None,
        }
    }

    fn description(&self) -> String {
        format!("text #{} == {:?}", self.skip + 1, self.text)
    }
}

/// The label of each item of `plan`; item `n` is at index `n - 1`.
fn labels(plan: &Plan) -> Vec<String> {
    plan.sections
        .iter()
        .flat_map(|s| s.items.iter().map(|i| i.label.clone()))
        .collect()
}

/// The rectangle of item `n`'s label text. When an earlier item has the
/// same label, the matching text is counted past it.
fn label(sim: &mut Sim, plan: &Plan, n: usize) -> Rectangle {
    let all = labels(plan);
    let text = all[n - 1].clone();
    let skip = all[..n - 1].iter().filter(|l| **l == text).count();
    sim.find(Nth { text, skip })
        .unwrap_or_else(|e| panic!("label of item {n}: {e:?}"))
}

/// The rectangle of the heading of section `s` (counted from 0), found as
/// the first text with that content.
fn heading(sim: &mut Sim, plan: &Plan, s: usize) -> Rectangle {
    let text = plan.sections[s].heading.clone();
    sim.find(Nth {
        text: text.clone(),
        skip: 0,
    })
    .unwrap_or_else(|e| panic!("heading {text}: {e:?}"))
}

/// The rectangle of item `n`'s row.
fn item(sim: &mut Sim, n: usize) -> Rectangle {
    rect(sim, &format!("item-{n}"))
}

/// The rectangle of item `n`'s control.
fn control(sim: &mut Sim, n: usize) -> Rectangle {
    rect(sim, &format!("control-{n}"))
}

/// The rectangle of section `s` (counted from 1).
fn section(sim: &mut Sim, s: usize) -> Rectangle {
    rect(sim, &format!("section-{s}"))
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn mid_y(r: Rectangle) -> f32 {
    r.y + r.height / 2.0
}

/// Whether `r` lies within `area`, give or take `TOL`.
fn inside(r: Rectangle, area: Rectangle) -> bool {
    r.x >= area.x - TOL
        && r.y >= area.y - TOL
        && r.x + r.width <= area.x + area.width + TOL
        && r.y + r.height <= area.y + area.height + TOL
}

/// Where `r`, a layout rectangle inside the scroll area, is drawn once
/// the content is scrolled by `t`.
fn shifted(r: Rectangle, t: Vector) -> Rectangle {
    Rectangle {
        x: r.x - t.x,
        y: r.y - t.y,
        ..r
    }
}

/// The sections do not overlap: each one's top edge is at or below the
/// bottom edge of the one before, both before scrolling and scrolled to
/// the end.
#[test]
fn c_settings_window_18() {
    puron_testkit::limit("C-settings.window-18", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        for at_end in [false, true] {
            let t = if at_end {
                scroll_to_end(&mut sim)
            } else {
                scroll(&mut sim).1
            };
            let r: Vec<Rectangle> = (1..=5).map(|s| shifted(section(&mut sim, s), t)).collect();
            for s in 1..5 {
                assert!(
                    r[s].y >= r[s - 1].y + r[s - 1].height,
                    "end {at_end}: section {} top {} above section {} bottom {}",
                    s + 1,
                    r[s].y,
                    s,
                    r[s - 1].y + r[s - 1].height
                );
            }
        }
    });
}

/// In each section the heading is above the first item, each item is
/// below the one before, and there are 17 items in all.
#[test]
fn c_settings_window_19() {
    puron_testkit::limit("C-settings.window-19", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        let mut n = 0;
        for (s, sec) in plan.sections.iter().enumerate() {
            let h = heading(&mut sim, &plan, s);
            let mut above = h.y + h.height;
            for _ in &sec.items {
                n += 1;
                let r = item(&mut sim, n);
                assert!(
                    r.y >= above,
                    "section {}: item {n} top {} above {above}",
                    s + 1,
                    r.y
                );
                above = r.y + r.height;
            }
        }
        assert_eq!(n, 17, "items");
    });
}

/// The sections form one column: at 480 and at 1440 pixels wide, all five
/// share the same left and right edges.
#[test]
fn c_settings_window_20() {
    puron_testkit::limit("C-settings.window-20", LIMIT, || {
        let plan = plan();
        for width in [480.0, 1440.0] {
            let mut sim = sim(&plan, width);
            let first = section(&mut sim, 1);
            for s in 2..=5 {
                let r = section(&mut sim, s);
                assert!(
                    near(r.x, first.x, TOL) && near(r.x + r.width, first.x + first.width, TOL),
                    "width {width}: section {s} {r:?} against section 1 {first:?}"
                );
            }
        }
    });
}

/// Each label starts at the left edge of its item row.
#[test]
fn c_settings_window_21() {
    puron_testkit::limit("C-settings.window-21", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        for n in 1..=17 {
            let (l, i) = (label(&mut sim, &plan, n), item(&mut sim, n));
            assert!(near(l.x, i.x, TOL), "item {n}: label {l:?} item {i:?}");
        }
    });
}

/// Each control ends at the right edge of its item row.
#[test]
fn c_settings_window_22() {
    puron_testkit::limit("C-settings.window-22", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        for n in 1..=17 {
            let (c, i) = (control(&mut sim, n), item(&mut sim, n));
            assert!(
                near(c.x + c.width, i.x + i.width, TOL),
                "item {n}: control {c:?} item {i:?}"
            );
        }
    });
}

/// Each item's label and control are centered vertically on the item
/// row, within one pixel.
#[test]
fn c_settings_window_23() {
    puron_testkit::limit("C-settings.window-23", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        for n in 1..=17 {
            let i = item(&mut sim, n);
            let l = label(&mut sim, &plan, n);
            let c = control(&mut sim, n);
            assert!(
                near(mid_y(l), mid_y(i), 1.0),
                "item {n}: label {l:?} item {i:?}"
            );
            assert!(
                near(mid_y(c), mid_y(i), 1.0),
                "item {n}: control {c:?} item {i:?}"
            );
        }
    });
}

/// At the minimum window width (360), every label still ends before its
/// control, every control ends inside the scroll area, and every control
/// keeps the width it has at 480: the label gives way, not the control.
#[test]
fn c_settings_window_24() {
    puron_testkit::limit("C-settings.window-24", LIMIT, || {
        let plan = plan();
        // Each control's width in a window 480 wide.
        let mut wide = sim(&plan, 480.0);
        let w480: Vec<f32> = (1..=17).map(|n| control(&mut wide, n).width).collect();
        let mut sim = sim(&plan, 360.0);
        let (area, _) = scroll(&mut sim);
        for n in 1..=17 {
            let l = label(&mut sim, &plan, n);
            let c = control(&mut sim, n);
            assert!(
                l.x + l.width <= c.x + TOL,
                "item {n}: label {l:?} over control {c:?}"
            );
            assert!(
                c.x + c.width <= area.x + area.width + TOL,
                "item {n}: control {c:?} past the scroll area {area:?}"
            );
            assert!(
                near(c.width, w480[n - 1], TOL),
                "item {n}: control width {} at 360, {} at 480",
                c.width,
                w480[n - 1]
            );
        }
    });
}

/// The number field shows its − button, text input and + button from left
/// to right, centered on one line, with a text input 48 pixels wide; the
/// buttons use the expected icon names.
#[test]
fn c_settings_window_25() {
    puron_testkit::limit("C-settings.window-25", LIMIT, || {
        assert_eq!(window::MINUS_ICON, "list-remove-symbolic", "− icon");
        assert_eq!(window::PLUS_ICON, "list-add-symbolic", "+ icon");
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        let r = [
            rect(&mut sim, "minus"),
            rect(&mut sim, "input"),
            rect(&mut sim, "plus"),
        ];
        for k in 1..3 {
            assert!(
                r[k].x >= r[k - 1].x + r[k - 1].width,
                "order: {:?} before {:?}",
                r[k - 1],
                r[k]
            );
            assert!(near(mid_y(r[k]), mid_y(r[0]), 1.0), "centers: {r:?}");
        }
        assert!(near(r[1].width, 48.0, TOL), "input width {}", r[1].width);
    });
}

/// Every part of the window can be reached: scrolled to the end, the last
/// item lies inside the scroll area, and before scrolling, the first
/// heading does.
#[test]
fn c_settings_window_26() {
    puron_testkit::limit("C-settings.window-26", LIMIT, || {
        let plan = plan();
        let mut sim = sim(&plan, 480.0);
        let t = scroll_to_end(&mut sim);
        let (area, _) = scroll(&mut sim);
        let last = shifted(item(&mut sim, 17), t);
        assert!(
            inside(last, area),
            "end: item 17 {last:?} t {t:?} area {area:?}"
        );
        let mut top = self::sim(&plan, 480.0);
        let (area, t) = scroll(&mut top);
        let h = shifted(heading(&mut top, &plan, 0), t);
        assert!(inside(h, area), "top: heading {h:?} area {area:?}");
    });
}

/// A snapshot of `sim` in the dark theme, as (width, height, RGBA bytes)
/// at scale 1. The snapshot goes through a PNG file in a temporary
/// directory, named after `tag`, which is removed afterwards.
fn pixels(sim: &mut Sim, tag: &str) -> (u32, u32, Vec<u8>) {
    let snap = sim.snapshot(&Theme::dark()).expect("snapshot");
    let dir = std::env::temp_dir().join(format!("puron-sw27-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    assert!(snap
        .matches_image(dir.join("snap.png"))
        .expect("write snapshot"));
    let file = std::fs::File::open(dir.join("snap-tiny-skia.png")).expect("snapshot png");
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .expect("png header");
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let _ = std::fs::remove_dir_all(&dir);
    (info.width, info.height, rgba)
}

/// The number of pixels that differ between `a` and `b`, leaving out
/// every pixel that touches `skip`.
fn changed(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>), skip: Option<Rectangle>) -> usize {
    assert_eq!((a.0, a.1), (b.0, b.1), "snapshot sizes");
    let mut n = 0;
    for y in 0..a.1 {
        for x in 0..a.0 {
            let (fx, fy) = (x as f32, y as f32);
            if let Some(r) = skip {
                if fx + 1.0 > r.x && fx < r.x + r.width && fy + 1.0 > r.y && fy < r.y + r.height {
                    continue;
                }
            }
            let i = ((y * a.0 + x) * 4) as usize;
            if a.2[i..i + 4] != b.2[i..i + 4] {
                n += 1;
            }
        }
    }
    n
}

/// Hovers the point `at` in a fresh simulator, by placing the cursor
/// there and sending one cursor move, and returns the snapshot.
fn hovered(plan: &Plan, at: Point, tag: &str) -> (u32, u32, Vec<u8>) {
    let mut sim = sim(plan, 480.0);
    sim.point_at(at);
    sim.simulate([UiEvent::Mouse(mouse::Event::CursorMoved { position: at })]);
    pixels(&mut sim, tag)
}

/// The tooltip belongs to the whole item row. Hovering item 8's label, or
/// its switch, changes pixels outside the switch, where the tooltip is
/// drawn. Item 11 has no tooltip, so hovering its label changes no pixel.
#[test]
fn c_settings_window_27() {
    puron_testkit::limit("C-settings.window-27", LIMIT, || {
        let plan = plan();
        let mut first = sim(&plan, 480.0);
        let base = pixels(&mut first, "base");
        let label8 = label(&mut first, &plan, 8).center();
        let control8 = control(&mut first, 8);
        let label11 = label(&mut first, &plan, 11).center();
        for (what, at) in [("label", label8), ("switch", control8.center())] {
            let after = hovered(&plan, at, what);
            let n = changed(&base, &after, Some(control8));
            assert!(
                n > 0,
                "item 8 {what} hover at {at:?}: no pixel changed outside {control8:?}"
            );
        }
        let after = hovered(&plan, label11, "item11");
        let n = changed(&base, &after, None);
        assert_eq!(n, 0, "item 11 hover at {label11:?}: {n} pixels changed");
    });
}
