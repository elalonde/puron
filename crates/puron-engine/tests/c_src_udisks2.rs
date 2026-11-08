//! Tests of the udisks2 source, through `start`, `read` and `stop` with a
//! private fixture bus as the bus address and a bus time limit of 1 s.

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_engine::udisks2::{Bus as Addr, Udisks2, MANAGER_PATH, SERVICE};
use puron_testkit::dbus::{self, drive, Bus, Fake, Owners};
use puron_testkit::{temp_dir, umd};

/// Test time limit: 5 s.
const LIMIT: u64 = 5;
/// The bus time limit of the tests.
const BUS_LIMIT: Duration = Duration::from_secs(1);

/// One expected reading: name, value, key; the Position is its index.
type Exp = (&'static str, Option<f64>, String);

fn adapter(address: &str) -> Udisks2 {
    Udisks2::new(Addr::Address(address.to_string()), BUS_LIMIT)
}

fn setup(test: &str, script: &str) -> (Bus, Fake) {
    let bus = Bus::start(&temp_dir("udisks2"));
    let fake = Fake::start(test, &bus, script);
    (bus, fake)
}

fn read_outcome(src: &mut Udisks2) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

fn read(src: &mut Udisks2) -> Vec<RawReading> {
    match read_outcome(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Every field: udisks2, Positions 0, 1, ..., kind temperature, no chip,
/// device, item or unit, and the expected name, value and key.
fn check(readings: &[RawReading], exp: &[Exp]) {
    let got: Vec<(String, Option<f64>, String)> = readings
        .iter()
        .map(|r| (r.name.clone().unwrap_or_default(), r.value, r.key.clone()))
        .collect();
    let want: Vec<(String, Option<f64>, String)> = exp
        .iter()
        .map(|(n, v, k)| (n.to_string(), *v, k.clone()))
        .collect();
    assert_eq!(got, want);
    for (i, r) in readings.iter().enumerate() {
        assert_eq!(r.provider, Provider::Udisks2);
        assert_eq!(r.position, i as u32);
        assert_eq!(r.kind, Some(Kind::Temperature));
        assert!(r.name.is_some());
        assert_eq!(
            (&r.chip, &r.device, &r.item, &r.unit),
            (&None, &None, &None, &None)
        );
    }
}

/// The log holds only the calls the adapter may make.
fn only_allowed(fake: &Fake) {
    for line in fake.log() {
        assert!(dbus::allowed(&line), "call not allowed: {line}");
    }
}

fn count(fake: &Fake, prefix: &str) -> usize {
    fake.log().iter().filter(|l| l.starts_with(prefix)).count()
}

const LIST: &str = "org.freedesktop.DBus.ObjectManager GetManagedObjects";
const GET: &str = "org.freedesktop.DBus.Properties Get";

fn ata() -> Vec<Exp> {
    vec![
        ("ST2000DM008-2FR102", Some(305.0 - 273.15), drive("ST_1")),
        (
            "WDC WD10EZEX-08WN4A0",
            Some(310.15 - 273.15),
            drive("WDC_2"),
        ),
    ]
}

#[test]
fn c_src_udisks2_1() {
    puron_testkit::limit("C-src.udisks2-1", LIMIT, || {
        dbus::child();
        let (_bus, fake) = setup("c_src_udisks2_1", "ata");
        let mut src = adapter(&_bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &ata());
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_2() {
    puron_testkit::limit("C-src.udisks2-2", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_2", "models");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        let v = Some(300.15 - 273.15);
        check(
            &read(&mut src),
            &[
                (" Spaced  Model ", v, drive("a")),
                ("Disque ÉTÉ", v, drive("b")),
                ("", v, drive("e")),
            ],
        );
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_3() {
    puron_testkit::limit("C-src.udisks2-3", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_3", "temps");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        let b = ("B", Some(310.15 - 273.15), drive("b"));
        let cases: [(&str, Option<f64>); 6] = [
            ("0", None),
            ("-1", None),
            ("NaN", None),
            ("0.5", Some(0.5 - 273.15)),
            ("273.15", Some(273.15 - 273.15)),
            ("400.0", Some(400.0 - 273.15)),
        ];
        for (k, a) in cases {
            fake.ctl(&format!("set {} SmartTemperature {k}", drive("a")));
            let mut exp = Vec::new();
            if let Some(v) = a {
                exp.push(("A", Some(v), drive("a")));
            }
            exp.push(b.clone());
            check(&read(&mut src), &exp);
        }
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_4() {
    puron_testkit::limit("C-src.udisks2-4", LIMIT, || {
        dbus::child();
        let (bus1, fake) = setup("c_src_udisks2_4", "none");
        let bus2 = Bus::start(&temp_dir("udisks2"));
        let nobus = format!("unix:path={}", temp_dir("udisks2").join("nobus").display());
        for address in [&bus1.address, &bus2.address, &nobus] {
            let mut src = adapter(address);
            assert_eq!(src.start(), StartOutcome::Unavailable, "{address}");
            src.stop();
        }
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_5() {
    puron_testkit::limit("C-src.udisks2-5", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_5", "ata");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &ata());
        fake.ctl(&format!("add {} New 300.15", drive("New")));
        fake.ctl(&format!("set {} Model Other", drive("ST_1")));
        fake.ctl(&format!("remove {}", drive("WDC_2")));
        check(
            &read(&mut src),
            &[
                ("ST2000DM008-2FR102", Some(305.0 - 273.15), drive("ST_1")),
                ("WDC WD10EZEX-08WN4A0", None, drive("WDC_2")),
            ],
        );
        assert_eq!(count(&fake, LIST), 1);
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_6() {
    puron_testkit::limit("C-src.udisks2-6", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_6", "fail");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        let v = Some(310.15 - 273.15);
        check(
            &read(&mut src),
            &[
                ("A", None, drive("a")),
                ("B", None, drive("b")),
                ("C", v, drive("c")),
            ],
        );
        let t = Instant::now();
        let r2 = read(&mut src);
        let took = t.elapsed();
        check(
            &r2,
            &[
                ("A", v, drive("a")),
                ("B", None, drive("b")),
                ("C", v, drive("c")),
            ],
        );
        assert!(
            took >= Duration::from_millis(1000) && took <= Duration::from_millis(1500),
            "read 2 took {took:?}"
        );
        check(
            &read(&mut src),
            &[
                ("A", v, drive("a")),
                ("B", v, drive("b")),
                ("C", v, drive("c")),
            ],
        );
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_7() {
    puron_testkit::limit("C-src.udisks2-7", LIMIT, || {
        dbus::child();
        let (mut bus, mut fake) = setup("c_src_udisks2_7", "ata");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &ata());
        only_allowed(&fake);
        bus.stop();
        fake.stop();
        assert_eq!(read_outcome(&mut src), ReadOutcome::Failed);
        std::fs::remove_file(&fake.log).expect("remove the first log");
        let bus2 = Bus::start(&bus.dir);
        assert_eq!(bus2.address, bus.address);
        let fake2 = Fake::start("c_src_udisks2_7", &bus2, "ata");
        check(&read(&mut src), &ata());
        assert_eq!(count(&fake2, LIST), 0);
        assert_eq!(count(&fake2, GET), 2);
        src.stop();
        only_allowed(&fake2);
    });
}

#[test]
fn c_src_udisks2_8() {
    puron_testkit::limit("C-src.udisks2-8", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_8", "hang");
        let mut src = adapter(&bus.address);
        let t = Instant::now();
        assert_eq!(src.start(), StartOutcome::Unavailable);
        let took = t.elapsed();
        assert!(
            took >= Duration::from_millis(1000) && took <= Duration::from_millis(1500),
            "start took {took:?}"
        );
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_9() {
    puron_testkit::limit("C-src.udisks2-9", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_9", "ata");
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &ata());
        assert_eq!(count(&fake, GET), 2);
        fake.ctl(&format!("set {} SmartTemperature 320.0", drive("ST_1")));
        check(
            &read(&mut src),
            &[
                ("ST2000DM008-2FR102", Some(320.0 - 273.15), drive("ST_1")),
                (
                    "WDC WD10EZEX-08WN4A0",
                    Some(310.15 - 273.15),
                    drive("WDC_2"),
                ),
            ],
        );
        let log = fake.log();
        assert_eq!(log.len(), 5, "{log:?}");
        assert_eq!(count(&fake, GET), 4);
        src.stop();
        only_allowed(&fake);
    });
}

#[test]
fn c_src_udisks2_10() {
    puron_testkit::limit("C-src.udisks2-10", LIMIT, || {
        dbus::child();
        let (bus, fake) = setup("c_src_udisks2_10", "ata");
        let owners = Owners::watch(&bus);
        let mut src = adapter(&bus.address);
        assert_eq!(src.start(), StartOutcome::Available);
        let mut new = Vec::new();
        while let Some((name, old, owner)) = owners.next(Duration::from_millis(200)) {
            if name.starts_with(':') && old.is_empty() && owner == name {
                new.push(name);
            }
        }
        assert_eq!(new.len(), 1, "new connections: {new:?}");
        for _ in 0..3 {
            check(&read(&mut src), &ata());
        }
        src.stop();
        let gone = loop {
            match owners.next(Duration::from_secs(2)) {
                Some((name, old, owner)) if name == new[0] && old == name && owner.is_empty() => {
                    break true
                }
                Some(_) => continue,
                None => break false,
            }
        };
        assert!(gone, "no NameOwnerChanged for {} after Stop", new[0]);
        let mut want = vec![format!("{LIST} {MANAGER_PATH}")];
        for _ in 0..3 {
            for d in ["ST_1", "WDC_2"] {
                want.push(format!(
                    "{GET} {} org.freedesktop.UDisks2.Drive.Ata SmartTemperature",
                    drive(d)
                ));
            }
        }
        assert_eq!(fake.log(), want);
    });
}

#[test]
fn c_src_udisks2_11() {
    puron_testkit::limit("C-src.udisks2-11", LIMIT, || {
        let src = registry::udisks2();
        assert_eq!(src.bus(), &Addr::System);
        assert_eq!(src.limit(), Duration::from_secs(5));
        assert_eq!(SERVICE, "org.freedesktop.UDisks2");
        assert_eq!(MANAGER_PATH, "/org/freedesktop/UDisks2");
    });
}
