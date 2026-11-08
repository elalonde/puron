//! Engine tests and the registry's switch-change tests, with fake
//! providers on the virtual clock, 5 s each. `c_engine_20`,
//! `c_engine_1__b` and `c_engine_21__c` run on the system clock; the
//! last two time the fakes on a `RealClock` of their own. One engine
//! check is the script `tools/c_engine_15.sh`; the logging test is in
//! `c_engine_25.rs`.

#![allow(non_snake_case)] // test names with a case suffix

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use puron_core::rows::Provider::{self, *};
use puron_core::settings::Settings;
use puron_engine::clock::{Clock, SystemClock};
use puron_engine::engine::{Engine, LmReport, Snapshot};
use puron_engine::registry::ReadOutcome;
use puron_testkit::eng::{
    adapters, r1, r2, CallKind, Fake, ReadStep, StartStep, StopStep, VirtualClock,
};

const LIMIT: u64 = 5;

/// Settings with only `ps` switched on and interval `n`.
fn on(ps: &[Provider], n: u32) -> Settings {
    Settings {
        provider_lm_sensors: ps.contains(&LmSensors),
        provider_liquidctl: ps.contains(&Liquidctl),
        provider_nvidia: ps.contains(&Nvidia),
        provider_udisks2: ps.contains(&Udisks2),
        provider_nvme: ps.contains(&NvmeCli),
        poll_interval_secs: n,
        ..Settings::default()
    }
}

const ALL: [Provider; 5] = [LmSensors, Liquidctl, Nvidia, Udisks2, NvmeCli];

struct Run {
    clock: Arc<VirtualClock>,
    e: Arc<Engine>,
    last: Snapshot,
}

impl Run {
    fn new() -> Run {
        let clock = VirtualClock::new();
        let e = Arc::new(Engine::new(clock.dyn_clock()));
        Run {
            clock,
            e,
            last: Snapshot {
                readings: vec![],
                lm_sensors: LmReport::NoneYet,
            },
        }
    }

    fn fake(&self, p: Provider) -> Fake {
        Fake::new(self.clock.dyn_clock(), p)
    }

    fn start(&self, settings: &Settings, fakes: &[Fake]) {
        self.e.start(settings, adapters(fakes));
        self.clock.settle();
    }

    fn to(&self, t: f64) {
        self.clock.advance_to(t);
    }

    fn set(&self, s: &Settings) {
        self.e.set_providers(s);
        self.clock.settle();
    }

    /// The latest snapshot published so far.
    fn snap(&mut self) -> Snapshot {
        if let Some(s) = self.e.try_next() {
            self.last = s;
        }
        self.last.clone()
    }

    /// `p`'s readings in the latest snapshot: (name, position, value).
    fn of(&mut self, p: Provider) -> Vec<(String, u32, Option<f64>)> {
        self.snap()
            .readings
            .iter()
            .filter(|r| r.provider == p)
            .map(|r| (r.name.clone().unwrap(), r.position, r.value))
            .collect()
    }
}

fn vals(a: f64, b: f64) -> Vec<(String, u32, Option<f64>)> {
    vec![("a".into(), 0, Some(a)), ("b".into(), 1, Some(b))]
}

fn r1v() -> Vec<(String, u32, Option<f64>)> {
    vals(40.0, 50.0)
}

fn r2v() -> Vec<(String, u32, Option<f64>)> {
    vals(41.0, 51.0)
}

fn novalue() -> Vec<(String, u32, Option<f64>)> {
    vec![("a".into(), 0, None), ("b".into(), 1, None)]
}

fn gives(o: ReadOutcome) -> ReadStep {
    ReadStep::Gives(o)
}

/// Real time measured by the test, apart from the engine's clock: the
/// fakes of the system-clock cases record their calls on it.
struct RealClock {
    origin: Instant,
    lock: Mutex<()>,
    cv: Condvar,
}

