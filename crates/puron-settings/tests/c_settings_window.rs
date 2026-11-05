//! Tests of the settings window's state machine and window plan, and the
//! window settings checked by c_settings_window_17. Each test runs on its
//! own temporary configuration root.
#![allow(non_snake_case)]

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use puron_config::{Config, Delivery, StoreResult, Value};
use puron_core::settings::TemperatureUnit;
use puron_settings::window::{window_settings, Control, Event, Output, Plan, Window};
use puron_testkit::cfg::{self, Cfg};

/// Time limit of each plan test, in seconds.
const LIMIT: u64 = 5;
/// The wait for one delivery.
const WAIT: Duration = Duration::from_secs(2);

const WRITER: &str = "PURON_CFG_WRITER";

/// The config writer, as a child mode of this binary: stores
/// `$PURON_CFG_WRITER` = `<root>\t<key>\t<RON value>`, exit 0 on *stored*.
#[test]
fn x_cfg_writer() {
    let Ok(args) = std::env::var(WRITER) else {
        return;
    };
    // Exit 0 within 2 s on a writable root.
    puron_testkit::limit("X-cfg-writer", 2, move || {
        let mut parts = args.splitn(3, '\t');
        let (root, key, ron) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        assert_eq!(cfg::writer_main(Path::new(root), key, ron), 0, "stored");
    });
}

/// Runs the config writer as a second process and waits for its exit 0.
fn writer(root: &Path, key: &str, ron: &str) {
    let status = puron_testkit::rerun_self("x_cfg_writer")
        .env(WRITER, format!("{}\t{key}\t{ron}", root.display()))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run X-cfg-writer");
    assert!(status.success(), "X-cfg-writer {key} {ron}: {status}");
}

fn not_root() {
    puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
}

/// Opens a window on the fixture's root; checks that Open ends with
/// Watch start and stores nothing.
fn open(fx: &Cfg) -> Window {
    let mut w = Window::new(Config::new(&fx.root));
    assert_eq!(w.handle(Event::Open), vec![Output::WatchStart], "Open");
    w
}

/// Starts the watch the production loop starts after Open.
fn watch(fx: &Cfg) -> (puron_config::Watch, mpsc::Receiver<Delivery>) {
    let (tx, rx) = mpsc::channel();
    let watch = Config::new(&fx.root)
        .watch(move |d| {
            let _ = tx.send(d);
        })
        .expect("the watch starts");
    (watch, rx)
}

/// Hands the first delivery (within 2 s) and any that follow it at once
/// to the window; the stores they cause are returned.
fn deliver(w: &mut Window, rx: &mpsc::Receiver<Delivery>) -> Vec<Output> {
    let mut out = Vec::new();
    let first = rx.recv_timeout(WAIT).expect("a delivery within 2 s");
    out.extend(w.handle(Event::Delivery(first)));
    while let Ok(d) = rx.recv_timeout(Duration::from_millis(200)) {
        out.extend(w.handle(Event::Delivery(d)));
    }
    out
}

/// The Store list of outputs: (key, value).
fn stores(out: &[Output]) -> Vec<(&'static str, Value)> {
    out.iter()
        .filter_map(|o| match o {
            Output::Store { key, value, .. } => Some((*key, value.clone())),
            _ => None,
        })
        .collect()
}

fn results(out: &[Output]) -> Vec<StoreResult> {
    out.iter()
        .filter_map(|o| match o {
            Output::Store { result, .. } => Some(*result),
            _ => None,
        })
        .collect()
}

/// The items of the plan in order.
fn items(plan: &Plan) -> Vec<&puron_settings::window::Item> {
    plan.sections.iter().flat_map(|s| s.items.iter()).collect()
}

/// Item `n` (1 to 20) of the plan.
fn item(plan: &Plan, n: usize) -> puron_settings::window::Item {
    items(plan)[n - 1].clone()
}

