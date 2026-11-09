//! The rules that turn sensor readings and settings into what Puron
//! shows: which readings become rows and how they are named (`rows`),
//! their order (`order`), which temperature values count (`summary`), the
//! popup's lines (`menu`), the panel's items (`panel`), the list of
//! readings shown in the panel (`pins`), value text (`format`) and the
//! settings in effect (`settings`).
//!
//! A pin is a reading that the user has checked in the popup so that the
//! panel shows it; the pinned list holds the identities of those readings
//! (see `rows::identities`). The crate does no I/O and keeps no state:
//! callers pass in everything a function needs and keep what it returns.

mod fold;
pub mod format;
pub mod menu;
pub mod order;
pub mod panel;
pub mod pins;
pub mod rows;
pub mod settings;
pub mod summary;
