//! Raw sensor readings to rows: class, family and display name.

use serde::Deserialize;

/// Chip-name words that mark a GPU chip.
pub const GPU_WORDS: [&str; 3] = ["radeon", "amdgpu", "nouveau"];
/// Chip-name words that mark a drive chip.
pub const DRIVE_WORDS: [&str; 2] = ["drivetemp", "nvme"];
/// Separator between the parts of an identity.
pub const IDENTITY_SEPARATOR: &str = "/";
/// The first part of a Maximum line's identity. Alone, it is the pin that
/// version 1.0.0 stored for its one Maximum row.
pub const MAXIMUM_WORD: &str = "@maximum";
/// The pin that development builds stored for their CPU row.
pub const OLD_CPU_PIN: &str = "@cpu";
/// The CPU heading in name order; the popup shows its translation.
pub const CPU_NAME: &str = "CPU";
/// The kernel label of the reading that stands for a whole drive.
pub const COMPOSITE_LABEL: &str = "Composite";
/// The most characters a shown name has.
pub const NAME_LIMIT: usize = 44;
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
    /// lm-sensors: the label the kernel driver gives, whatever the
    /// configuration calls the sensor.
    #[serde(default)]
    pub kernel_label: Option<String>,
    /// lm-sensors: the chip's own device is on the platform bus.
    #[serde(default)]
    pub platform: bool,
    /// How many leading characters of `name` are the device part; 0 for
    /// a name with no device. liquidctl names are built from `device`.
    #[serde(default)]
    pub device_chars: usize,
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
    /// How many leading characters of `name` are the device part; 0 for
    /// a name with no device.
    pub device_chars: usize,
}

/// Rows from raw readings: one row per reading that is kept, in input
/// order. An NVMe reading that lm-sensors also reports is not kept.
pub fn rows(raw: &[RawReading], shown: &ShownClasses) -> Vec<Row> {
    raw.iter()
        .filter(|r| lm_sensors_twin(r, raw).is_none())
        .filter_map(|r| row(r, shown))
        .collect()
}

/// What libsensors calls the chip of an NVMe controller at PCI `address`,
/// `<domain>:<bus>:<device>.<function>` in sysfs: "nvme-pci-" and, in hex
/// with at least four digits, domain << 16 | bus << 8 | device << 3 |
/// function. So 0000:01:00.0 is nvme-pci-0100, and 0000:01:1f.7 is
/// nvme-pci-01ff. Any other address, as on a fabric, has no such chip.
fn nvme_chip(address: &str) -> Option<String> {
    let number = |digits: &str, width: std::ops::RangeInclusive<usize>| {
        let hex = digits.bytes().all(|b| b.is_ascii_hexdigit());
        let fits = hex && width.contains(&digits.len());
        fits.then(|| u64::from_str_radix(digits, 16).ok()).flatten()
    };
    let (domain, rest) = address.split_once(':')?;
    let (bus, rest) = rest.split_once(':')?;
    let (device, function) = rest.split_once('.')?;
    let domain = number(domain, 4..=8)?;
    let bus = number(bus, 2..=2)?;
    let device = number(device, 2..=2).filter(|&d| d < 32)?;
    let function = number(function, 1..=1).filter(|&f| f < 8)?;
    let chip = domain << 16 | bus << 8 | device << 3 | function;
    Some(format!("nvme-pci-{chip:04x}"))
}

/// The lm-sensors key of the same file as NVMe reading `r`, when lm-sensors
/// reports it: the chip at the controller's address, and the same `temp<N>`.
/// Names and values play no part: a wrong match would hide a sensor.
fn lm_sensors_twin(r: &RawReading, raw: &[RawReading]) -> Option<String> {
    if r.provider != Provider::NvmeCli {
        return None;
    }
    let (address, file) = r.key.rsplit_once(IDENTITY_SEPARATOR)?;
    let key = [nvme_chip(address)?.as_str(), file].join(IDENTITY_SEPARATOR);
    raw.iter()
        .any(|o| {
            o.provider == Provider::LmSensors && o.kind == Some(Kind::Temperature) && o.key == key
        })
        .then_some(key)
}

/// The identity of each NVMe reading that is not kept, and of the
/// lm-sensors row that stays for it.
pub fn repeats(raw: &[RawReading]) -> Vec<(String, String)> {
    let identity = |provider: Provider, key: &str| {
        [Class::Temperature.word(), provider.word(), key].join(IDENTITY_SEPARATOR)
    };
    raw.iter()
        .filter_map(|r| {
            let twin = lm_sensors_twin(r, raw)?;
            Some((
                identity(Provider::NvmeCli, &r.key),
                identity(Provider::LmSensors, &twin),
            ))
        })
        .collect()
}

/// The key of each chip's first temperature with the kernel label
/// "Composite", whatever the configuration calls it. A CPU reading is not one.
pub fn composites(raw: &[RawReading]) -> Vec<String> {
    let roles = cpu_roles(raw);
    let mut chips: Vec<&str> = Vec::new();
    let mut keys = Vec::new();
    for r in raw.iter().filter(|r| {
        r.provider == Provider::LmSensors
            && r.kind == Some(Kind::Temperature)
            && r.kernel_label.as_deref() == Some(COMPOSITE_LABEL)
            && !roles.iter().any(|(key, _)| *key == r.key)
    }) {
        let Some(chip) = r.chip.as_deref() else {
            continue;
        };
        if !chips.contains(&chip) {
            chips.push(chip);
            keys.push(r.key.clone());
        }
    }
    keys
}

