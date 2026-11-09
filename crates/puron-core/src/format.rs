//! Value text: the number and unit that the popup and the panel show for
//! one value, under the display settings.

use crate::rows::Class;
use crate::settings::{Settings, TemperatureUnit};

/// The factor of the Celsius to Fahrenheit conversion: °F = °C ×
/// `FAHRENHEIT_FACTOR` + `FAHRENHEIT_OFFSET`.
pub const FAHRENHEIT_FACTOR: f64 = 1.8;
/// The offset of the Celsius to Fahrenheit conversion, added after the
/// multiplication.
pub const FAHRENHEIT_OFFSET: f64 = 32.0;
/// Decimal places of a temperature, indexed by `Settings::show_decimals`:
/// none when it is off, one when it is on.
pub const TEMPERATURE_DIGITS: [usize; 2] = [0, 1];
/// Decimal places of a voltage or power value. `show_decimals` does not
/// change them.
pub const VOLTAGE_POWER_DIGITS: usize = 2;

/// The unit of a value text. The caller chooses the symbol it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Celsius,
    Fahrenheit,
    Rpm,
    Volt,
    Watt,
}

/// A value text. `number` does not depend on the locale: it uses `.` as
/// the decimal point, and voltages and power always carry a sign. `unit`
/// is `None` when the settings hide the unit of this class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub number: String,
    pub unit: Option<Unit>,
}

/// The text of one value of a row of `class`. Returns `None` ("no value")
/// when `value` is `None`, not finite, or too large to convert to
/// Fahrenheit.
pub fn format(class: Class, value: Option<f64>, settings: &Settings) -> Option<Text> {
    // NaN and infinities have no text.
    let v = value.filter(|v| v.is_finite())?;
    let s = settings;
    let (number, unit, shown) = match class {
        Class::Temperature => {
            let (v, unit) = match s.temperature_unit {
                TemperatureUnit::Celsius => (v, Unit::Celsius),
                // One multiplication, then one addition, each rounded on
                // its own; no fused multiply-add.
                TemperatureUnit::Fahrenheit => {
                    (v * FAHRENHEIT_FACTOR + FAHRENHEIT_OFFSET, Unit::Fahrenheit)
                }
            };
            // A very large value can overflow to infinity here.
            if !v.is_finite() {
                return None;
            }
            let digits = TEMPERATURE_DIGITS[usize::from(s.show_decimals)];
            (format!("{v:.digits$}"), unit, s.show_temperature_unit)
        }
        // Fan speed is a whole number, truncated toward zero; a negative
        // value keeps its sign.
        Class::Fan => (
            format!("{:.0}", v.trunc()),
            Unit::Rpm,
            s.show_rotation_rate_unit,
        ),
        Class::Voltage => (
            format!("{v:+.VOLTAGE_POWER_DIGITS$}"),
            Unit::Volt,
            s.show_voltage_unit,
        ),
        Class::Power => (
            format!("{v:+.VOLTAGE_POWER_DIGITS$}"),
            Unit::Watt,
            s.show_power_unit,
        ),
    };
    Some(Text {
        number,
        unit: shown.then_some(unit),
    })
}
