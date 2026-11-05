//! The library part of puron-settings, which the program and its tests
//! share.

/// `i18n`: the source is shared with `puron-applet`.
#[macro_use]
#[path = "../../puron-applet/src/i18n.rs"]
pub mod i18n;

pub mod view;
pub mod window;

/// The program's start code: every statement `main` runs before the
/// toolkit. It never sets the C library locale from the environment.
pub fn start() {
    i18n::start();
}
