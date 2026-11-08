//! The popup: rendering and clicks. It draws `core.menu`'s layout and sends
//! pin and link clicks to `app`. It decides no entry (that is `core.menu`'s
//! job) and changes no pinned list (that is `core.pins`'s job).

use cosmic::applet;
use cosmic::iced::advanced::layout::Limits;
use cosmic::iced::advanced::text::{self as advanced_text, Paragraph as _};
use cosmic::iced::alignment::Vertical;
use cosmic::iced::widget::text::Wrapping;
use cosmic::iced::{Alignment, Length, Pixels, Size};
use cosmic::widget::{column, container, divider, icon, row, scrollable, text, Id, Space};
use puron_core::menu::{Entry, Heading, Item, LineWord, Menu, MessageKind, Section};
use puron_core::pins;
use puron_core::rows::{self as core_rows, NAME_LIMIT};

use crate::i18n;
use crate::panel::section_icon_name;

/// The check mark of a checked line.
pub const CHECK_MARK: &str = "object-select-symbolic";
/// The check mark's side, in px. It does not follow the panel's icon size.
pub const CHECK_MARK_SIZE: u16 = 14;
/// The check slot's width, in px, with a mark or without one: the mark, then
/// 6 px of gap to the text.
pub const CHECK_SLOT: f32 = 20.0;
/// A device heading's indent from the section heading's left edge, in px.
pub const DEVICE_INDENT: f32 = 16.0;
/// A line's indent from the section heading's left edge, in px.
pub const LINE_INDENT: f32 = 24.0;
/// The popup's least width, the one libcosmic's `applet::popup_container`
/// sets.
pub const WIDTH: f32 = 360.0;
/// The popup's greatest width: 44 characters on a line, at the default font's
/// 7 px on average: 48 of padding, 24 of indent, 20 of check slot, 308, 8 of
/// gap, 64 of value.
pub const MAX_WIDTH: f32 = 472.0;
/// libcosmic's height limit of `applet::popup_container`. A taller popup
/// scrolls.
const MAX_HEIGHT: f32 = 1000.0;
/// The space kept between a name and its value.
const NAME_GAP: f32 = 8.0;
/// The space between a section's icon and its text.
const SECTION_GAP: f32 = 8.0;
/// The width kept for a value, so that a value that grows by a digit does
/// not resize the popup: "100.0°F" fits.
const VALUE_ROOM: f32 = 64.0;
/// libcosmic's default text size in an applet.
const TEXT_SIZE: f32 = 14.0;

/// A line as drawn: check slot, text, value, left to right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub identity: String,
    /// The check mark is drawn (the line is pinned, per `core.pins`).
    pub pinned: bool,
    pub text: String,
    pub value: String,
}

/// One drawn entry of the popup plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// A section heading: a plain label.
    Section {
        icon: &'static str,
        text: String,
    },
    /// A device heading: a plain label.
    Device {
        text: String,
    },
    Line(Line),
    Separator,
    /// The message shown when there are no readings: a click sends
    /// `Event::HomepageLink`.
    Message {
        text: String,
    },
    HomepageLink {
        text: String,
    },
    SettingsLink {
        text: String,
    },
}

/// The popup plan: a value that tests read without a display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub entries: Vec<Drawn>,
    pub icon_size: u16,
}

/// An event to `app`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    PinClick(String),
    HomepageLink,
    SettingsLink,
}

/// What a click gives: the event sent, if any, and whether the popup
/// closes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub event: Option<Event>,
    pub close: bool,
}

/// The widget message of a click: drawn entry `.0` of the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clicked(pub usize);

/// A widget's click on `plan`.
pub fn clicked(plan: &Plan, clicked: Clicked) -> Reply {
    match plan.entries.get(clicked.0) {
        Some(entry) => click(entry),
        None => Reply {
            event: None,
            close: false,
        },
    }
}

/// A click on a drawn entry. A line sends `Event::PinClick` and leaves the
/// popup open. The message and the homepage link send `Event::HomepageLink`,
/// the settings link sends `Event::SettingsLink`, and these close the popup.
/// Nothing else answers.
pub fn click(entry: &Drawn) -> Reply {
    let (event, close) = match entry {
        Drawn::Line(l) => (Some(Event::PinClick(l.identity.clone())), false),
        Drawn::Section { .. } | Drawn::Device { .. } | Drawn::Separator => (None, false),
        Drawn::Message { .. } | Drawn::HomepageLink { .. } => (Some(Event::HomepageLink), true),
        Drawn::SettingsLink { .. } => (Some(Event::SettingsLink), true),
    };
    Reply { event, close }
}

