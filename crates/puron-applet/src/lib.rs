//! The panel applet's code, as a library so that the integration tests
//! can drive it without a display. The binary in `main.rs` starts these
//! parts and adds the libcosmic window code.

#[macro_use]
pub mod i18n;
pub mod app;
pub mod panel;
pub mod popup;

/// Set-up that `main` runs before the toolkit starts. Today it only
/// chooses the language of the translated texts from the environment
/// (see `i18n::start`). It does not call `setlocale`, so the C library's
/// locale stays `C`.
pub fn start() {
    i18n::start();
}