/// The row of one raw reading, or none when the reading is dropped.
fn row(r: &RawReading, shown: &ShownClasses) -> Option<Row> {
    let (class, family, name, device_chars) = match r.provider {
        Provider::Liquidctl => {
            let (class, family) = liquidctl_class(r.unit.as_deref()?)?;
            let device = r.device.as_deref()?;
            let name = liquidctl_name(device, r.item.as_deref()?);
            (class, family, name, device_part(device).chars().count())
        }
        provider => {
            let (class, family) = class_of(provider, r.kind?, r.chip.as_deref().unwrap_or(""))?;
            (
                class,
                family,
                r.name.clone().unwrap_or_default(),
                r.device_chars,
            )
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
        device_chars,
    })
}

/// The name as shown: at most [`NAME_LIMIT`] characters. A longer name is
/// cut inside its device part, so the rest stays whole; a name with no
/// device, or whose rest leaves no room for any of the device, is cut at
/// its end.
pub fn shown_name(row: &Row) -> String {
    let name = row.name.as_str();
    let total = name.chars().count();
    if total <= NAME_LIMIT {
        return name.to_string();
    }
    let at = |chars: usize| {
        name.char_indices()
            .nth(chars)
            .map_or(name.len(), |(i, _)| i)
    };
    let rest = total - row.device_chars;
    match (NAME_LIMIT - 1).checked_sub(rest) {
        Some(room) if room > 0 && row.device_chars > 0 => {
            let start = name[..at(room)].trim_end();
            format!("{start}…{}", &name[at(row.device_chars)..])
        }
        _ => cut_at_end(name, NAME_LIMIT),
    }
}

/// `text` when it has at most `limit` characters, else its start and "…"
/// in `limit` characters.
pub fn cut_at_end(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let keep = limit.saturating_sub(1);
    let end = text.char_indices().nth(keep).map_or(text.len(), |(i, _)| i);
    format!("{}…", text[..end].trim_end())
}

/// What follows the device in a row's name: the configured label if there
/// is one, else the kernel's.
pub fn reading_label(row: &Row) -> &str {
    let at = row
        .name
        .char_indices()
        .nth(row.device_chars)
        .map_or(row.name.len(), |(i, _)| i);
    row.name[at..].trim_start()
}

/// The device part of a row's name; the whole name when it has none, as
/// with a configured label.
pub fn device_name(row: &Row) -> String {
    if row.device_chars == 0 {
        return row.name.clone();
    }
    row.name.chars().take(row.device_chars).collect()
}

/// The name of the key's chip up to its first `-`, without a final
/// `_<digits>`.
pub fn chip_word(row: &Row) -> &str {
    chip_stem(&row.key)
}

/// A chip's name up to its first `-`, without a final `_<digits>`.
pub fn chip_stem(chip: &str) -> &str {
    let chip = chip.split(['/', '-']).next().unwrap_or_default();
    match chip.rsplit_once('_') {
        Some((stem, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => chip,
    }
}

/// A line that names its chip: `<chip>: <label>`.
pub fn chip_line_name(row: &Row) -> String {
    format!("{}: {}", chip_word(row), reading_label(row))
}

/// What a reading is to the CPU's group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuRole {
    /// The CPU's own sensor of the whole package, with its number: the lowest
    /// is the default line.
    Package(u64),
    /// The board controller's reading of the CPU.
    Board,
    /// The control value of a chip that also has its real one: never the
    /// default line.
    Control,
    /// Any other reading of a CPU's own chip, such as one core.
    Part,
}

/// The role of each lm-sensors temperature of the CPU, by key. The kernel
/// label decides, so a label from the configuration changes nothing.
pub fn cpu_roles(raw: &[RawReading]) -> Vec<(String, CpuRole)> {
    let label = |r: &RawReading| -> Option<(String, String)> {
        let usable = r.provider == Provider::LmSensors && r.kind == Some(Kind::Temperature);
        let driver = r.chip.as_deref()?.split('-').next()?;
        usable.then(|| {
            (
                driver.to_string(),
                r.kernel_label.clone().unwrap_or_default(),
            )
        })
    };
    let has_tdie = |chip: &Option<String>| {
        raw.iter()
            .any(|r| &r.chip == chip && label(r).is_some_and(|(_, l)| l == "Tdie"))
    };
    raw.iter()
        .filter_map(|r| {
            let (driver, label) = label(r)?;
            let role = match driver.as_str() {
                "coretemp" => match numbered(&label, "Package id ") {
                    Some(n) => CpuRole::Package(n),
                    None => CpuRole::Part,
                },
                "k10temp" | "zenpower" if label == "Tdie" => CpuRole::Package(0),
                "k10temp" | "zenpower" if label == "Tctl" && has_tdie(&r.chip) => CpuRole::Control,
                "k10temp" | "zenpower" if label == "Tctl" => CpuRole::Package(0),
                "k10temp" | "zenpower" => CpuRole::Part,
                _ if r.platform && holds_cpu_word(&label) => CpuRole::Board,
                _ => return None,
            };
            Some((r.key.clone(), role))
        })
        .collect()
}

/// The number after `prefix`, when digits and nothing else follow it.
fn numbered(label: &str, prefix: &str) -> Option<u64> {
    let digits = label.strip_prefix(prefix)?;
    let all_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    all_digits.then(|| digits.parse().unwrap_or(u64::MAX))
}

/// The word "CPU" on its own, case ignored; "CPUTIN" does not hold it.
fn holds_cpu_word(label: &str) -> bool {
    label
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(CPU_NAME))
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
    if text == MAXIMUM_WORD || text == OLD_CPU_PIN {
        return Some(Class::Temperature);
    }
    let (word, _) = text.split_once(IDENTITY_SEPARATOR)?;
    if word == MAXIMUM_WORD {
        return Some(Class::Temperature);
    }
    Class::from_word(word)
}
