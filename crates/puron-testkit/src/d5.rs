//! The text dump of what Puron would show, a mode of `puron-soak`.
//!
//! It runs an engine with the providers that the settings switch on,
//! takes a snapshot once each of them has reported once (or a time limit
//! has passed), and builds the shown rows, the menu and the panel items
//! from it with the same `puron_core` functions that the applet calls.
//! Only the printing is this module's own. The dump has one line per
//! item, fields separated by tabs:
//! - `raw <identity> <value>`: each reading of the snapshot, `none`
//!   for a reading without a value;
//! - `menu <depth> <kind> <icon> <label> <value> <pinned>`: each menu
//!   entry, in menu order, `-` for a field the entry does not have, and
//!   `1` or `0` for whether a reading line is pinned;
//! - `panel <icon> <value>`: each panel item, in order.

use std::os::fd::BorrowedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use puron_config::Config;
use puron_core::format::{Text, Unit};
use puron_core::menu::{
    self, Entry, Heading, IconKind, Item, LineWord, MessageKind, Report, Section, Shown,
};
use puron_core::panel::{self, Icon, State};
use puron_core::pins;
use puron_core::rows::{self, Class, Kind, Provider, RawReading};
use puron_core::settings::Settings;
use puron_engine::clock::Clock;
use puron_engine::engine::{Adapters, Engine, LmReport, Snapshot};
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};

/// The usual time limit for every provider's first report, 15 s.
pub const WAIT: Duration = Duration::from_secs(15);

/// Every provider; the dump waits for those the settings switch on.
const PROVIDERS: [Provider; 5] = [
    Provider::LmSensors,
    Provider::Liquidctl,
    Provider::Nvidia,
    Provider::Udisks2,
    Provider::NvmeCli,
];

/// Wraps a provider's adapter to see when the provider has reported: it
/// sets `flag` when a `read` returns, or when `start` returns
/// `Unavailable` (such a provider never reads).
struct Reported {
    inner: Box<dyn Source>,
    flag: Arc<AtomicBool>,
}

impl Source for Reported {
    fn start(&mut self) -> StartOutcome {
        let o = self.inner.start();
        if o == StartOutcome::Unavailable {
            self.flag.store(true, Ordering::SeqCst);
        }
        o
    }

    fn read(&mut self, stop: BorrowedFd<'_>) -> ReadOutcome {
        let o = self.inner.read(stop);
        self.flag.store(true, Ordering::SeqCst);
        o
    }

    fn stop(&mut self) {
        self.inner.stop()
    }
}

/// The real adapters of `puron_engine::registry`, one per provider, for
/// a dump of the machine's own sensors.
pub fn registry_adapters() -> Adapters {
    Box::new(|p| -> Box<dyn Source> {
        match p {
            Provider::LmSensors => Box::new(registry::lm_sensors()),
            Provider::Liquidctl => Box::new(registry::liquidctl()),
            Provider::Nvidia => Box::new(registry::nvidia()),
            Provider::Udisks2 => Box::new(registry::udisks2()),
            Provider::NvmeCli => Box::new(registry::nvme()),
        }
    })
}

