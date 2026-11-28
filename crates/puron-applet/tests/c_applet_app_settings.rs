//! Tests of the Settings page through the applet's state machine: what
//! each control's change stores, how the shown values follow changes
//! reported by the settings watch, what leaving the page takes, and where
//! the page stands among the popup's pages.
//!
//! The tests drive `App` with a fake engine handle and without a display.
//! Each one builds its own settings tree from a fixture of
//! `puron_testkit::cfg` in a temporary directory. Where a test needs
//! another process to store a setting, it runs this test binary again as
//! `x_cfg_writer`.
#![allow(non_snake_case)]

mod common;

use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use common::*;
use puron_applet::app::{App, Command, Event, Output, Page};
use puron_applet::settings::{Choice, Edit, Item, Kind, Plan};
use puron_applet::view::View;
use puron_config::{Config, Delivery, StoreResult, Value};
use puron_core::menu::Update;
use puron_core::settings::{TemperatureUnit, TextSize};
use puron_testkit::cfg::{self, Cfg};
use puron_testkit::eng;

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

/// An applet over the fixture's settings, started, with the Settings
/// page shown. The event Settings stores nothing.
fn open_with(fx: &Cfg, h: &Arc<eng::Handle>) -> App {
    common::start_in_english();
    let mut a = app(&fx.root, h);
    a.handle(Event::Start);
    assert_eq!(stores(&a.handle(Event::Settings)), vec![], "Settings");
    a
}

fn open(fx: &Cfg) -> App {
    open_with(fx, &handle(0))
}

/// The Settings plan that the applet draws now.
fn plan(a: &App) -> Plan {
    View::of(a).settings_page.expect("a Settings plan")
}

/// Starts a watch of the fixture's settings, as the event loop does, and
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

/// Waits up to 2 s for a delivery, hands it to the applet, then hands on
/// any further delivery that arrives within 200 ms of the previous one.
/// Returns the applet's outputs.
fn deliver(a: &mut App, rx: &mpsc::Receiver<Delivery>) -> Vec<Output> {
    let mut out = Vec::new();
    let first = rx.recv_timeout(WAIT).expect("a delivery within 2 s");
    out.extend(a.handle(Event::Delivery(first)));
    while let Ok(d) = rx.recv_timeout(Duration::from_millis(200)) {
        out.extend(a.handle(Event::Delivery(d)));
    }
    out
}

/// The key and value of each stored setting in `out`, in order.
fn stores(out: &[Output]) -> Vec<(&'static str, Value)> {
    out.iter()
        .filter_map(|o| match o {
            Output::Setting { key, value, .. } => Some((*key, value.clone())),
            _ => None,
        })
        .collect()
}

/// The result of each stored setting in `out`, in order.
fn results(out: &[Output]) -> Vec<StoreResult> {
    out.iter()
        .filter_map(|o| match o {
            Output::Setting { result, .. } => Some(*result),
            _ => None,
        })
        .collect()
}

fn item(plan: &Plan, n: usize) -> Item {
    plan.items()[n - 1].clone()
}

/// What each item shows, as text: the number field's value, `on` or
/// `off` for a switch, or the text of the selected list entry.
fn shown(plan: &Plan) -> Vec<String> {
    plan.items()
        .iter()
        .map(|i| match &i.control {
            Kind::Number { value, .. } => value.to_string(),
            Kind::Switch(on) => if *on { "on" } else { "off" }.to_string(),
            Kind::List { items, shown } => items[*shown].clone(),
        })
        .collect()
}

fn switch_of(plan: &Plan, n: usize) -> bool {
    match item(plan, n).control {
        Kind::Switch(on) => on,
        c => panic!("item {n} is not a switch: {c:?}"),
    }
}

/// The number field's state: its value, its text, and whether − and +
/// are enabled.
fn field(plan: &Plan) -> (u32, String, bool, bool) {
    match item(plan, 1).control {
        Kind::Number {
            value,
            text,
            minus,
            plus,
            ..
        } => (value, text, minus, plus),
        c => panic!("item 1 is not the number field: {c:?}"),
    }
}

