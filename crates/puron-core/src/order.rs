//! The order of the rows. No I/O, no locale, no state.

use std::cmp::Ordering;

use crate::fold;
use crate::rows::{Class, Family, Provider, Row};

/// Provider rank, which also breaks ties in name order: first is rank 1.
pub const PROVIDER_RANK: [Provider; 5] = [
    Provider::LmSensors,
    Provider::Liquidctl,
    Provider::Nvidia,
    Provider::Udisks2,
    Provider::NvmeCli,
];

/// The Unicode version of the case-folding table behind name order.
pub const FOLD_UNICODE_VERSION: (u8, u8, u8) = fold::UNICODE_VERSION;

/// The rows in display order, each exactly once.
pub fn order(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| {
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
    });
    rows
}

/// The part of a row, 1 to 6: GPU, general and drive temperatures,
/// then fans, voltages and power.
fn part(row: &Row) -> u8 {
    match (row.class, row.family) {
        (Class::Temperature, Some(Family::Gpu)) => 1,
        (Class::Temperature, Some(Family::Drive)) => 3,
        // `rows` gives every temperature row a family.
        (Class::Temperature, _) => 2,
        (Class::Fan, _) => 4,
        (Class::Voltage, _) => 5,
        (Class::Power, _) => 6,
    }
}

/// Provider order: by provider rank, then by position.
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

/// Name order, steps 1 and 2; `Equal` leaves the rows to step 3, the
/// provider order.
pub fn name_order(a: &str, b: &str) -> Ordering {
    // Step 1: folded forms by code point; a prefix sorts first.
    let folded = a.chars().map(fold_char).cmp(b.chars().map(fold_char));
    if folded != Ordering::Equal {
        return folded;
    }
    // Step 2: simple folding maps one character to one, so the names
    // have the same length; the first case pair decides.
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

/// Simple case folding of one character (status C and S).
fn fold_char(c: char) -> char {
    match fold::FOLD.binary_search_by_key(&c, |&(from, _)| from) {
        Ok(i) => fold::FOLD[i].1,
        Err(_) => c,
    }
}