/// The shown value of each item: the number, `on`/`off`, or the shown
/// list item's text.
fn shown(plan: &Plan) -> Vec<String> {
    items(plan)
        .iter()
        .map(|i| match &i.control {
            Control::Number { value, .. } => value.to_string(),
            Control::Switch(on) => if *on { "on" } else { "off" }.to_string(),
            Control::List { items, shown } => items[*shown].clone(),
        })
        .collect()
}

fn switch_of(plan: &Plan, n: usize) -> bool {
    match item(plan, n).control {
        Control::Switch(on) => on,
        c => panic!("item {n} is not a switch: {c:?}"),
    }
}

/// The number field: value, text, − enabled, + enabled.
fn field(plan: &Plan) -> (u32, String, bool, bool) {
    match item(plan, 1).control {
        Control::Number {
            value,
            text,
            minus,
            plus,
        } => (value, text, minus, plus),
        c => panic!("item 1 is not the number field: {c:?}"),
    }
}

fn toggle(w: &mut Window, n: usize) -> Vec<Output> {
    let on = switch_of(&w.plan(), n);
    w.handle(Event::Switch { item: n, on: !on })
}

fn tip(plan: &Plan, n: usize) -> Option<String> {
    item(plan, n).tooltip.map(|(_, t)| t)
}

/// `grep -c <pattern> i18n/en/puron.ftl`'s stdout.
fn grep_count(pattern: &str) -> String {
    let out = Command::new("grep")
        .args(["-c", pattern, "i18n/en/puron.ftl"])
        .current_dir(puron_testkit::workspace_root())
        .output()
        .expect("run grep");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// The setting defaults, in item order.
const DEFAULTS: [&str; 20] = [
    "5", "on", "°C", "on", "on", "on", "on", "off", "on", "off", "off", "off", "off", "on", "on",
    "on", "on", "on", "on", "on",
];
/// The values of the cfg fixture good.txt, in item order.
const GOOD: [&str; 20] = [
    "7", "off", "°F", "off", "off", "off", "off", "on", "off", "on", "on", "on", "on", "off",
    "off", "off", "off", "off", "off", "off",
];

#[test]
fn c_settings_window_1() {
    puron_testkit::limit("C-settings.window-1", LIMIT, || {
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        let mut dump = String::new();
        for s in &plan.sections {
            for i in &s.items {
                let kind = match i.control {
                    Control::Number { .. } => "number",
                    Control::Switch(_) => "switch",
                    Control::List { .. } => "list",
                };
                let tip = i.tooltip.as_ref().map_or("none", |(id, _)| id);
                dump += &format!("{}\t{}\t{kind}\t{}\t{tip}\n", s.heading, i.label, i.key);
            }
        }
        let got = fx.tmp.join("plan.tsv");
        std::fs::write(&got, dump).unwrap();
        let want = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/c_settings_window_1.tsv");
        let out = Command::new("diff")
            .arg(&want)
            .arg(&got)
            .output()
            .expect("run diff");
        assert!(
            out.status.success(),
            "diff exit {}:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout)
        );
        assert_eq!(plan.sections.len(), 6, "six headings");
    });
}

#[test]
fn c_settings_window_2() {
    puron_testkit::limit("C-settings.window-2", LIMIT, || {
        let empty = cfg::build("empty-none");
        assert_eq!(shown(&open(&empty).plan()), DEFAULTS, "X-cfg-empty");
        let good = cfg::build("good");
        assert_eq!(shown(&open(&good).plan()), GOOD, "X-cfg-good");
    });
}

#[test]
fn c_settings_window_3() {
    puron_testkit::limit("C-settings.window-3", LIMIT, || {
        let fx = cfg::build("wrong");
        let key = Path::new(cfg::ID_DIR).join("show_voltage");
        let mut before = fx.tree();
        before.remove(&key);
        let mut w = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = Vec::new();
        writer(&fx.root, "show_voltage", "true");
        out.extend(deliver(&mut w, &rx));
        let plan = w.plan();
        out.extend(w.handle(Event::Close));
        assert_eq!(stores(&out), vec![], "Store list");
        assert_eq!(out.last(), Some(&Output::Exit), "Close exits");
        let mut after = fx.tree();
        after.remove(&key);
        assert_eq!(after, before, "entries other than show_voltage, none added");
        assert!(switch_of(&plan, 17), "show_power shown on");
        assert_eq!(shown(&plan)[2], "°C", "temperature_unit shown");
    });
}

/// The keys of the 18 switches, items 2, 4 to 20.
const SWITCH_KEYS: [&str; 18] = [
    "show_panel_icons",
    "show_temperature_unit",
    "show_rotation_rate_unit",
    "show_voltage_unit",
    "show_power_unit",
    "show_decimals",
    "provider_lm_sensors",
    "provider_liquidctl",
    "provider_nvidia",
    "provider_udisks2",
    "provider_nvme",
    "show_temperature",
    "show_rotation_rate",
    "show_voltage",
    "show_power",
    "group_temperature",
    "group_rotation_rate",
    "group_voltage",
];

#[test]
fn c_settings_window_4() {
    puron_testkit::limit("C-settings.window-4", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        let switches: Vec<usize> = std::iter::once(2).chain(4..=20).collect();
        let mut out = Vec::new();
        for &n in &switches {
            out.extend(toggle(&mut w, n));
        }
        out.extend(w.handle(Event::Close));
        let want: Vec<(&str, Value)> = switches
            .iter()
            .zip(SWITCH_KEYS)
            .map(|(&n, k)| (k, Value::Bool(DEFAULTS[n - 1] == "off")))
            .collect();
        assert_eq!(stores(&out), want, "Store list");
        assert!(
            results(&out).iter().all(|r| *r == StoreResult::Stored),
            "each stored"
        );
        let loaded = Config::new(&fx.root).load();
        let mut again = Window::new(Config::new(&fx.root));
        again.handle(Event::Open);
        let got = shown(&again.plan());
        for &n in &switches {
            let want = if DEFAULTS[n - 1] == "on" { "off" } else { "on" };
            assert_eq!(got[n - 1], want, "item {n} after a new Load");
        }
        assert!(!loaded.settings.show_panel_icons && loaded.settings.provider_liquidctl);
    });
}

