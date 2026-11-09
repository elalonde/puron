//! What the popup shows. [`shown`] turns a snapshot's readings into
//! lines: temperatures grouped by device, each device under a heading,
//! then fans, voltages and power. [`menu_with`] lays those lines out as
//! popup entries. The module returns no translated text: the words and
//! headings that need a translation are values (`LineWord`,
//! `Heading::Cpu`) for the popup to translate. It keeps no state; the
//! caller passes back the previous layout's `Key`, which tells whether
//! the popup must be rebuilt.

use crate::format::{format, Text};
use crate::order;
use crate::rows::{
    self, Class, CpuRole, Family, Provider, RawReading, Row, ShownClasses, IDENTITY_SEPARATOR,
    MAXIMUM_WORD,
};
use crate::settings::Settings;
use crate::summary;

/// The project homepage. The no-readings message and the homepage link
/// at the bottom of the popup open it.
pub const HOMEPAGE_URL: &str = "https://elalonde.dev/software/puron/";

/// A word shown after a line's name, which the popup translates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineWord {
    /// A device's Composite reading (see `rows::composites`).
    Composite,
    /// A CPU package reading whose name is a label from the lm-sensors
    /// configuration.
    Package,
    /// The temperature of an NVIDIA or udisks2 device, or an NVMe reading
    /// whose name has no device part; such a line has no name of its own.
    Temperature,
    /// The highest temperature of a device.
    Maximum,
    /// The line of a stored older pin (`@maximum` or `@cpu`) that waits
    /// for lm-sensors readings. The word is the line's whole text; the
    /// line is in the CPU group.
    Waiting,
}

/// One line of the popup: a reading, or the Maximum line of a device,
/// which shows the highest of its temperatures.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownLine {
    pub class: Class,
    pub family: Option<Family>,
    pub identity: String,
    /// The name shown in the popup, without its word.
    pub name: String,
    /// A word shown after the name, with one space.
    pub word: Option<LineWord>,
    /// The value after the temperature rules of `summary`; `None` is "no
    /// value".
    pub value: Option<f64>,
    /// Whether this is its device's default line, the one that a new
    /// install pins (see `Shown::new_install_pins`).
    pub default: bool,
}

impl ShownLine {
    pub fn is_maximum(&self) -> bool {
        self.word == Some(LineWord::Maximum)
    }
}

/// The CPU group's heading before translation.
pub const CPU_HEADING: &str = rows::CPU_NAME;

/// The heading above a device's temperatures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Heading {
    /// The CPU group; the popup shows a translation of [`CPU_HEADING`].
    Cpu,
    /// A device name, shown as it is.
    Name(String),
}

/// The temperature lines of one device, under one heading.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub heading: Heading,
    pub lines: Vec<ShownLine>,
}

/// Every line that the popup can show for one snapshot, before layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// The temperatures, in popup order.
    pub devices: Vec<Device>,
    /// The fans, the voltages and the power readings, in popup order.
    pub others: Vec<ShownLine>,
    /// Identities of NVMe readings left out as repeats of a reading of
    /// lm-sensors, each with the identity of the line that stays.
    pub repeats: Vec<(String, String)>,
}

impl Shown {
    /// Every line, in popup order.
    pub fn lines(&self) -> impl Iterator<Item = &ShownLine> {
        let temperatures = self.devices.iter().flat_map(|d| &d.lines);
        temperatures.chain(&self.others)
    }

    /// The line of `identity`.
    pub fn line(&self, identity: &str) -> Option<&ShownLine> {
        self.lines().find(|l| l.identity == identity)
    }

    /// Where the line of `identity` is in popup order.
    pub fn place(&self, identity: &str) -> Option<usize> {
        self.lines().position(|l| l.identity == identity)
    }

    /// The pins of a new install: every default line, or, when no device
    /// has one, the first temperature line. Empty when there is no
    /// temperature line.
    pub fn new_install_pins(&self) -> Vec<String> {
        let defaults = self.devices.iter().flat_map(|d| &d.lines);
        let defaults: Vec<String> = defaults
            .filter(|l| l.default)
            .map(|l| l.identity.clone())
            .collect();
        if defaults.is_empty() {
            self.first_temperature().into_iter().collect()
        } else {
            defaults
        }
    }

