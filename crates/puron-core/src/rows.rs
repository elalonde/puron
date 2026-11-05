//! Raw sensor readings to rows: class, family and display name.

use serde::Deserialize;

/// Chip-name words that mark a GPU chip.
pub const GPU_WORDS: [&str; 3] = ["radeon", "amdgpu", "nouveau"];
/// Chip-name words that mark a drive chip.
pub const DRIVE_WORDS: [&str; 2] = ["drivetemp", "nvme"];
/// Separator between the parts of an identity.
pub const IDENTITY_SEPARATOR: &str = "/";
/// The identity of the Maximum row.
pub const MAXIMUM_IDENTITY: &str = "@maximum";
/// liquidctl units: temperature, fan, voltage.
pub const LIQUIDCTL_UNITS: [&str; 3] = ["°C", "rpm", "V"];
/// liquidctl item words, each with its leading space.
pub const LIQUIDCTL_ITEM_WORDS: [&str; 3] = [" temperature", " speed", " voltage"];

/// The five providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum Provider {
    #[serde(rename = "lm-sensors")]
    LmSensors,
    #[serde(rename = "liquidctl")]
    Liquidctl,
    #[serde(rename = "NVIDIA")]
    Nvidia,
    #[serde(rename = "udisks2")]
    Udisks2,
    #[serde(rename = "nvme-cli")]
    NvmeCli,
}

impl Provider {
    /// The provider word of an identity.
    pub fn word(self) -> &'static str {
        match self {
            Provider::LmSensors => "lm-sensors",
            Provider::Liquidctl => "liquidctl",
            Provider::Nvidia => "NVIDIA",
            Provider::Udisks2 => "udisks2",
            Provider::NvmeCli => "nvme-cli",
        }
    }
}

/// Kind of a raw reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Temperature,
    Fan,
    Voltage,
    Power,
}

/// Class of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Temperature,
    Fan,
    Voltage,
    Power,
}

impl Class {
    /// The class word, the first part of an identity.
    pub fn word(self) -> &'static str {
        match self {
            Class::Temperature => "temperature",
            Class::Fan => "fan",
            Class::Voltage => "voltage",
            Class::Power => "power",
        }
    }

    /// The class of a class word, character for character.
    fn from_word(word: &str) -> Option<Class> {
        [Class::Temperature, Class::Fan, Class::Voltage, Class::Power]
            .into_iter()
            .find(|c| c.word() == word)
    }
}

/// Family of a temperature row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Gpu,
    General,
    Drive,
}

/// One raw reading. Fields a provider does not give are `None`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RawReading {
    pub provider: Provider,
    pub position: u32,
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub chip: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    pub key: String,
    /// `None` is "no value".
    pub value: Option<f64>,
}

/// The shown classes: the Show Sensors switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShownClasses {
    pub temperature: bool,
    pub fan: bool,
    pub voltage: bool,
    pub power: bool,
}

impl ShownClasses {
    /// All four classes shown (the default).
    pub const ALL: ShownClasses = ShownClasses {
        temperature: true,
        fan: true,
        voltage: true,
        power: true,
    };

    pub fn contains(&self, class: Class) -> bool {
        match class {
            Class::Temperature => self.temperature,
            Class::Fan => self.fan,
            Class::Voltage => self.voltage,
            Class::Power => self.power,
        }
    }
}

/// One row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub class: Class,
    /// Temperature rows only.
    pub family: Option<Family>,
    pub name: String,
    /// `None` is "no value".
    pub value: Option<f64>,
    pub provider: Provider,
    pub position: u32,
    pub key: String,
}

/// Rows from raw readings: one row per reading that is kept, in input
/// order.
pub fn rows(raw: &[RawReading], shown: &ShownClasses) -> Vec<Row> {
    raw.iter().filter_map(|r| row(r, shown)).collect()
}

/// The row of one raw reading, or none when the reading is dropped.
fn row(r: &RawReading, shown: &ShownClasses) -> Option<Row> {
    let (class, family, name) = match r.provider {
        Provider::Liquidctl => {
            let (class, family) = liquidctl_class(r.unit.as_deref()?)?;
            let name = liquidctl_name(r.device.as_deref()?, r.item.as_deref()?);
            (class, family, name)
        }
        provider => {
            let (class, family) = class_of(provider, r.kind?, r.chip.as_deref().unwrap_or(""))?;
            (class, family, r.name.clone().unwrap_or_default())
        }
    };
    // The class is decided first, then checked against the shown classes.
    if !shown.contains(class) {
        return None;
    }
    Some(Row {
        class,
        family,
        name,
        value: r.value,
        provider: r.provider,
        position: r.position,
        key: r.key.clone(),
    })
}

