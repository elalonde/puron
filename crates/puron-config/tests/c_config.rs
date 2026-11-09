//! Tests of `puron_config`: loading, storing and watching the settings.
//!
//! Each test builds its settings tree from a fixture of `puron_testkit::cfg`
//! in a temporary directory, and ends the run if it takes more than 5 s.
//! Each wait for a watch delivery or a `store` return gives up after 2 s.
//! Where a test needs a second process, it runs this test binary again
//! with `PURON_CFG_CHILD` naming what the child does.

#![allow(non_snake_case)] // case names: c_<id>__<case>

use std::sync::mpsc;
use std::time::Duration;

use puron_config::{Config, Delivery, Problem, ProblemKind, StoreResult, Value};
use puron_core::settings::{Settings, TemperatureUnit};
use puron_testkit::cfg;

const LIMIT: u64 = 5;
const WAIT: Duration = Duration::from_secs(2);

/// The values of the `good` fixture, written out by hand.
fn good() -> Settings {
    Settings {
        poll_interval_secs: 7,
        show_panel_icons: false,
        temperature_unit: TemperatureUnit::Fahrenheit,
        show_temperature_unit: false,
        show_rotation_rate_unit: false,
        show_voltage_unit: false,
        show_power_unit: false,
        show_decimals: true,
        provider_lm_sensors: false,
        provider_liquidctl: true,
        provider_nvidia: true,
        provider_udisks2: true,
        provider_nvme: true,
        show_temperature: false,
        show_rotation_rate: false,
        show_voltage: false,
        show_power: false,
        debug_logging: true,
        pinned: Some(vec!["a".to_string(), "b".to_string()]),
        old_cpu_source: None,
    }
}

/// The default of each key, written out by hand rather than taken from
/// `Settings::default`.
fn defaults() -> Settings {
    Settings {
        poll_interval_secs: 5,
        show_panel_icons: true,
        temperature_unit: TemperatureUnit::Celsius,
        show_temperature_unit: true,
        show_rotation_rate_unit: true,
        show_voltage_unit: true,
        show_power_unit: true,
        show_decimals: false,
        provider_lm_sensors: true,
        provider_liquidctl: false,
        provider_nvidia: false,
        provider_udisks2: false,
        provider_nvme: false,
        show_temperature: true,
        show_rotation_rate: true,
        show_voltage: true,
        show_power: true,
        debug_logging: false,
        pinned: None,
        old_cpu_source: None,
    }
}

/// The values of the `good` fixture as `Config::store` inputs, one per key.
fn good_values() -> Vec<(&'static str, Value)> {
    let s = good();
    vec![
        ("poll_interval_secs", Value::U32(s.poll_interval_secs)),
        ("show_panel_icons", Value::Bool(s.show_panel_icons)),
        (
            "temperature_unit",
            Value::TemperatureUnit(s.temperature_unit),
        ),
        (
            "show_temperature_unit",
            Value::Bool(s.show_temperature_unit),
        ),
        (
            "show_rotation_rate_unit",
            Value::Bool(s.show_rotation_rate_unit),
        ),
        ("show_voltage_unit", Value::Bool(s.show_voltage_unit)),
        ("show_power_unit", Value::Bool(s.show_power_unit)),
        ("show_decimals", Value::Bool(s.show_decimals)),
        ("provider_lm_sensors", Value::Bool(s.provider_lm_sensors)),
        ("provider_liquidctl", Value::Bool(s.provider_liquidctl)),
        ("provider_nvidia", Value::Bool(s.provider_nvidia)),
        ("provider_udisks2", Value::Bool(s.provider_udisks2)),
        ("provider_nvme", Value::Bool(s.provider_nvme)),
        ("show_temperature", Value::Bool(s.show_temperature)),
        ("show_rotation_rate", Value::Bool(s.show_rotation_rate)),
        ("show_voltage", Value::Bool(s.show_voltage)),
        ("show_power", Value::Bool(s.show_power)),
        ("debug_logging", Value::Bool(s.debug_logging)),
        ("pinned", Value::List(s.pinned.expect("good has pins"))),
    ]
}

fn problem(key: &'static str, kind: ProblemKind) -> Problem {
    Problem {
        key: Some(key),
        kind,
    }
}

/// Fails the test when it runs as root, which can read and write any file
/// and so would defeat a test of file permissions.
fn not_root() {
    puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
}

/// What this process must do when it is a second process started by a
/// test, or `None` in the test's own process.
fn child() -> Option<String> {
    std::env::var("PURON_CFG_CHILD").ok()
}