fn edit(a: &mut App, e: Edit) -> Vec<Output> {
    a.handle(Event::Edit(e))
}

fn set_switch(a: &mut App, n: usize, on: bool) -> Vec<Output> {
    edit(
        a,
        Edit::Change {
            item: n,
            choice: Choice::Switch(on),
        },
    )
}

/// Turns the switch of item `n` to the other position.
fn toggle(a: &mut App, n: usize) -> Vec<Output> {
    let on = switch_of(&plan(a), n);
    set_switch(a, n, !on)
}

/// What each item shows when nothing is stored, in item order.
const DEFAULTS: [&str; 20] = [
    "5",
    "on",
    "Match panel size",
    "Match dock size",
    "°C",
    "on",
    "on",
    "on",
    "on",
    "off",
    "on",
    "off",
    "off",
    "on",
    "off",
    "off",
    "on",
    "on",
    "on",
    "on",
];

/// Showing the page on entries of the wrong type, receiving a change that
/// another process made, going Back, showing the page again and closing
/// the popup stores nothing and changes no other entry. The items whose
/// entries are of the wrong type show their defaults.
#[test]
fn c_applet_app_35() {
    puron_testkit::limit("C-applet.app-35", LIMIT, || {
        let fx = cfg::build("wrong");
        let key = Path::new(cfg::ID_DIR).join("show_voltage");
        let mut before = fx.tree();
        before.remove(&key);
        let mut a = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = Vec::new();
        writer(&fx.root, "show_voltage", "true");
        out.extend(deliver(&mut a, &rx));
        let shown_plan = plan(&a);
        out.extend(a.handle(Event::Back));
        out.extend(a.handle(Event::Settings));
        out.extend(a.handle(Event::PopupClosed));
        assert_eq!(stores(&out), vec![], "Store list");
        let mut after = fx.tree();
        after.remove(&key);
        assert_eq!(after, before, "entries other than show_voltage, none added");
        assert!(switch_of(&shown_plan, 20), "show_power shown on");
        assert_eq!(shown(&shown_plan)[4], "°C", "temperature_unit shown");
    });
}

/// The keys of the 16 switches (items 2 and 6 to 20), in item order.
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

/// Turning every switch once from its default stores each new value at
/// once, under the switch's own key, in order, and each store succeeds.
/// A new load of the settings gives every switch turned.
#[test]
fn c_applet_app_36() {
    puron_testkit::limit("C-applet.app-36", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        let switches: Vec<usize> = std::iter::once(2).chain(6..=20).collect();
        let mut out = Vec::new();
        for &n in &switches {
            let o = toggle(&mut a, n);
            assert_eq!(stores(&o).len(), 1, "item {n} stores at its change");
            out.extend(o);
        }
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
        let mut again = open(&fx);
        let got = shown(&plan(&again));
        for &n in &switches {
            let want = if DEFAULTS[n - 1] == "on" { "off" } else { "on" };
            assert_eq!(got[n - 1], want, "item {n} after a new Load");
        }
        assert_eq!(stores(&again.handle(Event::Back)), vec![]);
    });
}