    /// The line that stands for the whole machine, where a stored
    /// `@maximum` pin moves: the first device's default line, or its first
    /// line when it has none (a device with a single reading has none).
    /// The CPU group, when there is one, is the first device.
    pub fn default_line(&self) -> Option<&str> {
        let first = self.devices.first()?;
        let line = first.lines.iter().find(|l| l.default);
        line.or(first.lines.first()).map(|l| l.identity.as_str())
    }

    fn first_temperature(&self) -> Option<String> {
        let line = self.devices.first()?.lines.first()?;
        Some(line.identity.clone())
    }
}

/// The latest state of lm-sensors, as the engine reports it. It decides
/// which message the popup shows when there are no lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    NoneYet,
    Unavailable,
    Answered,
}

/// The message that the popup shows when there are no lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// lm-sensors is switched on and has not reported that it is
    /// unavailable: the message suggests running `sensors-detect`.
    SensorsDetect,
    /// lm-sensors is switched off or unavailable: the message suggests
    /// installing it.
    Install,
}

/// A summary of the popup's content: the message kind, or the identities
/// of its lines in order. The caller passes it back as `last_key`; while
/// it is unchanged, the popup can be updated in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Message(MessageKind),
    Identities(Vec<String>),
}

/// Whether the popup is rebuilt or updated in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Rebuild,
    InPlace,
}

/// The icon of a line, by class and, for temperatures, by family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    GeneralTemperature,
    GpuTemperature,
    DriveTemperature,
    Fan,
    Voltage,
    Power,
}

/// A section of the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Temperature,
    Fan,
    Voltage,
    Power,
}

/// A line of the popup layout, with its value as text; `value` is `None`
/// when the line has no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub identity: String,
    /// The name, without its word. It is not yet shortened: the popup
    /// shortens it to fit.
    pub name: String,
    /// A word shown after the name, with one space.
    pub word: Option<LineWord>,
    pub default: bool,
    pub value: Option<Text>,
}

/// One entry of the popup layout (`Menu::entries`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Section(Section),
    Device(Heading),
    Line(Item),
    Separator,
    /// The message shown when there are no lines, with the homepage.
    Message {
        kind: MessageKind,
        url: &'static str,
    },
    HomepageLink {
        url: &'static str,
    },
    SettingsLink,
}

/// The popup layout that [`menu`] and [`menu_with`] return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub entries: Vec<Entry>,
    /// The lines' identities, in popup order.
    pub identities: Vec<String>,
    pub key: Key,
    pub update: Update,
}

/// Every line that the popup can show for a snapshot's raw readings: the
/// rows of the classes that `settings` shows, in display order, with the
/// temperatures grouped by device.
pub fn shown(raw: &[RawReading], settings: &Settings) -> Shown {
    let s = settings;
    // The settings' class switches decide which classes make rows.
    let classes = ShownClasses {
        temperature: s.show_temperature,
        fan: s.show_rotation_rate,
        voltage: s.show_voltage,
        power: s.show_power,
    };
    let ordered = order::order(rows::rows(raw, &classes));
    let identities = rows::identities(&ordered);
    // `summary` applies the value rules (a temperature below zero has no
    // value) and keeps the input order, so the identities still line up.
    let pairs: Vec<(Row, String)> = summary::summary(ordered)
        .into_iter()
        .zip(identities)
        .collect();
    let (temperatures, others): (Vec<_>, Vec<_>) = pairs
        .into_iter()
        .partition(|(row, _)| row.class == Class::Temperature);
    let others = others
        .into_iter()
        .map(|(row, identity)| ShownLine {
            class: row.class,
            family: row.family,
            name: rows::shown_name(&row),
            identity,
            word: None,
            value: row.value,
            default: false,
        })
        .collect();
    Shown {
        devices: devices(temperatures, raw),
        others,
        repeats: rows::repeats(raw),
    }
}

/// The popup group of a temperature: the CPU group, or one device of a
/// provider, named by its chip or by its key.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Group {
    Cpu,
    Device(Provider, String),
}

