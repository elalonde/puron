//! Tests of the rows dump: the captured raw readings through the fake
//! provider, with the two settings roots under `fixtures/d5/` (the
//! captured one and an empty one), and the readings that give no row
//! with a pin that is never found; the dump equals the fixture written
//! by hand from the rules (`cmp` exit 0).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use puron_config::Config;
use puron_core::rows::{Kind, Provider, RawReading};
use puron_core::settings::Settings;
use puron_engine::clock::SystemClock;
use puron_engine::registry::ReadOutcome;
use puron_testkit::d5;
use puron_testkit::eng::{adapters, CallKind, Fake, ReadStep, StartStep};

fn capture() -> Vec<RawReading> {
    serde_json::from_str(include_str!("../fixtures/raw/capture.json")).expect("capture.json")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn loaded(settings: &str) -> Settings {
    let loaded = Config::new(
        root()
            .join("crates/puron-testkit/fixtures/d5")
            .join(settings),
    )
    .load();
    assert_eq!(loaded.problems, vec![], "settings problems");
    loaded.settings
}

/// The dump of the captured raw readings with the settings of
/// `settings`, compared with `fixtures/d5/<fixture>` by `cmp`.
fn check(settings: &str, fixture: &str) {
    compare(
        &loaded(settings),
        &[(Provider::LmSensors, capture())],
        fixture,
    );
}

/// The dump of `readings` (one fake provider each) with `settings`,
/// compared with `fixtures/d5/<fixture>` by `cmp`.
fn compare(settings: &Settings, readings: &[(Provider, Vec<RawReading>)], fixture: &str) {
    let root = root();
    let clock = SystemClock::new();
    let fakes: Vec<Fake> = readings
        .iter()
        .map(|(p, r)| {
            let gives = || ReadStep::Gives(ReadOutcome::Readings(r.clone()));
            Fake::new(clock.clone(), *p).reads([gives(), gives(), gives()])
        })
        .collect();
    let text = d5::dump(clock, settings, adapters(&fakes), d5::WAIT);
    let out = std::env::temp_dir().join(format!("puron-d5-{}-{fixture}", std::process::id()));
    std::fs::write(&out, text).expect("write the dump");
    let expected = root.join("crates/puron-testkit/fixtures/d5").join(fixture);
    let st = Command::new("cmp")
        .arg(&out)
        .arg(&expected)
        .status()
        .expect("cmp");
    if st.success() {
        let _ = std::fs::remove_file(&out);
    }
    assert!(
        st.success(),
        "cmp {} {}: {st}",
        out.display(),
        expected.display()
    );
}

#[test]
fn d5_capture() {
    puron_testkit::limit("D5", 5, || check("X-p5-capture", "capture.txt"));
}

#[test]
fn d5_empty() {
    puron_testkit::limit("D5", 5, || check("X-p5-empty", "empty.txt"));
}

fn lm(chip: &str, kind: Kind, name: &str, key: &str, value: f64) -> RawReading {
    RawReading {
        provider: Provider::LmSensors,
        position: 0,
        kind: Some(kind),
        chip: Some(chip.into()),
        name: Some(name.into()),
        device: None,
        item: None,
        unit: None,
        key: format!("{chip}/{key}"),
        value: Some(value),
    }
}

fn lqc(item: &str, unit: &str, value: Option<f64>) -> RawReading {
    RawReading {
        provider: Provider::Liquidctl,
        position: 0,
        kind: None,
        chip: None,
        name: None,
        device: Some("NZXT Kraken X (X53)".into()),
        item: Some(item.into()),
        unit: Some(unit.into()),
        key: format!("kraken/{item}"),
        value,
    }
}

/// Each reading that gives no row (one for each reason, and a hidden
/// class for both forms of the class word) has its `raw` line, and a pin
/// that is never found has the `slot` icon. Voltage hidden; liquidctl on.
#[test]
fn d5_norow() {
    puron_testkit::limit("D5", 5, || {
        let mut s = loaded("X-p5-empty");
        s.provider_liquidctl = true;
        s.show_voltage = false;
        s.pinned = vec![
            "temperature/lm-sensors/coretemp-isa-0000/temp9".into(),
            "@maximum".into(),
        ];
        let c = "coretemp-isa-0000";
        let lm = vec![
            lm(c, Kind::Temperature, "CPU", "temp1", 50.0),
            lm(c, Kind::Power, "power1", "power1", 5.5),
            lm(
                "amdgpu-nvme-pci-0300",
                Kind::Temperature,
                "edge",
                "temp1",
                40.0,
            ),
            lm("BAT0-acpi-0", Kind::Voltage, "in0", "in0", 8.494),
        ];
        let lqc = vec![lqc("Pump duty", "%", Some(60.0)), lqc("+12V", "V", None)];
        compare(
            &s,
            &[(Provider::LmSensors, lm), (Provider::Liquidctl, lqc)],
            "norow.txt",
        );
    });
}

/// The class word of a liquidctl reading from its unit, for each unit;
/// every class hidden, so no reading gives a row.
#[test]
fn d5_liquidctl_units() {
    puron_testkit::limit("D5", 5, || {
        let mut s = loaded("X-p5-empty");
        s.provider_lm_sensors = false;
        s.provider_liquidctl = true;
        s.show_temperature = false;
        s.show_rotation_rate = false;
        s.show_voltage = false;
        s.show_power = false;
        let r = vec![
            lqc("Liquid temperature", "°C", Some(30.5)),
            lqc("Pump speed", "rpm", Some(1800.0)),
            lqc("+12V", "V", Some(12.0)),
            lqc("Duty", "%", Some(50.0)),
        ];
        let fake = Fake::new(SystemClock::new(), Provider::Liquidctl)
            .reads([ReadStep::Gives(ReadOutcome::Readings(r))]);
        let text = d5::dump(SystemClock::new(), &s, adapters(&[fake]), d5::WAIT);
        let raw: Vec<&str> = text.lines().filter(|l| l.starts_with("raw\t")).collect();
        assert_eq!(
            raw,
            [
                "raw\ttemperature/liquidctl/kraken/Liquid temperature\t30.5",
                "raw\tfan/liquidctl/kraken/Pump speed\t1800",
                "raw\tvoltage/liquidctl/kraken/+12V\t12",
                "raw\t-/liquidctl/kraken/Duty\t50",
            ]
        );
    });
}

/// Each provider's Stop reaches the adapter; an unavailable Start counts
/// as reported (no wait to the limit), and the dump shows the message.
#[test]
fn d5_unavailable() {
    puron_testkit::limit("D5", 5, || {
        let mut s = d5::settings(&root().join("crates/puron-testkit/fixtures/d5/X-p5-empty"));
        s.pinned.clear();
        let clock = SystemClock::new();
        let fake = Fake::new(clock.clone(), Provider::LmSensors).starts([StartStep::Unavailable]);
        let t = std::time::Instant::now();
        let text = d5::dump(clock, &s, adapters(&[fake.clone()]), Duration::from_secs(3));
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "waited {:?}",
            t.elapsed()
        );
        let m: Vec<&str> = text.lines().filter(|l| l.contains("\tmessage\t")).collect();
        assert!(
            m == ["menu\t0\tmessage\t-\tsensors-detect\t-\t-"]
                || m == ["menu\t0\tmessage\t-\tinstall\t-\t-"],
            "{text}"
        );
        let fake = Fake::new(SystemClock::new(), Provider::LmSensors)
            .reads([ReadStep::Gives(ReadOutcome::Readings(capture()))]);
        d5::dump(SystemClock::new(), &s, adapters(&[fake.clone()]), d5::WAIT);
        assert_eq!(fake.count(CallKind::Stop), 1);
    });
}

#[test]
fn d5_settings() {
    puron_testkit::limit("D5", 5, || {
        let s = d5::settings(&root().join("crates/puron-testkit/fixtures/d5/X-p5-capture"));
        assert_eq!(s.pinned.len(), 4);
    });
}