impl RealClock {
    fn new() -> Arc<RealClock> {
        Arc::new(RealClock {
            origin: Instant::now(),
            lock: Mutex::new(()),
            cv: Condvar::new(),
        })
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn wait_until(&self, deadline: Option<Duration>, ready: &mut dyn FnMut() -> bool) -> bool {
        let mut g = self.lock.lock().unwrap();
        loop {
            if ready() {
                return true;
            }
            let left = match deadline {
                Some(d) if self.now() >= d => return false,
                Some(d) => (d - self.now()).min(Duration::from_millis(10)),
                None => Duration::from_millis(10),
            };
            g = self.cv.wait_timeout(g, left).unwrap().0;
        }
    }

    fn wake(&self) {
        let _g = self.lock.lock().unwrap();
        self.cv.notify_all();
    }
}

/// Sleeps until `t` seconds of real time since `origin`.
fn sleep_to(origin: Instant, t: f64) {
    let left = t - origin.elapsed().as_secs_f64();
    if left > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(left));
    }
}

#[test]
fn c_engine_1__a() {
    puron_testkit::limit("C-engine-1", LIMIT, || {
        let r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([ReadStep::Takes(1.0, r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(16.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 5.0, 10.0, 15.0]);
    });
}

#[test]
fn c_engine_1__b() {
    puron_testkit::limit("C-engine-1", 15, || {
        let real = RealClock::new();
        let lm = Fake::new(real.clone(), LmSensors).reads([gives(r1(LmSensors))]);
        let e = Engine::new(SystemClock::new());
        let t0 = Instant::now();
        e.start(&on(&[LmSensors], 1), adapters(std::slice::from_ref(&lm)));
        sleep_to(t0, 3.6);
        let reads: Vec<f64> = lm
            .times(CallKind::Read)
            .into_iter()
            .filter(|&t| t <= 3.6)
            .collect();
        eprintln!("C-engine-1 case B reads: {reads:?}");
        assert_eq!(reads.len(), 4, "reads up to 3.6 s: {reads:?}");
        for (i, t) in reads.iter().enumerate() {
            let lo = i as f64;
            assert!(*t >= lo && *t <= lo + 0.5, "read {i} at {t} s");
        }
        e.shutdown();
    });
}

#[test]
fn c_engine_2() {
    puron_testkit::limit("C-engine-2", LIMIT, || {
        let mut r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([ReadStep::Takes(3.0, r1(LmSensors))]);
        let nv = r.fake(Nvidia).reads([gives(r1(Nvidia))]);
        r.start(&on(&[LmSensors, Nvidia], 5), &[lm, nv]);
        r.to(0.5);
        assert_eq!(r.of(Nvidia), r1v());
        assert_eq!(r.of(LmSensors), []);
        r.to(3.0);
        assert_eq!(r.of(Nvidia), r1v());
        assert_eq!(r.of(LmSensors), r1v());
    });
}

#[test]
fn c_engine_3() {
    puron_testkit::limit("C-engine-3", LIMIT, || {
        let r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([ReadStep::Takes(12.25, r1(LmSensors))]);
        r.start(&on(&[LmSensors], 1), std::slice::from_ref(&lm));
        r.to(25.5);
        r.e.set_interval(60);
        r.clock.settle();
        r.to(80.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 12.25, 24.5, 36.75]);
    });
}

#[test]
fn c_engine_4() {
    puron_testkit::limit("C-engine-4", LIMIT, || {
        let r = Run::new();
        let lq = r
            .fake(Liquidctl)
            .reads([ReadStep::Takes(2.5, r1(Liquidctl))]);
        r.start(&on(&[Liquidctl], 1), std::slice::from_ref(&lq));
        r.to(7.0);
        assert_eq!(lq.times(CallKind::Read), [0.0, 3.0, 6.0]);
    });
}

#[test]
fn c_engine_5() {
    puron_testkit::limit("C-engine-5", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(6.0);
        r.e.set_interval(3);
        r.to(13.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 5.0, 9.0, 12.0]);
    });
}