#[test]
fn c_settings_window_5() {
    puron_testkit::limit("C-settings.window-5", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        match item(&w.plan(), 3).control {
            Control::List { items, .. } => assert_eq!(items, ["°C", "°F"], "suffix- entries"),
            c => panic!("item 3: {c:?}"),
        }
        let mut out = w.handle(Event::Choose(1));
        out.extend(w.handle(Event::Choose(1)));
        out.extend(w.handle(Event::Choose(0)));
        assert_eq!(
            stores(&out),
            vec![
                (
                    "temperature_unit",
                    Value::TemperatureUnit(TemperatureUnit::Fahrenheit)
                ),
                (
                    "temperature_unit",
                    Value::TemperatureUnit(TemperatureUnit::Celsius)
                ),
            ],
            "Store list"
        );
    });
}

#[test]
fn c_settings_window_6() {
    puron_testkit::limit("C-settings.window-6", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        let mut out = w.handle(Event::Plus);
        out.extend(w.handle(Event::Minus));
        out.extend(w.handle(Event::Minus));
        assert_eq!(
            stores(&out),
            [6, 5, 4].map(|v| ("poll_interval_secs", Value::U32(v))),
            "Store list"
        );
        let low = cfg::build("range-0");
        let mut w = open(&low);
        assert_eq!(field(&w.plan()), (1, "1".into(), false, true), "at 1");
        assert_eq!(w.handle(Event::Minus), vec![], "− at 1");
        assert_eq!(field(&w.plan()).0, 1);
        let high = cfg::build("range-61");
        let mut w = open(&high);
        assert_eq!(field(&w.plan()), (60, "60".into(), true, false), "at 60");
        assert_eq!(w.handle(Event::Plus), vec![], "+ at 60");
        assert_eq!(field(&w.plan()).0, 60);
        // A press takes the typed text first, then steps.
        let mut w = open(&fx);
        let mut out = Vec::new();
        for (text, press) in [
            ("12", Event::Plus),
            ("3", Event::Minus),
            ("60", Event::Plus),
        ] {
            out.extend(w.handle(Event::Typed(text.into())));
            out.extend(w.handle(press));
        }
        assert_eq!(
            stores(&out),
            [12, 13, 3, 2, 60].map(|v| ("poll_interval_secs", Value::U32(v))),
            "Store list, typed then pressed"
        );
        assert_eq!(
            field(&w.plan()),
            (60, "60".into(), true, false),
            "after the presses"
        );
    });
}

