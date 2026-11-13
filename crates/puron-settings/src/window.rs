//! The settings window's state and behavior, without any drawing.
//!
//! `Window` takes the window's events one at a time, stores each change
//! the user makes through `puron_config`, and describes what the window
//! shows as a `Plan`: sections of items, each item a label and a control.
//! `view` turns a plan into widgets. Because no display is needed, the
//! tests drive `Window` directly.
//!
//! The window has 18 items, numbered from 1 in the order they are shown.
//! Events and the tables below address items by that number.

use puron_config::{Config, Delivery, StoreResult, Value};
use puron_core::settings::{Settings, TemperatureUnit, POLL_INTERVAL_MAX, POLL_INTERVAL_MIN};

/// The window title. It is not translated.
pub const TITLE: &str = "Puron";
/// The application ID given to libcosmic. It differs from
/// `puron_config::CONFIG_ID` so that libcosmic's own state for this
/// program is kept apart from Puron's settings.
pub const APP_ID: &str = "io.github.elalonde.Puron.Settings";

/// The window properties `main` passes to libcosmic. Sizes are (width,
/// height) in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSettings {
    pub title: &'static str,
    pub app_id: &'static str,
    pub initial: (f32, f32),
    pub minimum: (f32, f32),
}

/// The settings window's title, application ID and sizes, which `main`
/// passes to libcosmic. The window opens at 480 × 720 and cannot be made
/// smaller than 360 × 240.
pub fn window_settings() -> WindowSettings {
    WindowSettings {
        title: TITLE,
        app_id: APP_ID,
        initial: (480.0, 720.0),
        minimum: (360.0, 240.0),
    }
}

/// An input to `Window::handle`: something the user did, or a change the
/// settings watch reported.
#[derive(Clone, Debug)]
pub enum Event {
    /// The window opens: load the settings and ask for the watch.
    Open,
    /// The user set the switch of item `item` to `on`.
    Switch { item: usize, on: bool },
    /// The user chose entry `index` of the temperature unit list (item
    /// 3); see `UNITS` for the order.
    Choose(usize),
    /// The number field's + button.
    Plus,
    /// The number field's − button.
    Minus,
    /// The user edited the number field; the text as it now stands.
    Typed(String),
    /// Enter in the number field, or the field losing focus: the typed
    /// text, if any, is read and stored.
    Commit,
    /// The settings changed on disk, as `Config::watch` reported.
    Delivery(Delivery),
    /// The user closes the window. Typed text that is not empty is
    /// committed first, as when the field loses focus.
    Close,
}

/// Something [`Window::handle`] did or asks the caller to do. A call
/// returns its outputs in the order they happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `handle` has already stored `value` under `key`; this reports it
    /// and its result.
    Store {
        key: &'static str,
        value: Value,
        result: StoreResult,
    },
    /// The caller should now start `Config::watch` and send each delivery
    /// back as `Event::Delivery`. Only `Event::Open` returns it.
    WatchStart,
    /// The program is ending. Only `Event::Close` returns it.
    Exit,
}

/// The control of an item, with what it currently shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// The number field (item 1, the poll interval): its value, the text
    /// it shows (the user's text while they are typing, else the value),
    /// and whether − and + are enabled.
    Number {
        value: u32,
        text: String,
        minus: bool,
        plus: bool,
    },
    Switch(bool),
    /// The drop-down list (item 3, the temperature unit): its entry texts
    /// and the index of the selected one.
    List {
        items: Vec<String>,
        shown: usize,
    },
}

/// One row of the window: a label and the control that edits one setting.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub control: Control,
    /// The settings key the control stores (one of `puron_config::KEYS`).
    pub key: &'static str,
    /// The tooltip, if the item has one: the ID of its translation
    /// message and the translated text.
    pub tooltip: Option<(&'static str, String)>,
}

/// A group of items under a heading.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub heading: String,
    pub items: Vec<Item>,
}