#[test]
fn c_engine_7() {
    puron_testkit::limit("C-engine-7", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        let got: Arc<Mutex<Vec<(f64, Snapshot)>>> = Arc::default();
        let (e, c, g) = (Arc::clone(&r.e), Arc::clone(&r.clock), Arc::clone(&got));
        r.clock.spawn(move || {
            for _ in 0..2 {
                let s = e.next();
                g.lock().unwrap().push((c.secs(), s));
            }
        });
        r.clock.settle();
        assert!(
            got.lock().unwrap().is_empty(),
            "a wait returned before start"
        );
        r.start(&on(&[LmSensors], 5), &[lm]);
        {
            let g = got.lock().unwrap();
            assert_eq!(g.len(), 1, "the first wait has not returned at 0");
            assert_eq!(g[0].0, 0.0);
            assert_eq!(g[0].1.readings, r1(LmSensors).readings());
        }
        r.to(4.999);
        assert_eq!(
            got.lock().unwrap().len(),
            1,
            "the second wait returned early"
        );
        r.to(5.0);
        let g = got.lock().unwrap();
        assert_eq!(g.len(), 2, "the second wait has not returned at 5");
        assert_eq!(g[1].0, 5.0);
        assert_eq!(g[1].1.readings, r1(LmSensors).readings());
    });
}

trait Readings {
    fn readings(self) -> Vec<puron_core::rows::RawReading>;
}

impl Readings for ReadOutcome {
    fn readings(self) -> Vec<puron_core::rows::RawReading> {
        match self {
            ReadOutcome::Readings(v) => v,
            _ => panic!("not readings"),
        }
    }
}

#[test]
fn c_engine_8() {
    puron_testkit::limit("C-engine-8", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([
            gives(r1(LmSensors)),
            gives(r2(LmSensors)),
            gives(r1(LmSensors)),
        ]);
        r.start(&on(&[LmSensors], 5), &[lm]);
        r.to(11.0);
        let s = r.e.next();
        assert_eq!(
            s.readings,
            r1(LmSensors).readings(),
            "not the snapshot of 10"
        );
        let back: Arc<Mutex<bool>> = Arc::default();
        let (e, b) = (Arc::clone(&r.e), Arc::clone(&back));
        r.clock.spawn(move || {
            e.next();
            *b.lock().unwrap() = true;
        });
        r.clock.settle();
        assert!(
            !*back.lock().unwrap(),
            "a read snapshot woke the wait again"
        );
    });
}

fn c_engine_9_case(read1: ReadStep, before: f64, at: f64) {
    let mut r = Run::new();
    let lm = r.fake(LmSensors).reads([read1, ReadStep::Hangs]);
    r.start(&on(&[LmSensors], 5), &[lm]);
    r.to(before);
    assert_eq!(r.of(LmSensors), r1v(), "values at {before}");
    r.to(at);
    assert_eq!(r.of(LmSensors), novalue(), "no value at {at}");
}

#[test]
fn c_engine_9__a() {
    puron_testkit::limit("C-engine-9", LIMIT, || {
        c_engine_9_case(gives(r1(LmSensors)), 14.999, 15.0);
    });
}

#[test]
fn c_engine_9__b() {
    puron_testkit::limit("C-engine-9", LIMIT, || {
        c_engine_9_case(ReadStep::Takes(7.0, r1(LmSensors)), 16.999, 17.0);
    });
}

