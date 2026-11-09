//! Helpers shared by the tests that drive `App`: a fake engine handle,
//! an `App` over a temporary config directory, snapshots and pinned lists
//! read from the testkit's fixtures, and readers of an event's outputs.
//! Not every test file uses every helper.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use puron_applet::app::{App, Command, Event, Handle, Output, Programs};
use puron_config::{Config, Delivery, Loaded};
use puron_core::format::Unit;
use puron_core::panel::{Icon, Item, Panel};
use puron_core::rows::RawReading;
use puron_core::settings::Settings;
use puron_engine::engine::{LmReport, Snapshot};
use puron_testkit::eng;

/// The testkit's fake engine handle, wrapped so that it can implement the
/// applet's `Handle` trait: both the trait and the fake belong to other
/// crates, so the orphan rule needs a local type.
pub struct H(pub Arc<eng::Handle>);

impl Handle for H {
    fn start(&self, s: &Settings) {
        self.0.start(s)
    }
    fn poll_now(&self) {
        self.0.poll_now()
    }
    fn set_interval(&self, secs: u32) {
        self.0.set_interval(secs)
    }
    fn set_providers(&self, s: &Settings) {
        self.0.set_providers(s)
    }
    fn shutdown(&self) {
        self.0.shutdown()
    }
    fn next(&self) -> Snapshot {
        self.0.next()
    }
    fn try_next(&self) -> Option<Snapshot> {
        self.0.try_next()
    }
}

/// A fake engine handle whose `shutdown` takes `shutdown_secs` to return.
pub fn handle(shutdown_secs: u64) -> Arc<eng::Handle> {
    eng::Handle::new(Duration::from_secs(shutdown_secs))
}

/// Program locations in an empty temporary directory, for tests that
/// start no program: neither program can be found there.
pub fn no_programs() -> Programs {
    let dir = puron_testkit::temp_dir("noprog");
    Programs {
        settings_dir: dir.clone(),
        search_path: dir.into_os_string(),
    }
}

pub fn app_with(root: &Path, h: &Arc<eng::Handle>, programs: Programs) -> App {
    App::new(Arc::new(H(Arc::clone(h))), Config::new(root), programs)
}

pub fn app(root: &Path, h: &Arc<eng::Handle>) -> App {
    app_with(root, h, no_programs())
}

fn raw(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

/// A snapshot holding the readings of the `capture.json` fixture, a
/// recording of a real machine's sensors, with lm-sensors answered.
pub fn capture() -> Snapshot {
    let readings: Vec<RawReading> = serde_json::from_str(&raw("capture.json")).expect("capture");
    Snapshot {
        readings,
        lm_sensors: LmReport::Answered,
    }
}

/// A snapshot holding the reading set `name` of the `rebuild.json`
/// fixture, with lm-sensors answered.
pub fn rebuild(name: &str) -> Snapshot {
    let mut sets: BTreeMap<String, Vec<RawReading>> =
        serde_json::from_str(&raw("rebuild.json")).expect("rebuild");
    Snapshot {
        readings: sets.remove(name).expect("X-raw-rebuild set"),
        lm_sensors: LmReport::Answered,
    }
}

/// The pinned list `name` of the `pins.json` fixture.
pub fn pins(name: &str) -> Vec<String> {
    serde_json::from_str::<BTreeMap<String, Vec<String>>>(&raw("pins.json"))
        .expect("pins")
        .remove(name)
        .expect("X-raw-pins set")
}

/// A config change as `Config::watch` delivers it: the changed `keys`
/// and the app's current settings after `f` has changed them, with no
/// problems.
pub fn delivery(app: &App, keys: &[&'static str], f: impl FnOnce(&mut Settings)) -> Event {
    let mut s = app.settings().clone();
    f(&mut s);
    Event::Delivery(Delivery {
        keys: keys.to_vec(),
        loaded: Loaded {
            settings: s,
            problems: Vec::new(),
        },
    })
}

/// The engine calls among an event's outputs, in order.
pub fn commands(out: &[Output]) -> Vec<Command> {
    out.iter()
        .filter_map(|o| match o {
            Output::Engine(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

/// The last panel output of an event.
pub fn panel_of(out: &[Output]) -> Option<Panel> {
    out.iter().rev().find_map(|o| match o {
        Output::Panel(p) => Some(p.clone()),
        _ => None,
    })
}

pub fn has_layout(out: &[Output]) -> bool {
    out.iter().any(|o| matches!(o, Output::Layout { .. }))
}

/// The panel items as short texts: "⚠" for a pin with no line in the
/// popup, "" for a line without a value, the number and its unit for a
/// value, and "∅state" for the empty-state item.
pub fn values(p: &Panel) -> Vec<String> {
    p.items
        .iter()
        .map(|i| match i {
            Item::EmptyState => "∅state".to_string(),
            Item::Pin { value: None, .. } => "⚠".to_string(),
            Item::Pin {
                value: Some(None), ..
            } => String::new(),
            Item::Pin {
                value: Some(Some(t)),
                ..
            } => {
                let unit = match t.unit {
                    None => "",
                    Some(Unit::Celsius) => "°C",
                    Some(Unit::Fahrenheit) => "°F",
                    Some(Unit::Rpm) => " rpm",
                    Some(Unit::Volt) => " V",
                    Some(Unit::Watt) => " W",
                };
                format!("{}{unit}", t.number)
            }
        })
        .collect()
}

/// Each pin's icon, in item order; `None` for the empty-state item.
pub fn icons(p: &Panel) -> Vec<Option<Icon>> {
    p.items
        .iter()
        .map(|i| match i {
            Item::Pin { icon, .. } => Some(*icon),
            Item::EmptyState => None,
        })
        .collect()
}
