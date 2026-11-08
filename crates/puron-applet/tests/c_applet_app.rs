//! Tests of `app` that drive the state machine without a display and
//! read no diag datagram, so they can share one binary. 5 s each. The
//! start record is fixture `good-p5` (every key set by hand to a
//! non-default value, with pinned list P5) unless a test uses fixture
//! `p5` (the `core.settings` defaults with P5).
#![allow(non_snake_case)]

mod common;

use std::process::Command as Proc;
use std::sync::mpsc;
use std::time::Duration;

use common::*;
use puron_applet::app::{self, Command, Event, Output, PinPhase, Source};
use puron_config::{Config, StoreResult, KEYS};
use puron_core::menu::{Entry, MessageKind};
use puron_core::panel::{Icon, Item};
use puron_core::settings::TemperatureUnit;
use puron_testkit::cfg;
use puron_testkit::eng::HandleCall;

const LIMIT: u64 = 5;

/// Start with the fixture `name`; the outputs of `Event::Start`.
fn started(
    name: &str,
) -> (
    cfg::Cfg,
    std::sync::Arc<puron_testkit::eng::Handle>,
    app::App,
    Vec<Output>,
) {
    let c = cfg::build(name);
    let h = handle(0);
    let mut a = app(&c.root, &h);
    let out = a.handle(Event::Start);
    (c, h, a, out)
}

/// The `p5` root with the captured readings applied as a snapshot.
fn as_4() -> (
    cfg::Cfg,
    std::sync::Arc<puron_testkit::eng::Handle>,
    app::App,
) {
    let (c, h, mut a, _) = started("p5");
    let out = a.handle(Event::Snapshot(capture()));
    assert_eq!(values(&panel_of(&out).unwrap()), ["73°C", "33°C"]);
    (c, h, a)
}

#[test]
fn c_applet_app_1() {
    puron_testkit::limit("C-applet.app-1", LIMIT, || {
        let f = puron_testkit::temp_dir("c-applet-app-1").join("keys.txt");
        std::fs::write(
            &f,
            KEYS.iter().map(|k| format!("{k}\n")).collect::<String>(),
        )
        .unwrap();
        let o = Proc::new("grep")
            .args(["-ciE", "position|index"])
            .arg(&f)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "0\n");
        assert_eq!(o.status.code(), Some(1));
    });
}

#[test]
fn c_applet_app_2() {
    puron_testkit::limit("C-applet.app-2", LIMIT, || {
        let (c, h, _a, out) = started("p5");
        let loaded = puron_config::Config::new(&c.root).load().settings;
        let panel_at = out.iter().position(|o| matches!(o, Output::Panel(_)));
        let layout_at = out.iter().position(|o| matches!(o, Output::Layout { .. }));
        let start_at = out
            .iter()
            .position(|o| matches!(o, Output::Engine(Command::Start(_))));
        let (Some(p), Some(l), Some(s)) = (panel_at, layout_at, start_at) else {
            panic!("start outputs {out:?}");
        };
        assert!(p < s && l < s, "Start before the items: {out:?}");
        let Output::Panel(panel) = &out[p] else {
            unreachable!()
        };
        assert_eq!(values(panel), ["⚠", "⚠"]);
        assert_eq!(icons(panel), [Some(Icon::EmptySlot), Some(Icon::EmptySlot)]);
        let Output::Layout { menu, .. } = &out[l] else {
            unreachable!()
        };
        assert!(
            matches!(
                menu.entries[0],
                Entry::Message {
                    kind: MessageKind::SensorsDetect,
                    ..
                }
            ),
            "{:?}",
            menu.entries
        );
        assert_eq!(h.calls().first(), Some(&HandleCall::Start(loaded)));
        let (_c, _h, _a, out) = started("nopins");
        let panel = panel_of(&out).expect("panel at start");
        assert_eq!(panel.items, [Item::EmptyState]);
    });
}