/// The unit list offers °C and °F. Choosing °F stores it; choosing °F
/// again stores nothing; choosing °C stores it.
#[test]
fn c_applet_app_37() {
    puron_testkit::limit("C-applet.app-37", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        match item(&plan(&a), 5).control {
            Kind::List { items, .. } => assert_eq!(items, ["°C", "°F"], "suffix- entries"),
            c => panic!("item 5: {c:?}"),
        }
        let choose = |index| {
            Event::Edit(Edit::Change {
                item: 5,
                choice: Choice::Entry(index),
            })
        };
        let mut out = a.handle(choose(1));
        out.extend(a.handle(choose(1)));
        out.extend(a.handle(choose(0)));
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
fn c_applet_app_38() {
    puron_testkit::limit("C-applet.app-38", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        let mut out = edit(&mut a, Edit::Plus);
        out.extend(edit(&mut a, Edit::Minus));
        out.extend(edit(&mut a, Edit::Minus));
        assert_eq!(
            stores(&out),
            [6, 5, 4].map(|v| ("poll_interval_secs", Value::U32(v))),
            "Store list"
        );
        let low = cfg::build("range-0");
        let mut a = open(&low);
        assert_eq!(field(&plan(&a)), (1, "1".into(), false, true), "at 1");
        assert_eq!(stores(&edit(&mut a, Edit::Minus)), vec![], "− at 1");
        assert_eq!(field(&plan(&a)).0, 1);
        let high = cfg::build("range-61");
        let mut a = open(&high);
        assert_eq!(field(&plan(&a)), (60, "60".into(), true, false), "at 60");
        assert_eq!(stores(&edit(&mut a, Edit::Plus)), vec![], "+ at 60");
        assert_eq!(field(&plan(&a)).0, 60);
        // Typed text is taken before the press steps: "12" then + stores
        // 12 and 13; "60" then + stores only 60.
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        let mut out = Vec::new();
        for (text, press) in [("12", Edit::Plus), ("3", Edit::Minus), ("60", Edit::Plus)] {
            out.extend(edit(&mut a, Edit::Typed(text.into())));
            out.extend(edit(&mut a, press));
        }
        assert_eq!(
            stores(&out),
            [12, 13, 3, 2, 60].map(|v| ("poll_interval_secs", Value::U32(v))),
            "Store list, typed then pressed"
        );
        assert_eq!(
            field(&plan(&a)),
            (60, "60".into(), true, false),
            "after the presses"
        );
    });
}

/// Typed text is stored only when it is taken, by Enter or by a focus
/// loss, as the field reads it, and only when the result differs from the
/// shown value. After each one the field shows the value, not the typed
/// text.
#[test]
fn c_applet_app_39() {
    puron_testkit::limit("C-applet.app-39", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        let typed = [
            "12", "0", "61", " 7 ", "5.5", "+7", "7.9", "-3", "2e1", "9x", "abc", "30", "",
        ];
        let mut out = Vec::new();
        let mut last = 5;
        for (k, text) in typed.into_iter().enumerate() {
            // Every prefix of the text arrives as a `Typed` message, as
            // keystrokes would; none of them stores.
            for end in 1..=text.len() {
                let o = edit(&mut a, Edit::Typed(text[..end].to_string()));
                assert_eq!(stores(&o), vec![], "keystroke of {text:?}");
            }
            edit(&mut a, Edit::Typed(text.to_string()));
            let take = if k % 2 == 0 {
                Edit::Enter
            } else {
                Edit::FocusLost
            };
            let o = edit(&mut a, take);
            if let Some((_, Value::U32(v))) = stores(&o).last() {
                last = *v;
            }
            out.extend(o);
            assert_eq!(field(&plan(&a)).1, last.to_string(), "shown after {text:?}");
        }
        let want: Vec<_> = [12, 1, 60, 7, 5, 7, 1, 20, 9, 1, 30, 1]
            .map(|v| ("poll_interval_secs", Value::U32(v)))
            .to_vec();
        assert_eq!(stores(&out), want, "Store list");
    });
}

/// Adds write permission for the owner of `path`.
fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(perms.mode() | 0o200);
    std::fs::set_permissions(path, perms).unwrap();
}

