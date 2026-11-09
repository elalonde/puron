//! The panel button's content: a `Plan` built from the items that
//! `puron_core::panel::panel` chose, and the widgets drawn from it. This
//! module chooses no item, value or icon kind itself and holds no state.
//! It also owns the map from icon kinds to icon names, which the popup
//! uses too.

use cosmic::applet::Context;
use cosmic::iced::{Alignment, Length, Padding};
use cosmic::widget::{column, container, icon, row, text, Id, Space};
use puron_core::menu::{IconKind, Section};
use puron_core::panel::{Icon, Item, Panel};

use crate::i18n;

/// The space between neighbouring elements, in px, in both orientations.
/// It is zero: the icon padding and the label padding alone separate them.
pub const SPACING: f32 = 0.0;

/// The padding in px before and after every icon, along a horizontal
/// panel.
pub const ICON_PADDING_HORIZONTAL: f32 = 9.0;

/// The padding in px above and below every icon, along a vertical panel.
pub const ICON_PADDING_VERTICAL: f32 = 5.0;

/// Whether the panel runs across the screen or down its side. The
/// elements are laid out along the panel in this direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    /// The orientation that libcosmic reports for the applet's panel.
    pub fn of(context: &Context) -> Self {
        if context.is_horizontal() {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        }
    }
}

/// The side in px of a square icon, as libcosmic suggests it for the
/// panel's size.
pub fn icon_size(context: &Context) -> u16 {
    context.suggested_size(true).0
}

/// The themed icon name for an icon kind. This is the only map from kinds
/// to names; the popup's section headings use it too. Voltage and power
/// share one icon.
pub fn icon_name(kind: IconKind) -> &'static str {
    match kind {
        IconKind::GeneralTemperature => "puron-temperature-symbolic",
        IconKind::GpuTemperature => "puron-gpu-temperature-symbolic",
        IconKind::DriveTemperature => "drive-harddisk-system-symbolic",
        IconKind::Fan => "puron-fan-symbolic",
        IconKind::Voltage | IconKind::Power => "puron-voltage-symbolic",
    }
}

/// The icon name for a popup section heading: the icon of the matching
/// kind, through `icon_name`. Temperature sections use the general
/// temperature icon.
pub fn section_icon_name(section: Section) -> &'static str {
    icon_name(match section {
        Section::Temperature => IconKind::GeneralTemperature,
        Section::Fan => IconKind::Fan,
        Section::Voltage => IconKind::Voltage,
        Section::Power => IconKind::Power,
    })
}

/// One element of the panel button. `item` is the index in
/// `Panel::items` of the item it was built from: a pinned reading gives
/// an optional icon or slot followed by its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    Icon {
        item: usize,
        name: &'static str,
    },
    /// An empty square of the icon's size, drawn while icons are on but
    /// the pin's icon kind is not known yet, so that its text keeps its
    /// place.
    Slot {
        item: usize,
    },
    Text {
        item: usize,
        text: String,
    },
}

/// Everything the panel button draws, as plain data. `view` builds the
/// widgets from it alone, so tests can check a plan without a display.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub orientation: Orientation,
    pub elements: Vec<Element>,
    pub spacing: f32,
    /// The padding in px before and after each icon or slot along the
    /// panel, outside its square.
    pub icon_padding: f32,
    /// The padding in px left and right of each text, from
    /// `Panel::padding`.
    pub padding: u32,
    pub icon_size: u16,
}

/// The plan for the items of `panel`. `Item::EmptyState`, shown when no
/// pin is of a shown class, becomes the GPU temperature icon alone. A pin
/// becomes its icon (or an empty slot, or nothing when icons are off)
/// followed by its value text.
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

/// The widget ID of the element at `index` in `Plan::elements`, so that
/// tests can find its rectangle.
pub fn element_id(index: usize) -> Id {
    Id::new(format!("e{index}"))
}

/// The panel button's content, built from the plan alone: a row on a
/// horizontal panel, a column on a vertical one, centred across the
/// panel. Each element sits in a container whose widget ID is
/// `element_id(index)`. That container is exactly the text's or the
/// square's own rectangle; the padding is applied by an outer container,
/// so it is not part of the rectangle.
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

/// The plan's icon padding along the panel only: left and right on a
/// horizontal panel, top and bottom on a vertical one.
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
