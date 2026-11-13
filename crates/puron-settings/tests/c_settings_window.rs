//! Tests of `puron_settings::window`: what the window shows, what it
//! stores in response to each event, and how it follows changes made by
//! another process.
//!
//! The tests drive `Window` directly, without a display. Each one builds
//! its own settings tree from a fixture of `puron_testkit::cfg` in a
//! temporary directory. Where a test needs another process to store a
//! setting, it runs this test binary again as `x_cfg_writer`.
#![allow(non_snake_case)]

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use puron_config::{Config, Delivery, StoreResult, Value};
use puron_core::settings::TemperatureUnit;
use puron_settings::window::{window_settings, Control, Event, Output, Plan, Window};
use puron_testkit::cfg::{self, Cfg};

/// The time limit of each test, in seconds.
const LIMIT: u64 = 5;
/// How long to wait for a watch delivery.
const WAIT: Duration = Duration::from_secs(2);

/// The variable that turns this binary into the config writer, and holds
/// what it stores.
const WRITER: &str = "PURON_CFG_WRITER";

/// The config writer: not a test, but a helper that `writer` runs in a
/// second process. When `PURON_CFG_WRITER` holds `<root>\t<key>\t<RON
/// value>`, it stores that value and passes only if the store succeeds.
/// Without the variable it does nothing and passes.
#[test]
fn x_cfg_writer() {
    let Ok(args) = std::env::var(WRITER) else {
        return;
    };
    // The store must finish within 2 s.
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

/// Stores `ron` under `key` from a second process, as another program
/// would, and fails the test unless that store succeeds.
fn writer(root: &Path, key: &str, ron: &str) {
    let status = puron_testkit::rerun_self("x_cfg_writer")
        .env(WRITER, format!("{}\t{key}\t{ron}", root.display()))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run X-cfg-writer");
    assert!(status.success(), "X-cfg-writer {key} {ron}: {status}");
}

/// Fails the test when it runs as root, which can write any file and so
/// would defeat a test of a read-only settings directory.
fn not_root() {
    puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
}

/// Opens a window on the fixture's settings and checks that `Event::Open`
/// asks for the watch (`Output::WatchStart`) and stores nothing.
fn open(fx: &Cfg) -> Window {
    let mut w = Window::new(Config::new(&fx.root));
    assert_eq!(w.handle(Event::Open), vec![Output::WatchStart], "Open");
    w
}

/// Starts the watch that the program starts on `Output::WatchStart`, and
/// returns a receiver of its deliveries.
fn watch(fx: &Cfg) -> (puron_config::Watch, mpsc::Receiver<Delivery>) {
    let (tx, rx) = mpsc::channel();
    let watch = Config::new(&fx.root)
        .watch(move |d| {
            let _ = tx.send(d);
        })
        .expect("the watch starts");
    (watch, rx)
}

/// Waits up to 2 s for a delivery, hands it to the window, then hands on
/// any further delivery that arrives within 200 ms of the previous one.
/// Returns the window's outputs, so that a test can check they hold no
/// store.
fn deliver(w: &mut Window, rx: &mpsc::Receiver<Delivery>) -> Vec<Output> {
    let mut out = Vec::new();
    let first = rx.recv_timeout(WAIT).expect("a delivery within 2 s");
    out.extend(w.handle(Event::Delivery(first)));
    while let Ok(d) = rx.recv_timeout(Duration::from_millis(200)) {
        out.extend(w.handle(Event::Delivery(d)));
    }
    out
}

/// The key and value of each `Output::Store` in `out`, in order.
fn stores(out: &[Output]) -> Vec<(&'static str, Value)> {
    out.iter()
        .filter_map(|o| match o {
            Output::Store { key, value, .. } => Some((*key, value.clone())),
            _ => None,
        })
        .collect()
}

/// The result of each `Output::Store` in `out`, in order.
fn results(out: &[Output]) -> Vec<StoreResult> {
    out.iter()
        .filter_map(|o| match o {
            Output::Store { result, .. } => Some(*result),
            _ => None,
        })
        .collect()
}

/// Every item of `plan`, across its sections, in window order.
fn items(plan: &Plan) -> Vec<&puron_settings::window::Item> {
    plan.sections.iter().flat_map(|s| s.items.iter()).collect()
}

/// Item number `n` (1 to 18) of `plan`.
fn item(plan: &Plan, n: usize) -> puron_settings::window::Item {
    items(plan)[n - 1].clone()
}

/// What each item shows, as text: the number field's value, `on` or
/// `off` for a switch, or the text of the selected list entry.
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

/// The number field's state: its value, its text, and whether − and +
/// are enabled.
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

/// How many lines of the English translation file `i18n/en/puron.ftl`
/// match `pattern`, as `grep -c` prints it.
fn grep_count(pattern: &str) -> String {
    let out = Command::new("grep")
        .args(["-c", pattern, "i18n/en/puron.ftl"])
        .current_dir(puron_testkit::workspace_root())
        .output()
        .expect("run grep");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// What each item shows when nothing is stored, in item order.
const DEFAULTS: [&str; 18] = [
    "5", "on", "°C", "on", "on", "on", "on", "off", "on", "off", "off", "on", "off", "off", "on",
    "on", "on", "on",
];
/// What each item shows for the `good` fixture, in item order. Every value
/// differs from its default.
const GOOD: [&str; 18] = [
    "7", "off", "°F", "off", "off", "off", "off", "on", "off", "on", "on", "off", "on", "on",
    "off", "off", "off", "off",
];

/// The plan's layout (each item's heading, label, control kind, key and
/// tooltip ID) matches the expected table `c_settings_window_1.tsv` line
/// for line, with five sections and 18 items.
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
        assert_eq!(plan.sections.len(), 5, "five headings");
        assert_eq!(items(&plan).len(), 18, "eighteen items");
    });
}