/// In a read-only settings directory, turning a switch off reports one
/// failed store, sends no engine command, keeps the switch off, leaves
/// the entry as it was and shows the notice. Once the directory is
/// writable again, the next two changes store and succeed, the first of
/// them removes the notice, and the delivery of the last one asks the
/// engine for one poll.
#[test]
fn c_applet_app_40() {
    puron_testkit::limit("C-applet.app-40", LIMIT, || {
        not_root();
        let fx = cfg::build("readonly");
        let h = handle(0);
        let mut a = open_with(&fx, &h);
        let (_watch, rx) = watch(&fx);
        assert_eq!(plan(&a).notice(), None, "no notice at Settings");
        let first = set_switch(&mut a, 10, false);
        assert_eq!(
            first,
            vec![Output::Setting {
                key: "show_decimals",
                value: Value::Bool(false),
                result: StoreResult::Failed
            }],
            "first Store failed, nothing else"
        );
        assert!(!switch_of(&plan(&a), 10), "switch shown off");
        assert_eq!(fx.entry("show_decimals"), b"true", "entry still true");
        assert_eq!(
            plan(&a).notice(),
            Some("Settings could not be saved"),
            "notice after the failed store"
        );
        // A failed store reaches no watch, so nothing is applied.
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(a.settings().show_decimals, "the failed value not applied");
        // Make the settings directory and its parent writable again.
        make_writable(&fx.root.join("cosmic/io.github.elalonde.Puron"));
        make_writable(&fx.id_dir());
        let mut out = set_switch(&mut a, 10, true);
        assert_eq!(plan(&a).notice(), None, "notice gone after a stored value");
        out.extend(set_switch(&mut a, 10, false));
        assert_eq!(
            stores(&out),
            vec![
                ("show_decimals", Value::Bool(true)),
                ("show_decimals", Value::Bool(false))
            ]
        );
        assert_eq!(results(&out), vec![StoreResult::Stored; 2], "stored");
        assert_eq!(commands(&out), vec![], "no engine command at a change");
        assert_eq!(fx.entry("show_decimals"), b"false", "entry false");
        let delivered = deliver(&mut a, &rx);
        assert_eq!(commands(&delivered), vec![Command::PollNow], "its delivery");
    });
}

/// Values stored by another process reach the shown page through the
/// watch: the switch and the number field show them, and the applet
/// stores nothing in return.
#[test]
fn c_applet_app_41() {
    puron_testkit::limit("C-applet.app-41", LIMIT, || {
        let fx = cfg::build("good");
        let mut a = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = Vec::new();
        writer(&fx.root, "show_power", "true");
        out.extend(deliver(&mut a, &rx));
        writer(&fx.root, "poll_interval_secs", "9");
        out.extend(deliver(&mut a, &rx));
        let p = plan(&a);
        assert!(switch_of(&p, 20), "Show Power shown on");
        assert_eq!(field(&p).0, 9, "the field 9");
        assert_eq!(field(&p).1, "9", "the field's text 9");
        assert_eq!(stores(&out), vec![], "Store list");
    });
}

/// Text being typed survives a change from another process: after the
/// delivery the field still shows "3", and Enter then stores 3.
#[test]
fn c_applet_app_42() {
    puron_testkit::limit("C-applet.app-42", LIMIT, || {
        let fx = cfg::build("good");
        let mut a = open(&fx);
        let (_watch, rx) = watch(&fx);
        let mut out = edit(&mut a, Edit::Typed("3".into()));
        writer(&fx.root, "poll_interval_secs", "9");
        out.extend(deliver(&mut a, &rx));
        assert_eq!(field(&plan(&a)).1, "3", "text through the delivery");
        out.extend(edit(&mut a, Edit::Enter));
        assert_eq!(
            stores(&out),
            vec![("poll_interval_secs", Value::U32(3))],
            "Store list"
        );
    });
}

/// Set in the second process of `c_applet_app_43`.
const NO_ROOT: &str = "PURON_SETTINGS_PAGE_NO_ROOT";