#[test]
fn c_applet_app_4() {
    puron_testkit::limit("C-applet.app-4", LIMIT, || {
        let (_c, _h, mut a) = as_4();
        let out = a.handle(delivery(&a, &["show_temperature_unit"], |s| {
            s.show_temperature_unit = false
        }));
        assert_eq!(commands(&out), []);
        assert_eq!(
            values(&panel_of(&out).expect("rebuilt at once")),
            ["73", "33"]
        );
        let out = a.handle(delivery(&a, &["show_temperature_unit"], |s| {
            s.show_temperature_unit = true
        }));
        assert_eq!(commands(&out), []);
        assert_eq!(
            values(&panel_of(&out).expect("rebuilt at once")),
            ["73°C", "33°C"]
        );
    });
}

#[test]
fn c_applet_app_5() {
    puron_testkit::limit("C-applet.app-5", LIMIT, || {
        let (_c, _h, mut a) = as_4();
        let out = a.handle(delivery(&a, &["temperature_unit"], |s| {
            s.temperature_unit = TemperatureUnit::Fahrenheit
        }));
        assert_eq!(commands(&out), [Command::PollNow]);
        assert!(panel_of(&out).is_none() && !has_layout(&out), "{out:?}");
        assert_eq!(values(a.panel()), ["73°C", "33°C"]);
        let out = a.handle(Event::Snapshot(capture()));
        assert_eq!(values(&panel_of(&out).unwrap()), ["163°F", "91°F"]);
    });
}

#[test]
fn c_applet_app_6() {
    puron_testkit::limit("C-applet.app-6", LIMIT, || {
        let (_c, _h, mut a) = as_4();
        let ev = delivery(&a, &["provider_nvme"], |s| s.provider_nvme = true);
        let Event::Delivery(d) = &ev else {
            unreachable!()
        };
        let new = d.loaded.settings.clone();
        let out = a.handle(ev);
        assert_eq!(commands(&out), [Command::SetProviders(new)]);
    });
}

#[test]
fn c_applet_app_7() {
    puron_testkit::limit("C-applet.app-7", LIMIT, || {
        let c = cfg::build("p5");
        cfg::write(
            &c.id_dir().join("pinned"),
            br#"["temperature/lm-sensors/iwlwifi_1-virtual-0/temp1", "@maximum"]"#,
        );
        assert_eq!(
            puron_config::Config::new(&c.root).load().settings.pinned,
            Some(pins("P3"))
        );
        let h = handle(0);
        let mut a = app(&c.root, &h);
        a.handle(Event::Start);
        a.handle(Event::Snapshot(capture()));
        let before = a.panel().items.clone();
        let out = a.handle(delivery(&a, &["show_temperature"], |s| {
            s.show_temperature = false
        }));
        assert_eq!(commands(&out), [Command::PollNow]);
        assert!(!has_layout(&out) && panel_of(&out).is_none(), "{out:?}");
        assert_eq!(a.panel().items, before);
        let out = a.handle(Event::Snapshot(capture()));
        let Some(Output::Layout { menu, forced }) =
            out.iter().find(|o| matches!(o, Output::Layout { .. }))
        else {
            panic!("no layout: {out:?}");
        };
        assert!(*forced, "Forced no");
        assert!(
            !menu
                .identities
                .iter()
                .any(|i| i.starts_with("temperature/")),
            "{:?}",
            menu.identities
        );
        assert_eq!(panel_of(&out).unwrap().items, [Item::EmptyState]);
        let out = a.handle(Event::Snapshot(capture()));
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Layout { forced: false, .. })),
            "{out:?}"
        );
        // One delivery with a unit switch and Show Sensors together.
        let out = a.handle(delivery(
            &a,
            &["show_temperature", "show_temperature_unit"],
            |s| {
                s.show_temperature = true;
                s.show_temperature_unit = false;
            },
        ));
        assert_eq!(commands(&out), [Command::PollNow], "combined delivery");
        let Some(Output::Layout { menu, forced }) =
            out.iter().find(|o| matches!(o, Output::Layout { .. }))
        else {
            panic!("no layout at the combined delivery: {out:?}");
        };
        assert!(!*forced, "Forced yes at the combined delivery");
        assert!(
            menu.identities
                .iter()
                .any(|i| i.starts_with("temperature/")),
            "{:?}",
            menu.identities
        );
        let p = panel_of(&out).expect("panel at the combined delivery");
        assert_eq!(values(&p), ["73", "33"], "pins without the unit");
        let out = a.handle(Event::Snapshot(capture()));
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::Layout { forced: true, .. })),
            "Forced not kept: {out:?}"
        );
    });
}