/// The popup plan of a layout with the in-memory pinned list: one drawn entry
/// per layout entry, in the layout's order.
pub fn plan(menu: &Menu, pinned: &[String], icon_size: u16) -> Plan {
    let entries = menu
        .entries
        .iter()
        .map(|entry| match entry {
            Entry::Section(section) => Drawn::Section {
                icon: section_icon_name(*section),
                text: section_text(*section),
            },
            Entry::Device(heading) => Drawn::Device {
                text: heading_text(heading),
            },
            Entry::Line(item) => Drawn::Line(line_of(item, pinned)),
            Entry::Separator => Drawn::Separator,
            Entry::Message { kind, .. } => Drawn::Message {
                text: match kind {
                    MessageKind::SensorsDetect => fl!("message-detect"),
                    MessageKind::Install => fl!("message-install"),
                },
            },
            Entry::HomepageLink { .. } => Drawn::HomepageLink {
                text: fl!("link-homepage"),
            },
            Entry::SettingsLink => Drawn::SettingsLink {
                text: fl!("link-settings"),
            },
        })
        .collect();
    Plan { entries, icon_size }
}

/// A line's name and its word, with one space when it has both.
fn line_name(item: &Item) -> String {
    let word = match item.word {
        None => return item.name.clone(),
        Some(LineWord::Composite) => fl!("line-composite"),
        Some(LineWord::Package) => fl!("line-package"),
        Some(LineWord::Temperature) => fl!("line-temperature"),
        Some(LineWord::Maximum) => fl!("row-maximum"),
    };
    if item.name.is_empty() {
        word
    } else {
        format!("{} {word}", item.name)
    }
}

fn line_of(item: &Item, pinned: &[String]) -> Line {
    let suffix = if item.default {
        format!(" {}", fl!("cpu-default"))
    } else {
        String::new()
    };
    let room = NAME_LIMIT.saturating_sub(suffix.chars().count());
    Line {
        identity: item.identity.clone(),
        pinned: pins::is_pinned(pinned, &item.identity),
        text: format!("{}{suffix}", core_rows::cut_at_end(&line_name(item), room)),
        value: i18n::value_text(item.value.as_ref()),
    }
}

fn section_text(section: Section) -> String {
    match section {
        Section::Temperature => fl!("class-temperature"),
        Section::Fan => fl!("section-fan"),
        Section::Voltage => fl!("class-voltage"),
        Section::Power => fl!("class-power"),
    }
}

fn heading_text(heading: &Heading) -> String {
    let text = match heading {
        Heading::Cpu => fl!("row-cpu"),
        Heading::Name(name) => name.clone(),
    };
    core_rows::cut_at_end(&text, NAME_LIMIT)
}

/// The popup's width: its widest entry. Between [`WIDTH`] and [`MAX_WIDTH`].
pub fn width(menu: &Menu, icon_size: u16) -> f32 {
    let padding = applet::menu_control_padding();
    fit_width(menu, icon_size, padding.left + padding.right, &measure)
}

/// [`width`] with the entry's padding and the measure of a text as parameters.
pub fn fit_width(menu: &Menu, icon_size: u16, padding: f32, measure: &dyn Fn(&str) -> f32) -> f32 {
    let side = f32::from(icon_size);
    let widest = plan(menu, &[], icon_size)
        .entries
        .iter()
        .map(|e| match e {
            Drawn::Line(l) => {
                LINE_INDENT
                    + CHECK_SLOT
                    + measure(&l.text)
                    + NAME_GAP
                    + measure(&l.value).max(VALUE_ROOM)
            }
            Drawn::Section { text, .. } => side + SECTION_GAP + measure(text),
            Drawn::Device { text } => DEVICE_INDENT + measure(text),
            _ => 0.0,
        })
        .fold(0.0, f32::max);
    (padding + widest).clamp(WIDTH, MAX_WIDTH)
}

/// A text's one-line width at the applet's default font and size.
fn measure(content: &str) -> f32 {
    type Paragraph = <cosmic::Renderer as advanced_text::Renderer>::Paragraph;
    Paragraph::with_text(advanced_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(TEXT_SIZE),
        line_height: advanced_text::LineHeight::default(),
        font: cosmic::font::default(),
        align_x: advanced_text::Alignment::Default,
        align_y: Vertical::Top,
        shaping: advanced_text::Shaping::Advanced,
        wrapping: Wrapping::None,
        ellipsize: advanced_text::Ellipsize::None,
    })
    .min_width()
}