/// With nothing stored, every item shows its default; with the `good`
/// fixture, every item shows the stored value.
#[test]
fn c_settings_window_2() {
    puron_testkit::limit("C-settings.window-2", LIMIT, || {
        let empty = cfg::build("empty-none");
        assert_eq!(shown(&open(&empty).plan()), DEFAULTS, "X-cfg-empty");
        let good = cfg::build("good");
        assert_eq!(shown(&open(&good).plan()), GOOD, "X-cfg-good");
    });
}

/// Opening a window on entries of the wrong type, then receiving a change
/// made by another process, and closing, stores nothing and changes no
/// other entry. The items whose entries are of the wrong type show their
/// defaults, and closing returns `Output::Exit`.
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
        assert!(switch_of(&plan, 18), "show_power shown on");
        assert_eq!(shown(&plan)[2], "°C", "temperature_unit shown");
    });
}

/// The keys of the 16 switches (items 2 and 4 to 18), in item order.
const SWITCH_KEYS: [&str; 16] = [
    "show_panel_icons",
    "show_temperature_unit",
    "show_rotation_rate_unit",
    "show_voltage_unit",
    "show_power_unit",
    "show_decimals",
    "provider_lm_sensors",
    "provider_liquidctl",
    "provider_nvidia",
    "provider_lm_sensors_drives",
    "provider_udisks2",
    "provider_nvme",
    "show_temperature",
    "show_rotation_rate",
    "show_voltage",
    "show_power",
];

/// Flipping every switch once from its default stores each new value,
/// under the switch's own key, in order, and each store succeeds. A new
/// window opened afterwards shows every switch flipped.
#[test]
fn c_settings_window_4() {
    puron_testkit::limit("C-settings.window-4", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        let switches: Vec<usize> = std::iter::once(2).chain(4..=18).collect();
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

/// The unit list offers °C and °F. Choosing °F stores it; choosing °F
/// again stores nothing; choosing °C stores it.
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

/// The + and − buttons step the poll interval by one and store each new
/// value. At 1, − is disabled and a press does nothing; at 60, the same
/// holds for +. With text typed, a press first stores the typed value,
/// then steps from it, unless that value is already at the limit.
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
        // Typed text is committed before the press steps: "12" then +
        // stores 12 and 13; "60" then + stores only 60.
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

/// Typed text is stored only when it is committed, as `read_interval`
/// reads it, and only when the result differs from the shown value. After
/// each commit the field shows the value, not the typed text.
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
            // Every prefix of the text arrives as a `Typed` event, as
            // keystrokes would; none of them stores.
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

/// Item 8's tooltip reads "Show additional digits after decimal point",
/// and the English translation file has no line with the misspelling
/// "additionnal".
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

/// Of items 9 to 18, only the lm-sensors, liquidctl and "lm-sensors
/// (drives)" switches (items 9, 10 and 12) have a tooltip, with the
/// expected text.
#[test]
fn c_settings_window_10() {
    puron_testkit::limit("C-settings.window-10", LIMIT, || {
        let fx = cfg::build("good");
        let plan = open(&fx).plan();
        let got: Vec<_> = (9..=18).map(|n| tip(&plan, n)).collect();
        let mut want = vec![
            Some("Read sensors from lm-sensors".to_string()),
            Some("Read sensors from liquidctl (requires v1.7.0 or later)".to_string()),
            None,
            Some("Read drive temperatures from lm-sensors".to_string()),
        ];
        want.extend(std::iter::repeat_n(None, 6));
        assert_eq!(got, want);
    });
}

/// Adds write permission for the owner of `path`.
fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(perms.mode() | 0o200);
    std::fs::set_permissions(path, perms).unwrap();
}

/// In a read-only settings directory, flipping a switch reports one
/// failed store, the switch keeps its new position, the entry keeps its
/// old value, and the window shows a notice. Once the directory is
/// writable again, the next two flips store and succeed, and the first
/// of them removes the notice.
#[test]
fn c_settings_window_11() {
    puron_testkit::limit("C-settings.window-11", LIMIT, || {
        not_root();
        let fx = cfg::build("readonly");
        let mut w = open(&fx);
        assert_eq!(w.plan().notice, None, "no notice at open");
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
        assert_eq!(
            w.plan().notice.as_deref(),
            Some("Settings could not be saved"),
            "notice after the failed store"
        );
        // Make the settings directory and its parent writable again.
        make_writable(&fx.root.join("cosmic/io.github.elalonde.Puron"));
        make_writable(&fx.id_dir());
        let mut out = w.handle(Event::Switch { item: 8, on: true });
        assert_eq!(w.plan().notice, None, "notice gone after a stored value");
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

/// Closing the window commits typed text as a focus loss does, then the
/// program exits. An emptied field stores nothing, and text already
/// committed by Enter is not stored a second time.
#[test]
fn c_settings_window_29() {
    puron_testkit::limit("C-settings.window-29", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        assert_eq!(w.handle(Event::Typed("30".to_string())), vec![]);
        assert_eq!(
            w.handle(Event::Close),
            vec![
                Output::Store {
                    key: "poll_interval_secs",
                    value: Value::U32(30),
                    result: StoreResult::Stored
                },
                Output::Exit
            ],
            "typed text stored, then exit"
        );
        assert_eq!(fx.entry("poll_interval_secs"), b"30", "entry 30");

        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        w.handle(Event::Typed(String::new()));
        assert_eq!(w.handle(Event::Close), vec![Output::Exit], "empty text");

        let fx = cfg::build("empty-none");
        let mut w = open(&fx);
        w.handle(Event::Typed("12".to_string()));
        let out = w.handle(Event::Commit);
        assert_eq!(stores(&out), [("poll_interval_secs", Value::U32(12))]);
        assert_eq!(w.handle(Event::Close), vec![Output::Exit], "committed");
    });
}

/// Values stored by another process reach the open window through the
/// watch: the switch and the number field show them, and the window
/// stores nothing in return.
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
        assert!(switch_of(&plan, 18), "Show Power shown on");
        assert_eq!(field(&plan).0, 9, "the field 9");
        assert_eq!(field(&plan).1, "9", "the field's text 9");
        assert_eq!(stores(&out), vec![], "Store list");
    });
}