#[test]
fn c_applet_app_8() {
    puron_testkit::limit("C-applet.app-8", LIMIT, || {
        let (_c, _h, mut a, _) = started("good-p5");
        assert_eq!(a.settings().poll_interval_secs, 7);
        let out = a.handle(delivery(&a, &["poll_interval_secs"], |s| {
            s.poll_interval_secs = 10
        }));
        assert_eq!(commands(&out), [Command::SetInterval(10)]);
    });
}

#[test]
fn c_applet_app_9() {
    puron_testkit::limit("C-applet.app-9", LIMIT, || {
        let (_c, _h, mut a) = as_4();
        let out = a.handle(delivery(&a, &["show_panel_icons"], |s| {
            s.show_panel_icons = false
        }));
        assert_eq!(commands(&out), []);
        let p = panel_of(&out).expect("panel rebuilt at once");
        assert_eq!(icons(&p), [Some(Icon::None), Some(Icon::None)]);
        assert_eq!(values(&p), ["73°C", "33°C"]);
    });
}

#[test]
fn c_applet_app_10() {
    puron_testkit::limit("C-applet.app-10", LIMIT, || {
        let (_c, _h, mut a, _) = started("good-p5");
        let ev = delivery(
            &a,
            &["poll_interval_secs", "provider_nvme", "show_power"],
            |s| {
                s.poll_interval_secs = 10;
                s.provider_nvme = !s.provider_nvme;
                s.show_power = !s.show_power;
            },
        );
        let Event::Delivery(d) = &ev else {
            unreachable!()
        };
        let new = d.loaded.settings.clone();
        let out = a.handle(ev);
        assert_eq!(
            commands(&out),
            [Command::SetInterval(10), Command::SetProviders(new)]
        );
        let out = a.handle(delivery(&a, &["show_power"], |_| {}));
        assert_eq!(out, []);
    });
}

#[test]
fn c_applet_app_11() {
    puron_testkit::limit("C-applet.app-11", LIMIT, || {
        let (_c, _h, mut a, _) = started("good-p5");
        let items = a.panel().items.clone();
        let out = a.handle(delivery(&a, &["pinned"], |s| s.pinned = Some(pins("P1"))));
        assert_eq!(a.pinned(), pins("P5"));
        assert_eq!(a.panel().items, items);
        assert!(panel_of(&out).is_none(), "{out:?}");
        assert!(
            !out.iter().any(|o| matches!(o, Output::Store(..))),
            "{out:?}"
        );
        assert_eq!(commands(&out), [Command::PollNow]);
    });
}

#[test]
fn c_applet_app_12() {
    puron_testkit::limit("C-applet.app-12", LIMIT, || {
        let (c, _h, mut a) = as_4();
        let fan = a
            .menu()
            .unwrap()
            .identities
            .iter()
            .find(|i| i.starts_with("fan/") && i.contains("fan1"))
            .cloned()
            .expect("CPU fan in the menu");
        let out = a.handle(Event::PinClick(fan.clone()));
        let mut want = pins("P5");
        want.push(fan);
        let stores: Vec<_> = out
            .iter()
            .filter(|o| matches!(o, Output::Store(..)))
            .collect();
        assert_eq!(stores, [&Output::Store(want.clone(), StoreResult::Stored)]);
        assert_eq!(
            puron_config::Config::new(&c.root).load().settings.pinned,
            Some(want)
        );
        assert_eq!(commands(&out), [Command::PollNow]);
        let p = panel_of(&out).expect("panel in the click's event");
        assert_eq!(values(&p), ["73°C", "33°C", "0 rpm"]);
        assert_eq!(
            icons(&p)[2],
            Some(Icon::Kind(puron_core::menu::IconKind::Fan))
        );
        let ev = Event::Delivery(puron_config::Delivery {
            keys: vec!["pinned"],
            loaded: puron_config::Config::new(&c.root).load(),
        });
        let out = a.handle(ev);
        assert_eq!(commands(&out), []);
    });
}