/// The configuration root that the parent passed to a second process.
fn child_root() -> std::path::PathBuf {
    std::env::var_os("PURON_CFG_ROOT")
        .expect("PURON_CFG_ROOT")
        .into()
}

/// Runs the test `test` again in a second process with the extra
/// environment `env`, and returns its exit status.
fn second_process(test: &str, env: &[(&str, &std::ffi::OsStr)]) -> std::process::ExitStatus {
    let mut cmd = puron_testkit::rerun_self(test);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdout(std::process::Stdio::null())
        .status()
        .expect("run the second process")
}

/// Starts a watch that sends each delivery to the returned receiver.
fn watch(config: &Config) -> (puron_config::Watch, mpsc::Receiver<Delivery>) {
    let (tx, rx) = mpsc::channel();
    let watch = config
        .watch(move |d| {
            let _ = tx.send(d);
        })
        .expect("the watch starts");
    (watch, rx)
}

/// The `good` fixture, which holds a usable entry for every key, loads as
/// exactly those values with no problem. `KEYS` has the 19 keys, in the
/// order of `good_values`.
#[test]
fn c_config_1() {
    puron_testkit::limit("C-config-1", LIMIT, || {
        let fx = cfg::build("good");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, good());
        assert_eq!(loaded.problems, vec![]);
        assert_eq!(puron_config::KEYS.len(), 19);
        let stored: Vec<&str> = good_values().iter().map(|(key, _)| *key).collect();
        assert_eq!(stored, puron_config::KEYS);

        // Entries left by older builds that are not keys record no
        // problem. Of the old row choices, only the last one for the CPU
        // row is read, and the load leaves every entry as it was.
        for key in ["group_temperature", "group_rotation_rate", "group_voltage"] {
            cfg::write(&fx.id_dir().join(key), b"false");
        }
        cfg::write(
            &fx.id_dir().join("row_sources"),
            br#"[("@cpu", "chip/temp1"), ("@cpu", "chip/temp2"), ("@other", "chip/temp3")]"#,
        );
        let before = fx.tree();
        let loaded = Config::new(&fx.root).load();
        let expected = Settings {
            old_cpu_source: Some("chip/temp2".to_string()),
            ..good()
        };
        assert_eq!(loaded.settings, expected);
        assert_eq!(loaded.problems, vec![]);
        assert_eq!(fx.tree(), before, "load changed an entry");

        // Old row choices with none for the CPU row leave
        // `old_cpu_source` empty.
        cfg::write(
            &fx.id_dir().join("row_sources"),
            br#"[("@other", "chip/temp3")]"#,
        );
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, good());
        assert_eq!(loaded.problems, vec![]);
    });
}

/// Settings stored under another version (`v2`), or under the ID in lower
/// case, are not read: the load gives every default and no problem.
#[test]
fn c_config_2() {
    puron_testkit::limit("C-config-2", LIMIT, || {
        let fx = cfg::build("otherid");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, defaults());
        assert_eq!(loaded.problems, vec![]);
    });
}

/// An empty root loads as every default with no problem and stays as it
/// was. Case `none` has an empty settings directory; case `cosmic` has
/// only `cosmic/`, with no settings directory.
fn c_config_3_case(name: &str) {
    let fx = cfg::build(name);
    let before = fx.tree();
    let loaded = Config::new(&fx.root).load();
    assert_eq!(loaded.settings, defaults());
    assert_eq!(loaded.problems, vec![]);
    assert_eq!(fx.tree(), before, "load changed the root");
}

#[test]
fn c_config_3__none() {
    puron_testkit::limit("C-config-3", LIMIT, || c_config_3_case("empty-none"));
}

#[test]
fn c_config_3__cosmic() {
    puron_testkit::limit("C-config-3", LIMIT, || c_config_3_case("empty-cosmic"));
}

/// Four entries whose content is not of the key's type each take their
/// default and record `WrongType`, in key order. An old `row_sources` entry
/// of the wrong type records nothing. The load changes no entry.
#[test]
fn c_config_4() {
    puron_testkit::limit("C-config-4", LIMIT, || {
        let fx = cfg::build("wrong");
        let before = fx.tree();
        let loaded = Config::new(&fx.root).load();
        let expected = Settings {
            show_power: true,
            temperature_unit: TemperatureUnit::Celsius,
            poll_interval_secs: 5,
            pinned: None,
            ..good()
        };
        assert_eq!(loaded.settings, expected);
        assert_eq!(
            loaded.problems,
            vec![
                problem("poll_interval_secs", ProblemKind::WrongType),
                problem("temperature_unit", ProblemKind::WrongType),
                problem("show_power", ProblemKind::WrongType),
                problem("pinned", ProblemKind::WrongType),
            ]
        );
        assert_eq!(fx.tree(), before, "load changed an entry");
    });
}

