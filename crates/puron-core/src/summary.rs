//! Which temperatures count, and the highest of them.

use crate::rows::{Class, Row};

/// Whether a temperature value counts: finite and not below zero.
pub fn counts(value: f64) -> bool {
    // Negative zero is not below zero; NaN fails both tests.
    value.is_finite() && value >= 0.0
}

/// The rows in input order; a temperature that does not count has "no value".
pub fn summary(rows: Vec<Row>) -> Vec<Row> {
    rows.into_iter()
        .map(|mut row| {
            if row.class == Class::Temperature && !row.value.is_some_and(counts) {
                row.value = None;
            }
            row
        })
        .collect()
}

/// The highest of `values` that counts; `None` when none does.
pub fn highest(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    values
        .into_iter()
        .flatten()
        .filter(|v| counts(*v))
        // IEEE total order: 0 is above negative zero.
        .fold(None, |best: Option<f64>, v| {
            if best.is_none_or(|b| v.total_cmp(&b).is_gt()) {
                Some(v)
            } else {
                best
            }
        })
}
