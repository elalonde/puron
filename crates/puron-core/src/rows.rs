//! Turns raw sensor readings into rows. A row has a class (temperature,
//! fan, voltage or power), a family when it is a temperature (GPU, drive
//! or general), and a display name. Each row also has an identity, the
//! text `<class>/<provider>/<key>` that names it in the pinned list and in
//! the popup (see [`identities`]).
//!
//! The module also finds the readings that belong to the CPU
//! ([`cpu_roles`]), each drive's Composite reading ([`composites`]), and
//! the NVMe readings that lm-sensors reports too ([`repeats`]).

use serde::Deserialize;

/// Words that mark an lm-sensors chip as a GPU when its name contains one;
/// case matters.
pub const GPU_WORDS: [&str; 3] = ["radeon", "amdgpu", "nouveau"];
/// Words that mark an lm-sensors chip as a drive when its name contains
/// one; case matters.
pub const DRIVE_WORDS: [&str; 2] = ["drivetemp", "nvme"];
/// Separator between the parts of an identity.
pub const IDENTITY_SEPARATOR: &str = "/";
/// The first part of the identity of a Maximum line, the popup line that
/// shows the highest temperature of one device. On its own, `@maximum` is
/// the pin that version 1.0.0 stored for its single Maximum row.
pub const MAXIMUM_WORD: &str = "@maximum";
/// The pin that development builds stored for their single CPU row. It
/// moves to a line of the CPU group (see `pins::moved`).
pub const OLD_CPU_PIN: &str = "@cpu";
/// The CPU group's heading before translation. It is also the word that
/// marks a board sensor's reading of the CPU (see [`cpu_roles`]).
pub const CPU_NAME: &str = "CPU";
/// The kernel label of the reading that stands for a whole drive.
pub const COMPOSITE_LABEL: &str = "Composite";
/// The longest shown name, in characters; [`shown_name`] shortens longer
/// names.
pub const NAME_LIMIT: usize = 44;
/// The liquidctl units that make a row: temperature, fan speed and
/// voltage. A reading with any other unit is dropped.
pub const LIQUIDCTL_UNITS: [&str; 3] = ["°C", "rpm", "V"];
/// Words removed from a liquidctl item name to make the row name, each
/// with its leading space. Only the leftmost one found is removed (see
/// `item_part`).
pub const LIQUIDCTL_ITEM_WORDS: [&str; 3] = [" temperature", " speed", " voltage"];

/// A source of readings. The serde names are the provider words of
/// [`Provider::word`].
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
    /// The provider's word, the middle part of an identity.
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

/// What a raw reading measures, as its provider reports it. liquidctl
/// readings have no kind: their unit decides the class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Temperature,
    Fan,
    Voltage,
    Power,
}

/// What a row measures. The class decides the popup section, the value
/// format and the settings switch that can hide the row.
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

    /// The class whose word is exactly `word`; case matters.
    fn from_word(word: &str) -> Option<Class> {
        [Class::Temperature, Class::Fan, Class::Voltage, Class::Power]
            .into_iter()
            .find(|c| c.word() == word)
    }
}

/// The kind of device that a temperature row belongs to. It decides the
/// row's icon and its place in display order.
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
    /// The reading's place in its provider's output; it breaks the last
    /// ties in display order.
    pub position: u32,
    #[serde(default)]
    pub kind: Option<Kind>,
    /// lm-sensors: the chip name as libsensors gives it, such as
    /// `nvme-pci-0100`.
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
    /// The provider's key for the sensor, the last part of the identity.
    pub key: String,
    /// `None` is "no value".
    pub value: Option<f64>,
    /// lm-sensors: the label that the kernel driver gives, whatever label
    /// the lm-sensors configuration file sets for the sensor.
    #[serde(default)]
    pub kernel_label: Option<String>,
    /// lm-sensors: true when the chip's device is on the platform bus.
    /// Only such a chip's reading labeled with the word CPU is taken as
    /// the board's reading of the CPU (see [`cpu_roles`]).
    #[serde(default)]
    pub platform: bool,
    /// How many leading characters of `name` are the device part; 0 for
    /// a name with no device. liquidctl names are built from `device`.
    #[serde(default)]
    pub device_chars: usize,
}