#[test]
fn c_applet_app_18() {
    puron_testkit::limit("C-applet.app-18", LIMIT, || {
        let c = cfg::build("p5");
        let h = handle(0);
        let a = app(&c.root, &h);
        let (tx, rx) = mpsc::channel();
        let (vtx, vrx) = mpsc::channel::<(Source, Vec<Output>)>();
        std::thread::spawn(move || {
            app::run(a, tx, rx, move |s, _, out| {
                let _ = vtx.send((s, out.to_vec()));
            })
        });
        let (s, _) = vrx.recv().unwrap();
        assert_eq!(s, Source::Start);
        let waiting = |c: &[HandleCall]| {
            c.iter().rposition(|x| *x == HandleCall::WaitBegin)
                > c.iter().rposition(|x| *x == HandleCall::WaitEnd)
        };
        assert!(
            h.wait_for(Duration::from_secs(2), waiting),
            "no subscription wait begun: {:?}",
            h.calls()
        );
        h.deliver(capture());
        let (s, out) = vrx.recv().unwrap();
        assert_eq!(s, Source::Subscription, "{out:?}");
        assert_eq!(values(&panel_of(&out).expect("panel")), ["73°C", "33°C"]);
        std::mem::forget(c);
    });
}

/// Set `name` of the CPU fixture as a snapshot.
fn cpu_snapshot(name: &str) -> puron_engine::engine::Snapshot {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", "cpu.json")).unwrap();
    let mut sets: std::collections::BTreeMap<String, Vec<puron_core::rows::RawReading>> =
        serde_json::from_str(&text).unwrap();
    puron_engine::engine::Snapshot {
        readings: sets.remove(name).expect("X-raw-cpu set"),
        lm_sensors: puron_engine::engine::LmReport::Answered,
    }
}

fn drive_snapshot(name: &str) -> puron_engine::engine::Snapshot {
    let text = std::fs::read_to_string(puron_testkit::fixture("raw", "drives.json")).unwrap();
    let mut sets: std::collections::BTreeMap<String, Vec<puron_core::rows::RawReading>> =
        serde_json::from_str(&text).unwrap();
    puron_engine::engine::Snapshot {
        readings: sets.remove(name).expect("X-raw-drives set"),
        lm_sensors: puron_engine::engine::LmReport::Answered,
    }
}

const NVME_S1: &str = "temperature/nvme-cli/0000:01:00.0/temp2";
const NVME_S2: &str = "temperature/nvme-cli/0000:01:00.0/temp3";
const DRIVE_S1: &str = "temperature/lm-sensors/nvme-pci-0100/temp2";
const DRIVE_S2: &str = "temperature/lm-sensors/nvme-pci-0100/temp3";

/// An app started with `pinned` stored.
fn pinned_app(
    pinned: &[&str],
) -> (
    cfg::Cfg,
    std::sync::Arc<puron_testkit::eng::Handle>,
    app::App,
    Config,
) {
    let c = cfg::build("p5");
    let config = Config::new(c.root.clone());
    let list = pinned.iter().map(|e| e.to_string()).collect();
    let stored = config.store(puron_config::PINNED, &puron_config::Value::List(list));
    assert_eq!(stored, StoreResult::Stored);
    let h = handle(0);
    let mut a = app(&c.root, &h);
    a.handle(Event::Start);
    (c, h, a, config)
}

fn pin_stores(out: &[Output]) -> usize {
    out.iter()
        .filter(|o| matches!(o, Output::Store(..)))
        .count()
}