/// No configuration root: the test runs again in a second process without
/// `HOME`, `XDG_CONFIG_HOME`, `FLATPAK_ID` and `HOST_XDG_CONFIG_HOME`, in
/// an empty current directory. The applet over `Config::user` starts and
/// shows the Settings page, then the switch of item 10 is turned.
/// Expects no root and no watch, the defaults shown, one store of that
/// switch that returns `Failed` with the notice, and the current
/// directory still empty: nothing is written relative to it.
#[test]
fn c_applet_app_43() {
    puron_testkit::limit("C-applet.app-43", LIMIT, || {
        if std::env::var_os(NO_ROOT).is_some() {
            assert_eq!(puron_config::user_root(), None);
            assert!(Config::user().watch(|_| {}).is_none(), "no watch");
            common::start_in_english();
            let mut a = App::new(Arc::new(H(handle(0))), Config::user(), no_programs());
            a.handle(Event::Start);
            a.handle(Event::Settings);
            assert_eq!(shown(&plan(&a)), DEFAULTS);
            let out = toggle(&mut a, 10);
            assert_eq!(results(&out), [StoreResult::Failed]);
            assert_eq!(plan(&a).notice(), Some("Settings could not be saved"));
            return;
        }
        let cwd = puron_testkit::temp_dir("spage-noroot");
        let status = puron_testkit::rerun_self("c_applet_app_43")
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

/// Leaving the page takes typed text as a focus loss does: "Back" stores
/// it and shows the temperatures, and so does the popup closing. An
/// emptied field stores nothing, and text already taken by Enter is not
/// stored a second time when the popup closes.
#[test]
fn c_applet_app_44() {
    puron_testkit::limit("C-applet.app-44", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed("30".to_string()));
        let out = a.handle(Event::Back);
        assert_eq!(
            out,
            vec![Output::Setting {
                key: "poll_interval_secs",
                value: Value::U32(30),
                result: StoreResult::Stored
            }],
            "typed text stored at Back"
        );
        assert_eq!(fx.entry("poll_interval_secs"), b"30", "entry 30");
        assert_eq!(a.page(), Page::Temperatures, "Back shows the temperatures");

        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed("25".to_string()));
        let out = a.handle(Event::PopupClosed);
        assert_eq!(stores(&out), [("poll_interval_secs", Value::U32(25))]);
        assert_eq!(results(&out), [StoreResult::Stored], "popup closed");

        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed(String::new()));
        assert_eq!(stores(&a.handle(Event::Back)), vec![], "empty text");

        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed("12".to_string()));
        let out = edit(&mut a, Edit::Enter);
        assert_eq!(stores(&out), [("poll_interval_secs", Value::U32(12))]);
        assert_eq!(
            stores(&a.handle(Event::PopupClosed)),
            vec![],
            "already taken"
        );
    });
}

/// A field emptied by keystrokes stores nothing when it loses the focus.
/// The text input reports its focus loss before the click on "Back"
/// arrives, so this is the order a user who clears the field and leaves
/// produces. With 30 stored, the focus loss stores nothing and the field
/// shows 30 again, then "Back" stores nothing and shows the temperatures;
/// the popup closing in place of "Back" stores nothing either. The entry
/// stays 30 in both cases, not the 1 that an empty text reads as.
#[test]
fn c_applet_app_47() {
    puron_testkit::limit("C-applet.app-47", LIMIT, || {
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed("30".to_string()));
        let out = edit(&mut a, Edit::Enter);
        assert_eq!(stores(&out), [("poll_interval_secs", Value::U32(30))]);
        edit(&mut a, Edit::Typed("3".to_string()));
        edit(&mut a, Edit::Typed(String::new()));
        let out = edit(&mut a, Edit::FocusLost);
        assert_eq!(stores(&out), vec![], "focus loss on an empty text");
        let (value, text, _, _) = field(&plan(&a));
        assert_eq!((value, text.as_str()), (30, "30"), "field after focus loss");
        assert_eq!(
            fx.entry("poll_interval_secs"),
            b"30",
            "entry after focus loss"
        );
        assert_eq!(stores(&a.handle(Event::Back)), vec![], "Back");
        assert_eq!(fx.entry("poll_interval_secs"), b"30", "entry after Back");
        assert_eq!(a.page(), Page::Temperatures, "Back shows the temperatures");

        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        edit(&mut a, Edit::Typed("30".to_string()));
        let out = edit(&mut a, Edit::Enter);
        assert_eq!(stores(&out), [("poll_interval_secs", Value::U32(30))]);
        edit(&mut a, Edit::Typed(String::new()));
        let out = edit(&mut a, Edit::FocusLost);
        assert_eq!(stores(&out), vec![], "focus loss on an empty text");
        let out = a.handle(Event::PopupClosed);
        assert_eq!(stores(&out), vec![], "popup closed");
        assert_eq!(fx.entry("poll_interval_secs"), b"30", "entry after closing");
    });
}

