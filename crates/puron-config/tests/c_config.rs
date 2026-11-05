//! Tests of config, 5 s each; each wait for a delivery or a return is
//! 2 s. Fixtures are built at test time. A second process is this test
//! binary run again with `PURON_CFG_CHILD` set.

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
        group_temperature: false,
        group_rotation_rate: false,
        group_voltage: false,
        debug_logging: true,
        pinned: vec!["a".to_string(), "b".to_string()],
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
        group_temperature: true,
        group_rotation_rate: true,
        group_voltage: true,
        debug_logging: false,
        pinned: vec!["@maximum".to_string()],
    }
}

/// The values of the `good` fixture as Store inputs, one per key.
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
        ("group_temperature", Value::Bool(s.group_temperature)),
        ("group_rotation_rate", Value::Bool(s.group_rotation_rate)),
        ("group_voltage", Value::Bool(s.group_voltage)),
        ("debug_logging", Value::Bool(s.debug_logging)),
        ("pinned", Value::List(s.pinned.clone())),
    ]
}

fn problem(key: &'static str, kind: ProblemKind) -> Problem {
    Problem {
        key: Some(key),
        kind,
    }
}

fn not_root() {
    puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
}

/// The child mode of this run, if it is a second process.
fn child() -> Option<String> {
    std::env::var("PURON_CFG_CHILD").ok()
}

fn child_root() -> std::path::PathBuf {
    std::env::var_os("PURON_CFG_ROOT")
        .expect("PURON_CFG_ROOT")
        .into()
}

/// Runs this test again as a second process with `env`; returns its
/// exit status.
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

#[test]
fn c_config_1() {
    puron_testkit::limit("C-config-1", LIMIT, || {
        let fx = cfg::build("good");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, good());
        assert_eq!(loaded.problems, vec![]);
    });
}

#[test]
fn c_config_2() {
    puron_testkit::limit("C-config-2", LIMIT, || {
        let fx = cfg::build("otherid");
        let loaded = Config::new(&fx.root).load();
        assert_eq!(loaded.settings, defaults());
        assert_eq!(loaded.problems, vec![]);
    });
}

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
            pinned: vec!["@maximum".to_string()],
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
                group_voltage: true,
                ..good()
            }
        );
        assert_eq!(
            loaded.problems,
            vec![
                problem("show_voltage", ProblemKind::Unreadable),
                problem("group_voltage", ProblemKind::Unreadable),
            ]
        );
    });
}

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

/// The stores of `c_config_8`, in order.
fn c_config_8_step(step: &str) -> (&'static str, Value) {
    match step {
        "1" => ("show_power", Value::Bool(false)),
        "2" => ("poll_interval_secs", Value::U32(9)),
        "3" => ("poll_interval_secs", Value::U32(0)),
        other => panic!("step {other}"),
    }
}

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

/// The system-default test, run in a child whose `XDG_DATA_HOME` and
/// `XDG_DATA_DIRS` are both the fixture's data directory.
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