/// The surface limits of a popup of `width`, otherwise libcosmic's.
pub fn limits(width: f32) -> Limits {
    Limits::NONE
        .min_height(1.0)
        .min_width(width)
        .max_width(width)
        .max_height(MAX_HEIGHT)
}

/// The widget id of drawn entry `index`: the container of its entry.
pub fn entry_id(index: usize) -> Id {
    Id::new(format!("e{index}"))
}

/// The widget id of an element of drawn entry `index`: a line's `check`, `mark`,
/// `name` and `value`, a section's `icon` and `heading`.
pub fn element_id(index: usize, element: &str) -> Id {
    Id::new(format!("e{index}.{element}"))
}

fn symbolic<'a, M: 'a>(name: &'static str, size: u16) -> cosmic::Element<'a, M> {
    icon::from_name(name)
        .size(size)
        .symbolic(true)
        .fallback(None)
        .icon()
        .into()
}

/// The popup content, built from the plan alone, in libcosmic's applet menu
/// widgets, so it has COSMIC's own popup style. Each drawn entry is a container
/// with the id `e<index>`; a line's elements have the ids `e<index>.check`,
/// `.mark` (only when checked), `.name` and `.value`, and a section's the ids
/// `e<index>.icon` and `.heading`, a text's id on the text's own rectangle. A
/// name stays on one line; a name wider than its space is clipped there, and
/// never covers the value. Content taller than the popup scrolls.
pub fn view<'a>(plan: &Plan) -> cosmic::Element<'a, Clicked> {
    let size = plan.icon_size;
    let mark_side = f32::from(CHECK_MARK_SIZE);
    let children: Vec<cosmic::Element<'a, Clicked>> = plan
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| -> cosmic::Element<'a, Clicked> {
            match e {
                Drawn::Line(l) => {
                    let check: cosmic::Element<'a, Clicked> = if l.pinned {
                        container(symbolic(CHECK_MARK, CHECK_MARK_SIZE))
                            .id(element_id(i, "mark"))
                            .into()
                    } else {
                        Space::new()
                            .width(Length::Fixed(mark_side))
                            .height(Length::Fixed(mark_side))
                            .into()
                    };
                    let content = row::with_children(vec![
                        container(check)
                            .id(element_id(i, "check"))
                            .width(Length::Fixed(CHECK_SLOT))
                            .height(Length::Fixed(mark_side))
                            .into(),
                        container(
                            container(text(l.text.clone()).wrapping(Wrapping::None))
                                .id(element_id(i, "name")),
                        )
                        .width(Length::Fill)
                        .clip(true)
                        .into(),
                        container(text(l.value.clone()))
                            .id(element_id(i, "value"))
                            .into(),
                    ])
                    .align_y(Alignment::Center);
                    let entry = applet::menu_button(content).on_press(Clicked(i));
                    container(container(entry).id(entry_id(i)))
                        .padding([0.0, 0.0, 0.0, LINE_INDENT])
                        .into()
                }
                Drawn::Section { icon, text: t } => {
                    let parts = row::with_children(vec![
                        container(symbolic(icon, size))
                            .id(element_id(i, "icon"))
                            .into(),
                        container(text::heading(t.clone()).wrapping(Wrapping::None))
                            .id(element_id(i, "heading"))
                            .into(),
                    ])
                    .spacing(SECTION_GAP)
                    .align_y(Alignment::Center);
                    container(applet::padded_control(parts))
                        .id(entry_id(i))
                        .into()
                }
                Drawn::Device { text: t } => {
                    let label = text(t.clone()).wrapping(Wrapping::None);
                    container(container(applet::padded_control(label)).id(entry_id(i)))
                        .padding([0.0, 0.0, 0.0, DEVICE_INDENT])
                        .into()
                }
                Drawn::Separator => {
                    container(applet::padded_control(divider::horizontal::default()))
                        .id(entry_id(i))
                        .into()
                }
                Drawn::Message { text: t }
                | Drawn::HomepageLink { text: t }
                | Drawn::SettingsLink { text: t } => {
                    container(applet::menu_button(text(t.clone())).on_press(Clicked(i)))
                        .id(entry_id(i))
                        .into()
                }
            }
        })
        .collect();
    scrollable(column::with_children(children).width(Length::Fill)).into()
}