#[test]
fn c_applet_app_23() {
    puron_testkit::limit("C-applet.app-23", LIMIT, || {
        // With lm-sensors off a pin on the NVMe reading works as it did.
        let (_c, _h, mut a, config) = pinned_app(&[NVME_S2]);
        let out = a.handle(Event::Snapshot(drive_snapshot("lm-sensors-off")));
        assert_eq!(a.pinned(), [NVME_S2.to_string()]);
        assert_eq!(pin_stores(&out), 0);
        // Both report it: the pin moves to the line that stays, once.
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [DRIVE_S2.to_string()]);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![DRIVE_S2.to_string()])
        );
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(pin_stores(&out), 0);
        // Lost again: it stays where it moved to.
        a.handle(Event::Snapshot(drive_snapshot("lm-sensors-off")));
        assert_eq!(a.pinned(), [DRIVE_S2.to_string()]);
        // A pin on the line that stays and on its repeat: one pin.
        let (_c, _h, mut a, _) = pinned_app(&[NVME_S1, DRIVE_S1]);
        a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [DRIVE_S1.to_string()]);
        // No proof of a repeat: the pin stays.
        let (_c, _h, mut a, _) = pinned_app(&[NVME_S1]);
        a.handle(Event::Snapshot(drive_snapshot("other-address")));
        assert_eq!(a.pinned(), [NVME_S1.to_string()]);
    });
}

const PACKAGE: &str = "temperature/lm-sensors/coretemp-isa-0000/temp1";
const BOARD: &str = "temperature/lm-sensors/example_acpi-isa-0000/temp1";
const COMPOSITE: &str = "temperature/lm-sensors/nvme-pci-0100/temp1";
const FAN: &str = "fan/lm-sensors/example_acpi-isa-0000/fan1";

/// A new install checks the default lines of what it reads, and stores
/// nothing until a click; stored pins of older versions move once.
#[test]
fn c_applet_app_26() {
    puron_testkit::limit("C-applet.app-26", LIMIT, || {
        // New install: nothing stored.
        let c = cfg::build("p5");
        std::fs::remove_file(c.id_dir().join("pinned")).expect("no pins stored");
        let config = Config::new(c.root.clone());
        let h = handle(0);
        let mut a = app(&c.root, &h);
        let out = a.handle(Event::Start);
        assert!(a.pinned().is_empty());
        assert_eq!(panel_of(&out).unwrap().items, [Item::EmptyState]);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        let defaults = [PACKAGE.to_string(), COMPOSITE.to_string()];
        assert_eq!(a.pinned(), defaults);
        assert_eq!(pin_stores(&out), 0);
        assert_eq!(config.load().settings.pinned, None);
        assert_eq!(values(&panel_of(&out).unwrap()), ["48°C", "38°C"]);
        // A click: that list less the line, stored, and defaults no longer apply.
        let out = a.handle(Event::PinClick(COMPOSITE.to_string()));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![PACKAGE.to_string()])
        );
        a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        // Stored empty stays empty.
        let (_c, _h, mut a, _) = pinned_app(&[]);
        a.handle(Event::Snapshot(drive_snapshot("example")));
        assert!(a.pinned().is_empty());

        // Version 1.0.0's Maximum: the CPU's default line, once, no drive.
        let (_c, _h, mut a, config) = pinned_app(&["@maximum"]);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![PACKAGE.to_string()])
        );
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(pin_stores(&out), 0);
        // In its place, among others; a hidden class and no report yet wait.
        let (_c, _h, mut a, _) = pinned_app(&[FAN, "@maximum", BOARD]);
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(
            a.pinned(),
            [FAN.to_string(), PACKAGE.to_string(), BOARD.to_string()]
        );
        let (_c, _h, mut a, _) = pinned_app(&["@maximum"]);
        let mut early = cpu_snapshot("example");
        early.lm_sensors = puron_engine::engine::LmReport::NoneYet;
        let out = a.handle(Event::Snapshot(early));
        assert_eq!(a.pinned(), ["@maximum".to_string()]);
        assert_eq!(pin_stores(&out), 0);
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        // No CPU: the first line.
        let (_c, _h, mut a, _) = pinned_app(&["@maximum"]);
        a.handle(Event::Snapshot(cpu_snapshot("no-cpu")));
        assert_eq!(
            a.pinned(),
            ["temperature/lm-sensors/iwlwifi_1-virtual-0/temp1".to_string()]
        );

        // A development build's CPU pin, with what it was reading.
        let with_source = |source: &str| {
            let (c, h, _, _) = pinned_app(&["@cpu"]);
            let pairs = format!(r#"[("@cpu", "{source}")]"#);
            cfg::write(&c.id_dir().join("row_sources"), pairs.as_bytes());
            let mut a = app(&c.root, &h);
            a.handle(Event::Start);
            (c, a)
        };
        let (_c, mut a) = with_source("example_acpi-isa-0000/temp1");
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(a.pinned(), [BOARD.to_string()]);
        let (_c, mut a) = with_source("@maximum");
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(a.pinned(), ["@maximum/cpu".to_string()]);
        // A sensor that is gone, and no pair at all: the default line.
        let (_c, mut a) = with_source("nothing-here/temp1");
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        let (_c, _h, mut a, _) = pinned_app(&["@cpu"]);
        a.handle(Event::Snapshot(cpu_snapshot("monolith")));
        assert_eq!(a.pinned(), ["@maximum/cpu".to_string()]);
        // Both on the one line: one pin. Neither is lost, none doubled.
        let (_c, _h, mut a, config) = pinned_app(&["@maximum", "@cpu", PACKAGE]);
        a.handle(Event::Snapshot(cpu_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![PACKAGE.to_string()])
        );
    });
}

