//! Pin and unpin rules. Stores nothing, keeps no state.

use crate::rows::{class_of_identity, ShownClasses};

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
    // a hidden class.
    list.retain(|e| {
        menu_identities.contains(e) || class_of_identity(e).is_some_and(|c| !shown.contains(c))
    });
    // Step 3: once each.
    once_each(list)
}

/// The in-memory list at start: unknown identities kept,
/// a repeated identity keeps its first entry.
pub fn start(stored: &[String]) -> Vec<String> {
    once_each(stored.to_vec())
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
