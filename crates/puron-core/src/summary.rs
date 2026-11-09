//! Value rules for temperatures. A temperature value counts when it is
//! finite and not below zero. A temperature that does not count is shown
//! with no value, and it is left out of a device's highest value.

use crate::rows::{Class, Row};

/// Whether a temperature value counts: it is finite and not below zero.
pub fn counts(value: f64) -> bool {
    // -0.0 >= 0.0 holds, so negative zero counts; NaN fails both tests.
    value.is_finite() && value >= 0.0
}

/// Returns `rows` in the same order, with the value of each temperature
/// row that does not count set to `None` ("no value"). Rows of the other
/// classes are left as they are.
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

/// The highest of `values` that counts, or `None` when none counts.
/// `None` entries, readings with no value, are skipped.
pub fn highest(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    values
        .into_iter()
        .flatten()
        .filter(|v| counts(*v))
        // `total_cmp` puts 0.0 above -0.0, so when both occur the result is
        // 0.0, whatever their order.
        .fold(None, |best: Option<f64>, v| {
            if best.is_none_or(|b| v.total_cmp(&b).is_gt()) {
                Some(v)
            } else {
                best
            }
        })
}
