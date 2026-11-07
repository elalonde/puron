//! Panel view model. Renders nothing and keeps no state: the caller
//! passes the panel state back in.

use std::collections::BTreeMap;

use crate::format::{format, Text};
use crate::menu::{icon_kind, IconKind, Shown};
use crate::rows::{class_of_identity, Class};
use crate::settings::Settings;

/// Label padding with and without icons.
pub const PADDING_WITH_ICONS: u32 = 0;
pub const PADDING_WITHOUT_ICONS: u32 = 4;

/// The icon of a pin item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// `show_panel_icons` off.
    None,
    Kind(IconKind),
    EmptySlot,
}

/// One item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// `value`: `None` is missing ("⚠"), `Some(None)` is "no value".
    Pin {
        icon: Icon,
        value: Option<Option<Text>>,
    },
    /// No value, the empty-state icon.
    EmptyState,
}

/// The panel state: icon memory. The default is the start state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub icons: BTreeMap<String, IconKind>,
}

/// The output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub items: Vec<Item>,
    /// Label padding in px, on each side.
    pub padding: u32,
    pub state: State,
}

/// The panel items.
pub fn panel(shown: &Shown, pinned: &[String], settings: &Settings, state: &State) -> Panel {
    let s = settings;
    let icons_on = s.show_panel_icons;
    // Step 1: remember the icon kind of each pin found now. With icons
    // off, the memory starts empty.
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
    // Step 2: forget the icons of entries that are no longer pinned.
    icons.retain(|id, _| pinned.contains(id));
    // Step 3: one item per entry that is not hidden, in popup order; entries
    // not in the popup follow, in their own order.
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
            // The value of the rows passed in, whatever the cause.
            let value = found(shown, id, s).map(|(_, v)| v);
            Item::Pin { icon, value }
        })
        .collect();
    if items.is_empty() {
        // Nothing pinned, or every pin hidden.
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

/// The icon kind and value text of a found identity.
fn found(shown: &Shown, id: &str, settings: &Settings) -> Option<(IconKind, Option<Text>)> {
    let line = shown.line(id)?;
    // The line's value goes through `format`, as in the menu.
    Some((icon_kind(line), format(line.class, line.value, settings)))
}

/// A pinned entry whose class's Show Sensors key is off; an
/// entry with no class is never hidden.
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