/// Everything the window shows, as data: the title, the notice and the
/// sections in order. `view::view` draws it, and the tests inspect it.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub title: String,
    /// The translated line shown under the header bar while the latest
    /// store has failed; `None` otherwise.
    pub notice: Option<String>,
    pub sections: Vec<Section>,
}

/// The settings key of each item: entry `n - 1` belongs to item `n`.
/// `debug_logging` and `pinned` have no item; the window does not edit
/// them.
const KEYS: [&str; 18] = [
    puron_config::POLL_INTERVAL_SECS,
    puron_config::SHOW_PANEL_ICONS,
    puron_config::TEMPERATURE_UNIT,
    puron_config::SHOW_TEMPERATURE_UNIT,
    puron_config::SHOW_ROTATION_RATE_UNIT,
    puron_config::SHOW_VOLTAGE_UNIT,
    puron_config::SHOW_POWER_UNIT,
    puron_config::SHOW_DECIMALS,
    puron_config::PROVIDER_LM_SENSORS,
    puron_config::PROVIDER_LIQUIDCTL,
    puron_config::PROVIDER_NVIDIA,
    puron_config::PROVIDER_LM_SENSORS_DRIVES,
    puron_config::PROVIDER_UDISKS2,
    puron_config::PROVIDER_NVME,
    puron_config::SHOW_TEMPERATURE,
    puron_config::SHOW_ROTATION_RATE,
    puron_config::SHOW_VOLTAGE,
    puron_config::SHOW_POWER,
];

/// The five sections, as the first and last item number of each. `heading`
/// gives their headings in the same order.
const SECTIONS: [(usize, usize); 5] = [(1, 8), (9, 10), (11, 11), (12, 14), (15, 18)];

/// The translated heading of section `section`, counted from 0.
fn heading(section: usize) -> String {
    match section {
        0 => fl!("heading-display"),
        1 => fl!("heading-generic"),
        2 => fl!("heading-gpu"),
        3 => fl!("heading-drive"),
        _ => fl!("heading-shown"),
    }
}

/// The label of item `item`. Items 9 to 14 switch a sensor source on or
/// off. All of them but item 12 are labelled with the source's product
/// name, which is not translated. Item 12 switches a part of a source,
/// the drives that lm-sensors reads, so its label holds a translated
/// word. The other labels are translated.
fn label(item: usize) -> String {
    match item {
        1 => fl!("label-poll"),
        2 => fl!("label-panel-icons"),
        3 => fl!("label-temp-unit"),
        4 => fl!("label-temp-suffix"),
        5 => fl!("label-fan-suffix"),
        6 => fl!("label-volt-suffix"),
        7 => fl!("label-watt-suffix"),
        8 => fl!("label-decimals"),
        9 => "lm-sensors".to_string(),
        10 => "liquidctl".to_string(),
        11 => "Nvidia".to_string(),
        12 => fl!("label-lm-sensors-drives"),
        13 => "Udisks2".to_string(),
        14 => "nvme-cli".to_string(),
        15 => fl!("class-temperature"),
        16 => fl!("class-rotation"),
        17 => fl!("class-voltage"),
        _ => fl!("class-power"),
    }
}

/// The tooltip of item `item`; only items 8, 9, 10 and 12 have one.
fn tooltip(item: usize) -> Option<(&'static str, String)> {
    match item {
        8 => Some(("tip-decimals", fl!("tip-decimals"))),
        9 => Some(("tip-lm-sensors", fl!("tip-lm-sensors"))),
        10 => Some(("tip-liquidctl", fl!("tip-liquidctl"))),
        12 => Some(("tip-lm-sensors-drives", fl!("tip-lm-sensors-drives"))),
        _ => None,
    }
}

/// The temperature units in the order of the list's entries; `Choose`
/// takes an index into it.
const UNITS: [TemperatureUnit; 2] = [TemperatureUnit::Celsius, TemperatureUnit::Fahrenheit];