/// Which classes are shown, from the settings' class switches. Rows of a
/// class that is not shown are dropped.
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

/// One row: a reading that `rows` kept, with its class, family and
/// display name.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub class: Class,
    /// `Some` for temperature rows only.
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

/// One row per raw reading that is kept, in input order. A reading is
/// dropped when it makes no row (see `row`) and when it is an NVMe reading
/// that lm-sensors also reports (see [`repeats`]).
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

/// For an NVMe reading `r`, the key of the lm-sensors temperature that
/// reads the same sysfs file, when `raw` holds one. The match uses only
/// the chip at the controller's PCI address and the file name `temp<N>`.
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

/// For each NVMe reading that [`rows`] drops because lm-sensors reports
/// the same sensor: its identity, and the identity of the lm-sensors row
/// that stays. A pin on the first moves to the second (see `pins::moved`).
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

/// The keys of the lm-sensors temperatures that the kernel labels
/// "Composite", the first one per chip, whatever label the configuration
/// sets. A reading of the CPU group is never one. In a device with two
/// or more readings, this reading is the default line.
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

/// The name as shown, at most [`NAME_LIMIT`] characters. A longer name is
/// cut inside its device part, with "…" in place of the cut, so that the
/// reading label after it stays whole. A name with no device part, or
/// whose label leaves no room for any of the device part, is cut at its
/// end instead (see [`cut_at_end`]).
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
        Some(room) if room > 0 => {
            let start = name[..at(room)].trim_end();
            format!("{start}…{}", &name[at(row.device_chars)..])
        }
        _ => cut_at_end(name, NAME_LIMIT),
    }
}

/// `text` when it has at most `limit` characters, else its start and "…"
/// in `limit` characters. Spaces before the "…" are dropped.
pub fn cut_at_end(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let keep = limit.saturating_sub(1);
    let end = text.char_indices().nth(keep).map_or(text.len(), |(i, _)| i);
    format!("{}…", text[..end].trim_end())
}

/// The part of a row's name after its device part, without leading
/// spaces. A name set by a label in the lm-sensors configuration has no
/// device part, so this is the whole name.
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

/// The short name of the chip in a row's key (see [`chip_stem`]); for the
/// key `coretemp-isa-0000/temp1` it is `coretemp`.
pub fn chip_word(row: &Row) -> &str {
    chip_stem(&row.key)
}

/// The short name of a chip: the text before the first `-` or `/`,
/// without a final `_<digits>`.
pub fn chip_stem(chip: &str) -> &str {
    let chip = chip.split(['/', '-']).next().unwrap_or_default();
    match chip.rsplit_once('_') {
        Some((stem, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => chip,
    }
}

/// A line name that also names the chip: `<short chip name>: <reading
/// label>`, such as `coretemp: Core 0`.
pub fn chip_line_name(row: &Row) -> String {
    format!("{}: {}", chip_word(row), reading_label(row))
}

/// The part that a reading plays in the CPU group, the popup group that
/// collects the CPU's temperatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuRole {
    /// A sensor of a whole CPU package, with the package number: `Package
    /// id N` on Intel (`coretemp`); 0 for AMD's Tdie, or for Tctl when the
    /// chip has no Tdie. The lowest number gives the default line.
    Package(u64),
    /// A board controller's reading of the CPU. It gives the default line
    /// when there is no package sensor.
    Board,
    /// AMD's Tctl, the control value, on a chip that also reports Tdie, the
    /// real temperature. It never gives the default line.
    Control,
    /// Any other reading of the CPU's own chip, such as one core.
    Part,
}

/// The role of each lm-sensors temperature that belongs to the CPU group,
/// as (key, role) pairs; other readings are not listed. Intel (`coretemp`)
/// and AMD (`k10temp`, `zenpower`) chips give packages and parts, and a
/// platform-bus chip gives a board reading. The kernel label decides, so a
/// label set in the lm-sensors configuration changes nothing.
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

/// The number after `prefix`, when digits and nothing else follow it. A
/// number too large for `u64` becomes `u64::MAX`.
fn numbered(label: &str, prefix: &str) -> Option<u64> {
    let digits = label.strip_prefix(prefix)?;
    let all_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    all_digits.then(|| digits.parse().unwrap_or(u64::MAX))
}

/// Whether `label` holds "CPU" as a whole word, ignoring ASCII case.
/// Words end at any character that is not a letter or digit, so "CPU
/// Temp" holds it and "CPUTIN" does not.
fn holds_cpu_word(label: &str) -> bool {
    label
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(CPU_NAME))
}