/// The entry texts of list item `n` and the text of its selected entry.
fn list_of(plan: &Plan, n: usize) -> (Vec<String>, String) {
    match item(plan, n).control {
        Kind::List { items, shown } => {
            let text = items[shown].clone();
            (items, text)
        }
        c => panic!("item {n} is not a list: {c:?}"),
    }
}

/// Chooses the entry with the text `text` in list item `n`, as a click on
/// that entry would.
fn choose(a: &mut App, n: usize, text: &str) -> Vec<Output> {
    let (items, _) = list_of(&plan(a), n);
    let index = items
        .iter()
        .position(|t| t == text)
        .unwrap_or_else(|| panic!("item {n} has no entry {text:?}: {items:?}"));
    edit(
        a,
        Edit::Change {
            item: n,
            choice: Choice::Entry(index),
        },
    )
}

/// With nothing stored, the panel text size list (item 3) offers "Match
/// panel size" and 10 to 24 px, the dock list (item 4) the same with
/// "Match dock size" first, and each shows its match entry. Choosing each
/// entry of item 3 in turn, starting with the last, stores each matching
/// size under `panel_text_size` only, and choosing the shown entry again
/// stores nothing; the same in item 4 stores under `dock_text_size`.
/// After each choice the chosen list shows the choice and the other list
/// keeps its entry, and a new load reads both lists' last choice. On
/// entries that are not usable, the page shows both match entries, Back
/// stores nothing and both entries stay as they were.
#[test]
fn c_applet_app_45() {
    puron_testkit::limit("C-applet.app-45", LIMIT, || {
        let fixed = [
            "10 px", "12 px", "14 px", "16 px", "18 px", "20 px", "24 px",
        ];
        let fx = cfg::build("empty-none");
        let mut a = open(&fx);
        let p = plan(&a);
        let mut panel_items = vec!["Match panel size".to_string()];
        panel_items.extend(fixed.map(String::from));
        let mut dock_items = vec!["Match dock size".to_string()];
        dock_items.extend(fixed.map(String::from));
        assert_eq!(list_of(&p, 3), (panel_items, "Match panel size".into()));
        assert_eq!(list_of(&p, 4), (dock_items, "Match dock size".into()));

        let order = [
            TextSize::Px24,
            TextSize::MatchPanel,
            TextSize::Px10,
            TextSize::Px12,
            TextSize::Px14,
            TextSize::Px16,
            TextSize::Px18,
            TextSize::Px20,
        ];
        let panel_texts = [
            "24 px",
            "Match panel size",
            "10 px",
            "12 px",
            "14 px",
            "16 px",
            "18 px",
            "20 px",
        ];
        let mut dock_texts = panel_texts;
        dock_texts[1] = "Match dock size";
        let mut out = Vec::new();
        for text in panel_texts.into_iter().chain(["20 px"]) {
            out.extend(choose(&mut a, 3, text));
            let p = plan(&a);
            assert_eq!(list_of(&p, 3).1, text, "item 3 after {text}");
            assert_eq!(list_of(&p, 4).1, "Match dock size", "item 4 after {text}");
        }
        for text in dock_texts {
            out.extend(choose(&mut a, 4, text));
            let p = plan(&a);
            assert_eq!(list_of(&p, 4).1, text, "item 4 after {text}");
            assert_eq!(list_of(&p, 3).1, "20 px", "item 3 after {text}");
        }
        let want: Vec<(&str, Value)> = order
            .map(|t| ("panel_text_size", Value::TextSize(t)))
            .into_iter()
            .chain(order.map(|t| ("dock_text_size", Value::TextSize(t))))
            .collect();
        assert_eq!(stores(&out), want, "Store list");
        assert_eq!(results(&out), vec![StoreResult::Stored; 16], "each stored");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings.panel_text_size, TextSize::Px20, "Load");
        assert_eq!(loaded.settings.dock_text_size, TextSize::Px20, "Load");

        let bad = cfg::build("empty-none");
        cfg::write(&bad.id_dir().join("panel_text_size"), b"Default");
        cfg::write(&bad.id_dir().join("dock_text_size"), b"24");
        let before = bad.tree();
        let mut a = open(&bad);
        let p = plan(&a);
        assert_eq!(list_of(&p, 3).1, "Match panel size", "unusable panel");
        assert_eq!(list_of(&p, 4).1, "Match dock size", "unusable dock");
        let out = a.handle(Event::Back);
        assert_eq!(stores(&out), vec![], "Store list at Back");
        assert_eq!(bad.tree(), before, "entries unchanged");
    });
}

