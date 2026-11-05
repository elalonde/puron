//! The rows dump, a mode of `puron-soak`. It starts the providers that
//! the settings turn on, waits until each has reported once or 15 s have
//! passed, builds the shown rows, the menu layout and the panel items
//! with the puron-core functions that the applet calls, and prints them.
//! Only the printing is its own.

use std::os::fd::BorrowedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use puron_config::Config;
use puron_core::format::{Text, Unit};
use puron_core::menu::{self, Entry, GroupKind, IconKind, Item, MessageKind, Report};
use puron_core::panel::{self, Icon, State};
use puron_core::pins;
use puron_core::rows::{self, Class, Kind, Provider, RawReading};
use puron_core::settings::Settings;
use puron_engine::clock::Clock;
use puron_engine::engine::{Adapters, Engine, LmReport, Snapshot};
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};

/// The wait for the first report of each provider.
pub const WAIT: Duration = Duration::from_secs(15);

const PROVIDERS: [Provider; 5] = [
    Provider::LmSensors,
    Provider::Liquidctl,
    Provider::Nvidia,
    Provider::Udisks2,
    Provider::NvmeCli,
];

/// An adapter that marks its provider as reported after its first Read
/// returns, or when its Start is unavailable (it never reads then).
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

/// The production adapters of each provider (as `puron-soak` with the
/// real liquidctl).
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

/// The latest snapshot once every provider that `settings` turns on has
/// reported, or the latest at `wait`; `None` when none came.
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
    // A snapshot taken after every flag was seen set holds each first
    // report: a flag is set before its report reaches the engine.
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

/// The dump of one snapshot: `raw`, `menu` and `panel` lines.
pub fn lines(snapshot: Option<&Snapshot>, settings: &Settings) -> String {
    let raw: &[RawReading] = snapshot.map_or(&[], |s| &s.readings);
    let mut out = String::new();
    for r in raw {
        let id = raw_identity(r);
        let n = r.value.map_or("none".to_string(), |v| format!("{v}"));
        out.push_str(&format!("raw\t{id}\t{n}\n"));
    }
    // As the applet does: the start (no rows, no report yet), then the
    // snapshot with the start's Key and Panel state passed back in.
    let pinned = pins::start(&settings.pinned);
    let none = menu::shown(&[], settings);
    let m0 = menu::menu(&none, settings, Report::NoneYet, None, false);
    let p0 = panel::panel(&none, &pinned, settings, &State::default());
    let shown = menu::shown(raw, settings);
    let report = match snapshot.map(|s| s.lm_sensors) {
        None | Some(LmReport::NoneYet) => Report::NoneYet,
        Some(LmReport::Unavailable) => Report::Unavailable,
        Some(LmReport::Answered) => Report::Answered,
    };
    let m = menu::menu(&shown, settings, report, Some(&m0.key), false);
    let p = panel::panel(&shown, &pinned, settings, &p0.state);
    for e in &m.entries {
        match e {
            Entry::Sensor(i) => item(&mut out, 0, "row", &i.name, i, &pinned),
            Entry::Maximum(i) => item(&mut out, 0, "maximum", "Maximum", i, &pinned),
            Entry::Group {
                kind,
                header,
                members,
            } => {
                let k = group_word(*kind);
                let v = header
                    .as_ref()
                    .map_or("-".to_string(), |h| text(h.as_ref()));
                menu_line(&mut out, 0, "group", k, k, &v, "-");
                for i in members {
                    item(&mut out, 1, "row", &i.name, i, &pinned);
                }
            }
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

/// The rows dump on `settings` with `adapters`: the snapshot, then its
/// lines.
pub fn dump(
    clock: Arc<dyn Clock>,
    settings: &Settings,
    adapters: Adapters,
    wait: Duration,
) -> String {
    let s = snapshot(clock, settings, adapters, wait);
    lines(s.as_ref(), settings)
}

/// The settings of the configuration root `root`, as the applet loads
/// them; each Problem goes to standard error.
pub fn settings(root: &Path) -> Settings {
    let loaded = Config::new(root).load();
    for p in &loaded.problems {
        eprintln!("D5: settings problem {p:?}");
    }
    loaded.settings
}

/// The identity of a `raw` line: class word, provider, Key. The class
/// word is that of the Kind, or for liquidctl that of the unit, `-` for
/// another unit; for a reading that gives a row this is the row's
/// identity. Every adapter but liquidctl's gives a Kind; a reading
/// without one gets `-` as well.
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

/// The class word of a liquidctl unit, character for character as the
/// row rules of `puron-core` map it.
fn liquidctl_word(unit: Option<&str>) -> &'static str {
    let [celsius, rpm, volt] = rows::LIQUIDCTL_UNITS;
    match unit {
        Some(u) if u == celsius => Class::Temperature.word(),
        Some(u) if u == rpm => Class::Fan.word(),
        Some(u) if u == volt => Class::Voltage.word(),
        _ => "-",
    }
}

fn item(out: &mut String, depth: u8, kind: &str, label: &str, i: &Item, pinned: &[String]) {
    let mark = if pins::is_pinned(pinned, &i.identity) {
        "1"
    } else {
        "0"
    };
    let v = text(i.value.as_ref());
    menu_line(out, depth, kind, icon_word(i.icon), label, &v, mark);
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

/// The value text as the panel and the popup show it in English
/// (`i18n/en/puron.ftl`: `no-value`, `suffix-*`).
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
        IconKind::Maximum => "maximum",
    }
}

fn group_word(k: GroupKind) -> &'static str {
    match k {
        GroupKind::Temperature => "temperature",
        GroupKind::Fan => "fan",
        GroupKind::Voltage => "voltage",
    }
}

fn message_word(k: MessageKind) -> &'static str {
    match k {
        MessageKind::SensorsDetect => "sensors-detect",
        MessageKind::Install => "install",
    }
}
