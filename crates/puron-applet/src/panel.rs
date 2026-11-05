//! The panel button: rendering and icon names. It decides no item, value
//! or icon kind (that is `core.panel`'s job) and holds no state.

use cosmic::applet::Context;
use cosmic::iced::{Alignment, Length, Padding};
use cosmic::widget::{column, container, icon, row, text, Id, Space};
use puron_core::menu::{GroupKind, IconKind};
use puron_core::panel::{Icon, Item, Panel};

use crate::i18n;

/// Spacing between elements, both orientations.
pub const SPACING: f32 = 0.0;

/// Icon padding on a horizontal panel's main axis, before and after
/// every icon element.
pub const ICON_PADDING_HORIZONTAL: f32 = 9.0;

/// Icon padding on a vertical panel's main axis.
pub const ICON_PADDING_VERTICAL: f32 = 5.0;

/// The panel's orientation, from libcosmic's applet context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    /// The orientation of the panel's anchor.
    pub fn of(context: &Context) -> Self {
        if context.is_horizontal() {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        }
    }
}

/// libcosmic's suggested applet icon size for the panel's size, square;
/// every icon name here is symbolic.
pub fn icon_size(context: &Context) -> u16 {
    context.suggested_size(true).0
}

/// The icon name of an icon kind. This is the only map; the popup uses
/// it too.
pub fn icon_name(kind: IconKind) -> &'static str {
    match kind {
        IconKind::GeneralTemperature | IconKind::Maximum => "puron-temperature-symbolic",
        IconKind::GpuTemperature => "puron-gpu-temperature-symbolic",
        IconKind::DriveTemperature => "drive-harddisk-system-symbolic",
        IconKind::Fan => "puron-fan-symbolic",
        IconKind::Voltage | IconKind::Power => "puron-voltage-symbolic",
    }
}

/// The icon name of a group kind, from the same map.
pub fn group_icon_name(kind: GroupKind) -> &'static str {
    icon_name(match kind {
        GroupKind::Temperature => IconKind::GeneralTemperature,
        GroupKind::Fan => IconKind::Fan,
        GroupKind::Voltage => IconKind::Voltage,
    })
}

/// One element of the panel plan, with its item index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    Icon {
        item: usize,
        name: &'static str,
    },
    /// The empty slot: the icon size, draws nothing.
    Slot {
        item: usize,
    },
    Text {
        item: usize,
        text: String,
    },
}

/// The panel plan: a value that tests read without a display.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub orientation: Orientation,
    pub elements: Vec<Element>,
    pub spacing: f32,
    /// Icon padding in px, before and after each icon element on the
    /// main axis, outside its square.
    pub icon_padding: f32,
    /// Label padding in px, left and right of each text.
    pub padding: u32,
    pub icon_size: u16,
}

/// The panel plan of `core.panel`'s items.
pub fn plan(panel: &Panel, orientation: Orientation, icon_size: u16) -> Plan {
    let mut elements = Vec::new();
    for (item, it) in panel.items.iter().enumerate() {
        match it {
            Item::EmptyState => elements.push(Element::Icon {
                item,
                name: icon_name(IconKind::GpuTemperature),
            }),
            Item::Pin { icon, value } => {
                match icon {
                    Icon::None => {}
                    Icon::Kind(k) => elements.push(Element::Icon {
                        item,
                        name: icon_name(*k),
                    }),
                    Icon::EmptySlot => elements.push(Element::Slot { item }),
                }
                let text = i18n::panel_value(value.as_ref().map(|v| v.as_ref()));
                elements.push(Element::Text { item, text });
            }
        }
    }
    Plan {
        orientation,
        elements,
        spacing: SPACING,
        icon_padding: match orientation {
            Orientation::Horizontal => ICON_PADDING_HORIZONTAL,
            Orientation::Vertical => ICON_PADDING_VERTICAL,
        },
        padding: panel.padding,
        icon_size,
    }
}

/// The widget id of element `index` of the plan.
pub fn element_id(index: usize) -> Id {
    Id::new(format!("e{index}"))
}

/// The button content, built from the plan alone. Each element is a
/// container with the id `e<index>`; a text's container is the text's
/// own rectangle, inside the label padding; an icon's or slot's
/// container is its square, inside the icon padding.
pub fn view<'a, M: 'a>(plan: &Plan) -> cosmic::Element<'a, M> {
    let size = plan.icon_size;
    let pad = plan.padding as f32;
    let around = icon_padding(plan);
    let children: Vec<cosmic::Element<'a, M>> = plan
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| match e {
            Element::Icon { name, .. } => container(
                container(
                    icon::from_name(*name)
                        .size(size)
                        .symbolic(true)
                        .fallback(None)
                        .icon(),
                )
                .id(element_id(i)),
            )
            .padding(around)
            .into(),
            Element::Slot { .. } => container(
                container(
                    Space::new()
                        .width(Length::Fixed(f32::from(size)))
                        .height(Length::Fixed(f32::from(size))),
                )
                .id(element_id(i)),
            )
            .padding(around)
            .into(),
            Element::Text { text: t, .. } => {
                container(container(text(t.clone())).id(element_id(i)))
                    .padding([0.0, pad])
                    .into()
            }
        })
        .collect();
    match plan.orientation {
        Orientation::Horizontal => row::with_children(children)
            .spacing(plan.spacing)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .into(),
        Orientation::Vertical => column::with_children(children)
            .spacing(plan.spacing)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
    }
}

/// The icon padding of the plan on its main axis, none on the cross
/// axis.
fn icon_padding(plan: &Plan) -> Padding {
    let p = plan.icon_padding;
    match plan.orientation {
        Orientation::Horizontal => Padding {
            top: 0.0,
            right: p,
            bottom: 0.0,
            left: p,
        },
        Orientation::Vertical => Padding {
            top: p,
            right: 0.0,
            bottom: p,
            left: 0.0,
        },
    }
}
