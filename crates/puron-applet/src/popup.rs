//! The popup: rendering, groups and clicks. It draws `core.menu`'s
//! layout, keeps which groups are open, and sends pin and link clicks to
//! `app`. It decides no entry (that is `core.menu`'s job) and changes no
//! pinned list (that is `core.pins`'s job).

use cosmic::applet;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{column, container, divider, icon, row, text, Id, Space};
use puron_core::menu::{Entry, GroupKind, Item, Menu, MessageKind, Update};
use puron_core::pins;

use crate::i18n;
use crate::panel::{group_icon_name, icon_name};

/// The check mark of a pinned row.
pub const CHECK_MARK: &str = "object-select-symbolic";
/// The group indicator, closed and open.
pub const CLOSED: &str = "pan-end-symbolic";
pub const OPEN: &str = "pan-down-symbolic";
/// A group member's indent from the header's left edge, in px.
pub const INDENT: f32 = 16.0;
/// The popup's width: libcosmic's `applet::popup_container` sets its
/// minimum and maximum width to this value; `app` wraps the view
/// in it.
pub const WIDTH: f32 = 360.0;

/// A sensor row or the Maximum row as drawn: check slot, icon,
/// name, value, left to right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub identity: String,
    /// The check mark is drawn (the row is pinned, per `core.pins`).
    pub pinned: bool,
    pub icon: &'static str,
    pub name: String,
    pub value: String,
    /// A member of an open group, indented by `INDENT`.
    pub member: bool,
}

/// One drawn entry of the popup plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    Row(Row),
    /// A group header: its members follow it when open.
    Header {
        kind: GroupKind,
        icon: &'static str,
        text: String,
        /// The header value (temperature groups only).
        value: Option<String>,
        open: bool,
        indicator: &'static str,
    },
    Separator,
    /// The message shown when there are no readings: a click sends
    /// Homepage link.
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
    /// The open groups, by group kind.
    pub open: Vec<GroupKind>,
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

/// The widget message of a click on drawn entry `.0` of the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clicked(pub usize);

/// The popup's state: the set of open groups.
#[derive(Clone, Debug, Default)]
pub struct Popup {
    open: Vec<GroupKind>,
}

impl Popup {
    /// A layout call's output arrived: a rebuild closes every group, an
    /// in-place update keeps them as they are.
    pub fn layout(&mut self, menu: &Menu) {
        if menu.update == Update::Rebuild {
            self.open.clear();
        }
    }

    /// The popup surface opens: every group starts closed.
    pub fn opened(&mut self) {
        self.open.clear();
    }

    /// Whether the group of `kind` is open.
    pub fn is_open(&self, kind: GroupKind) -> bool {
        self.open.contains(&kind)
    }

    /// A click on a drawn entry. A header toggles its group alone and
    /// sends nothing. A row sends Pin click and leaves the popup open.
    /// The message and the homepage link send Homepage link, the settings
    /// link sends Settings link, and both close the popup.
    pub fn click(&mut self, entry: &Drawn) -> Reply {
        let (event, close) = match entry {
            Drawn::Row(r) => (Some(Event::PinClick(r.identity.clone())), false),
            Drawn::Header { kind, .. } => {
                if self.is_open(*kind) {
                    self.open.retain(|k| k != kind);
                } else {
                    self.open.push(*kind);
                }
                (None, false)
            }
            Drawn::Separator => (None, false),
            Drawn::Message { .. } | Drawn::HomepageLink { .. } => (Some(Event::HomepageLink), true),
            Drawn::SettingsLink { .. } => (Some(Event::SettingsLink), true),
        };
        Reply { event, close }
    }

