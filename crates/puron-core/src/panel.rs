//! What the panel shows: one item per pin of a shown class, or a single
//! icon when there is none. This module builds the items and draws
//! nothing. Its one piece of state, the icon memory (`State`), is returned
//! to the caller, who passes it back on the next call.

use std::collections::BTreeMap;

use crate::format::{format, Text};
use crate::menu::{icon_kind, IconKind, Shown};
use crate::rows::{class_of_identity, Class};
use crate::settings::Settings;

/// The padding in pixels on each side of an item's label when panel icons
/// are shown.
pub const PADDING_WITH_ICONS: u32 = 0;
/// The same padding when panel icons are switched off.
pub const PADDING_WITHOUT_ICONS: u32 = 4;

/// The icon next to a pinned reading in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Panel icons are switched off (`Settings::show_panel_icons`).
    None,
    /// The icon of the reading's class and family.
    Kind(IconKind),
    /// Icons are on, but the pin's line has not been found since icons
    /// were switched on or the state was created, so its kind is unknown.
    EmptySlot,
}

/// One panel item: a pin, or the empty state when no pin is of a shown
/// class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// `value` is `None` when the pin has no line in the popup (the panel
    /// shows "⚠"), and `Some(None)` when its line has no value.
    Pin {
        icon: Icon,
        value: Option<Option<Text>>,
    },
    /// Shown alone when there is no item to show: an icon and no value.
    EmptyState,
}

/// The icon memory: the icon kind last found for each pin, so that a pin
/// whose line disappears keeps its icon. `State::default()` is the state
/// at start-up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub icons: BTreeMap<String, IconKind>,
}

/// What `panel` returns: the items, the label padding, and the state to
/// pass back on the next call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub items: Vec<Item>,
    /// Label padding in px, on each side.
    pub padding: u32,
    pub state: State,
}

/// The panel for the pinned list `pinned`, given the popup's lines `shown`
/// and the state that the previous call returned.
pub fn panel(shown: &Shown, pinned: &[String], settings: &Settings, state: &State) -> Panel {
    let s = settings;
    let icons_on = s.show_panel_icons;
    // Remember the icon kind of each pin whose line is found now. With
    // icons off, the memory is cleared and nothing is remembered.
    let mut icons = if icons_on {
        state.icons.clone()
    } else {
        BTreeMap::new()
    };
    if icons_on {
        for id in pinned {
            if let Some((kind, _)) = found(shown, id, s) {
                icons.insert(id.clone(), kind);
            }
        }
    }
    // Forget the icons of entries that are no longer pinned.
    icons.retain(|id, _| pinned.contains(id));
    // One item per pin of a shown class, in popup order; pins with no
    // line in the popup follow, in pinned-list order.
    let mut visible: Vec<&String> = pinned.iter().filter(|id| !hidden(id, s)).collect();
    visible.sort_by_key(|id| shown.place(id).unwrap_or(usize::MAX));
    let mut items: Vec<Item> = visible
        .into_iter()
        .map(|id| {
            let icon = if !icons_on {
                Icon::None
            } else {
                icons.get(id).map_or(Icon::EmptySlot, |k| Icon::Kind(*k))
            };
            // `None` whenever `shown` has no line for the pin, whatever the
            // reason.
            let value = found(shown, id, s).map(|(_, v)| v);
            Item::Pin { icon, value }
        })
        .collect();
    if items.is_empty() {
        // Nothing is pinned, or every pin is of a hidden class.
        items.push(Item::EmptyState);
    }
    let padding = if icons_on {
        PADDING_WITH_ICONS
    } else {
        PADDING_WITHOUT_ICONS
    };
    Panel {
        items,
        padding,
        state: State { icons },
    }
}

/// The icon kind and value text of the line of `id`, or `None` when
/// `shown` has no such line.
fn found(shown: &Shown, id: &str, settings: &Settings) -> Option<(IconKind, Option<Text>)> {
    let line = shown.line(id)?;
    // The line's value goes through `format`, as in the menu.
    Some((icon_kind(line), format(line.class, line.value, settings)))
}

/// Whether pin `id` is of a class that the settings hide (the
/// `show_temperature`, `show_rotation_rate`, `show_voltage` and
/// `show_power` switches). A pin whose class cannot be told from its text
/// is never hidden.
fn hidden(id: &str, settings: &Settings) -> bool {
    let s = settings;
    match class_of_identity(id) {
        Some(Class::Temperature) => !s.show_temperature,
        Some(Class::Fan) => !s.show_rotation_rate,
        Some(Class::Voltage) => !s.show_voltage,
        Some(Class::Power) => !s.show_power,
        None => false,
    }
}