#[test]
fn c_settings_window_7() {
    puron_testkit::limit("C-settings.window-7", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        let typed = [
            "12", "0", "61", " 7 ", "5.5", "+7", "7.9", "-3", "2e1", "9x", "abc", "30", "",
        ];
        let mut out = Vec::new();
        let mut last = 5;
        for text in typed {
            // Each keystroke is a Typed event; none stores.
            for end in 1..=text.len() {
                let o = w.handle(Event::Typed(text[..end].to_string()));
                assert_eq!(o, vec![], "keystroke of {text:?}");
            }
            w.handle(Event::Typed(text.to_string()));
            let o = w.handle(Event::Commit);
            if let Some((_, Value::U32(v))) = stores(&o).last() {
                last = *v;
            }
            out.extend(o);
            assert_eq!(field(&w.plan()).1, last.to_string(), "shown after {text:?}");
        }
        let want: Vec<_> = [12, 1, 60, 7, 5, 7, 1, 20, 9, 1, 30, 1]
            .map(|v| ("poll_interval_secs", Value::U32(v)))
            .to_vec();
        assert_eq!(stores(&out), want, "Store list");
    });
}

#[test]
fn c_settings_window_8() {
    puron_testkit::limit("C-settings.window-8", LIMIT, || {
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        assert_eq!(
            tip(&plan, 8).as_deref(),
            Some("Show additional digits after decimal point")
        );
        assert_eq!(grep_count("additionnal"), "0");
    });
}

#[test]
fn c_settings_window_9() {
    puron_testkit::limit("C-settings.window-9", LIMIT, || {
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        let got: Vec<_> = (18..=20).map(|n| tip(&plan, n)).collect();
        assert_eq!(
            got,
            [
                "Group temperature sensors (one or more)",
                "Group two or more rotation rate sensors",
                "Group two or more voltage sensors",
            ]
            .map(|s| Some(s.to_string()))
        );
        assert_eq!(grep_count("three or more"), "0");
    });
}

#[test]
fn c_settings_window_10() {
    puron_testkit::limit("C-settings.window-10", LIMIT, || {
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        let got: Vec<_> = (9..=17).map(|n| tip(&plan, n)).collect();
        let mut want = vec![
            Some("Read sensors from lm-sensors".to_string()),
            Some("Read sensors from liquidctl (requires v1.7.0 or later)".to_string()),
        ];
        want.extend(std::iter::repeat_n(None, 7));
        assert_eq!(got, want);
    });
}

fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(perms.mode() | 0o200);
    std::fs::set_permissions(path, perms).unwrap();
}

#[test]
fn c_settings_window_11() {
    puron_testkit::limit("C-settings.window-11", LIMIT, || {
        not_root();
        let fx = cfg::build("readonly");
        let mut w = open(&fx);
        let first = w.handle(Event::Switch { item: 8, on: false });
        assert_eq!(
            first,
            vec![Output::Store {
                key: "show_decimals",
                value: Value::Bool(false),
                result: StoreResult::Failed
            }],
            "first Store failed, nothing else"
        );
        assert!(!switch_of(&w.plan(), 8), "switch shown off");
        assert_eq!(fx.entry("show_decimals"), b"true", "entry still true");
        // Make the ID directory and v1 writable again.
        make_writable(&fx.root.join("cosmic/io.github.elalonde.Puron"));
        make_writable(&fx.id_dir());
        let mut out = w.handle(Event::Switch { item: 8, on: true });
        out.extend(w.handle(Event::Switch { item: 8, on: false }));
        assert_eq!(
            stores(&out),
            vec![
                ("show_decimals", Value::Bool(true)),
                ("show_decimals", Value::Bool(false))
            ]
        );
        assert_eq!(results(&out), vec![StoreResult::Stored; 2], "stored");
        assert_eq!(fx.entry("show_decimals"), b"false", "entry false");
    });
}

