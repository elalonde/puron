//! The library part of puron-applet, which the program and its tests
//! share.

#[macro_use]
pub mod i18n;
pub mod app;
pub mod panel;
pub mod popup;

/// The program's start code: every statement `main` runs before the
/// toolkit. It never sets the C library locale from the environment.
pub fn start() {
    i18n::start();
}
