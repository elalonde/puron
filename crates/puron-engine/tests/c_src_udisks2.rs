//! Tests of the udisks2 adapter (`puron_engine::udisks2`), through its
//! `start`, `read` and `stop`.
//!
//! Each test starts a private D-Bus daemon and, on it, a fake udisks2
//! service (`puron_testkit::dbus::Fake`) that plays a script from
//! `fixtures/dbus/`. The fake is the same test binary run again, so
//! every test calls `dbus::child()` first: in the fake's process that
//! call runs the fake and never returns. The adapter connects to the
//! private bus with a time limit of 1 s for the connection and for each
//! call. udisks2 reports temperatures in kelvin; the adapter gives °C.

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_engine::udisks2::{Bus as Addr, Udisks2, MANAGER_PATH, SERVICE};
use puron_testkit::dbus::{self, drive, Bus, Fake, Owners};
use puron_testkit::{temp_dir, umd};

/// The time limit of each test here, in seconds.
const LIMIT: u64 = 5;
/// The adapter's time limit in these tests, for connecting and for each
/// call on the bus.
const BUS_LIMIT: Duration = Duration::from_secs(1);

/// One expected reading: its name, value and key (the drive's object
/// path). Its position is its index in the list of expected readings.
type Exp = (&'static str, Option<f64>, String);

/// An adapter for the bus at `address`, with the time limit `BUS_LIMIT`.
fn adapter(address: &str) -> Udisks2 {
    Udisks2::new(Addr::Address(address.to_string()), BUS_LIMIT)
}

/// Starts a private bus in a fresh temporary directory, and on it the
/// fake udisks2 service with the script `fixtures/dbus/<script>.json`.
/// `test` is the name of the calling test, which the fake's process
/// runs.
fn setup(test: &str, script: &str) -> (Bus, Fake) {
    let bus = Bus::start(&temp_dir("udisks2"));
    let fake = Fake::start(test, &bus, script);
    (bus, fake)
}

/// Reads once, with a stop descriptor that is never raised.
fn read_outcome(src: &mut Udisks2) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// Reads once and returns the readings; panics if the read gives
/// anything else.
fn read(src: &mut Udisks2) -> Vec<RawReading> {
    match read_outcome(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Checks every field of `readings`: name, value and key as in `exp`, in
/// that order; provider `Udisks2`; positions 0, 1, 2 and so on; kind
/// temperature; and no chip, device, item or unit.
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

/// Checks that the fake's call log holds only calls the adapter is
/// allowed to make (see `dbus::allowed`).
fn only_allowed(fake: &Fake) {
    for line in fake.log() {
        assert!(dbus::allowed(&line), "call not allowed: {line}");
    }
}

/// The number of lines of the fake's call log that start with `prefix`.
fn count(fake: &Fake, prefix: &str) -> usize {
    fake.log().iter().filter(|l| l.starts_with(prefix)).count()
}

/// The start of a call-log line for the listing of the drives.
const LIST: &str = "org.freedesktop.DBus.ObjectManager GetManagedObjects";
/// The start of a call-log line for a property read.
const GET: &str = "org.freedesktop.DBus.Properties Get";

/// The readings of the `ata` script: two drives at 305 K and 310.15 K.
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

/// Two ATA drives. Expects a start that reports available, one reading
/// per drive named by its model, keyed by its object path and with its
/// temperature in °C, and no call outside the allowed ones.
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

/// Drives whose `Model` is " Spaced  Model ", "Disque ÉTÉ", the empty
/// string, an integer, or missing, listed out of path order. Expects
/// readings for the three string models only, named exactly as given,
/// in object path order.
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

/// Drive A's `SmartTemperature` set in turn to 0, -1, NaN, 0.5, 273.15
/// and 400.0 K, with drive B at 310.15 K. Expects no reading for A at
/// all for the first three values, which are not above 0 K, and A's
/// value in °C for the others; B's reading is the same every time.
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

/// Three buses on which `start` must report unavailable: one whose
/// udisks2 has a drive without the `Drive.Ata` interface, one where no
/// udisks2 runs, and an address where no bus exists.
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

/// After a start, a drive is added, the first drive's model changes and
/// the second drive is removed. Expects the read to keep the drives and
/// names found at start: no reading for the new drive, the old name for
/// the first, the removed one without a value, and the drives listed
/// only once, at start.
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

/// Calls that fail for one drive. In the first read, A's call gets an
/// error reply and B's gets a string instead of a number; in the second,
/// B's call does not answer. Expects only the drive concerned without a
/// value each time, the others with theirs; the second read to take
/// between 1 and 1.5 s, since B's call runs to the time limit; and every
/// value back in the third read.
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

/// The bus and the fake stop after a read, and a new bus and fake start
/// at the same address. Expects the read while the bus is gone to fail,
/// and the next one to connect again and give the same readings, with
/// one property call per drive and no new listing: the drives found at
/// start are kept.
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

/// A udisks2 that never answers the listing. Expects `start` to report
/// unavailable after 1 to 1.5 s, the time limit.
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

/// The first drive's temperature changes between two reads. Expects each
/// read to make one property call per drive, and the second read to show
/// the new value; the call log then holds the listing and four property
/// calls, nothing else.
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

/// Watches the bus's name owners while the adapter starts, reads three
/// times and stops. Expects exactly one new connection at start, kept
/// for every read and closed after `stop`, and a call log of exactly the
/// one listing and one `SmartTemperature` read per drive per read.
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

/// Expects the adapter that the registry builds for production to use
/// the system bus with a 5 s time limit, and the udisks2 service name
/// and object manager path to be the real ones.
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
