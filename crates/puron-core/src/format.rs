//! Value text: the number and unit shown for one value.

use crate::rows::Class;
use crate::settings::{Settings, TemperatureUnit};

/// Fahrenheit factor and offset.
pub const FAHRENHEIT_FACTOR: f64 = 1.8;
pub const FAHRENHEIT_OFFSET: f64 = 32.0;
/// Temperature digits with decimals off and on.
pub const TEMPERATURE_DIGITS: [usize; 2] = [0, 1];
/// Voltage and power digits.
pub const VOLTAGE_POWER_DIGITS: usize = 2;

/// The unit of a value text. The GUI crate renders its suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Celsius,
    Fahrenheit,
    Rpm,
    Volt,
    Watt,
}

/// A value text: locale-neutral number text and the unit, `None`
/// when the class's unit switch is off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub number: String,
    pub unit: Option<Unit>,
}

/// The text of one value; `None` is "no value".
pub fn format(class: Class, value: Option<f64>, settings: &Settings) -> Option<Text> {
    // A value that is not finite is "no value", before any conversion.
    let v = value.filter(|v| v.is_finite())?;
    let s = settings;
    let (number, unit, shown) = match class {
        Class::Temperature => {
            let (v, unit) = match s.temperature_unit {
                TemperatureUnit::Celsius => (v, Unit::Celsius),
                // One multiplication, then one addition, each rounded.
                TemperatureUnit::Fahrenheit => {
                    (v * FAHRENHEIT_FACTOR + FAHRENHEIT_OFFSET, Unit::Fahrenheit)
                }
            };
            if !v.is_finite() {
                return None;
            }
            let digits = TEMPERATURE_DIGITS[usize::from(s.show_decimals)];
            (format!("{v:.digits$}"), unit, s.show_temperature_unit)
        }
        // Fan speed: cut toward zero, sign kept.
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