/// A stored poll interval outside 1..=60 (0, or 61) loads as the nearest
/// bound (`expected`) and records `Clamped`. The load changes no entry.
fn c_config_5_case(name: &str, expected: u32) {
    let fx = cfg::build(name);
    let before = fx.tree();
    let loaded = Config::new(&fx.root).load();
    assert_eq!(
        loaded.settings,
        Settings {
            poll_interval_secs: expected,
            ..good()
        }
    );
    assert_eq!(
        loaded.problems,
        vec![problem("poll_interval_secs", ProblemKind::Clamped)]
    );
    assert_eq!(fx.tree(), before, "load changed an entry");
}

#[test]
fn c_config_5__0() {
    puron_testkit::limit("C-config-5", LIMIT, || c_config_5_case("range-0", 1));
}

#[test]
fn c_config_5__61() {
    puron_testkit::limit("C-config-5", LIMIT, || c_config_5_case("range-61", 60));
}

/// An entry that cannot be opened (mode 000) and an entry that is not
/// UTF-8 each take their default and record `Unreadable`. An old
/// `row_sources` entry that is not UTF-8 records nothing.
#[test]
fn c_config_6() {
    puron_testkit::limit("C-config-6", LIMIT, || {
        not_root();
        let fx = cfg::build("unreadable");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(
            loaded.settings,
            Settings {
                show_voltage: true,
                show_power: true,
                ..good()
            }
        );
        assert_eq!(
            loaded.problems,
            vec![
                problem("show_voltage", ProblemKind::Unreadable),
                problem("show_power", ProblemKind::Unreadable),
            ]
        );
    });
}

/// Storing each value of `good` into an empty settings directory returns
/// `Stored` every time, and a load in a new process reads all of them back
/// with no problem.
#[test]
fn c_config_7() {
    puron_testkit::limit("C-config-7", LIMIT, || {
        if child().as_deref() == Some("load") {
            let loaded = Config::new(child_root()).load();
            assert_eq!(loaded.settings, good());
            assert_eq!(loaded.problems, vec![]);
            return;
        }
        let fx = cfg::build("empty-none");
        let config = Config::new(&fx.root);
        for (key, value) in good_values() {
            assert_eq!(
                config.store(key, &value),
                StoreResult::Stored,
                "store {key}"
            );
        }
        let status = second_process(
            "c_config_7",
            &[
                ("PURON_CFG_CHILD", "load".as_ref()),
                ("PURON_CFG_ROOT", fx.root.as_os_str()),
            ],
        );
        assert!(status.success(), "the load in a new process: {status}");
    });
}

/// The store that the second process of `c_config_8` makes at each step.
fn c_config_8_step(step: &str) -> (&'static str, Value) {
    match step {
        "1" => ("show_power", Value::Bool(false)),
        "2" => ("poll_interval_secs", Value::U32(9)),
        "3" => ("poll_interval_secs", Value::U32(0)),
        other => panic!("step {other}"),
    }
}

/// A watch in this process sees the stores of another process: one
/// delivery per store, naming the changed key, with the settings after
/// that store. The third store (a poll interval of 0) arrives as 1 with a
/// `Clamped` problem. No fourth delivery comes within 500 ms.
#[test]
fn c_config_8() {
    puron_testkit::limit("C-config-8", LIMIT, || {
        if child().as_deref() == Some("store") {
            let step = std::env::var("PURON_CFG_STEP").expect("PURON_CFG_STEP");
            let (key, value) = c_config_8_step(&step);
            assert_eq!(
                Config::new(child_root()).store(key, &value),
                StoreResult::Stored
            );
            return;
        }
        let fx = cfg::build("good");
        let config = Config::new(&fx.root);
        let _ = config.load();
        let (_watch, rx) = watch(&config);
        let expected = [
            (
                vec!["show_power"],
                Settings {
                    show_power: false,
                    ..good()
                },
                vec![],
            ),
            (
                vec!["poll_interval_secs"],
                Settings {
                    show_power: false,
                    poll_interval_secs: 9,
                    ..good()
                },
                vec![],
            ),
            (
                vec!["poll_interval_secs"],
                Settings {
                    show_power: false,
                    poll_interval_secs: 1,
                    ..good()
                },
                vec![problem("poll_interval_secs", ProblemKind::Clamped)],
            ),
        ];
        for (step, (keys, settings, problems)) in ["1", "2", "3"].iter().zip(expected) {
            let status = second_process(
                "c_config_8",
                &[
                    ("PURON_CFG_CHILD", "store".as_ref()),
                    ("PURON_CFG_STEP", step.as_ref()),
                    ("PURON_CFG_ROOT", fx.root.as_os_str()),
                ],
            );
            assert!(status.success(), "store {step}: {status}");
            let d = rx.recv_timeout(WAIT).expect("a delivery within 2 s");
            assert_eq!(d.keys, keys, "step {step}");
            assert_eq!(d.loaded.settings, settings, "step {step}");
            assert_eq!(d.loaded.problems, problems, "step {step}");
        }
        assert!(
            rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "a fourth delivery"
        );
    });
}

