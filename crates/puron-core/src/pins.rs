//! The pinned list: the readings that the panel shows. Clicking a line in
//! the popup pins or unpins its reading. A pin is the reading's identity
//! (see `rows::identities`), or one of two older forms that earlier
//! versions stored: `@maximum`, the single Maximum row of version 1.0.0,
//! and `@cpu`, the CPU row of development builds. An older pin waits until
//! the readings it stands for appear, then moves to a current line (see
//! [`moved`]). These functions return a new list; storing it is the
//! caller's job.

use crate::menu::{Heading, Shown, ShownLine};
use crate::rows::{
    class_of_identity, Class, Provider, ShownClasses, IDENTITY_SEPARATOR, MAXIMUM_WORD, OLD_CPU_PIN,
};

/// The pinned list after the user clicks the line of `clicked`.
/// `menu_identities` are the identities of the popup's lines, and `shown`
/// the classes that the settings show.
pub fn click(
    pinned: &[String],
    clicked: &str,
    menu_identities: &[String],
    shown: &ShownClasses,
) -> Vec<String> {
    // Toggle: a pinned identity is removed, every copy of it; any other
    // is added at the end.
    let mut list = pinned.to_vec();
    if list.iter().any(|e| e == clicked) {
        list.retain(|e| e != clicked);
    } else {
        list.push(clicked.to_string());
    }
    // Drop pins that have no line in the popup, except pins of a class
    // that the settings hide and older pins that wait to move.
    list.retain(|e| {
        menu_identities.contains(e)
            || waits(e)
            || class_of_identity(e).is_some_and(|c| !shown.contains(c))
    });
    // Keep the first copy of each entry.
    once_each(list)
}

/// The pinned list at start-up, from the stored list. Every entry is
/// kept, even one that matches no reading yet; of a repeated entry, only
/// the first copy stays.
pub fn start(stored: &[String]) -> Vec<String> {
    once_each(stored.to_vec())
}

/// Moves older pins to the lines of `shown`, each pin keeping its place in
/// the list, then keeps only the first copy of each entry.
///
/// A pin on an NVMe reading that lm-sensors also reports moves to the
/// lm-sensors line that stays (see `Shown::repeats`), at any time. When
/// `ready` is true and the popup has a temperature line, a stored
/// `@maximum` pin moves to the default line (`Shown::default_line`), and a
/// stored `@cpu` pin moves to the CPU group's line that `cpu_source` names,
/// or to the default line when `cpu_source` is `None` or names no line.
/// Otherwise those two stay as they are.
pub fn moved(
    pinned: &[String],
    shown: &Shown,
    cpu_source: Option<&str>,
    ready: bool,
) -> Vec<String> {
    let default = ready.then(|| shown.default_line()).flatten();
    let list = pinned.iter().map(|pin| {
        if let Some((_, stays)) = shown.repeats.iter().find(|(repeat, _)| repeat == pin) {
            return stays.clone();
        }
        let Some(default) = default else {
            return pin.clone();
        };
        match pin.as_str() {
            MAXIMUM_WORD => default.to_string(),
            OLD_CPU_PIN => cpu_source
                .and_then(|source| cpu_source_line(shown, source))
                .unwrap_or_else(|| default.to_string()),
            _ => pin.clone(),
        }
    });
    once_each(list.collect())
}

/// The CPU group's line for `source`, the sensor that the CPU row of a
/// development build read: the group's Maximum line when `source` is
/// `@maximum`, else the line of the lm-sensors temperature whose key is
/// `source`. `None` when there is no CPU group or no such line in it.
fn cpu_source_line(shown: &Shown, source: &str) -> Option<String> {
    let cpu = shown.devices.iter().find(|d| d.heading == Heading::Cpu)?;
    let lm = Provider::LmSensors.word();
    let sensor = [Class::Temperature.word(), lm, source].join(IDENTITY_SEPARATOR);
    let named = |l: &&ShownLine| {
        if source == MAXIMUM_WORD {
            l.is_maximum()
        } else {
            l.identity == sensor
        }
    };
    cpu.lines.iter().find(named).map(|l| l.identity.clone())
}

/// Whether `entry` is one of the older pins, `@maximum` or `@cpu`, which
/// wait for readings before they move to a line.
pub fn waits(entry: &str) -> bool {
    entry == MAXIMUM_WORD || entry == OLD_CPU_PIN
}

/// Whether `identity` is in `pinned`, compared as exact text.
pub fn is_pinned(pinned: &[String], identity: &str) -> bool {
    pinned.iter().any(|e| e == identity)
}

/// `list` with only the first copy of each entry, in list order.
fn once_each(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for e in list {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}