/// The list's entry texts, in the order of `UNITS`.
fn unit_items() -> Vec<String> {
    vec![fl!("suffix-celsius"), fl!("suffix-fahrenheit")]
}

/// The setting in `s` that the switch of item `item` changes. `None` for
/// an item without a switch (1 and 3) or a number outside 1 to 18.
fn switch(s: &mut Settings, item: usize) -> Option<&mut bool> {
    Some(match item {
        2 => &mut s.show_panel_icons,
        4 => &mut s.show_temperature_unit,
        5 => &mut s.show_rotation_rate_unit,
        6 => &mut s.show_voltage_unit,
        7 => &mut s.show_power_unit,
        8 => &mut s.show_decimals,
        9 => &mut s.provider_lm_sensors,
        10 => &mut s.provider_liquidctl,
        11 => &mut s.provider_nvidia,
        12 => &mut s.provider_lm_sensors_drives,
        13 => &mut s.provider_udisks2,
        14 => &mut s.provider_nvme,
        15 => &mut s.show_temperature,
        16 => &mut s.show_rotation_rate,
        17 => &mut s.show_voltage,
        18 => &mut s.show_power,
        _ => return None,
    })
}

/// The poll interval, in seconds, that the typed text `text` gives.
///
/// Leading ASCII white space is skipped. The longest prefix that reads as
/// a decimal number is taken: an optional sign, digits with at most one
/// decimal point, and an exponent only if the exponent has a digit. The
/// rest of the text is ignored, and text with no digit at its start reads
/// as 0. The number is then clamped to 1..=60 and its fraction dropped,
/// so "abc" gives 1, "7.9s" gives 7 and "1e3" gives 60.
pub fn read_interval(text: &str) -> u32 {
    let t = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    let mut dot = false;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            digits += 1;
        } else if b[i] == b'.' && !dot {
            dot = true;
        } else {
            break;
        }
        i += 1;
    }
    let number = if digits == 0 {
        0.0
    } else {
        // Take the exponent only if it has at least one digit, so that a
        // trailing "e" or "e+" is ignored.
        let mut j = i;
        if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
            j += 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            let start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > start {
                i = j;
            }
        }
        t[..i].parse::<f64>().unwrap_or(0.0)
    };
    number.clamp(POLL_INTERVAL_MIN as f64, POLL_INTERVAL_MAX as f64) as u32
}

/// The settings window's state.
pub struct Window {
    config: Config,
    /// The settings as the window shows them.
    shown: Settings,
    /// The text being typed in the number field and not yet committed.
    typed: Option<String>,
    /// Whether the latest store failed. The next store that succeeds
    /// clears it.
    failed: bool,
}

impl Window {
    /// A window over the settings in `config`. It shows the defaults until
    /// `Event::Open` loads the stored settings.
    pub fn new(config: Config) -> Window {
        Window {
            config,
            shown: Settings::default(),
            typed: None,
            failed: false,
        }
    }

    fn store(&mut self, key: &'static str, value: Value) -> Output {
        let result = self.config.store(key, &value);
        // A failed store is not undone: the control keeps the new value,
        // and the window shows a notice until a later store succeeds. The
        // value is stored again only when the user changes it again.
        self.failed = result == StoreResult::Failed;
        Output::Store { key, value, result }
    }

    fn set_interval(&mut self, value: u32) -> Vec<Output> {
        if value == self.shown.poll_interval_secs {
            return Vec::new();
        }
        self.shown.poll_interval_secs = value;
        vec![self.store(KEYS[0], Value::U32(value))]
    }

    /// Commits the typed text, if any: reads it with `read_interval` and
    /// stores the result if it differs from the shown interval.
    fn take_typed(&mut self) -> Vec<Output> {
        match self.typed.take() {
            Some(text) => self.set_interval(read_interval(&text)),
            None => Vec::new(),
        }
    }

