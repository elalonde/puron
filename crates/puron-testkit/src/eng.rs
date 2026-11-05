//! Engine test fixtures: a virtual clock, a scripted fake provider whose
//! calls can hang, and a recorded engine handle.

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::clock::Clock;
use puron_engine::engine::Adapters;
use puron_engine::registry::{ProviderKind, ReadOutcome, Source, StartOutcome, PROVIDERS};

thread_local! {
    /// The thread is one of the clock's (an engine thread, or one of
    /// `VirtualClock::spawn`): its waits count as idle.
    static COUNTED: Cell<bool> = const { Cell::new(false) };
}

/// The virtual clock: time 0 at creation, moved only by `advance_to`. The
/// clock moves only when every counted thread waits on it, so each
/// wait that ends at or before the new time runs, in time order,
/// before `advance_to` returns.
pub struct VirtualClock {
    nanos: AtomicU64,
    m: Mutex<Inner>,
    cv: Condvar,
}

struct Inner {
    /// Counted threads that do not wait.
    running: usize,
    /// Counted threads that wait.
    waiting: usize,
    epoch: u64,
    deadlines: BTreeMap<Duration, usize>,
}

impl VirtualClock {
    pub fn new() -> Arc<VirtualClock> {
        Arc::new(VirtualClock {
            nanos: AtomicU64::new(0),
            m: Mutex::new(Inner {
                running: 0,
                waiting: 0,
                epoch: 0,
                deadlines: BTreeMap::new(),
            }),
            cv: Condvar::new(),
        })
    }

    pub fn dyn_clock(self: &Arc<Self>) -> Arc<dyn Clock> {
        Arc::clone(self) as Arc<dyn Clock>
    }

    fn broadcast(&self, g: &mut Inner) {
        g.running += g.waiting;
        g.waiting = 0;
        g.epoch += 1;
        self.cv.notify_all();
    }

    /// Moves the clock to `t` seconds, through every deadline before it.
    pub fn advance_to(&self, t: f64) {
        let t = secs(t);
        assert!(t >= self.now(), "the clock does not go back");
        let mut g = self.m.lock().unwrap();
        loop {
            while g.running > 0 {
                g = self.cv.wait(g).unwrap();
            }
            let next = match g.deadlines.keys().next() {
                Some(&d) if d <= t => d,
                _ => t,
            };
            if next <= self.now() {
                return;
            }
            self.nanos.store(next.as_nanos() as u64, Ordering::SeqCst);
            self.broadcast(&mut g);
        }
    }

    /// Lets every counted thread run until it waits; the clock stays.
    pub fn settle(&self) {
        self.advance_to(self.secs());
    }

    pub fn secs(&self) -> f64 {
        self.now().as_secs_f64()
    }

    /// A counted thread: `settle` and `advance_to` wait for it.
    pub fn spawn<F: FnOnce() + Send + 'static>(self: &Arc<Self>, body: F) {
        puron_engine::clock::spawn(&self.dyn_clock(), "test".into(), body);
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
    }

    fn wait_until(&self, deadline: Option<Duration>, ready: &mut dyn FnMut() -> bool) -> bool {
        let counted = COUNTED.with(|c| c.get());
        let mut g = self.m.lock().unwrap();
        loop {
            if ready() {
                return true;
            }
            if let Some(d) = deadline {
                if self.now() >= d {
                    return false;
                }
                *g.deadlines.entry(d).or_default() += 1;
            }
            if counted {
                g.running -= 1;
                g.waiting += 1;
                if g.running == 0 {
                    self.cv.notify_all();
                }
            }
            let e = g.epoch;
            while g.epoch == e {
                g = self.cv.wait(g).unwrap();
            }
            if let Some(d) = deadline {
                let n = g.deadlines.get_mut(&d).unwrap();
                *n -= 1;
                if *n == 0 {
                    g.deadlines.remove(&d);
                }
            }
        }
    }

    fn wake(&self) {
        let mut g = self.m.lock().unwrap();
        self.broadcast(&mut g);
    }

    fn spawning(&self) {
        self.m.lock().unwrap().running += 1;
    }

    fn entered(&self) {
        COUNTED.with(|c| c.set(true));
    }

    fn exited(&self) {
        COUNTED.with(|c| c.set(false));
        let mut g = self.m.lock().unwrap();
        g.running -= 1;
        if g.running == 0 {
            self.cv.notify_all();
        }
    }
}

