//! Tests of the engine (`puron_engine::engine`): when it reads each
//! provider, what its snapshots show, and how it starts, stops and shuts
//! down providers as the settings change.
//!
//! Most tests run the engine on the virtual clock of `puron_testkit::eng`
//! with fake providers (`Fake`), so that every time is exact. Three tests
//! run on the system clock instead: `c_engine_20`, `c_engine_1__b` and
//! `c_engine_21__c`; the last two let their fakes record times on a
//! `RealClock` of their own. One more engine test is the script
//! `tools/c_engine_15.sh`, and the test of the engine's diagnostics is in
//! `c_engine_25.rs`.
//!
//! Words used below. Liquidctl is the only child-run provider: its
//! adapter runs a program for each read, the engine sets no read deadline
//! for it, and its read ends early when the engine raises the stop
//! signal, which it does when it switches the provider off or shuts
//! down. Every other provider is in-process, and the engine shows its
//! readings without values while a read runs past `READ_DEADLINE`
//! (10 s). Each start of a provider begins a new generation, and the
//! engine drops a result that belongs to an earlier one. `r1` gives the
//! readings `a` = 40 and `b` = 50, `r2` gives 41 and 51.

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

/// The time limit of most tests here, in seconds.
const LIMIT: u64 = 5;

/// Settings with the providers in `ps` switched on, every other provider
/// switched off, and a poll interval of `n` seconds.
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

/// Every provider, in the engine's order.
const ALL: [Provider; 5] = [LmSensors, Liquidctl, Nvidia, Udisks2, NvmeCli];

/// An engine on a virtual clock, with the steps most tests take.
/// `Engine::try_next` returns each snapshot only once, so `last` keeps
/// the most recent one that the test has taken.
struct Run {
    clock: Arc<VirtualClock>,
    e: Arc<Engine>,
    last: Snapshot,
}

impl Run {
    /// A virtual clock at time 0, an engine on it that has not been
    /// started yet, and an empty snapshot as the last one seen.
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

    /// A fake for provider `p` that records its calls on this run's
    /// clock.
    fn fake(&self, p: Provider) -> Fake {
        Fake::new(self.clock.dyn_clock(), p)
    }

    /// Starts the engine with `settings` and these fakes, then waits
    /// until every engine thread waits on the clock again, so that
    /// everything the start does at the current time has happened.
    fn start(&self, settings: &Settings, fakes: &[Fake]) {
        self.e.start(settings, adapters(fakes));
        self.clock.settle();
    }

    /// Moves the clock to `t` seconds and returns once the engine has
    /// handled everything that was due up to then.
    fn to(&self, t: f64) {
        self.clock.advance_to(t);
    }

    /// Sends new provider switches and waits until the engine has
    /// handled them, without moving the clock.
    fn set(&self, s: &Settings) {
        self.e.set_providers(s);
        self.clock.settle();
    }

    /// The latest snapshot the engine has published: a new one if there
    /// is one, otherwise the last one seen.
    fn snap(&mut self) -> Snapshot {
        if let Some(s) = self.e.try_next() {
            self.last = s;
        }
        self.last.clone()
    }

    /// The readings of provider `p` in the latest snapshot, as (name,
    /// position, value), in snapshot order.
    fn of(&mut self, p: Provider) -> Vec<(String, u32, Option<f64>)> {
        self.snap()
            .readings
            .iter()
            .filter(|r| r.provider == p)
            .map(|r| (r.name.clone().unwrap(), r.position, r.value))
            .collect()
    }
}

/// The readings `a` at position 0 and `b` at position 1, with these
/// values, in the form `Run::of` returns.
fn vals(a: f64, b: f64) -> Vec<(String, u32, Option<f64>)> {
    vec![("a".into(), 0, Some(a)), ("b".into(), 1, Some(b))]
}

/// What `Run::of` returns for a provider that shows the readings of `r1`.
fn r1v() -> Vec<(String, u32, Option<f64>)> {
    vals(40.0, 50.0)
}

/// What `Run::of` returns for a provider that shows the readings of `r2`.
fn r2v() -> Vec<(String, u32, Option<f64>)> {
    vals(41.0, 51.0)
}

/// What `Run::of` returns for a provider that shows the lines `a` and `b`
/// without values.
fn novalue() -> Vec<(String, u32, Option<f64>)> {
    vec![("a".into(), 0, None), ("b".into(), 1, None)]
}

fn gives(o: ReadOutcome) -> ReadStep {
    ReadStep::Gives(o)
}

/// A clock on real time, separate from the engine's `SystemClock`, for
/// the fakes of the tests that run on the system clock. A fake records
/// its call times on it, in seconds since the `RealClock` was created,
/// so that the test can compare them with the times it sleeps to. A wait
/// checks `ready` again at least every 10 ms.
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

/// Sleeps until `t` seconds of real time have passed since `origin`, or
/// returns at once if they already have.
fn sleep_to(origin: Instant, t: f64) {
    let left = t - origin.elapsed().as_secs_f64();
    if left > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(left));
    }
}