#[test]
fn c_engine_10() {
    puron_testkit::limit("C-engine-10", LIMIT, || {
        let mut r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([gives(r1(LmSensors)), ReadStep::Hangs]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(20.0);
        assert_eq!(r.of(LmSensors), novalue());
        lm.release_read(r2(LmSensors));
        r.clock.settle();
        assert_eq!(r.of(LmSensors), r2v());
    });
}

#[test]
fn c_engine_11() {
    puron_testkit::limit("C-engine-11", LIMIT, || {
        let mut r = Run::new();
        let lm = r.fake(LmSensors).reads([
            gives(r1(LmSensors)),
            gives(ReadOutcome::Failed),
            gives(r2(LmSensors)),
            gives(ReadOutcome::Failed),
        ]);
        r.start(&on(&[LmSensors], 5), &[lm]);
        assert_eq!(r.of(LmSensors), r1v());
        r.to(5.0);
        assert_eq!(r.of(LmSensors), novalue());
        r.to(10.0);
        assert_eq!(r.of(LmSensors), r2v());
        r.to(16.0);
        r.set(&on(&[], 5));
        r.set(&on(&[LmSensors], 5));
        assert_eq!(r.of(LmSensors), []);
    });
}

#[test]
fn c_engine_12__a() {
    puron_testkit::limit("C-engine-12", LIMIT, || {
        let mut r = Run::new();
        let lq = r.fake(Liquidctl).reads([
            gives(r1(Liquidctl)),
            gives(ReadOutcome::Failed),
            gives(r1(Liquidctl)),
        ]);
        r.start(&on(&[Liquidctl], 5), &[lq]);
        assert_eq!(r.of(Liquidctl), r1v());
        r.to(5.0);
        assert_eq!(r.of(Liquidctl), []);
        r.to(10.0);
        assert_eq!(r.of(Liquidctl), r1v());
    });
}

#[test]
fn c_engine_12__b() {
    puron_testkit::limit("C-engine-12", LIMIT, || {
        let mut r = Run::new();
        let lq = r
            .fake(Liquidctl)
            .reads([gives(r1(Liquidctl)), ReadStep::Hangs]);
        r.start(&on(&[Liquidctl], 5), &[lq]);
        r.to(15.0);
        assert_eq!(r.of(Liquidctl), r1v());
        r.to(30.0);
        assert_eq!(r.of(Liquidctl), r1v());
    });
}

#[test]
fn c_engine_13() {
    puron_testkit::limit("C-engine-13", LIMIT, || {
        let mut r = Run::new();
        let ud = r
            .fake(Udisks2)
            .starts([StartStep::Hangs])
            .reads([gives(r1(Udisks2))]);
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        r.start(&on(&[LmSensors, Udisks2], 5), &[lm.clone(), ud.clone()]);
        r.to(11.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 5.0, 10.0]);
        assert_eq!(r.of(Udisks2), []);
        assert_eq!(ud.times(CallKind::Read), Vec::<f64>::new());
        ud.release_start();
        r.clock.settle();
        assert_eq!(ud.times(CallKind::Read), [11.0]);
    });
}

#[test]
fn c_engine_14__read() {
    puron_testkit::limit("C-engine-14", LIMIT, || {
        let mut r = Run::new();
        let lm =
            r.fake(LmSensors)
                .reads([gives(r1(LmSensors)), ReadStep::Panics, gives(r1(LmSensors))]);
        let nv = r.fake(Nvidia).reads([gives(r2(Nvidia))]);
        r.start(&on(&[LmSensors, Nvidia], 5), &[lm, nv]);
        assert_eq!(r.of(LmSensors), r1v());
        assert_eq!(r.e.workers(), 2);
        r.to(5.0);
        assert_eq!(r.of(LmSensors), novalue());
        assert_eq!(r.of(Nvidia), r2v());
        assert_eq!(r.e.workers(), 2);
        r.to(10.0);
        assert_eq!(r.of(LmSensors), r1v());
        assert_eq!(r.e.workers(), 2);
    });
}

#[test]
fn c_engine_14__start() {
    puron_testkit::limit("C-engine-14", LIMIT, || {
        let mut r = Run::new();
        let lm = r
            .fake(LmSensors)
            .starts([StartStep::Panics, StartStep::Available])
            .reads([gives(r1(LmSensors))]);
        let nv = r.fake(Nvidia).reads([gives(r2(Nvidia))]);
        r.start(&on(&[LmSensors, Nvidia], 5), &[lm.clone(), nv]);
        r.to(11.0);
        assert_eq!(r.of(LmSensors), []);
        assert_eq!(lm.count(CallKind::Read), 0, "Read after a panicking start");
        assert_eq!(r.snap().lm_sensors, LmReport::Unavailable);
        r.set(&on(&[Nvidia], 5));
        r.set(&on(&[LmSensors, Nvidia], 5));
        assert_eq!(lm.times(CallKind::Read), [11.0]);
        assert_eq!(r.of(LmSensors), r1v());
        assert_eq!(r.e.workers(), 2);
    });
}