pub fn secs(t: f64) -> Duration {
    Duration::from_secs_f64(t)
}

/// A temperature reading of `provider`.
pub fn reading(provider: Provider, position: u32, name: &str, value: f64) -> RawReading {
    RawReading {
        provider,
        position,
        kind: Some(Kind::Temperature),
        chip: None,
        name: Some(name.to_string()),
        device: None,
        item: None,
        unit: None,
        key: name.to_string(),
        value: Some(value),
    }
}

/// R1: `a` 40 and `b` 50.
pub fn r1(p: Provider) -> ReadOutcome {
    ReadOutcome::Readings(vec![reading(p, 0, "a", 40.0), reading(p, 1, "b", 50.0)])
}

/// R2: `a` 41 and `b` 51.
pub fn r2(p: Provider) -> ReadOutcome {
    ReadOutcome::Readings(vec![reading(p, 0, "a", 41.0), reading(p, 1, "b", 51.0)])
}

#[derive(Clone, Debug)]
pub enum StartStep {
    Available,
    Unavailable,
    /// Available after `d` seconds.
    Takes(f64),
    /// Available when released.
    Hangs,
    Panics,
}

#[derive(Clone, Debug)]
pub enum ReadStep {
    Gives(ReadOutcome),
    Takes(f64, ReadOutcome),
    /// Gives what `release_read` passes.
    Hangs,
    Panics,
    /// Panics with this text as the payload (`&'static str`).
    PanicsText(&'static str),
    /// Panics with a payload that is not text (a `u32`).
    PanicsValue,
}

#[derive(Clone, Debug)]
pub enum StopStep {
    AtOnce,
    Panics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Start,
    Read,
    Stop,
}

/// One entry of the call record.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub provider: Provider,
    pub call: CallKind,
    /// The number of Start calls up to this call.
    pub generation: u32,
    pub start: f64,
    pub end: Option<f64>,
    pub stop_seen: Option<f64>,
}

/// A scripted fake provider. Each script is used in turn; its
/// last step repeats.
#[derive(Clone)]
pub struct Fake(Arc<FakeInner>);

struct FakeInner {
    clock: Arc<dyn Clock>,
    provider: Provider,
    kind: ProviderKind,
    s: Mutex<FakeState>,
}

struct FakeState {
    starts: VecDeque<StartStep>,
    reads: VecDeque<ReadStep>,
    stops: VecDeque<StopStep>,
    calls: Vec<Call>,
    generation: u32,
    start_gate: bool,
    read_gate: Option<ReadOutcome>,
}

fn step<T: Clone>(q: &mut VecDeque<T>) -> T {
    if q.len() > 1 {
        q.pop_front().unwrap()
    } else {
        q.front().cloned().expect("a script step")
    }
}

impl Fake {
    /// Start available, every Read empty readings, Stop at once.
    pub fn new(clock: Arc<dyn Clock>, provider: Provider) -> Fake {
        let kind = PROVIDERS
            .iter()
            .find(|e| e.provider == provider)
            .unwrap()
            .kind;
        Fake(Arc::new(FakeInner {
            clock,
            provider,
            kind,
            s: Mutex::new(FakeState {
                starts: [StartStep::Available].into(),
                reads: [ReadStep::Gives(ReadOutcome::Readings(vec![]))].into(),
                stops: [StopStep::AtOnce].into(),
                calls: vec![],
                generation: 0,
                start_gate: false,
                read_gate: None,
            }),
        }))
    }

    pub fn starts(self, steps: impl IntoIterator<Item = StartStep>) -> Fake {
        self.0.s.lock().unwrap().starts = steps.into_iter().collect();
        self
    }

    pub fn reads(self, steps: impl IntoIterator<Item = ReadStep>) -> Fake {
        self.0.s.lock().unwrap().reads = steps.into_iter().collect();
        self
    }

    pub fn stops(self, steps: impl IntoIterator<Item = StopStep>) -> Fake {
        self.0.s.lock().unwrap().stops = steps.into_iter().collect();
        self
    }

    /// Replaces the scripts of the steps not yet used.
    pub fn script_reads(&self, steps: impl IntoIterator<Item = ReadStep>) {
        self.0.s.lock().unwrap().reads = steps.into_iter().collect();
    }

    pub fn script_starts(&self, steps: impl IntoIterator<Item = StartStep>) {
        self.0.s.lock().unwrap().starts = steps.into_iter().collect();
    }