impl Group {
    fn of(row: &Row, roles: &[(String, CpuRole)], raw: &[RawReading]) -> Group {
        if role(row, roles).is_some() {
            return Group::Cpu;
        }
        let key = row.key.as_str();
        let device = match row.provider {
            Provider::LmSensors => raw
                .iter()
                .find(|r| r.provider == Provider::LmSensors && r.key == row.key)
                .and_then(|r| r.chip.as_deref())
                .unwrap_or(key),
            // The whole key names the device. A udisks2 key can itself
            // contain `/`, so it is not split.
            Provider::Nvidia | Provider::Udisks2 => key,
            Provider::Liquidctl | Provider::NvmeCli => key
                .rsplit_once(IDENTITY_SEPARATOR)
                .map_or(key, |(device, _)| device),
        };
        Group::Device(row.provider, device.to_string())
    }

    /// The identity of the group's Maximum line: `@maximum/cpu` for the
    /// CPU group, `@maximum/<provider>/<device>` for any other. It depends
    /// only on the group, so a pin on it holds while the readings change.
    fn maximum_identity(&self) -> String {
        match self {
            Group::Cpu => format!("{MAXIMUM_WORD}{IDENTITY_SEPARATOR}cpu"),
            Group::Device(provider, device) => {
                [MAXIMUM_WORD, provider.word(), device].join(IDENTITY_SEPARATOR)
            }
        }
    }
}

/// The CPU role of an lm-sensors row; `None` for any other row.
fn role(row: &Row, roles: &[(String, CpuRole)]) -> Option<CpuRole> {
    if row.provider != Provider::LmSensors {
        return None;
    }
    roles
        .iter()
        .find(|(key, _)| *key == row.key)
        .map(|&(_, role)| role)
}

/// Groups the temperature rows by device, in popup order: the CPU group,
/// then GPU devices, drive devices and all other devices, each kind sorted
/// by heading in name order. The family of a device's first row decides
/// its kind.
fn devices(temperatures: Vec<(Row, String)>, raw: &[RawReading]) -> Vec<Device> {
    let roles = rows::cpu_roles(raw);
    let composites = rows::composites(raw);
    let mut groups: Vec<(Group, Vec<(Row, String)>)> = Vec::new();
    for pair in temperatures {
        let group = Group::of(&pair.0, &roles, raw);
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, members)) => members.push(pair),
            None => groups.push((group, vec![pair])),
        }
    }
    let mut devices: Vec<(u8, Device)> = groups
        .into_iter()
        .map(|(group, members)| {
            let kind = match (&group, members[0].0.family) {
                (Group::Cpu, _) => 0,
                (_, Some(Family::Gpu)) => 1,
                (_, Some(Family::Drive)) => 2,
                _ => 3,
            };
            let device = match group {
                Group::Cpu => cpu_device(members, &roles),
                _ => other_device(&group, members, &composites),
            };
            (kind, device)
        })
        .collect();
    devices.sort_by(|(ka, a), (kb, b)| {
        ka.cmp(kb).then_with(|| match (&a.heading, &b.heading) {
            (Heading::Name(a), Heading::Name(b)) => order::name_order(a, b),
            _ => std::cmp::Ordering::Equal,
        })
    });
    devices.into_iter().map(|(_, d)| d).collect()
}

/// The line of a temperature row, given with its identity in `pair`.
fn line(pair: &(Row, String), name: String, word: Option<LineWord>, default: bool) -> ShownLine {
    let (row, identity) = pair;
    ShownLine {
        class: row.class,
        family: row.family,
        identity: identity.clone(),
        name,
        word,
        value: row.value,
        default,
    }
}

/// The Maximum line of a group: the highest value of its members that
/// counts (see `summary::highest`). Only a group of two or more readings
/// has one.
fn maximum_line(group: &Group, members: &[(Row, String)], default: bool) -> Option<ShownLine> {
    (members.len() >= 2).then(|| ShownLine {
        class: Class::Temperature,
        family: members[0].0.family,
        identity: group.maximum_identity(),
        name: String::new(),
        word: Some(LineWord::Maximum),
        value: summary::highest(members.iter().map(|(row, _)| row.value)),
        default,
    })
}