const MAXIMUM: &str = "@maximum";

/// A stored Maximum waits for readings of lm-sensors: a failed start or a
/// failed read moves nothing and stores nothing; the first readings move it,
/// once. With lm-sensors switched off it waits no more.
#[test]
fn c_applet_app_27() {
    puron_testkit::limit("C-applet.app-27", LIMIT, || {
        let (c, _h, mut a, config) = pinned_app(&[MAXIMUM]);
        let file = c.id_dir().join("pinned");
        let stored = std::fs::read(&file).unwrap();
        assert_eq!(a.pin_phase(), PinPhase::Waiting);
        for lm in [
            puron_engine::engine::LmReport::Unavailable,
            puron_engine::engine::LmReport::Answered,
        ] {
            let mut failed = drive_snapshot("lm-sensors-off");
            failed.lm_sensors = lm;
            let out = a.handle(Event::Snapshot(failed));
            assert_eq!(a.pinned(), [MAXIMUM.to_string()]);
            assert_eq!(pin_stores(&out), 0);
            assert_eq!(values(&panel_of(&out).unwrap()), ["⚠"]);
            assert_eq!(a.pin_phase(), PinPhase::Waiting);
        }
        assert_eq!(std::fs::read(&file).unwrap(), stored);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![PACKAGE.to_string()])
        );
        assert_eq!(a.pin_phase(), PinPhase::Settled);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(pin_stores(&out), 0);
        // lm-sensors switched off: the first device's default line.
        let (_c, _h, mut a, _) = pinned_app(&[MAXIMUM]);
        let off = delivery(&a, &["provider_lm_sensors"], |s| {
            s.provider_lm_sensors = false
        });
        a.handle(off);
        let out = a.handle(Event::Snapshot(drive_snapshot("lm-sensors-off")));
        assert_eq!(pin_stores(&out), 1);
        assert!(
            a.pinned()[0].starts_with("temperature/nvme-cli/"),
            "{:?}",
            a.pinned()
        );
        assert_eq!(a.pin_phase(), PinPhase::Settled);
    });
}

/// A click while a stored Maximum waits keeps it in the stored list, and it
/// still moves once lm-sensors reads.
#[test]
fn c_applet_app_28() {
    puron_testkit::limit("C-applet.app-28", LIMIT, || {
        let (_c, _h, mut a, config) = pinned_app(&[MAXIMUM]);
        let mut failed = drive_snapshot("lm-sensors-off");
        failed.lm_sensors = puron_engine::engine::LmReport::Unavailable;
        a.handle(Event::Snapshot(failed));
        let out = a.handle(Event::PinClick(NVME_S1.to_string()));
        let both = vec![MAXIMUM.to_string(), NVME_S1.to_string()];
        assert_eq!(a.pinned(), both);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(config.load().settings.pinned, Some(both));
        assert_eq!(a.pin_phase(), PinPhase::Waiting);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        let moved = vec![PACKAGE.to_string(), DRIVE_S1.to_string()];
        assert_eq!(a.pinned(), moved);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(config.load().settings.pinned, Some(moved));
    });
}