/// lm-sensors reads take 1 s each, at a 5 s interval. Expects reads to
/// start at 0, 5, 10 and 15 s: the interval counts from the start of a
/// read, not from its end.
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

/// The same on the system clock: lm-sensors reads return at once, at a
/// 1 s interval. Expects exactly 4 reads in the first 3.6 s, read `i`
/// starting between `i` and `i` + 0.5 s.
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

/// lm-sensors reads take 3 s and NVIDIA reads return at once. Expects
/// the snapshot at 0.5 s to show NVIDIA's readings and no lm-sensors
/// lines, and the one at 3 s to show both: a slow provider does not
/// delay another one.
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

/// lm-sensors reads take 12.25 s at a 1 s interval; at 25.5 s the
/// interval becomes 60 s. Expects reads at 0, 12.25, 24.5 and 36.75 s:
/// while a read runs, the polls queue at most one more read, which
/// starts as soon as the running one ends. The read queued before the
/// interval change still runs; the next poll comes only at 85.5 s.
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

/// Liquidctl reads take 2.5 s at a 1 s interval. Expects reads at 0, 3
/// and 6 s: for a child-run provider, the polls that arrive while its
/// program runs are dropped, so the next read waits for the next poll.
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

/// lm-sensors at a 5 s interval; at 6 s the interval becomes 3 s.
/// Expects reads at 0, 5, 9 and 12 s: the new interval counts from the
/// change.
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

/// A thread calls `Engine::next` twice, starting before the engine is
/// started. Expects the first call not to return before the start, then
/// to return at 0 s with lm-sensors' first readings; the second call
/// returns at 5 s, after the second read, and not before.
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

/// The readings of a `ReadOutcome::Readings`, to compare with those of a
/// snapshot. Panics on any other outcome.
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

/// lm-sensors gives `r1`, `r2` and `r1` at 0, 5 and 10 s, and nobody
/// takes a snapshot until 11 s. Expects `Engine::next` then to return the
/// snapshot of 10 s at once, since only the latest one is kept, and a
/// second `next` to keep waiting: a snapshot already returned does not
/// end a wait again.
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

/// One case of the read deadline: lm-sensors' first read is `read1`,
/// which gives `r1`, and its second read never returns. Expects the
/// lines with values at `before` and without values at `at`, the time
/// when the second read's deadline passes.
fn c_engine_9_case(read1: ReadStep, before: f64, at: f64) {
    let mut r = Run::new();
    let lm = r.fake(LmSensors).reads([read1, ReadStep::Hangs]);
    r.start(&on(&[LmSensors], 5), &[lm]);
    r.to(before);
    assert_eq!(r.of(LmSensors), r1v(), "values at {before}");
    r.to(at);
    assert_eq!(r.of(LmSensors), novalue(), "no value at {at}");
}

/// The first read returns at once, so the second starts at 5 s and its
/// deadline passes at 15 s.
#[test]
fn c_engine_9__a() {
    puron_testkit::limit("C-engine-9", LIMIT, || {
        c_engine_9_case(gives(r1(LmSensors)), 14.999, 15.0);
    });
}

/// The first read takes 7 s, so the second read, asked for at 5 s,
/// starts at 7 s, and its deadline passes at 17 s: the deadline counts
/// from the start of the read, not from the poll.
#[test]
fn c_engine_9__b() {
    puron_testkit::limit("C-engine-9", LIMIT, || {
        c_engine_9_case(ReadStep::Takes(7.0, r1(LmSensors)), 16.999, 17.0);
    });
}

/// lm-sensors' second read hangs past its deadline and then gives `r2`.
/// Expects the lines without values at 20 s, and `r2`'s values once the
/// read returns: a late result is still used.
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

/// lm-sensors reads give `r1`, a failure, `r2`, then failures. Expects
/// the lines with values, then without values after the failure, then
/// `r2`'s values. After lm-sensors is switched off and on again at 16 s,
/// its next read fails and it shows no lines at all: the restart has
/// forgotten the earlier readings.
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

/// Liquidctl reads give readings, a failure, then readings again.
/// Expects its lines at 0 s, no lines after the failed read at 5 s, and
/// its lines again at 10 s: a failed child-run read removes the lines.
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

/// Liquidctl's second read never returns. Expects its lines to keep
/// their values at 15 and 30 s: the engine sets no read deadline for a
/// child-run provider.
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

/// udisks2's start hangs while lm-sensors is read normally. Expects
/// lm-sensors to be read at 0, 5 and 10 s, and udisks2 to show no lines
/// and get no read until its start returns at 11 s; it is then read at
/// once.
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

/// lm-sensors' second read panics. Expects the lines without values
/// after it (a panic counts as a failed read), NVIDIA's readings
/// unchanged, both worker threads still running, and the values back
/// after the next read at 10 s.
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

/// lm-sensors' first start panics. Expects no lines and no read up to
/// 11 s, with the lm-sensors state `Unavailable`. After lm-sensors is
/// switched off and on again, it starts, is read at once and shows its
/// values, on the same worker thread (2 in all).
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