    pub fn script_stops(&self, steps: impl IntoIterator<Item = StopStep>) {
        self.0.s.lock().unwrap().stops = steps.into_iter().collect();
    }

    pub fn provider(&self) -> Provider {
        self.0.provider
    }

    /// Releases a hung Start (available).
    pub fn release_start(&self) {
        self.0.s.lock().unwrap().start_gate = true;
        self.0.clock.wake();
    }

    /// Releases a hung Read with `outcome`.
    pub fn release_read(&self, outcome: ReadOutcome) {
        self.0.s.lock().unwrap().read_gate = Some(outcome);
        self.0.clock.wake();
    }

    pub fn calls(&self) -> Vec<Call> {
        self.0.s.lock().unwrap().calls.clone()
    }

    /// The start times of the calls of `kind`.
    pub fn times(&self, kind: CallKind) -> Vec<f64> {
        self.calls()
            .iter()
            .filter(|c| c.call == kind)
            .map(|c| c.start)
            .collect()
    }

    pub fn count(&self, kind: CallKind) -> usize {
        self.times(kind).len()
    }

    fn now(&self) -> f64 {
        self.0.clock.now().as_secs_f64()
    }

    fn begin(&self, call: CallKind) -> usize {
        let start = self.now();
        let mut s = self.0.s.lock().unwrap();
        if call == CallKind::Start {
            s.generation += 1;
        }
        let generation = s.generation;
        s.calls.push(Call {
            provider: self.0.provider,
            call,
            generation,
            start,
            end: None,
            stop_seen: None,
        });
        s.calls.len() - 1
    }

    fn end(&self, i: usize) {
        let t = self.now();
        self.0.s.lock().unwrap().calls[i].end = Some(t);
    }

    /// Waits until `gate` gives a value, `until` passes, or (child-run)
    /// the stop signal fires. `None` when stopped or timed out.
    fn wait<T>(
        &self,
        i: usize,
        until: Option<Duration>,
        stop: Option<BorrowedFd<'_>>,
        gate: impl Fn(&mut FakeState) -> Option<T>,
    ) -> Option<T> {
        let mut got = None;
        let mut stopped = false;
        let watch = if self.0.kind == ProviderKind::ChildRun {
            stop
        } else {
            None
        };
        self.0.clock.wait_until(until, &mut || {
            if let Some(v) = gate(&mut self.0.s.lock().unwrap()) {
                got = Some(v);
                return true;
            }
            if let Some(fd) = watch {
                if readable(fd) {
                    stopped = true;
                    return true;
                }
            }
            false
        });
        if stopped {
            let t = self.now();
            self.0.s.lock().unwrap().calls[i].stop_seen = Some(t);
        }
        got
    }
}

fn readable(fd: BorrowedFd<'_>) -> bool {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd, no wait.
    unsafe { libc::poll(&mut p, 1, 0) == 1 }
}

impl Source for Fake {
    fn start(&mut self) -> StartOutcome {
        let i = self.begin(CallKind::Start);
        let st = step(&mut self.0.s.lock().unwrap().starts);
        let out = match st {
            StartStep::Available => StartOutcome::Available,
            StartStep::Unavailable => StartOutcome::Unavailable,
            StartStep::Takes(d) => {
                let until = self.0.clock.now() + secs(d);
                self.wait(i, Some(until), None, |_| None::<()>);
                StartOutcome::Available
            }
            StartStep::Hangs => {
                self.wait(i, None, None, |s| s.start_gate.then_some(()));
                self.0.s.lock().unwrap().start_gate = false;
                StartOutcome::Available
            }
            StartStep::Panics => {
                self.end(i);
                panic!("fake start panics");
            }
        };
        self.end(i);
        out
    }

    fn read(&mut self, stop: BorrowedFd<'_>) -> ReadOutcome {
        let i = self.begin(CallKind::Read);
        let st = step(&mut self.0.s.lock().unwrap().reads);
        let out = match st {
            ReadStep::Gives(o) => o,
            ReadStep::Takes(d, o) => {
                let until = self.0.clock.now() + secs(d);
                match self.wait(i, Some(until), Some(stop), |_| None::<()>) {
                    _ if self.calls()[i].stop_seen.is_some() => ReadOutcome::Failed,
                    _ => o,
                }
            }
            ReadStep::Hangs => self
                .wait(i, None, Some(stop), |s| s.read_gate.take())
                .unwrap_or(ReadOutcome::Failed),
            ReadStep::Panics => {
                self.end(i);
                panic!("fake read panics");
            }
            ReadStep::PanicsText(t) => {
                self.end(i);
                std::panic::panic_any(t);
            }
            ReadStep::PanicsValue => {
                self.end(i);
                std::panic::panic_any(7u32);
            }
        };
        self.end(i);
        out
    }

