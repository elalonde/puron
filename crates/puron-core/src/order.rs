//! The display order of rows. It does not depend on the locale: names
//! compare by Unicode simple case folding (see [`name_order`]).

use std::cmp::Ordering;

use crate::fold;
use crate::rows::{Class, Family, Provider, Row};

/// The providers in rank order. Rows that tie on name, and GPU
/// temperatures, are ordered by this rank and then by `Row::position`.
pub const PROVIDER_RANK: [Provider; 5] = [
    Provider::LmSensors,
    Provider::Liquidctl,
    Provider::Nvidia,
    Provider::Udisks2,
    Provider::NvmeCli,
];

/// The Unicode version of the case-folding table behind name order.
pub const FOLD_UNICODE_VERSION: (u8, u8, u8) = fold::UNICODE_VERSION;

/// Sorts `rows` into display order (see [`compare`]). No row is added or
/// dropped.
pub fn order(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(compare);
    rows
}

/// Which of two rows comes first in display order: first by part (GPU,
/// general and drive temperatures, fans, voltages, power), then by name
/// (see [`name_order`]), then by provider rank and position. GPU
/// temperatures skip the name step and keep provider order.
pub fn compare(a: &Row, b: &Row) -> Ordering {
    let pa = part(a);
    pa.cmp(&part(b))
        .then_with(|| {
            if pa == 1 {
                Ordering::Equal
            } else {
                name_order(&a.name, &b.name)
            }
        })
        .then_with(|| provider_order(a, b))
}

/// The block of display order that a row falls in, 1 to 6: GPU, general
/// and drive temperatures, then fans, voltages and power.
fn part(row: &Row) -> u8 {
    match (row.class, row.family) {
        (Class::Temperature, Some(Family::Gpu)) => 1,
        (Class::Temperature, Some(Family::Drive)) => 3,
        // General temperatures. `rows::rows` gives every temperature row a
        // family, so `None` does not occur.
        (Class::Temperature, _) => 2,
        (Class::Fan, _) => 4,
        (Class::Voltage, _) => 5,
        (Class::Power, _) => 6,
    }
}

/// The last tie-breaker: provider rank (`PROVIDER_RANK`), then
/// `Row::position`, the reading's place in its provider's output.
fn provider_order(a: &Row, b: &Row) -> Ordering {
    rank(a.provider)
        .cmp(&rank(b.provider))
        .then(a.position.cmp(&b.position))
}

fn rank(provider: Provider) -> usize {
    PROVIDER_RANK
        .iter()
        .position(|&p| p == provider)
        .unwrap_or(PROVIDER_RANK.len())
}

/// Compares two names without regard to locale. The names are compared
/// first after simple case folding, by code point, a prefix first. When
/// the folded names are equal, the first position where one name has a
/// lowercase letter and the other an uppercase one decides: lowercase
/// first. `Equal` means neither step decides; `compare` then falls back
/// to provider order.
pub fn name_order(a: &str, b: &str) -> Ordering {
    // Compare the folded names by code point; a prefix sorts first.
    let folded = a.chars().map(fold_char).cmp(b.chars().map(fold_char));
    if folded != Ordering::Equal {
        return folded;
    }
    // The folded names are equal. Simple folding maps each character to
    // exactly one, so both names have the same length and can be zipped.
    // The first lowercase and uppercase pair decides.
    for (x, y) in a.chars().zip(b.chars()) {
        if x.is_lowercase() && y.is_uppercase() {
            return Ordering::Less;
        }
        if x.is_uppercase() && y.is_lowercase() {
            return Ordering::Greater;
        }
    }
    Ordering::Equal
}

/// The simple case folding of `c`: its mapping of status C or S in the
/// Unicode table in `fold`, or `c` itself when it has none.
fn fold_char(c: char) -> char {
    match fold::FOLD.binary_search_by_key(&c, |&(from, _)| from) {
        Ok(i) => fold::FOLD[i].1,
        Err(_) => c,
    }
}
