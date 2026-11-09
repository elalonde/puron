//! The library part of puron-settings, which the program and its tests
//! share.

/// Translation setup and the `fl!` macro. The file belongs to
/// `puron-applet`; this crate compiles the same source.
#[macro_use]
#[path = "../../puron-applet/src/i18n.rs"]
pub mod i18n;

pub mod view;
pub mod window;

/// Everything `main` runs before it starts the toolkit, kept here so that
/// tests can run the same start. It does not set the C library's locale
/// from the environment.
pub fn start() {
    i18n::start();
}