    fn stop(&mut self) {
        let i = self.begin(CallKind::Stop);
        let st = step(&mut self.0.s.lock().unwrap().stops);
        self.end(i);
        if let StopStep::Panics = st {
            panic!("fake stop panics");
        }
    }
}

/// The adapters of the engine: each provider's fake from `fakes`.
pub fn adapters(fakes: &[Fake]) -> Adapters {
    let fakes = fakes.to_vec();
    Box::new(move |p| {
        let f = fakes
            .iter()
            .find(|f| f.provider() == p)
            .unwrap_or_else(|| panic!("no fake for {p:?}"))
            .clone();
        Box::new(f)
    })
}

/// One call on the recorded engine handle, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum HandleCall {
    Start(puron_core::settings::Settings),
    PollNow,
    SetInterval(u32),
    SetProviders(puron_core::settings::Settings),
    Shutdown,
    /// The subscription's wait began.
    WaitBegin,
    /// The subscription's wait ended with a snapshot.
    WaitEnd,
    /// A read of the subscription without waiting.
    TryNext,
}

/// An engine handle that records its calls; the test hands it snapshots
/// (`deliver`), each of which wakes one subscription wait; Shut down
/// sleeps the given real time.
pub struct Handle {
    m: Mutex<(Vec<HandleCall>, Option<puron_engine::engine::Snapshot>)>,
    cv: Condvar,
    shutdown: Duration,
}

impl Handle {
    pub fn new(shutdown: Duration) -> Arc<Handle> {
        Arc::new(Handle {
            m: Mutex::new((Vec::new(), None)),
            cv: Condvar::new(),
            shutdown,
        })
    }

    fn rec(&self, c: HandleCall) {
        self.m.lock().unwrap().0.push(c);
        self.cv.notify_all();
    }

    pub fn start(&self, s: &puron_core::settings::Settings) {
        self.rec(HandleCall::Start(s.clone()));
    }
    pub fn poll_now(&self) {
        self.rec(HandleCall::PollNow);
    }
    pub fn set_interval(&self, secs: u32) {
        self.rec(HandleCall::SetInterval(secs));
    }
    pub fn set_providers(&self, s: &puron_core::settings::Settings) {
        self.rec(HandleCall::SetProviders(s.clone()));
    }
    pub fn shutdown(&self) {
        self.rec(HandleCall::Shutdown);
        std::thread::sleep(self.shutdown);
    }
    pub fn next(&self) -> puron_engine::engine::Snapshot {
        let mut g = self.m.lock().unwrap();
        g.0.push(HandleCall::WaitBegin);
        self.cv.notify_all();
        loop {
            if let Some(s) = g.1.take() {
                g.0.push(HandleCall::WaitEnd);
                self.cv.notify_all();
                return s;
            }
            g = self.cv.wait(g).unwrap();
        }
    }
    pub fn try_next(&self) -> Option<puron_engine::engine::Snapshot> {
        let mut g = self.m.lock().unwrap();
        g.0.push(HandleCall::TryNext);
        g.1.take()
    }
    /// Hands a snapshot to the subscription (it replaces an unread one).
    pub fn deliver(&self, s: puron_engine::engine::Snapshot) {
        self.m.lock().unwrap().1 = Some(s);
        self.cv.notify_all();
    }
    pub fn calls(&self) -> Vec<HandleCall> {
        self.m.lock().unwrap().0.clone()
    }
    /// Waits (real time, at most `max`) until `f` holds for the record.
    pub fn wait_for(&self, max: Duration, f: impl Fn(&[HandleCall]) -> bool) -> bool {
        let end = std::time::Instant::now() + max;
        let mut g = self.m.lock().unwrap();
        loop {
            if f(&g.0) {
                return true;
            }
            let now = std::time::Instant::now();
            if now >= end {
                return false;
            }
            g = self.cv.wait_timeout(g, end - now).unwrap().0;
        }
    }
}
