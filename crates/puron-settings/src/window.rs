//! The state machine of the settings window and its window plan,
//! without a display.

use puron_config::{Config, Delivery, StoreResult, Value};
use puron_core::settings::{Settings, TemperatureUnit, POLL_INTERVAL_MAX, POLL_INTERVAL_MIN};

/// Window title: a fixed text.
pub const TITLE: &str = "Puron";
/// App ID.
pub const APP_ID: &str = "io.github.elalonde.Puron.Settings";

/// The window settings `main` hands to libcosmic, sizes in logical
/// pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSettings {
    pub title: &'static str,
    pub app_id: &'static str,
    pub initial: (f32, f32),
    pub minimum: (f32, f32),
}

/// The program's window settings.
pub fn window_settings() -> WindowSettings {
    WindowSettings {
        title: TITLE,
        app_id: APP_ID,
        initial: (480.0, 720.0),
        minimum: (360.0, 240.0),
    }
}

/// An event of the window. Items are addressed by their number, 1 to 20,
/// in window order.
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    /// Change of a switch item to `on`.
    Switch {
        item: usize,
        on: bool,
    },
    /// Change of the list (item 3) to the item at `index` (0: `°C`).
    Choose(usize),
    /// The number field's + button.
    Plus,
    /// The number field's − button.
    Minus,
    /// The number field's text, as typed so far.
    Typed(String),
    /// Enter, or the number field losing focus.
    Commit,
    Delivery(Delivery),
    Close,
}

/// An output of [`Window::handle`], in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `config` Store of one key, with its result (handled in the same
    /// call).
    Store {
        key: &'static str,
        value: Value,
        result: StoreResult,
    },
    /// `config` Watch is to be started (the end of Open).
    WatchStart,
    /// The program exits (Close).
    Exit,
}

/// The control of an item and its shown state.
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// The number field: its value, the text it shows (the
    /// typed text while one is being typed), and whether − and + are
    /// enabled.
    Number {
        value: u32,
        text: String,
        minus: bool,
        plus: bool,
    },
    Switch(bool),
    /// The list: its item texts and the index of the shown one.
    List {
        items: Vec<String>,
        shown: usize,
    },
}

/// One item of the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub control: Control,
    pub key: &'static str,
    /// The tooltip's entry ID and text.
    pub tooltip: Option<(&'static str, String)>,
}

/// One section of the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub heading: String,
    pub items: Vec<Item>,
}

/// The window plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub title: String,
    pub sections: Vec<Section>,
}

/// The keys of the items, by item number less one.
const KEYS: [&str; 20] = [
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
    puron_config::PROVIDER_UDISKS2,
    puron_config::PROVIDER_NVME,
    puron_config::SHOW_TEMPERATURE,
    puron_config::SHOW_ROTATION_RATE,
    puron_config::SHOW_VOLTAGE,
    puron_config::SHOW_POWER,
    puron_config::GROUP_TEMPERATURE,
    puron_config::GROUP_ROTATION_RATE,
    puron_config::GROUP_VOLTAGE,
];

/// The sections: the first and last item number of each.
const SECTIONS: [(usize, usize); 6] = [(1, 8), (9, 10), (11, 11), (12, 13), (14, 17), (18, 20)];

fn heading(section: usize) -> String {
    match section {
        0 => fl!("heading-display"),
        1 => fl!("heading-generic"),
        2 => fl!("heading-gpu"),
        3 => fl!("heading-drive"),
        4 => fl!("heading-shown"),
        _ => fl!("heading-grouping"),
    }
}

/// The label of item `item`: an entry, or a product name (fixed text,
/// not translated).
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
        12 => "Udisks2".to_string(),
        13 => "nvme-cli".to_string(),
        14 | 18 => fl!("class-temperature"),
        15 | 19 => fl!("class-rotation"),
        16 | 20 => fl!("class-voltage"),
        _ => fl!("class-power"),
    }
}

fn tooltip(item: usize) -> Option<(&'static str, String)> {
    match item {
        8 => Some(("tip-decimals", fl!("tip-decimals"))),
        9 => Some(("tip-lm-sensors", fl!("tip-lm-sensors"))),
        10 => Some(("tip-liquidctl", fl!("tip-liquidctl"))),
        18 => Some(("tip-group-temp", fl!("tip-group-temp"))),
        19 => Some(("tip-group-fan", fl!("tip-group-fan"))),
        20 => Some(("tip-group-volt", fl!("tip-group-volt"))),
        _ => None,
    }
}

/// The list's units, in item order.
const UNITS: [TemperatureUnit; 2] = [TemperatureUnit::Celsius, TemperatureUnit::Fahrenheit];

fn unit_items() -> Vec<String> {
    vec![fl!("suffix-celsius"), fl!("suffix-fahrenheit")]
}

/// The switch of item `item` in `s`; `None` for items 1 and 3.
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
        12 => &mut s.provider_udisks2,
        13 => &mut s.provider_nvme,
        14 => &mut s.show_temperature,
        15 => &mut s.show_rotation_rate,
        16 => &mut s.show_voltage,
        17 => &mut s.show_power,
        18 => &mut s.group_temperature,
        19 => &mut s.group_rotation_rate,
        20 => &mut s.group_voltage,
        _ => return None,
    })
}

/// Reads a typed poll interval: leading white space skipped, the
/// longest decimal start read (0 if none), set into 1 to 60, then cut
/// toward zero.
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
        // An exponent counts only with at least one digit.
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
    shown: Settings,
    /// The text being typed in the number field, if any.
    typed: Option<String>,
}

impl Window {
    /// A window over `config`; nothing is loaded before Open.
    pub fn new(config: Config) -> Window {
        Window {
            config,
            shown: Settings::default(),
            typed: None,
        }
    }

    fn store(&self, key: &'static str, value: Value) -> Output {
        let result = self.config.store(key, &value);
        // On a failed store the shown value is kept and no error is shown;
        // the next change stores again.
        Output::Store { key, value, result }
    }

    fn set_interval(&mut self, value: u32) -> Vec<Output> {
        if value == self.shown.poll_interval_secs {
            return Vec::new();
        }
        self.shown.poll_interval_secs = value;
        vec![self.store(KEYS[0], Value::U32(value))]
    }

    /// Takes the typed text, if any, as `read_interval` reads it.
    fn take_typed(&mut self) -> Vec<Output> {
        match self.typed.take() {
            Some(text) => self.set_interval(read_interval(&text)),
            None => Vec::new(),
        }
    }

    /// Handles one event; returns its outputs in order.
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
            // The button's state follows the shown value; a press takes the
            // typed text first, then steps within 1 to 60.
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
                // Every control follows the record, except the text being
                // typed; nothing is stored.
                self.shown = delivery.loaded.settings;
                Vec::new()
            }
            Event::Close => vec![Output::Exit],
        }
    }

    /// The window plan of the shown values.
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
            sections,
        }
    }
}

/// The number field's − button icon.
pub const MINUS_ICON: &str = "list-remove-symbolic";
/// The number field's + button icon.
pub const PLUS_ICON: &str = "list-add-symbolic";
/// The number field's value input width, logical pixels.
pub const INPUT_WIDTH: f32 = 48.0;
