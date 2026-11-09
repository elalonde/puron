//! The popup that opens from the panel button: a `Plan` built from the
//! `Menu` that `puron_core::menu` laid out, the widgets drawn from it, and
//! what a click on them does. A click on a line becomes an `Event` for the
//! event loop in `app`; this module chooses no entry itself and never
//! changes the pinned list (`puron_core::pins` does that).

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

/// The icon name of the check mark drawn in front of a pinned line.
pub const CHECK_MARK: &str = "object-select-symbolic";
/// The check mark's side, in px. It does not follow the panel's icon size.
pub const CHECK_MARK_SIZE: u16 = 14;
/// The width in px of the slot in front of every line's text, whether or
/// not it holds a mark: the 14 px mark, then 6 px of gap to the text.
pub const CHECK_SLOT: f32 = 20.0;
/// A device heading's indent from the section heading's left edge, in px.
pub const DEVICE_INDENT: f32 = 16.0;
/// A line's indent from the section heading's left edge, in px.
pub const LINE_INDENT: f32 = 24.0;
/// The popup's narrowest width in px. libcosmic's
/// `applet::popup_container` fixes the width at this value; `limits`
/// replaces that with the width that `width` computes.
pub const WIDTH: f32 = 360.0;
/// The popup's widest width in px: room for a line whose name has 44
/// characters, taken as 7 px each on average in the default font. That
/// is 48 of padding, 24 of indent, 20 of check slot, 308 of name, 8 of
/// gap and 64 of value.
pub const MAX_WIDTH: f32 = 472.0;
/// The height limit in px that libcosmic's `applet::popup_container`
/// sets, kept by `limits`. Content taller than this scrolls.
const MAX_HEIGHT: f32 = 1000.0;
/// The least space in px that `fit_width` counts between a line's name
/// and its value. `view` draws no gap of its own.
const NAME_GAP: f32 = 8.0;
/// The space in px between a section heading's icon and its text.
const SECTION_GAP: f32 = 8.0;
/// The least width in px counted for a value, so that a value that grows
/// by a digit does not widen the popup. "100.0°F" fits in it.
const VALUE_ROOM: f32 = 64.0;
/// The text size in px that `measure` assumes: libcosmic's default size
/// for applet text.
const TEXT_SIZE: f32 = 14.0;

/// A reading's line in the popup, drawn left to right as the check slot,
/// the text and the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The reading's identity: the text that the pinned list stores and
    /// that a click on the line sends.
    pub identity: String,
    /// Whether the check mark is drawn, that is, whether `identity` is in
    /// the pinned list.
    pub pinned: bool,
    /// The name, shortened to fit `NAME_LIMIT` characters together with
    /// the default line's suffix.
    pub text: String,
    /// The value text. The line of a pin that waits for lm-sensors shows
    /// the missing-value text instead.
    pub value: String,
}

/// One entry of the popup, made from one entry of the `Menu`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// A section heading: the class's icon and name. A click does nothing.
    Section {
        icon: &'static str,
        text: String,
    },
    /// A device heading, such as a drive's name or "CPU". A click does
    /// nothing.
    Device {
        text: String,
    },
    Line(Line),
    Separator,
    /// The message shown when there are no readings, which tells the user
    /// what to do. A click on it opens the homepage, like the link.
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

/// Everything the popup draws, as plain data. `view` builds the widgets
/// from it alone, so tests can check a plan without a display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub entries: Vec<Drawn>,
    /// The side in px of a section heading's icon.
    pub icon_size: u16,
}

/// What a click asks the event loop in `app` to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A click on the line with this identity. `puron_core::pins::click`
    /// decides the new pinned list.
    PinClick(String),
    /// Open the project homepage in the browser.
    HomepageLink,
    /// Start the settings program.
    SettingsLink,
}

/// The answer to a click: the event to send, if any, and whether the
/// popup closes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub event: Option<Event>,
    pub close: bool,
}

/// The widget message of a click. It names what was clicked, not its
/// position: a line is named by its identity, so the click still means
/// the line the user saw even if a newer view has arrived between the
/// draw and the click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clicked {
    Line(String),
    Message,
    HomepageLink,
    SettingsLink,
}

/// The answer to a widget's click, given `plan`, the popup's plan at the
/// time of the click. A line that `plan` still shows is sent as
/// `Event::PinClick` and the popup stays open; a line that `plan` no
/// longer shows sends nothing. The message and the links answer as in
/// `click`.
pub fn clicked(plan: &Plan, clicked: Clicked) -> Reply {
    let (event, close) = match clicked {
        Clicked::Line(id) => {
            let shown = plan
                .entries
                .iter()
                .any(|e| matches!(e, Drawn::Line(l) if l.identity == id));
            (shown.then_some(Event::PinClick(id)), false)
        }
        Clicked::Message | Clicked::HomepageLink => (Some(Event::HomepageLink), true),
        Clicked::SettingsLink => (Some(Event::SettingsLink), true),
    };
    Reply { event, close }
}