#[test]
fn c_engine_14__stop() {
    puron_testkit::limit("C-engine-14", LIMIT, || {
        let mut r = Run::new();
        let lm = r
            .fake(LmSensors)
            .stops([StopStep::Panics, StopStep::AtOnce])
            .reads([gives(r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(1.0);
        r.set(&on(&[], 5));
        assert_eq!(r.of(LmSensors), []);
        r.set(&on(&[LmSensors], 5));
        assert_eq!(lm.times(CallKind::Start), [0.0, 1.0]);
        assert_eq!(lm.times(CallKind::Read), [0.0, 1.0]);
        assert_eq!(r.of(LmSensors), r1v());
        assert_eq!(r.e.workers(), 1);
        // Stop only after a start: not again at shut down.
        r.set(&on(&[], 5));
        r.e.shutdown();
        assert_eq!(lm.count(CallKind::Stop), 2);
    });
}

#[test]
fn c_engine_16() {
    puron_testkit::limit("C-engine-16", LIMIT, || {
        let mut r = Run::new();
        let lm = r.fake(LmSensors).reads([ReadStep::Hangs]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        for (t, s) in [(2.0, false), (3.0, true), (4.0, false), (5.0, true)] {
            r.to(t);
            r.set(&on(if s { &[LmSensors] } else { &[] }, 5));
        }
        r.to(6.0);
        lm.release_read(r1(LmSensors));
        r.clock.settle();
        assert_eq!(r.of(LmSensors), [], "R1 of the old generation shown");
        let after: Vec<CallKind> = lm
            .calls()
            .iter()
            .filter(|c| c.start == 6.0)
            .map(|c| c.call)
            .collect();
        use CallKind::*;
        assert_eq!(after, [Stop, Start, Stop, Start, Read]);
        lm.release_read(r2(LmSensors));
        r.clock.settle();
        assert_eq!(r.of(LmSensors), r2v());
        assert_eq!(r.e.workers(), 1);
    });
}

#[test]
fn c_engine_17() {
    puron_testkit::limit("C-engine-17", LIMIT, || {
        let mut r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([ReadStep::Hangs, gives(r2(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(3.0);
        r.set(&on(&[], 5));
        r.to(4.0);
        r.set(&on(&[LmSensors], 5));
        r.to(6.0);
        lm.release_read(r1(LmSensors));
        r.to(10.0);
        assert_eq!(r.of(LmSensors), r2v());
    });
}

#[test]
fn c_engine_18() {
    puron_testkit::limit("C-engine-18", LIMIT, || {
        let mut r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), &[lm]);
        r.to(2.0);
        assert_eq!(r.of(LmSensors), r1v());
        r.set(&on(&[], 5));
        assert_eq!(r.of(LmSensors), []);
        assert_eq!(r.snap().lm_sensors, LmReport::NoneYet);
        assert_eq!(r.clock.secs(), 2.0);
    });
}

#[test]
fn c_engine_19() {
    puron_testkit::limit("C-engine-19", LIMIT, || {
        let mut r = Run::new();
        let lm = r.fake(LmSensors).reads([
            gives(ReadOutcome::Unavailable),
            gives(r1(LmSensors)),
            gives(ReadOutcome::Failed),
            ReadStep::Hangs,
        ]);
        let nv = r.fake(Nvidia).reads([gives(ReadOutcome::Unavailable)]);
        assert_eq!(r.snap().lm_sensors, LmReport::NoneYet);
        r.start(&on(&[LmSensors, Nvidia], 5), &[lm, nv]);
        assert_eq!(r.snap().lm_sensors, LmReport::Unavailable);
        r.to(5.0);
        assert_eq!(r.snap().lm_sensors, LmReport::Answered);
        r.to(10.0);
        assert_eq!(r.snap().lm_sensors, LmReport::Answered);
        r.to(25.0);
        assert_eq!(r.of(LmSensors), novalue(), "the deadline has passed");
        assert_eq!(r.snap().lm_sensors, LmReport::Answered);
        r.set(&on(&[Nvidia], 5));
        assert_eq!(r.snap().lm_sensors, LmReport::NoneYet);
    });
}

#[test]
fn c_engine_20() {
    puron_testkit::limit("C-engine-20", LIMIT, || {
        let clock = SystemClock::new();
        let e = Arc::new(Engine::new(Arc::clone(&clock)));
        let fakes: Vec<Fake> = ALL
            .iter()
            .map(|&p| {
                Fake::new(Arc::clone(&clock), p)
                    .starts([StartStep::Hangs, StartStep::Available])
                    .reads([ReadStep::Hangs, gives(ReadOutcome::Failed)])
            })
            .collect();
        let back: Arc<Mutex<Option<Snapshot>>> = Arc::default();
        let (e2, b) = (Arc::clone(&e), Arc::clone(&back));
        std::thread::spawn(move || {
            let s = e2.next();
            *b.lock().unwrap() = Some(s);
        });
        let quick = |what: &str, f: &dyn Fn()| {
            let t = Instant::now();
            f();
            let d = t.elapsed().as_secs_f64();
            assert!(d < 0.25, "{what} took {d} s");
        };
        quick("Start", &|| e.start(&on(&ALL, 5), adapters(&fakes)));
        quick("Poll now", &|| e.poll_now());
        quick("Set interval", &|| e.set_interval(3));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(back.lock().unwrap().is_none(), "the wait returned");
        quick("Set providers off", &|| e.set_providers(&on(&[], 3)));
        let t = Instant::now();
        let snap = loop {
            if let Some(s) = back.lock().unwrap().take() {
                break s;
            }
            assert!(t.elapsed().as_secs_f64() < 1.0, "no stop snapshot");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(snap.readings, []);
        quick("Set providers on", &|| e.set_providers(&on(&ALL, 3)));
        for f in &fakes {
            f.release_start();
            f.release_read(ReadOutcome::Failed);
        }
        e.shutdown();
    });
}

#[test]
fn c_engine_21__a() {
    puron_testkit::limit("C-engine-21", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        let nv = r.fake(Nvidia).reads([gives(r2(Nvidia))]);
        r.start(&on(&[LmSensors, Nvidia], 5), &[lm.clone(), nv.clone()]);
        r.e.shutdown();
        assert_eq!(r.clock.secs(), 0.0, "the clock moved");
        assert_eq!(lm.count(CallKind::Stop), 1);
        assert_eq!(nv.count(CallKind::Stop), 1);
    });
}

#[test]
fn c_engine_21__b() {
    puron_testkit::limit("C-engine-21", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([ReadStep::Hangs]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        let back: Arc<Mutex<Option<f64>>> = Arc::default();
        let (e, c, b) = (Arc::clone(&r.e), Arc::clone(&r.clock), Arc::clone(&back));
        r.clock.spawn(move || {
            e.shutdown();
            *b.lock().unwrap() = Some(c.secs());
        });
        r.to(0.999);
        assert_eq!(*back.lock().unwrap(), None, "returned before 1 s");
        r.to(60.0);
        assert_eq!(*back.lock().unwrap(), Some(1.0));
        assert_eq!(lm.times(CallKind::Read), [0.0]);
    });
}

#[test]
fn c_engine_21__c() {
    puron_testkit::limit("C-engine-21", 8, || {
        let real = RealClock::new();
        let lm = Fake::new(real.clone(), LmSensors).reads([ReadStep::Hangs]);
        let e = Engine::new(SystemClock::new());
        let t0 = Instant::now();
        e.start(&on(&[LmSensors], 5), adapters(std::slice::from_ref(&lm)));
        sleep_to(t0, 2.0);
        let t = Instant::now();
        e.shutdown();
        let d = t.elapsed().as_secs_f64();
        lm.release_read(ReadOutcome::Failed);
        eprintln!("C-engine-21 case C: Shut down took {d} s");
        assert!((1.0..=1.5).contains(&d), "Shut down took {d} s");
    });
}

#[test]
fn c_engine_22__stop() {
    puron_testkit::limit("C-engine-22", LIMIT, || {
        let r = Run::new();
        let lq = r.fake(Liquidctl).reads([ReadStep::Hangs]);
        r.start(&on(&[Liquidctl], 5), std::slice::from_ref(&lq));
        r.to(2.0);
        r.set(&on(&[], 5));
        assert_eq!(lq.calls()[1].call, CallKind::Read);
        assert_eq!(lq.calls()[1].stop_seen, Some(2.0));
        // The next start clears the signal.
        r.set(&on(&[Liquidctl], 5));
        assert_eq!(lq.calls()[4].call, CallKind::Read);
        assert_eq!(lq.calls()[4].stop_seen, None);
    });
}

#[test]
fn c_engine_22__shutdown() {
    puron_testkit::limit("C-engine-22", LIMIT, || {
        let r = Run::new();
        let lq = r.fake(Liquidctl).reads([ReadStep::Hangs]);
        r.start(&on(&[Liquidctl], 5), std::slice::from_ref(&lq));
        r.to(2.0);
        let (e, c) = (Arc::clone(&r.e), Arc::clone(&r.clock));
        r.clock.spawn(move || {
            e.shutdown();
            let _ = c;
        });
        r.clock.settle();
        assert_eq!(lq.calls()[1].call, CallKind::Read);
        assert_eq!(lq.calls()[1].stop_seen, Some(2.0));
    });
}

#[test]
fn c_engine_24() {
    puron_testkit::limit("C-engine-24", LIMIT, || {
        let r = Run::new();
        let fakes: Vec<Fake> = ALL.iter().map(|&p| r.fake(p)).collect();
        r.e.poll_now();
        r.e.set_interval(2);
        r.set(&on(&ALL, 5));
        r.to(10.0);
        for f in &fakes {
            assert_eq!(f.times(CallKind::Read), [0.0; 0]);
        }
        assert_eq!(r.e.workers(), 0);
        assert!(r.e.try_next().is_none(), "a snapshot before Start");
        r.start(&Settings::default(), &fakes);
        assert_eq!(r.e.workers(), 1);
        r.set(&on(&ALL, 5));
        assert_eq!(r.e.workers(), 5);
        for _ in 0..100 {
            r.e.set_providers(&on(&[], 5));
            r.e.set_providers(&on(&ALL, 5));
        }
        r.clock.settle();
        assert_eq!(r.e.workers(), 5);
    });
}

#[test]
fn c_engine_26__a() {
    puron_testkit::limit("C-engine-26", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        for t in [2.0, 4.0] {
            r.to(t);
            r.e.poll_now();
        }
        r.to(15.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 2.0, 4.0, 9.0, 14.0]);
    });
}

#[test]
fn c_engine_26__b() {
    puron_testkit::limit("C-engine-26", LIMIT, || {
        let r = Run::new();
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        let nv = r.fake(Nvidia).reads([gives(r2(Nvidia))]);
        r.start(&on(&[LmSensors], 5), &[lm.clone(), nv.clone()]);
        r.to(2.0);
        r.e.set_providers(&on(&[LmSensors, Nvidia], 5));
        r.to(8.0);
        assert_eq!(nv.times(CallKind::Start), [2.0]);
        assert_eq!(nv.times(CallKind::Read), [2.0, 7.0]);
        assert_eq!(lm.times(CallKind::Read), [0.0, 2.0, 7.0]);
    });
}

#[test]
fn c_engine_26__c() {
    puron_testkit::limit("C-engine-26", LIMIT, || {
        let r = Run::new();
        let lm = r
            .fake(LmSensors)
            .reads([ReadStep::Takes(2.0, r1(LmSensors))]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        r.to(1.0);
        r.e.poll_now();
        r.to(7.0);
        assert_eq!(lm.times(CallKind::Read), [0.0, 2.0, 6.0]);
    });
}

#[test]
fn c_src_registry_3() {
    puron_testkit::limit("C-src.registry-3", LIMIT, || {
        let r = Run::new();
        let ud = r.fake(Udisks2).reads([gives(r1(Udisks2))]);
        r.start(&on(&[Udisks2], 5), std::slice::from_ref(&ud));
        r.to(11.0);
        assert_eq!(ud.times(CallKind::Start), [0.0]);
        assert_eq!(ud.times(CallKind::Read), [0.0, 5.0, 10.0]);
        r.to(12.0);
        r.set(&on(&[], 5));
        r.set(&on(&[Udisks2], 5));
        assert_eq!(ud.times(CallKind::Start), [0.0, 12.0]);
    });
}

#[test]
fn c_src_registry_4() {
    puron_testkit::limit("C-src.registry-4", LIMIT, || {
        let mut r = Run::new();
        let nv = r
            .fake(NvmeCli)
            .starts([StartStep::Unavailable, StartStep::Available])
            .reads([gives(r1(NvmeCli))]);
        r.start(&on(&[NvmeCli], 5), std::slice::from_ref(&nv));
        let empty = Snapshot {
            readings: vec![],
            lm_sensors: LmReport::NoneYet,
        };
        for t in [0.0, 5.0, 10.0, 11.0] {
            r.to(t);
            assert_eq!(r.snap(), empty, "at {t}");
        }
        assert_eq!(nv.count(CallKind::Read), 0);
        assert_eq!(nv.times(CallKind::Start), [0.0]);
        r.to(12.0);
        r.set(&on(&[], 5));
        r.set(&on(&[NvmeCli], 5));
        assert_eq!(nv.times(CallKind::Start), [0.0, 12.0]);
        assert_eq!(nv.times(CallKind::Read), [12.0]);
        assert_eq!(r.of(NvmeCli), r1v());
    });
}

#[test]
fn c_src_registry_5() {
    puron_testkit::limit("C-src.registry-5", LIMIT, || {
        let mut r = Run::new();
        let lm = r.fake(LmSensors).reads([
            gives(r1(LmSensors)),
            gives(ReadOutcome::Unavailable),
            gives(r1(LmSensors)),
        ]);
        r.start(&on(&[LmSensors], 5), std::slice::from_ref(&lm));
        assert_eq!(r.of(LmSensors), r1v());
        r.to(5.0);
        assert_eq!(r.of(LmSensors), []);
        r.to(10.0);
        assert_eq!(r.of(LmSensors), r1v());
        assert_eq!(lm.count(CallKind::Start), 1);
    });
}

// A handle dropped without a `shutdown` call still stops the workers.
#[test]
fn engine_drop_sends_shutdown() {
    puron_testkit::limit("p4f3-drop", LIMIT, || {
        let r = Run::new();
        let lq = r.fake(Liquidctl).reads([ReadStep::Hangs]);
        r.start(&on(&[Liquidctl], 5), std::slice::from_ref(&lq));
        r.to(2.0);
        let Run { clock, e, .. } = r;
        drop(Arc::try_unwrap(e).ok().expect("the only handle"));
        clock.settle();
        assert_eq!(lq.calls()[1].call, CallKind::Read);
        assert_eq!(lq.calls()[1].stop_seen, Some(2.0));
    });
}

// A stop drops the stopped provider's read deadline only; another
// provider's hung read still passes its deadline.
#[test]
fn stop_keeps_other_deadlines() {
    puron_testkit::limit("p4f3-deadlines", LIMIT, || {
        let mut r = Run::new();
        let ud = r.fake(Udisks2).reads([gives(r1(Udisks2)), ReadStep::Hangs]);
        let lm = r.fake(LmSensors).reads([gives(r1(LmSensors))]);
        r.start(&on(&[Udisks2, LmSensors], 5), &[ud, lm]);
        r.to(6.0);
        assert_eq!(r.of(Udisks2), r1v());
        r.set(&on(&[Udisks2], 5));
        r.to(15.5);
        assert_eq!(r.of(Udisks2), novalue());
    });
}