    /// The popup plan of a layout with the in-memory pinned list: one
    /// drawn entry per layout entry, in the layout's order, and the
    /// members of each open group after its header.
    pub fn plan(&self, menu: &Menu, pinned: &[String], icon_size: u16) -> Plan {
        let mut entries = Vec::new();
        for entry in &menu.entries {
            match entry {
                Entry::Sensor(item) => entries.push(Drawn::Row(row_of(item, pinned, false))),
                Entry::Maximum(item) => {
                    let mut r = row_of(item, pinned, false);
                    r.name = fl!("row-maximum");
                    entries.push(Drawn::Row(r));
                }
                Entry::Group {
                    kind,
                    header,
                    members,
                } => {
                    let open = self.is_open(*kind);
                    entries.push(Drawn::Header {
                        kind: *kind,
                        icon: group_icon_name(*kind),
                        text: group_text(*kind),
                        value: header.as_ref().map(|v| i18n::value_text(v.as_ref())),
                        open,
                        indicator: if open { OPEN } else { CLOSED },
                    });
                    if open {
                        for m in members {
                            entries.push(Drawn::Row(row_of(m, pinned, true)));
                        }
                    }
                }
                Entry::Separator => entries.push(Drawn::Separator),
                Entry::Message { kind, .. } => entries.push(Drawn::Message {
                    text: match kind {
                        MessageKind::SensorsDetect => fl!("message-detect"),
                        MessageKind::Install => fl!("message-install"),
                    },
                }),
                Entry::HomepageLink { .. } => entries.push(Drawn::HomepageLink {
                    text: fl!("link-homepage"),
                }),
                Entry::SettingsLink => entries.push(Drawn::SettingsLink {
                    text: fl!("link-settings"),
                }),
            }
        }
        Plan {
            entries,
            open: self.open.clone(),
            icon_size,
        }
    }
}

/// The translated heading of a group kind.
fn group_text(kind: GroupKind) -> String {
    match kind {
        GroupKind::Temperature => fl!("class-temperature"),
        GroupKind::Fan => fl!("class-rotation"),
        GroupKind::Voltage => fl!("class-voltage"),
    }
}

fn row_of(item: &Item, pinned: &[String], member: bool) -> Row {
    Row {
        identity: item.identity.clone(),
        pinned: pins::is_pinned(pinned, &item.identity),
        icon: icon_name(item.icon),
        name: item.name.clone(),
        value: i18n::value_text(item.value.as_ref()),
        member,
    }
}

/// The widget id of drawn entry `index`: the container of its row.
pub fn entry_id(index: usize) -> Id {
    Id::new(format!("e{index}"))
}

/// The widget id of a sensor row's element (`check`, `icon`, `name`,
/// `value`) of drawn entry `index`.
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

/// The popup content, built from the plan alone, in libcosmic's applet
/// menu widgets, so it has COSMIC's own popup style. Each drawn entry is a
/// container with the id `e<index>`; a row's elements have the ids
/// `e<index>.check`, `.icon`, `.name` and `.value`, a text's id on the
/// text's own rectangle.
pub fn view<'a>(plan: &Plan) -> cosmic::Element<'a, Clicked> {
    let size = plan.icon_size;
    let side = f32::from(size);
    let children: Vec<cosmic::Element<'a, Clicked>> = plan
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| -> cosmic::Element<'a, Clicked> {
            match e {
                Drawn::Row(r) => {
                    let check: cosmic::Element<'a, Clicked> = if r.pinned {
                        symbolic(CHECK_MARK, size)
                    } else {
                        Space::new()
                            .width(Length::Fixed(side))
                            .height(Length::Fixed(side))
                            .into()
                    };
                    let content = row::with_children(vec![
                        container(check)
                            .id(element_id(i, "check"))
                            .width(Length::Fixed(side))
                            .height(Length::Fixed(side))
                            .into(),
                        container(symbolic(r.icon, size))
                            .id(element_id(i, "icon"))
                            .into(),
                        container(container(text(r.name.clone())).id(element_id(i, "name")))
                            .width(Length::Fill)
                            .align_x(Alignment::Center)
                            .into(),
                        container(text(r.value.clone()))
                            .id(element_id(i, "value"))
                            .into(),
                    ])
                    .align_y(Alignment::Center);
                    let button = container(applet::menu_button(content).on_press(Clicked(i)))
                        .id(entry_id(i));
                    if r.member {
                        container(button).padding([0.0, 0.0, 0.0, INDENT]).into()
                    } else {
                        button.into()
                    }
                }
                Drawn::Header {
                    icon,
                    text: t,
                    value,
                    indicator,
                    ..
                } => {
                    let mut parts = vec![
                        symbolic(icon, size),
                        container(text(t.clone())).width(Length::Fill).into(),
                    ];
                    if let Some(v) = value {
                        parts.push(text(v.clone()).into());
                    }
                    parts.push(symbolic(indicator, size));
                    container(
                        applet::menu_button(row::with_children(parts).align_y(Alignment::Center))
                            .on_press(Clicked(i)),
                    )
                    .id(entry_id(i))
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
    column::with_children(children).width(Length::Fill).into()
}