/// The answer to a click on a drawn entry. A line sends `Event::PinClick`
/// and leaves the popup open. The message and the homepage link send
/// `Event::HomepageLink`, the settings link sends `Event::SettingsLink`,
/// and these three close the popup. Headings and separators do nothing.
pub fn click(entry: &Drawn) -> Reply {
    let (event, close) = match entry {
        Drawn::Line(l) => (Some(Event::PinClick(l.identity.clone())), false),
        Drawn::Section { .. } | Drawn::Device { .. } | Drawn::Separator => (None, false),
        Drawn::Message { .. } | Drawn::HomepageLink { .. } => (Some(Event::HomepageLink), true),
        Drawn::SettingsLink { .. } => (Some(Event::SettingsLink), true),
    };
    Reply { event, close }
}

/// The popup plan for `menu`, with a check mark on each line that is in
/// `pinned`: one drawn entry per menu entry, in the menu's order. The
/// texts come from the catalog of the chosen language.
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

/// A line's text before it is shortened: the reading's name, then the word
/// that the menu attached to the line (such as "composite"), with one
/// space between them. A line with no name shows the word alone. For the
/// line of a pin that waits for lm-sensors, the word is "CPU: waiting for
/// lm-sensors" in English.
fn line_name(item: &Item) -> String {
    let word = match item.word {
        None => return item.name.clone(),
        Some(LineWord::Composite) => fl!("line-composite"),
        Some(LineWord::Package) => fl!("line-package"),
        Some(LineWord::Temperature) => fl!("line-temperature"),
        Some(LineWord::Maximum) => fl!("row-maximum"),
        Some(LineWord::Waiting) => format!("{}: {}", fl!("row-cpu"), fl!("line-waiting")),
    };
    if item.name.is_empty() {
        word
    } else {
        format!("{} {word}", item.name)
    }
}

/// The drawn line for `item`. The default line gets the `cpu-default`
/// suffix; the name is shortened first so that name and suffix together
/// fit `NAME_LIMIT` characters.
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
        value: if item.word == Some(LineWord::Waiting) {
            fl!("missing-value")
        } else {
            i18n::value_text(item.value.as_ref())
        },
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

/// The popup's width in px: the width of its widest heading or line, plus
/// libcosmic's horizontal padding of a menu entry, but never less than
/// [`WIDTH`] or more than [`MAX_WIDTH`].
pub fn width(menu: &Menu, icon_size: u16) -> f32 {
    let padding = applet::menu_control_padding();
    fit_width(menu, icon_size, padding.left + padding.right, &measure)
}

/// The computation of [`width`], with the entry's horizontal padding and
/// the text measure as parameters. A line counts its indent, check slot,
/// name, gap and value (at least [`VALUE_ROOM`]); a section heading its
/// icon, gap and text; a device heading its indent and text. Separators,
/// the message and the links do not count.
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

/// The width in px of `content` on one line, in the default font at
/// [`TEXT_SIZE`].
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

/// The size limits of a popup `width` px wide: exactly that width, and
/// the same height limits as libcosmic's `applet::popup_container`.
pub fn limits(width: f32) -> Limits {
    Limits::NONE
        .min_height(1.0)
        .min_width(width)
        .max_width(width)
        .max_height(MAX_HEIGHT)
}

/// The widget ID of the container of the drawn entry at `index` in
/// `Plan::entries`, so that tests can find its rectangle.
pub fn entry_id(index: usize) -> Id {
    Id::new(format!("e{index}"))
}

/// The widget ID of one part of the drawn entry at `index`. A line's parts
/// are `check`, `mark`, `name` and `value`; a section heading's are `icon`
/// and `heading`.
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

/// The popup's content, built from the plan alone with libcosmic's applet
/// menu widgets, so that it looks like COSMIC's own popups.
///
/// Each drawn entry sits in a container with the ID `entry_id(index)`.
/// Inside it, a line's parts have the IDs of `element_id` with `check`,
/// `mark` (only when the line is pinned), `name` and `value`, and a
/// section heading's parts `icon` and `heading`; an ID on a text is on the
/// text's own rectangle. A name stays on one line: when it is wider than
/// its space it is clipped there and never covers the value. Content
/// taller than the popup scrolls.
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
                    let entry =
                        applet::menu_button(content).on_press(Clicked::Line(l.identity.clone()));
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
                    let message = match e {
                        Drawn::Message { .. } => Clicked::Message,
                        Drawn::HomepageLink { .. } => Clicked::HomepageLink,
                        _ => Clicked::SettingsLink,
                    };
                    container(applet::menu_button(text(t.clone())).on_press(message))
                        .id(entry_id(i))
                        .into()
                }
            }
        })
        .collect();
    scrollable(column::with_children(children).width(Length::Fill)).into()
}