/// Writing a file in the settings directory that is not one of `KEYS`
/// brings no delivery within 2 s.
#[test]
fn c_config_9() {
    puron_testkit::limit("C-config-9", LIMIT, || {
        let fx = cfg::build("good");
        let config = Config::new(&fx.root);
        let _ = config.load();
        let (_watch, rx) = watch(&config);
        cfg::write(&fx.id_dir().join("unknown_key"), b"true");
        assert!(rx.recv_timeout(WAIT).is_err(), "a delivery for unknown_key");
    });
}

/// In a settings directory that is not writable, `store` returns `Failed`
/// within 2 s, the entry keeps its old content, and a load still gives the
/// `good` values with no problem.
#[test]
fn c_config_10() {
    puron_testkit::limit("C-config-10", LIMIT, || {
        not_root();
        let fx = cfg::build("readonly");
        let before = fx.entry("pinned");
        let config = Config::new(&fx.root);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(config.store("pinned", &Value::List(vec!["x".to_string()])));
        });
        let result = rx.recv_timeout(WAIT).expect("store returns within 2 s");
        assert_eq!(result, StoreResult::Failed);
        assert_eq!(fx.entry("pinned"), before);
        assert_eq!(before, b"[\"a\", \"b\"]");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, good());
        assert_eq!(loaded.problems, vec![]);
    });
}

/// System-wide defaults are never read. The check runs in a second
/// process whose `XDG_DATA_HOME` and `XDG_DATA_DIRS` both point to the
/// fixture's data directory, which holds system entries for `show_power`
/// and `poll_interval_secs`. Case `a` has no user entry: the load gives
/// Puron's defaults with no problem, and changing a system entry brings no
/// delivery. Case `b` adds a user `show_power` of the wrong type: it
/// records `WrongType` and takes Puron's default, not the system entry.
fn c_config_11_case(test: &str, fixture: &str) {
    if child().as_deref() == Some("sysdefault") {
        let root = child_root();
        let config = Config::new(&root);
        let loaded = config.load();
        let data: std::path::PathBuf = std::env::var_os("XDG_DATA_HOME").unwrap().into();
        if fixture == "sysdefault-a" {
            assert_eq!(loaded.settings, defaults());
            assert_eq!(loaded.problems, vec![]);
            let (_watch, rx) = watch(&config);
            cfg::write(&data.join(cfg::ID_DIR).join("show_voltage"), b"false");
            assert!(
                rx.recv_timeout(WAIT).is_err(),
                "a delivery for a system entry"
            );
        } else {
            assert_eq!(loaded.settings, defaults());
            assert_eq!(
                loaded.problems,
                vec![problem("show_power", ProblemKind::WrongType)]
            );
        }
        return;
    }
    let fx = cfg::build(fixture);
    let status = second_process(
        test,
        &[
            ("PURON_CFG_CHILD", "sysdefault".as_ref()),
            ("PURON_CFG_ROOT", fx.root.as_os_str()),
            ("XDG_DATA_HOME", fx.data.as_os_str()),
            ("XDG_DATA_DIRS", fx.data.as_os_str()),
        ],
    );
    assert!(status.success(), "the child: {status}");
}

#[test]
fn c_config_11__a() {
    puron_testkit::limit("C-config-11", LIMIT, || {
        c_config_11_case("c_config_11__a", "sysdefault-a")
    });
}

#[test]
fn c_config_11__b() {
    puron_testkit::limit("C-config-11", LIMIT, || {
        c_config_11_case("c_config_11__b", "sysdefault-b")
    });
}