/// The CPU group. Its lines: the default line, then the other readings of
/// the whole CPU (packages, Tctl and the board's reading), then the rest
/// (cores and other parts), each set in row order, then the Maximum line.
/// The default line is the lowest-numbered package, else the board's
/// reading. A group with one reading has no default line; in a larger
/// group with neither, the Maximum line is the default.
fn cpu_device(members: Vec<(Row, String)>, roles: &[(String, CpuRole)]) -> Device {
    let role_of = |pair: &(Row, String)| role(&pair.0, roles);
    let package = |pair: &(Row, String)| match role_of(pair) {
        Some(CpuRole::Package(n)) => Some(n),
        _ => None,
    };
    let lowest = (0..members.len())
        .filter(|&i| package(&members[i]).is_some())
        .min_by_key(|&i| package(&members[i]));
    let board = (0..members.len()).find(|&i| role_of(&members[i]) == Some(CpuRole::Board));
    let default_at = lowest.or(board).filter(|_| members.len() >= 2);
    let mut places: Vec<usize> = (0..members.len()).collect();
    places.sort_by_key(|&i| {
        let tier = match role_of(&members[i]) {
            _ if Some(i) == default_at => 0,
            Some(CpuRole::Part) => 2,
            _ => 1,
        };
        (tier, i)
    });
    let mut lines: Vec<ShownLine> = places
        .into_iter()
        .map(|i| {
            let row = &members[i].0;
            let configured = row.device_chars == 0;
            let word = (configured && package(&members[i]).is_some()).then_some(LineWord::Package);
            line(
                &members[i],
                rows::chip_line_name(row),
                word,
                Some(i) == default_at,
            )
        })
        .collect();
    lines.extend(maximum_line(&Group::Cpu, &members, default_at.is_none()));
    Device {
        heading: Heading::Cpu,
        lines,
    }
}

/// The heading of a device: the configured label of its Composite
/// reading, if it has one; else the device part of the first name that
/// has one; else, for a single reading, that reading's name; else the
/// short name of the chip (see `rows::chip_stem`).
fn heading_of(group: &Group, members: &[(Row, String)], composite: Option<usize>) -> String {
    let configured = composite
        .map(|i| &members[i].0)
        .filter(|r| r.device_chars == 0);
    if let Some(row) = configured {
        return row.name.clone();
    }
    if let Some(row) = members.iter().map(|(r, _)| r).find(|r| r.device_chars > 0) {
        return rows::device_name(row);
    }
    match (members, group) {
        ([(row, _)], _) => row.name.clone(),
        (_, Group::Device(_, chip)) => rows::chip_stem(chip).to_string(),
        (_, Group::Cpu) => rows::CPU_NAME.to_string(),
    }
}

/// The group of a device other than the CPU. Its lines: the default line
/// (its Composite reading, else its first reading), the others in row
/// order, then the Maximum line. A device with one reading has no default
/// line.
fn other_device(group: &Group, members: Vec<(Row, String)>, composites: &[String]) -> Device {
    let composite = members
        .iter()
        .position(|(row, _)| row.provider == Provider::LmSensors && composites.contains(&row.key));
    let first = composite.unwrap_or(0);
    let several = members.len() >= 2;
    let name_of = |at: usize| {
        let row = &members[at].0;
        let configured = row.device_chars == 0;
        if Some(at) == composite {
            let label = if configured {
                row.name.clone()
            } else {
                String::new()
            };
            return (label, Some(LineWord::Composite));
        }
        match row.provider {
            Provider::Nvidia | Provider::Udisks2 => (String::new(), Some(LineWord::Temperature)),
            Provider::NvmeCli if configured => (String::new(), Some(LineWord::Temperature)),
            Provider::LmSensors if configured && !several && row.family != Some(Family::Drive) => {
                (rows::chip_line_name(row), None)
            }
            _ => (rows::reading_label(row).to_string(), None),
        }
    };
    let mut places: Vec<usize> = (0..members.len()).collect();
    places.sort_by_key(|&i| (i != first, i));
    let mut lines: Vec<ShownLine> = places
        .into_iter()
        .map(|i| {
            let (name, word) = name_of(i);
            line(&members[i], name, word, several && i == first)
        })
        .collect();
    lines.extend(maximum_line(group, &members, false));
    Device {
        heading: Heading::Name(heading_of(group, &members, composite)),
        lines,
    }
}

/// The popup layout when no older pin waits; see [`menu_with`].
pub fn menu(
    shown: &Shown,
    settings: &Settings,
    report: Report,
    last_key: Option<&Key>,
    forced: bool,
) -> Menu {
    menu_with(shown, settings, report, last_key, forced, &[])
}