/// Runs an engine on `clock` with `settings` and `adapters`, and returns
/// a snapshot taken after every provider that `settings` switches on has
/// reported (see `Reported`). If that does not happen within `wait` of
/// real time, returns the latest snapshot taken by then, or `None` if
/// there was none. With no provider switched on it returns `None` at
/// once. The engine is shut down before it returns.
pub fn snapshot(
    clock: Arc<dyn Clock>,
    settings: &Settings,
    mut adapters: Adapters,
    wait: Duration,
) -> Option<Snapshot> {
    let flags: Vec<(Provider, Arc<AtomicBool>)> = PROVIDERS
        .into_iter()
        .filter(|&p| registry::switch(settings, p))
        .map(|p| (p, Arc::new(AtomicBool::new(false))))
        .collect();
    let marks = flags.clone();
    let engine = Engine::new(clock);
    engine.start(
        settings,
        Box::new(move |p| -> Box<dyn Source> {
            let flag = marks.iter().find(|(q, _)| *q == p).map(|(_, f)| f.clone());
            let inner = adapters(p);
            match flag {
                Some(flag) => Box::new(Reported { inner, flag }),
                None => inner,
            }
        }),
    );
    let start = Instant::now();
    let mut last = None;
    // Every 10 ms, take the newest snapshot, and stop at the first one
    // taken after every flag was seen set. A flag is set as its adapter's
    // call returns, a moment before the engine has the result, so that
    // snapshot can still lack the last provider's first report; the dump
    // then has no readings of that provider. Nothing here waits for the
    // engine to take that result.
    while !flags.is_empty() {
        let all = flags.iter().all(|(_, f)| f.load(Ordering::SeqCst));
        if let Some(s) = engine.try_next() {
            last = Some(s);
            if all {
                break;
            }
        }
        if start.elapsed() >= wait {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.shutdown();
    last
}

/// The dump of one snapshot, or of no snapshot, under `settings`: its
/// `raw` lines, then its `menu` lines, then its `panel` lines (see the
/// module documentation).
pub fn lines(snapshot: Option<&Snapshot>, settings: &Settings) -> String {
    let raw: &[RawReading] = snapshot.map_or(&[], |s| &s.readings);
    let mut out = String::new();
    for r in raw {
        let id = raw_identity(r);
        let n = r.value.map_or("none".to_string(), |v| format!("{v}"));
        out.push_str(&format!("raw\t{id}\t{n}\n"));
    }
    // As the applet does: first the menu and panel of the start, with no
    // rows and no lm-sensors report yet; then those of the snapshot,
    // given the start menu's `key` and the start panel's `state`.
    let none = menu::shown(&[], settings);
    let m0 = menu::menu(&none, settings, Report::NoneYet, None, false);
    let start = pins::start(settings.pinned.as_deref().unwrap_or_default());
    let p0 = panel::panel(&none, &start, settings, &State::default());
    let shown = menu::shown(raw, settings);
    let report = match snapshot.map(|s| s.lm_sensors) {
        None | Some(LmReport::NoneYet) => Report::NoneYet,
        Some(LmReport::Unavailable) => Report::Unavailable,
        Some(LmReport::Answered) => Report::Answered,
    };
    let m = menu::menu(&shown, settings, report, Some(&m0.key), false);
    let pinned = match &settings.pinned {
        None => shown.new_install_pins(),
        Some(_) => pins::moved(&start, &shown, settings.old_cpu_source.as_deref(), true),
    };
    let p = panel::panel(&shown, &pinned, settings, &p0.state);
    for e in &m.entries {
        match e {
            Entry::Section(section) => menu_line(
                &mut out,
                0,
                "section",
                section_word(*section),
                "-",
                "-",
                "-",
            ),
            Entry::Device(heading) => {
                let name = match heading {
                    Heading::Cpu => menu::CPU_HEADING,
                    Heading::Name(name) => name,
                };
                menu_line(&mut out, 1, "device", "-", name, "-", "-");
            }
            Entry::Line(i) => item(&mut out, i, &shown, &pinned),
            Entry::Separator => menu_line(&mut out, 0, "separator", "-", "-", "-", "-"),
            Entry::Message { kind, .. } => {
                menu_line(&mut out, 0, "message", "-", message_word(*kind), "-", "-")
            }
            Entry::HomepageLink { .. } => menu_line(&mut out, 0, "homepage", "-", "-", "-", "-"),
            Entry::SettingsLink => menu_line(&mut out, 0, "settings", "-", "-", "-", "-"),
        }
    }
    for i in &p.items {
        match i {
            panel::Item::Pin { icon, value } => {
                let icon = match icon {
                    Icon::None => "none",
                    Icon::Kind(k) => icon_word(*k),
                    Icon::EmptySlot => "slot",
                };
                let v = value
                    .as_ref()
                    .map_or("missing".to_string(), |v| text(v.as_ref()));
                out.push_str(&format!("panel\t{icon}\t{v}\n"));
            }
            panel::Item::EmptyState => out.push_str("panel\tempty\t-\n"),
        }
    }
    out
}

/// The whole dump: `snapshot`, then `lines` of it.
pub fn dump(
    clock: Arc<dyn Clock>,
    settings: &Settings,
    adapters: Adapters,
    wait: Duration,
) -> String {
    let s = snapshot(clock, settings, adapters, wait);
    lines(s.as_ref(), settings)
}

/// The settings stored under the configuration root `root`, loaded with
/// `Config::load` as the applet loads them. Each problem found while
/// loading is printed to standard error.
pub fn settings(root: &Path) -> Settings {
    let loaded = Config::new(root).load();
    for p in &loaded.problems {
        eprintln!("D5: settings problem {p:?}");
    }
    loaded.settings
}

/// The identity printed on a `raw` line: the class word, the provider
/// word and the reading's key, joined with `rows::IDENTITY_SEPARATOR`,
/// as a row's identity is built. The class word comes from the reading's
/// `kind`, except for liquidctl, whose readings carry a unit instead (see
/// `liquidctl_word`). A reading with no class gets `-`.
fn raw_identity(r: &RawReading) -> String {
    let word = match (r.provider, r.kind) {
        (Provider::Liquidctl, _) => liquidctl_word(r.unit.as_deref()),
        (_, Some(Kind::Temperature)) => Class::Temperature.word(),
        (_, Some(Kind::Fan)) => Class::Fan.word(),
        (_, Some(Kind::Voltage)) => Class::Voltage.word(),
        (_, Some(Kind::Power)) => Class::Power.word(),
        (_, None) => "-",
    };
    [word, r.provider.word(), r.key.as_str()].join(rows::IDENTITY_SEPARATOR)
}

/// The class word of a liquidctl reading's unit; the unit must equal one
/// of `rows::LIQUIDCTL_UNITS` exactly, as in `puron_core::rows`. Any other
/// unit, or none, gives `-`.
fn liquidctl_word(unit: Option<&str>) -> &'static str {
    let [celsius, rpm, volt] = rows::LIQUIDCTL_UNITS;
    match unit {
        Some(u) if u == celsius => Class::Temperature.word(),
        Some(u) if u == rpm => Class::Fan.word(),
        Some(u) if u == volt => Class::Voltage.word(),
        _ => "-",
    }
}

/// The label of a menu line in the dump: the line's name, then its word
/// if it has one, then `(default)` for a default line, separated by
/// spaces.
fn label(i: &Item) -> String {
    let word = match i.word {
        None => "",
        Some(LineWord::Composite) => "composite",
        Some(LineWord::Package) => "package",
        Some(LineWord::Temperature) => "temperature",
        Some(LineWord::Maximum) => "Maximum",
        Some(LineWord::Waiting) => "CPU: waiting for lm-sensors",
    };
    let parts = [
        i.name.as_str(),
        word,
        if i.default { "(default)" } else { "" },
    ];
    let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    parts.join(" ")
}

/// Prints the `menu` line of a reading line: kind `maximum` or `row`,
/// its icon (`-` when `shown` does not have it), label, value, and
/// whether it is pinned.
fn item(out: &mut String, i: &Item, shown: &Shown, pinned: &[String]) {
    let mark = if pins::is_pinned(pinned, &i.identity) {
        "1"
    } else {
        "0"
    };
    let (kind, icon) = match shown.line(&i.identity) {
        Some(line) if line.is_maximum() => ("maximum", icon_word(menu::icon_kind(line))),
        Some(line) => ("row", icon_word(menu::icon_kind(line))),
        None => ("row", "-"),
    };
    let v = text(i.value.as_ref());
    menu_line(out, 2, kind, icon, &label(i), &v, mark);
}

fn menu_line(
    out: &mut String,
    depth: u8,
    kind: &str,
    icon: &str,
    label: &str,
    value: &str,
    mark: &str,
) {
    out.push_str(&format!(
        "menu\t{depth}\t{kind}\t{icon}\t{label}\t{value}\t{mark}\n"
    ));
}

/// A value as the panel and the menu show it in English: the number and
/// its unit's suffix, or `N/A` when there is no value. The texts copy
/// those of the English catalog `i18n/en/puron.ftl` (`no-value` and the
/// `suffix-*` messages).
fn text(t: Option<&Text>) -> String {
    let Some(t) = t else {
        return "N/A".to_string();
    };
    let suffix = match t.unit {
        None => "",
        Some(Unit::Celsius) => "°C",
        Some(Unit::Fahrenheit) => "°F",
        Some(Unit::Rpm) => " rpm",
        Some(Unit::Volt) => "V",
        Some(Unit::Watt) => "W",
    };
    format!("{}{suffix}", t.number)
}

fn icon_word(k: IconKind) -> &'static str {
    match k {
        IconKind::GeneralTemperature => "general",
        IconKind::GpuTemperature => "gpu",
        IconKind::DriveTemperature => "drive",
        IconKind::Fan => "fan",
        IconKind::Voltage => "voltage",
        IconKind::Power => "power",
    }
}

fn section_word(section: Section) -> &'static str {
    match section {
        Section::Temperature => "temperature",
        Section::Fan => "fan",
        Section::Voltage => "voltage",
        Section::Power => "power",
    }
}

fn message_word(k: MessageKind) -> &'static str {
    match k {
        MessageKind::SensorsDetect => "sensors-detect",
        MessageKind::Install => "install",
    }
}
