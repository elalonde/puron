//! Pin and unpin rules. Stores nothing, keeps no state.

use crate::menu::{Heading, Shown, ShownLine};
use crate::rows::{
    class_of_identity, Class, Provider, ShownClasses, IDENTITY_SEPARATOR, MAXIMUM_WORD, OLD_CPU_PIN,
};

/// The new pinned list after a click.
pub fn click(
    pinned: &[String],
    clicked: &str,
    menu_identities: &[String],
    shown: &ShownClasses,
) -> Vec<String> {
    // Step 1: toggle. Unpin removes the clicked identity's own entry.
    let mut list = pinned.to_vec();
    if list.iter().any(|e| e == clicked) {
        list.retain(|e| e != clicked);
    } else {
        list.push(clicked.to_string());
    }
    // Step 2: drop entries that are not in the menu, but keep entries of
    // a hidden class and the older pins that wait to move.
    list.retain(|e| {
        menu_identities.contains(e)
            || waits(e)
            || class_of_identity(e).is_some_and(|c| !shown.contains(c))
    });
    // Step 3: once each.
    once_each(list)
}

/// The in-memory list at start: unknown identities kept,
/// a repeated identity keeps its first entry.
pub fn start(stored: &[String]) -> Vec<String> {
    once_each(stored.to_vec())
}

/// The list after older pins move to the lines of `shown`, each in its place,
/// then once each: a pin on a repeat of an lm-sensors reading moves to the
/// line that stays. Once `ready`, and when a temperature line exists, a stored
/// Maximum pin moves to the CPU's default line, and a stored CPU pin to the
/// line that `cpu_source` names, else to the default line.
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

/// The line of what a development build's CPU row read: its Maximum line or a
/// sensor key.
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

/// Whether `entry` is an older pin that waits for readings to move to a line:
/// version 1.0.0's Maximum or a development build's CPU pin.
pub fn waits(entry: &str) -> bool {
    entry == MAXIMUM_WORD || entry == OLD_CPU_PIN
}

/// Whether `identity` is pinned: exact text match.
pub fn is_pinned(pinned: &[String], identity: &str) -> bool {
    pinned.iter().any(|e| e == identity)
}

/// The first entry of each identity, in list order.
fn once_each(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for e in list {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}