/// The line of an older pin that waits for lm-sensors. It has no value and no
/// name: the popup draws its whole text from the word.
fn waiting_item(pin: &str) -> Item {
    Item {
        identity: pin.to_string(),
        name: String::new(),
        word: Some(LineWord::Waiting),
        default: false,
        value: None,
    }
}

/// The popup layout, with a line for each pin of `waiting`, the stored
/// older pins that wait for lm-sensors: at the end of the CPU group, or in
/// a CPU group of their own before the other devices. Each such line can
/// be clicked like any other, so the pin can be removed while it waits.
/// `last_key` is the `key` of the previous layout, `None` before the
/// first. `forced` asks for a rebuild even when the key is unchanged.
pub fn menu_with(
    shown: &Shown,
    settings: &Settings,
    report: Report,
    last_key: Option<&Key>,
    forced: bool,
    waiting: &[String],
) -> Menu {
    let s = settings;
    let mut entries = Vec::new();
    let mut groups: Vec<(Heading, Vec<Item>)> = shown
        .devices
        .iter()
        .map(|d| {
            (
                d.heading.clone(),
                d.lines.iter().map(|l| item(l, s)).collect(),
            )
        })
        .collect();
    if !waiting.is_empty() {
        let lines = waiting.iter().map(|p| waiting_item(p));
        match groups.iter_mut().find(|(h, _)| *h == Heading::Cpu) {
            Some((_, items)) => items.extend(lines),
            None => groups.insert(0, (Heading::Cpu, lines.collect())),
        }
    }
    let identities: Vec<String> = groups
        .iter()
        .flat_map(|(_, items)| items.iter().map(|i| i.identity.clone()))
        .chain(shown.others.iter().map(|l| l.identity.clone()))
        .collect();
    let key = if identities.is_empty() {
        // lm-sensors is ready when it is switched on and has not reported
        // that it is unavailable; before its first report it counts as ready.
        let ready = s.provider_lm_sensors && report != Report::Unavailable;
        let kind = if ready {
            MessageKind::SensorsDetect
        } else {
            MessageKind::Install
        };
        entries.push(Entry::Message {
            kind,
            url: HOMEPAGE_URL,
        });
        Key::Message(kind)
    } else {
        if !groups.is_empty() {
            entries.push(Entry::Section(Section::Temperature));
            for (heading, items) in groups {
                entries.push(Entry::Device(heading));
                entries.extend(items.into_iter().map(Entry::Line));
            }
        }
        for (section, class) in [
            (Section::Fan, Class::Fan),
            (Section::Voltage, Class::Voltage),
            (Section::Power, Class::Power),
        ] {
            let mut lines = shown.others.iter().filter(|l| l.class == class).peekable();
            if lines.peek().is_some() {
                if !entries.is_empty() {
                    entries.push(Entry::Separator);
                }
                entries.push(Entry::Section(section));
                entries.extend(lines.map(|l| Entry::Line(item(l, s))));
            }
        }
        Key::Identities(identities.clone())
    };
    // The footer, after the sensor entries or the message.
    entries.extend([
        Entry::Separator,
        Entry::HomepageLink { url: HOMEPAGE_URL },
        Entry::SettingsLink,
    ]);
    // Rebuild when forced or when the key has changed; the key is unchanged
    // for the same message kind, or the same identities in the same order.
    let update = if forced || last_key != Some(&key) {
        Update::Rebuild
    } else {
        Update::InPlace
    };
    Menu {
        entries,
        identities,
        key,
        update,
    }
}

/// The layout item of a line, with its value formatted under `settings`.
fn item(line: &ShownLine, settings: &Settings) -> Item {
    Item {
        identity: line.identity.clone(),
        name: line.name.clone(),
        word: line.word,
        default: line.default,
        value: format(line.class, line.value, settings),
    }
}

/// The icon kind of a line; the panel uses it.
pub fn icon_kind(line: &ShownLine) -> IconKind {
    match (line.class, line.family) {
        (Class::Temperature, Some(Family::Gpu)) => IconKind::GpuTemperature,
        (Class::Temperature, Some(Family::Drive)) => IconKind::DriveTemperature,
        (Class::Temperature, _) => IconKind::GeneralTemperature,
        (Class::Fan, _) => IconKind::Fan,
        (Class::Voltage, _) => IconKind::Voltage,
        (Class::Power, _) => IconKind::Power,
    }
}