#[test]
fn c_settings_window_12() {
    puron_testkit::limit("C-settings.window-12", LIMIT, || {
        let fx = cfg::build("good");
        let mut w = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = Vec::new();
        writer(&fx.root, "show_power", "true");
        out.extend(deliver(&mut w, &rx));
        writer(&fx.root, "poll_interval_secs", "9");
        out.extend(deliver(&mut w, &rx));
        let plan = w.plan();
        assert!(switch_of(&plan, 17), "Show Power shown on");
        assert_eq!(field(&plan).0, 9, "the field 9");
        assert_eq!(field(&plan).1, "9", "the field's text 9");
        assert_eq!(stores(&out), vec![], "Store list");
    });
}

#[test]
fn c_settings_window_13() {
    puron_testkit::limit("C-settings.window-13", LIMIT, || {
        let fx = cfg::build("good");
        let mut w = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = w.handle(Event::Typed("3".into()));
        writer(&fx.root, "poll_interval_secs", "9");
        out.extend(deliver(&mut w, &rx));
        assert_eq!(field(&w.plan()).1, "3", "text through the delivery");
        out.extend(w.handle(Event::Commit));
        assert_eq!(
            stores(&out),
            vec![("poll_interval_secs", Value::U32(3))],
            "Store list"
        );
    });
}

const CHILD14: &str = "PURON_SETTINGS14_CHILD";

#[test]
fn c_settings_window_14() {
    puron_testkit::limit("C-settings.window-14", LIMIT, || {
        if std::env::var_os(CHILD14).is_none() {
            let mut cmd = puron_testkit::rerun_self("c_settings_window_14");
            for v in puron_testkit::loc::VARS {
                cmd.env_remove(v);
            }
            let out = cmd
                .env("LANG", "eo.UTF-8")
                .env(CHILD14, "1")
                .output()
                .expect("run child");
            assert!(
                out.status.success(),
                "child: {}\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
        use puron_settings::i18n;
        i18n::start_with(i18n::test_catalog::eo(), |k| std::env::var_os(k));
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        let en = puron_testkit::loc::english();
        let english = |t: &str| -> bool { en.iter().any(|(_, e)| e == t) };
        let eo = |t: &str| t.strip_prefix("EO ").is_some_and(english);
        assert_eq!(plan.title, "Puron", "title");
        let products = ["lm-sensors", "liquidctl", "Nvidia", "Udisks2", "nvme-cli"];
        for s in &plan.sections {
            assert!(eo(&s.heading), "heading {:?}", s.heading);
            for i in &s.items {
                if products.contains(&i.label.as_str()) {
                    assert!((9..=13).any(|n| item(&plan, n).label == i.label));
                } else {
                    assert!(eo(&i.label), "label {:?}", i.label);
                }
                if let Some((id, t)) = &i.tooltip {
                    assert!(eo(t), "tooltip {id} {t:?}");
                }
                if let Control::List { items, .. } = &i.control {
                    assert_eq!(items, &["EO °C", "EO °F"], "list items");
                }
            }
        }
        let labels: Vec<_> = (9..=13).map(|n| item(&plan, n).label).collect();
        assert_eq!(labels, products, "product names unchanged");
    });
}

/// The window settings part of tools/c_settings_window_17.sh, which
/// also checks the libcosmic features of puron-settings.
#[test]
fn c_settings_window_17() {
    puron_testkit::limit("C-settings.window-17", LIMIT, || {
        let s = window_settings();
        assert_eq!(s.title, "Puron");
        assert_eq!(s.app_id, "io.github.elalonde.Puron.Settings");
        assert_eq!(s.initial, (480.0, 720.0));
        assert_eq!(s.minimum, (360.0, 240.0));
        let fx = cfg::build("good");
        assert_eq!(open(&fx).plan().title, "Puron", "the plan's title");
    });
}
