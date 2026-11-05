//! The Maximum temperature, and which temperatures count.

use crate::rows::{Class, Row};

/// Output of `summary`. For `maximum` and `header`: `None` is
/// absent, `Some(None)` is "no value".
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The input rows in input order; a temperature that does not count
    /// has "no value".
    pub rows: Vec<Row>,
    pub maximum: Option<Option<f64>>,
    /// The temperature group header value.
    pub header: Option<Option<f64>>,
}

/// Whether a temperature value counts: finite and not below zero.
pub fn counts(value: f64) -> bool {
    // Negative zero is not below zero; NaN fails both tests.
    value.is_finite() && value >= 0.0
}

/// The summary of the shown rows, in any order.
pub fn summary(rows: Vec<Row>) -> Summary {
    let mut any_temperature = false;
    let mut highest: Option<f64> = None;
    let rows: Vec<Row> = rows
        .into_iter()
        .map(|mut row| {
            if row.class == Class::Temperature {
                any_temperature = true;
                match row.value {
                    Some(v) if counts(v) => {
                        // IEEE total order: 0 is above negative zero.
                        if highest.is_none_or(|h| v.total_cmp(&h).is_gt()) {
                            highest = Some(v);
                        }
                    }
                    // Below zero, not finite, or no value.
                    _ => row.value = None,
                }
            }
            row
        })
        .collect();
    let maximum = any_temperature.then_some(highest);
    Summary {
        rows,
        maximum,
        header: maximum,
    }
}