/// lm-sensors' first stop panics, at 1 s. Expects its lines to be gone,
/// and a new start and read at 1 s when it is switched on again, on the
/// one worker thread. After it is switched off and the engine shuts
/// down, `stop` has been called twice: shutdown does not stop an adapter
/// that is already stopped.
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
        // The adapter is stopped now, so shutdown must not stop it again.
        r.set(&on(&[], 5));
        r.e.shutdown();
        assert_eq!(lm.count(CallKind::Stop), 2);
    });
}

/// lm-sensors' first read hangs while it is switched off at 2 s, on at
/// 3 s, off at 4 s and on at 5 s. When the read returns `r1` at 6 s,
/// expects it to be dropped, since it belongs to an earlier generation,
/// and the worker then to handle in order: stop, start, stop, start and
/// one read. That read's `r2` is shown, and there is still one worker
/// thread.
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

/// lm-sensors' first read hangs; it is switched off at 3 s and on at
/// 4 s, and the hung read returns `r1` at 6 s. Expects the snapshot at
/// 10 s to show `r2`, from the reads of the new start, and not the
/// dropped `r1`.
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

/// lm-sensors is switched off at 2 s. Expects its lines to be gone and
/// its state to be `NoneYet` at once, without the clock moving.
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

/// The lm-sensors state of the snapshot (`LmReport`). lm-sensors reads
/// give unavailable, `r1`, a failure, then hang; NVIDIA is unavailable.
/// Expects `NoneYet` before the start, `Unavailable` after the first
/// read, `Answered` after the successful read and still after the failed
/// one, `Answered` when the hung read's deadline has passed at 25 s (the
/// lines shown without values), and `NoneYet` once lm-sensors is
/// switched off.
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

/// On the system clock, every provider's first start and first read
/// hang. Expects `start`, `poll_now`, `set_interval` and two
/// `set_providers` calls each to return within 0.25 s; a thread waiting
/// in `Engine::next` not to return within 100 ms of the start; and,
/// after every provider is switched off, that wait to return an empty
/// snapshot within 1 s. At the end the fakes are released and the
/// engine is shut down.
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

/// Shutdown with two providers that are not reading. Expects it to
/// return without the clock moving, after one `stop` call for each.
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

/// Shutdown while lm-sensors' read hangs. Expects it to return at
/// exactly 1 s of clock time (`SHUTDOWN_WAIT`), not before, and no read
/// after the first.
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

/// The same on the system clock: shutdown at 2 s while lm-sensors' read
/// hangs. Expects it to take between 1 and 1.5 s.
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

/// Liquidctl's read hangs, and liquidctl is switched off at 2 s.
/// Expects that read to see the stop signal at 2 s. After it is switched
/// on again, expects the read of the new start not to see the signal.
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
        // After this start, the fake's calls are: start, read, stop,
        // start, read.
        r.set(&on(&[Liquidctl], 5));
        assert_eq!(lq.calls()[4].call, CallKind::Read);
        assert_eq!(lq.calls()[4].stop_seen, None);
    });
}

/// Liquidctl's read hangs, and the engine shuts down at 2 s. Expects
/// that read to see the stop signal at 2 s.
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

/// `poll_now`, `set_interval` and `set_providers` before the first
/// `start`. Expects them to have no effect: no read up to 10 s, no
/// worker thread and no snapshot. After a start with the default
/// settings, expects one worker thread (lm-sensors); after every
/// provider is switched on, 5; and still 5 after 100 more switches off
/// and on, since a provider's worker thread is reused.
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

/// `poll_now` at 2 and 4 s, at a 5 s interval. Expects reads at 0, 2,
/// 4, 9 and 14 s: each `poll_now` reads at once and restarts the
/// interval.
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

/// NVIDIA is switched on at 2 s next to lm-sensors. Expects NVIDIA to be
/// started and read at 2 s, lm-sensors to be read at 2 s too, and both
/// to be read again at 7 s: a change of the switches polls every
/// provider and restarts the interval.
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

/// `poll_now` at 1 s while a 2 s lm-sensors read runs. Expects reads at
/// 0, 2 and 6 s: the asked-for read runs when the running one ends, and
/// the interval restarts from the `poll_now`.
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

/// udisks2 switched on. Expects one start at 0 s and reads every 5 s;
/// switched off and on again at 12 s, it is started again.
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

/// NVMe reports unavailable at start. Expects empty snapshots, no read
/// and no other start up to 11 s: the engine does not retry by itself.
/// After NVMe is switched off and on again at 12 s, it is started again,
/// read at once, and shows its readings.
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

/// lm-sensors' second read reports unavailable. Expects its lines at
/// 0 s, no lines at 5 s, its lines again at 10 s, and only one start:
/// an unavailable read does not restart the provider.
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

/// Liquidctl's read hangs, and at 2 s the only `Engine` handle is
/// dropped without a `shutdown` call. Expects the read to see the stop
/// signal at 2 s: dropping the handle shuts the engine down.
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

/// udisks2's read at 5 s hangs, and lm-sensors is switched off at 6 s.
/// Expects udisks2's lines without values at 15.5 s: switching off one
/// provider removes only that provider's read deadline.
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