/// The page's place among the popup's pages, on the defaults with the
/// pinned list P5. After Start and a first snapshot, Settings shows the
/// page and launches, commands and stores nothing. A snapshot that
/// rebuilds the temperatures and a delivery that hides the voltages keep
/// the page: the layout is computed, not drawn. Back shows the
/// temperatures with the latest layout; Settings and then Popup opened
/// show the temperatures again.
#[test]
fn c_applet_app_46() {
    puron_testkit::limit("C-applet.app-46", LIMIT, || {
        common::start_in_english();
        let fx = cfg::build("p5");
        let mut a = app(&fx.root, &handle(0));
        a.handle(Event::Start);
        a.handle(Event::Snapshot(rebuild("S1")));
        let out = a.handle(Event::Settings);
        assert_eq!(a.page(), Page::Settings);
        assert!(View::of(&a).settings_page.is_some(), "the Settings plan");
        assert!(
            !out.iter().any(|o| matches!(o, Output::Launch(..))),
            "no launch: {out:?}"
        );
        assert_eq!(commands(&out), vec![], "no engine command");
        assert_eq!(stores(&out), vec![], "no Store");

        let out = a.handle(Event::Snapshot(rebuild("S3")));
        let built = out
            .iter()
            .find_map(|o| match o {
                Output::Layout { menu, .. } => Some(menu.clone()),
                _ => None,
            })
            .expect("a layout for S3");
        assert_eq!(built.update, Update::Rebuild, "S3 rebuilds");
        assert_eq!(a.page(), Page::Settings, "after S3");
        let v = View::of(&a);
        assert!(v.settings_page.is_some(), "the page stays drawn after S3");
        assert_eq!(a.menu().map(|m| m.key.clone()), Some(built.key.clone()));

        let d = delivery(&a, &["show_voltage"], |s| s.show_voltage = false);
        a.handle(d);
        assert_eq!(a.page(), Page::Settings, "after the delivery");
        assert!(View::of(&a).settings_page.is_some());

        a.handle(Event::Back);
        assert_eq!(a.page(), Page::Temperatures, "after Back");
        let v = View::of(&a);
        assert!(v.settings_page.is_none());
        assert_eq!(v.menu.as_ref(), a.menu(), "the latest layout drawn");
        assert!(v.menu.is_some());

        a.handle(Event::Settings);
        a.handle(Event::PopupOpened);
        assert_eq!(a.page(), Page::Temperatures, "after Popup opened");
    });
}