/// Class and family of a reading of every provider but liquidctl.
fn class_of(provider: Provider, kind: Kind, chip: &str) -> Option<(Class, Option<Family>)> {
    match kind {
        Kind::Temperature => {
            let family = match provider {
                Provider::LmSensors => chip_family(chip)?,
                Provider::Nvidia => Family::Gpu,
                Provider::Udisks2 | Provider::NvmeCli => Family::Drive,
                Provider::Liquidctl => Family::General,
            };
            Some((Class::Temperature, Some(family)))
        }
        Kind::Fan => Some((Class::Fan, None)),
        Kind::Voltage => Some((Class::Voltage, None)),
        // Power rows come from lm-sensors GPU chips only; drive words are not
        // tested.
        Kind::Power => {
            if provider != Provider::LmSensors || !is_gpu_chip(chip) {
                return None;
            }
            Some((Class::Power, None))
        }
    }
}

/// A GPU chip's name contains a GPU word; case is significant.
fn is_gpu_chip(chip: &str) -> bool {
    GPU_WORDS.iter().any(|w| chip.contains(w))
}

fn is_drive_chip(chip: &str) -> bool {
    DRIVE_WORDS.iter().any(|w| chip.contains(w))
}

/// The family of an lm-sensors temperature; none for a chip with
/// both a GPU word and a drive word.
fn chip_family(chip: &str) -> Option<Family> {
    match (is_gpu_chip(chip), is_drive_chip(chip)) {
        (true, true) => None,
        (true, false) => Some(Family::Gpu),
        (false, true) => Some(Family::Drive),
        (false, false) => Some(Family::General),
    }
}

/// The class of a liquidctl unit, matched character for character.
fn liquidctl_class(unit: &str) -> Option<(Class, Option<Family>)> {
    let [celsius, rpm, volt] = LIQUIDCTL_UNITS;
    if unit == celsius {
        Some((Class::Temperature, Some(Family::General)))
    } else if unit == rpm {
        Some((Class::Fan, None))
    } else if unit == volt {
        Some((Class::Voltage, None))
    } else {
        None
    }
}

/// A liquidctl row name: device part, one space, item part.
fn liquidctl_name(device: &str, item: &str) -> String {
    format!("{} {}", device_part(device), item_part(item))
}

/// The device part: the manufacturer cut, then the " (" cut.
fn device_part(description: &str) -> &str {
    // A space after at least one character: cut up to and including it.
    let rest = match description.char_indices().find(|&(i, c)| i > 0 && c == ' ') {
        Some((i, _)) => &description[i + 1..],
        None => description,
    };
    // The first " (" with at least one character after it.
    match rest.find(" (") {
        Some(i) if i + 2 < rest.len() => &rest[..i],
        _ => rest,
    }
}

/// The item part: the leftmost item word removed, once.
fn item_part(item: &str) -> String {
    let first = LIQUIDCTL_ITEM_WORDS
        .iter()
        .filter_map(|w| item.find(w).map(|i| (i, w.len())))
        .min();
    match first {
        Some((i, len)) => format!("{}{}", &item[..i], &item[i + len..]),
        None => item.to_string(),
    }
}

/// Identities: one per row, in the same order.
pub fn identities(rows: &[Row]) -> Vec<String> {
    rows.iter().map(identity).collect()
}

/// An identity: class word, `/`, provider, `/`, key.
fn identity(row: &Row) -> String {
    [row.class.word(), row.provider.word(), row.key.as_str()].join(IDENTITY_SEPARATOR)
}

/// Class of an identity: the class of a pinned entry, or none.
pub fn class_of_identity(text: &str) -> Option<Class> {
    if text == MAXIMUM_IDENTITY {
        return Some(Class::Temperature);
    }
    let (word, _) = text.split_once(IDENTITY_SEPARATOR)?;
    Class::from_word(word)
}