/// Text being typed survives a change from another process: after the
/// delivery the field still shows "3", and the commit then stores 3.
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

/// Set in the child process of `c_settings_window_14`.
const CHILD14: &str = "PURON_SETTINGS14_CHILD";

/// Under a test catalog that prefixes every English text with "EO ", the
/// headings, labels, tooltips and list entries are translated, while the
/// title and the five product names (items 9 to 11, 13 and 14) stay as
/// they are. Item 12, "lm-sensors (drives only)", holds a word and is
/// translated.
/// The check runs in a child process with `LANG=eo.UTF-8` and the other
/// locale variables removed.
#[test]
fn c_settings_window_14() {
    /// The items whose label is a product name.
    const PRODUCT_ITEMS: [usize; 5] = [9, 10, 11, 13, 14];
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
                    assert!(PRODUCT_ITEMS
                        .iter()
                        .any(|&n| item(&plan, n).label == i.label));
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
        let labels: Vec<_> = PRODUCT_ITEMS.map(|n| item(&plan, n).label).to_vec();
        assert_eq!(labels, products, "product names unchanged");
        assert_eq!(item(&plan, 12).label, "EO lm-sensors (drives only)");
    });
}

/// The window's title, application ID, initial size and minimum size are
/// the expected ones, and the plan has the same title.
/// `tools/c_settings_window_17.sh` runs this test and also checks the
/// libcosmic features that puron-settings enables.
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

/// Set in the second process of `c_settings_window_28`.
const NO_ROOT: &str = "PURON_SWIN_NO_ROOT";

/// No configuration root: the test runs again in a second process without
/// `HOME`, `XDG_CONFIG_HOME`, `FLATPAK_ID` and `HOST_XDG_CONFIG_HOME`, in
/// an empty current directory. A window on `Config::user` opens, then the
/// switch of item 8 is flipped. Expects the defaults shown, one store
/// of that switch that returns `Failed`, and the current directory still
/// empty: nothing is written relative to it.
#[test]
fn c_settings_window_28() {
    puron_testkit::limit("C-settings.window-28", LIMIT, || {
        if std::env::var_os(NO_ROOT).is_some() {
            assert_eq!(puron_config::user_root(), None);
            let mut w = Window::new(Config::user());
            assert_eq!(w.handle(Event::Open), vec![Output::WatchStart], "Open");
            assert_eq!(shown(&w.plan()), DEFAULTS);
            let out = toggle(&mut w, 8);
            assert_eq!(results(&out), [StoreResult::Failed]);
            return;
        }
        let cwd = puron_testkit::temp_dir("swin-noroot");
        let status = puron_testkit::rerun_self("c_settings_window_28")
            .env(NO_ROOT, "1")
            .env_remove("HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("FLATPAK_ID")
            .env_remove("HOST_XDG_CONFIG_HOME")
            .current_dir(&cwd)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("run the second process");
        assert!(status.success(), "the second process: {status}");
        let left: Vec<_> = std::fs::read_dir(&cwd)
            .expect("read the current directory")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert!(
            left.is_empty(),
            "written in the current directory: {left:?}"
        );
    });
}