/// A stored pinned value that cannot be read is no new install: nothing is
/// checked, the stored value stays until a click, which replaces it.
#[test]
fn c_applet_app_29() {
    puron_testkit::limit("C-applet.app-29", LIMIT, || {
        let c = cfg::build("p5");
        let file = c.id_dir().join("pinned");
        cfg::write(&file, b"42");
        let config = Config::new(c.root.clone());
        let h = handle(0);
        let mut a = app(&c.root, &h);
        let out = a.handle(Event::Start);
        assert!(out.contains(&Output::Diag("config-problem")), "{out:?}");
        assert!(a.pinned().is_empty());
        assert_eq!(a.pin_phase(), PinPhase::Settled);
        for _ in 0..2 {
            let out = a.handle(Event::Snapshot(drive_snapshot("example")));
            assert!(a.pinned().is_empty());
            assert_eq!(pin_stores(&out), 0);
            assert_eq!(values(&panel_of(&out).unwrap()), ["∅state"]);
        }
        assert_eq!(std::fs::read(&file).unwrap(), b"42");
        let out = a.handle(Event::PinClick(PACKAGE.to_string()));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(
            config.load().settings.pinned,
            Some(vec![PACKAGE.to_string()])
        );
        // A read error: an entry with no read permission (not run as root).
        let c = cfg::build("p5");
        let file = c.id_dir().join("pinned");
        set_mode(&file, 0o000);
        let mut a = app(&c.root, &h);
        let out = a.handle(Event::Start);
        set_mode(&file, 0o600);
        assert!(out.contains(&Output::Diag("config-problem")), "{out:?}");
        assert_eq!(a.pin_phase(), PinPhase::Settled);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert!(a.pinned().is_empty());
        assert_eq!(pin_stores(&out), 0);
    });
}

fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A failed store of the pinned list is tried again at each snapshot, with
/// one diag event, until it is stored; then no more stores.
#[test]
fn pinned_store_retried() {
    puron_testkit::limit("pinned_store_retried", LIMIT, || {
        let (c, _h, mut a) = as_4();
        let (dir, file) = (c.id_dir(), c.id_dir().join("pinned"));
        set_mode(&file, 0o400);
        set_mode(&dir, 0o500);
        let fan = a.menu().unwrap().identities[2].clone();
        let out = a.handle(Event::PinClick(fan.clone()));
        let failed = |out: &[Output]| {
            out.iter()
                .filter(|o| matches!(o, Output::Store(_, StoreResult::Failed)))
                .count()
        };
        assert_eq!(failed(&out), 1);
        assert!(out.contains(&Output::Diag("pinned-store-failed")));
        assert!(a.store_pending());
        let out = a.handle(Event::Snapshot(capture()));
        assert_eq!((failed(&out), pin_stores(&out)), (1, 1));
        assert!(!out.contains(&Output::Diag("pinned-store-failed")));
        set_mode(&dir, 0o700);
        set_mode(&file, 0o600);
        let out = a.handle(Event::Snapshot(capture()));
        assert_eq!((failed(&out), pin_stores(&out)), (0, 1));
        assert!(!a.store_pending());
        let config = Config::new(c.root.clone());
        assert_eq!(config.load().settings.pinned.as_deref(), Some(a.pinned()));
        assert!(a.pinned().contains(&fan));
        let out = a.handle(Event::Snapshot(capture()));
        assert_eq!(pin_stores(&out), 0);
    });
}

/// A list written from outside during a new install is taken as stored: the
/// defaults are no longer checked, and a click builds on that list.
#[test]
fn outside_pins_end_new_install() {
    puron_testkit::limit("outside_pins_end_new_install", LIMIT, || {
        let c = cfg::build("p5");
        std::fs::remove_file(c.id_dir().join("pinned")).unwrap();
        let config = Config::new(c.root.clone());
        let h = handle(0);
        let mut a = app(&c.root, &h);
        a.handle(Event::Start);
        assert_eq!(a.pin_phase(), PinPhase::NewInstall);
        a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string(), COMPOSITE.to_string()]);
        let written = delivery(&a, &["pinned"], |s| {
            s.pinned = Some(vec![BOARD.to_string()])
        });
        let out = a.handle(written);
        assert_eq!(a.pinned(), [BOARD.to_string()]);
        assert_eq!(pin_stores(&out), 0);
        assert_eq!(values(&panel_of(&out).unwrap()).len(), 1);
        assert_eq!(a.pin_phase(), PinPhase::Settled);
        a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [BOARD.to_string()]);
        a.handle(Event::PinClick(PACKAGE.to_string()));
        let both = vec![BOARD.to_string(), PACKAGE.to_string()];
        assert_eq!(config.load().settings.pinned, Some(both));
    });
}

