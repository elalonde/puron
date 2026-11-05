//! Popup view model. No text, no state: the caller passes the last key
//! back in.

use crate::format::{format, Text};
use crate::rows::{self, Class, Family, RawReading, ShownClasses, MAXIMUM_IDENTITY};
use crate::settings::Settings;
use crate::{order, summary};

/// Group thresholds: the least number of rows that forms a group.
pub const TEMPERATURE_GROUP_THRESHOLD: usize = 1;
pub const FAN_VOLTAGE_GROUP_THRESHOLD: usize = 2;
/// The page the message and the homepage link open.
pub const HOMEPAGE_URL: &str = "https://elalonde.dev/software/puron/";

/// One shown row.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownRow {
    pub class: Class,
    pub family: Option<Family>,
    pub name: String,
    pub identity: String,
    /// The value after `summary`; `None` is "no value".
    pub value: Option<f64>,
}

/// The shown rows. For `maximum` and `header`: `None` is
/// absent, `Some(None)` is "no value".
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub rows: Vec<ShownRow>,
    pub maximum: Option<Option<f64>>,
    pub header: Option<Option<f64>>,
}

/// The outcome of the latest lm-sensors report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    NoneYet,
    Unavailable,
    Answered,
}

/// Message kind, shown when there are no rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    SensorsDetect,
    Install,
}

/// What the popup shows, as a key; the caller passes it back as `last_key`.
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

/// Icon kind of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    GeneralTemperature,
    GpuTemperature,
    DriveTemperature,
    Fan,
    Voltage,
    Power,
    Maximum,
}

/// Group kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Temperature,
    Fan,
    Voltage,
}

/// A sensor row or the Maximum row; `value` `None` is "no value".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub identity: String,
    /// Empty for the Maximum row (its name is an `i18n` entry).
    pub name: String,
    pub icon: IconKind,
    pub value: Option<Text>,
}

/// One entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Sensor(Item),
    Maximum(Item),
    /// `header`: `None` for fan and voltage groups; `Some(None)` is "no value".
    Group {
        kind: GroupKind,
        header: Option<Option<Text>>,
        members: Vec<Item>,
    },
    Separator,
    Message {
        kind: MessageKind,
        url: &'static str,
    },
    HomepageLink {
        url: &'static str,
    },
    SettingsLink,
}

/// The output of `menu`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub entries: Vec<Entry>,
    pub identities: Vec<String>,
    pub key: Key,
    pub update: Update,
}

/// The shown rows of a snapshot's raw readings.
pub fn shown(raw: &[RawReading], settings: &Settings) -> Shown {
    let s = settings;
    // Step 1: the Show Sensors switches are the shown classes.
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
    let summary = summary::summary(ordered);
    let rows = summary
        .rows
        .into_iter()
        .zip(identities)
        .map(|(row, identity)| ShownRow {
            class: row.class,
            family: row.family,
            name: row.name,
            identity,
            value: row.value,
        })
        .collect();
    Shown {
        rows,
        maximum: summary.maximum,
        header: summary.header,
    }
}

/// The menu layout. `last_key` `None` is "none".
pub fn menu(
    shown: &Shown,
    settings: &Settings,
    report: Report,
    last_key: Option<&Key>,
    forced: bool,
) -> Menu {
    let s = settings;
    let of = |class: Class| -> Vec<&ShownRow> {
        shown.rows.iter().filter(|r| r.class == class).collect()
    };
    let (t, f, v, p) = (
        of(Class::Temperature),
        of(Class::Fan),
        of(Class::Voltage),
        of(Class::Power),
    );
    let mut entries = Vec::new();
    let key = if t.is_empty() && f.is_empty() && v.is_empty() && p.is_empty() {
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
        // Temperatures, Maximum, fans, voltages, then power.
        let header = shown.header.map(|h| format(Class::Temperature, h, s));
        group(
            &mut entries,
            &t,
            GroupKind::Temperature,
            s.group_temperature,
            TEMPERATURE_GROUP_THRESHOLD,
            header,
            s,
        );
        if !t.is_empty() {
            entries.push(Entry::Separator);
            let value = format(Class::Temperature, shown.maximum.flatten(), s);
            entries.push(Entry::Maximum(Item {
                identity: MAXIMUM_IDENTITY.to_string(),
                name: String::new(),
                icon: IconKind::Maximum,
                value,
            }));
            if !f.is_empty() || !v.is_empty() {
                entries.push(Entry::Separator);
            }
        }
        group(
            &mut entries,
            &f,
            GroupKind::Fan,
            s.group_rotation_rate,
            FAN_VOLTAGE_GROUP_THRESHOLD,
            None,
            s,
        );
        if !f.is_empty() && !v.is_empty() {
            entries.push(Entry::Separator);
        }
        group(
            &mut entries,
            &v,
            GroupKind::Voltage,
            s.group_voltage,
            FAN_VOLTAGE_GROUP_THRESHOLD,
            None,
            s,
        );
        // Power is never grouped and has no separator before it.
        entries.extend(p.iter().map(|r| Entry::Sensor(item(r, s))));
        Key::Identities(identities(&entries))
    };
    // The footer, after the sensor entries or the message.
    entries.extend([
        Entry::Separator,
        Entry::HomepageLink { url: HOMEPAGE_URL },
        Entry::SettingsLink,
    ]);
    let identities = match &key {
        Key::Identities(ids) => ids.clone(),
        Key::Message(_) => Vec::new(),
    };
    // Rebuild unless the key is unchanged: the same message kind, or the
    // same identities in the same order.
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

/// Puts `rows` into one group of `kind` when `on` and at least
/// `threshold` rows, else directly.
fn group(
    entries: &mut Vec<Entry>,
    rows: &[&ShownRow],
    kind: GroupKind,
    on: bool,
    threshold: usize,
    header: Option<Option<Text>>,
    settings: &Settings,
) {
    let members = rows.iter().map(|r| item(r, settings));
    if on && rows.len() >= threshold {
        entries.push(Entry::Group {
            kind,
            header,
            members: members.collect(),
        });
    } else {
        entries.extend(members.map(Entry::Sensor));
    }
}

fn item(row: &ShownRow, settings: &Settings) -> Item {
    Item {
        identity: row.identity.clone(),
        name: row.name.clone(),
        icon: icon_kind(row),
        value: format(row.class, row.value, settings),
    }
}

/// The icon kind of a shown row; the panel uses it too.
pub(crate) fn icon_kind(row: &ShownRow) -> IconKind {
    match (row.class, row.family) {
        (Class::Temperature, Some(Family::Gpu)) => IconKind::GpuTemperature,
        (Class::Temperature, Some(Family::Drive)) => IconKind::DriveTemperature,
        (Class::Temperature, _) => IconKind::GeneralTemperature,
        (Class::Fan, _) => IconKind::Fan,
        (Class::Voltage, _) => IconKind::Voltage,
        (Class::Power, _) => IconKind::Power,
    }
}

/// Identities of the sensor rows and the Maximum row, in entry order,
/// group members in their place.
fn identities(entries: &[Entry]) -> Vec<String> {
    let mut ids = Vec::new();
    for e in entries {
        match e {
            Entry::Sensor(i) | Entry::Maximum(i) => ids.push(i.identity.clone()),
            Entry::Group { members, .. } => ids.extend(members.iter().map(|i| i.identity.clone())),
            _ => {}
        }
    }
    ids
}