    /// Handles one event and returns what it did, in order. A switch, a
    /// list choice, a button press or committed text is stored at once,
    /// but only when it changes the shown value; text being typed is not
    /// stored until it is committed.
    pub fn handle(&mut self, event: Event) -> Vec<Output> {
        match event {
            Event::Open => {
                self.shown = self.config.load().settings;
                self.typed = None;
                vec![Output::WatchStart]
            }
            Event::Switch { item, on } => {
                let Some(slot) = switch(&mut self.shown, item) else {
                    return Vec::new();
                };
                if *slot == on {
                    return Vec::new();
                }
                *slot = on;
                vec![self.store(KEYS[item - 1], Value::Bool(on))]
            }
            Event::Choose(index) => {
                let Some(&unit) = UNITS.get(index) else {
                    return Vec::new();
                };
                if unit == self.shown.temperature_unit {
                    return Vec::new();
                }
                self.shown.temperature_unit = unit;
                vec![self.store(KEYS[2], Value::TemperatureUnit(unit))]
            }
            // Whether a button acts depends on the shown value, not on any
            // typed text. A press first commits the typed text, then steps
            // the result by one if that stays within 1 to 60.
            Event::Plus => {
                if self.shown.poll_interval_secs >= POLL_INTERVAL_MAX {
                    return Vec::new();
                }
                let mut out = self.take_typed();
                let v = self.shown.poll_interval_secs;
                if v < POLL_INTERVAL_MAX {
                    out.extend(self.set_interval(v + 1));
                }
                out
            }
            Event::Minus => {
                if self.shown.poll_interval_secs <= POLL_INTERVAL_MIN {
                    return Vec::new();
                }
                let mut out = self.take_typed();
                let v = self.shown.poll_interval_secs;
                if v > POLL_INTERVAL_MIN {
                    out.extend(self.set_interval(v - 1));
                }
                out
            }
            Event::Typed(text) => {
                self.typed = Some(text);
                Vec::new()
            }
            Event::Commit => self.take_typed(),
            Event::Delivery(delivery) => {
                // Every control takes the reported value, but text that is
                // being typed stays. Nothing is stored.
                self.shown = delivery.loaded.settings;
                Vec::new()
            }
            Event::Close => {
                // An emptied field is left alone, so that closing does not
                // store the 1 that an empty text reads as.
                if self.typed.as_deref() == Some("") {
                    self.typed = None;
                }
                let mut out = self.take_typed();
                out.push(Output::Exit);
                out
            }
        }
    }

    /// The plan of what the window shows now.
    pub fn plan(&self) -> Plan {
        let mut shown = self.shown.clone();
        let sections = SECTIONS
            .iter()
            .enumerate()
            .map(|(s, &(first, last))| Section {
                heading: heading(s),
                items: (first..=last)
                    .map(|item| {
                        let control = match item {
                            1 => {
                                let value = shown.poll_interval_secs;
                                Control::Number {
                                    value,
                                    text: self.typed.clone().unwrap_or_else(|| value.to_string()),
                                    minus: value > POLL_INTERVAL_MIN,
                                    plus: value < POLL_INTERVAL_MAX,
                                }
                            }
                            3 => Control::List {
                                items: unit_items(),
                                shown: UNITS
                                    .iter()
                                    .position(|&u| u == shown.temperature_unit)
                                    .unwrap_or(0),
                            },
                            _ => Control::Switch(*switch(&mut shown, item).expect("a switch")),
                        };
                        Item {
                            label: label(item),
                            control,
                            key: KEYS[item - 1],
                            tooltip: tooltip(item),
                        }
                    })
                    .collect(),
            })
            .collect();
        Plan {
            title: TITLE.to_string(),
            notice: self.failed.then(|| fl!("notice-store-failed")),
            sections,
        }
    }
}

/// The icon name of the number field's − button.
pub const MINUS_ICON: &str = "list-remove-symbolic";
/// The icon name of the number field's + button.
pub const PLUS_ICON: &str = "list-add-symbolic";
/// The width of the number field's text input, in logical pixels.
pub const INPUT_WIDTH: f32 = 48.0;