/// The popup of the latest layout in `out`, with the in-memory list.
fn popup_of(out: &[Output], pinned: &[String]) -> Option<puron_applet::popup::Plan> {
    out.iter().rev().find_map(|o| match o {
        Output::Layout { menu, .. } => Some(puron_applet::popup::plan(menu, pinned, 16)),
        _ => None,
    })
}

/// The texts and values of the plan's lines, the CPU group's first.
fn waiting_line(plan: &puron_applet::popup::Plan) -> Option<(String, String, bool, String)> {
    use puron_applet::popup::Drawn;
    let at = plan
        .entries
        .iter()
        .position(|e| matches!(e, Drawn::Line(l) if l.identity == MAXIMUM))?;
    let Drawn::Line(l) = &plan.entries[at] else {
        unreachable!()
    };
    let heading = match &plan.entries[..at]
        .iter()
        .rev()
        .find(|e| matches!(e, Drawn::Device { .. }))
    {
        Some(Drawn::Device { text }) => text.clone(),
        _ => String::new(),
    };
    Some((l.text.clone(), l.value.clone(), l.pinned, heading))
}

/// A stored Maximum that waits for lm-sensors has a line in the popup's CPU
/// group, with the warning sign; a click on it removes the pin. When
/// lm-sensors reads, the line goes and the pin moves.
#[test]
fn c_applet_app_30() {
    puron_testkit::limit("C-applet.app-30", LIMIT, || {
        puron_applet::start();
        let failed = || {
            let mut s = drive_snapshot("lm-sensors-off");
            s.lm_sensors = puron_engine::engine::LmReport::Unavailable;
            s
        };
        let want = (
            "CPU: waiting for lm-sensors".to_string(),
            "⚠".to_string(),
            true,
            "CPU".to_string(),
        );
        let (_c, _h, mut a, config) = pinned_app(&[MAXIMUM]);
        let out = a.handle(Event::Snapshot(failed()));
        let plan = popup_of(&out, a.pinned()).expect("a layout");
        assert_eq!(waiting_line(&plan), Some(want.clone()));
        // The CPU group comes before the drives.
        let first = plan.entries.iter().find_map(|e| match e {
            puron_applet::popup::Drawn::Device { text } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(first.as_deref(), Some("CPU"));
        // The click removes it, stores the list and lays the popup out
        // without the line.
        let out = a.handle(Event::PinClick(MAXIMUM.to_string()));
        assert!(a.pinned().is_empty(), "{:?}", a.pinned());
        assert_eq!(pin_stores(&out), 1);
        assert_eq!(config.load().settings.pinned, Some(vec![]));
        assert_eq!(a.pin_phase(), PinPhase::Settled);
        let plan = popup_of(&out, a.pinned()).expect("a layout at the click");
        assert_eq!(waiting_line(&plan), None);
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert!(a.pinned().is_empty());
        assert_eq!(pin_stores(&out), 0);
        // Not removed: once lm-sensors reads, the line goes and the pin moves.
        let (_c, _h, mut a, _) = pinned_app(&[MAXIMUM]);
        let out = a.handle(Event::Snapshot(failed()));
        assert_eq!(
            waiting_line(&popup_of(&out, a.pinned()).unwrap()),
            Some(want)
        );
        let out = a.handle(Event::Snapshot(drive_snapshot("example")));
        assert_eq!(a.pinned(), [PACKAGE.to_string()]);
        assert_eq!(waiting_line(&popup_of(&out, a.pinned()).unwrap()), None);
    });
}