/// The class and family of a reading from any provider but liquidctl, or
/// `None` when the reading makes no row.
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
        // Power rows come from lm-sensors GPU chips only. Unlike for
        // temperatures, a chip that also has a drive word still counts.
        Kind::Power => {
            if provider != Provider::LmSensors || !is_gpu_chip(chip) {
                return None;
            }
            Some((Class::Power, None))
        }
    }
}

/// Whether `chip` contains one of [`GPU_WORDS`]; case matters.
fn is_gpu_chip(chip: &str) -> bool {
    GPU_WORDS.iter().any(|w| chip.contains(w))
}

/// Whether `chip` contains one of [`DRIVE_WORDS`]; case matters.
fn is_drive_chip(chip: &str) -> bool {
    DRIVE_WORDS.iter().any(|w| chip.contains(w))
}

/// The family of an lm-sensors temperature. `None` for a chip with both a
/// GPU word and a drive word: such a reading makes no row.
fn chip_family(chip: &str) -> Option<Family> {
    match (is_gpu_chip(chip), is_drive_chip(chip)) {
        (true, true) => None,
        (true, false) => Some(Family::Gpu),
        (false, true) => Some(Family::Drive),
        (false, false) => Some(Family::General),
    }
}

/// The class and family of a liquidctl reading from its unit, compared
/// exactly; `None` for any other unit. liquidctl temperatures are general.
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

/// The device part of a liquidctl device description: the first word, taken
/// to be the manufacturer, is removed, and so is everything from the first " (" on.
fn device_part(description: &str) -> &str {
    // Remove everything up to and including the first space that is not
    // the first character.
    let rest = match description.char_indices().find(|&(i, c)| i > 0 && c == ' ') {
        Some((i, _)) => &description[i + 1..],
        None => description,
    };
    // Cut at the first " (", but only when at least one character
    // follows it.
    match rest.find(" (") {
        Some(i) if i + 2 < rest.len() => &rest[..i],
        _ => rest,
    }
}

/// The item part of a liquidctl item name: the leftmost of
/// [`LIQUIDCTL_ITEM_WORDS`] is removed, once, so "Fan 1 speed" becomes
/// "Fan 1".
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

/// The identity of each row, in the same order.
pub fn identities(rows: &[Row]) -> Vec<String> {
    rows.iter().map(identity).collect()
}

/// A row's identity, `<class word>/<provider word>/<key>`, such as
/// `temperature/lm-sensors/coretemp-isa-0000/temp1`. It is the text that
/// the pinned list stores.
fn identity(row: &Row) -> String {
    [row.class.word(), row.provider.word(), row.key.as_str()].join(IDENTITY_SEPARATOR)
}

/// The class of a pinned entry, from its text: the class word that starts
/// an identity. `@maximum` entries and the older `@cpu` pin are
/// temperatures. `None` when the first part is no class word.
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
