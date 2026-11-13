//! Fixtures for the tests of the engine and of the code that drives it: a
//! virtual clock that moves only when the test moves it, a fake provider
//! that follows a script (its calls can take time, hang or panic) and
//! records its calls, and a stand-in engine handle that records its
//! calls.

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
    /// Whether the current thread is counted by the virtual clock: an
    /// engine thread, or one started by `VirtualClock::spawn`. The clock
    /// moves only while every counted thread waits on it.
    static COUNTED: Cell<bool> = const { Cell::new(false) };
}

/// A clock for engine tests whose time moves only when the test calls
/// `advance_to`; it starts at 0. It implements `puron_engine::clock::Clock`,
/// so an engine built on it blocks only in this clock's `wait_until`.
///
/// The clock counts the engine's threads through the `Clock` hooks. It
/// moves only when every counted thread is waiting on it, and it stops
/// at each waiting thread's deadline on the way. So when `advance_to`
/// returns, every wait that ends at or before the new time has ended,
/// in time order, and every counted thread is waiting again (or has
/// ended): the test sees a settled engine at a known time.
pub struct VirtualClock {
    nanos: AtomicU64,
    m: Mutex<Inner>,
    cv: Condvar,
}

struct Inner {
    /// Counted threads that are not waiting on the clock.
    running: usize,
    /// Counted threads that are waiting on the clock.
    waiting: usize,
    /// The number of wake-ups so far; a waiting thread sleeps until it
    /// changes.
    epoch: u64,
    /// The deadlines of the waiting threads, each with the number of
    /// threads that wait for it.
    deadlines: BTreeMap<Duration, usize>,
}

impl VirtualClock {
    /// A clock at time 0, with no counted thread.
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

    /// The same clock as the `Arc<dyn Clock>` that the engine takes.
    pub fn dyn_clock(self: &Arc<Self>) -> Arc<dyn Clock> {
        Arc::clone(self) as Arc<dyn Clock>
    }

    /// Wakes every waiting thread. They count as running until they wait
    /// again.
    fn broadcast(&self, g: &mut Inner) {
        g.running += g.waiting;
        g.waiting = 0;
        g.epoch += 1;
        self.cv.notify_all();
    }

    /// Moves the clock to `t` seconds. It first waits until every counted
    /// thread waits; then it moves to the earliest deadline up to `t`,
    /// wakes the waiting threads, and repeats until it has reached `t`
    /// and every counted thread waits again. Panics if `t` is before the
    /// current time. A counted thread that never waits makes it block.
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

    /// Waits until every counted thread waits on the clock, without
    /// moving it.
    pub fn settle(&self) {
        self.advance_to(self.secs());
    }

    /// The current time in seconds.
    pub fn secs(&self) -> f64 {
        self.now().as_secs_f64()
    }

    /// Runs `body` on a new counted thread, which `settle` and
    /// `advance_to` wait for, as for an engine thread.
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

/// `t` seconds as a `Duration`.
pub fn secs(t: f64) -> Duration {
    Duration::from_secs_f64(t)
}

/// A temperature reading of `provider` at `position`, named `name`, with
/// `name` also as its key and the value `value`; the other fields are
/// empty.
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
        kernel_label: None,
        platform: false,
        device_chars: 0,
    }
}

/// A read that gives two temperatures of `p`: `a` at 40 and `b` at 50.
pub fn r1(p: Provider) -> ReadOutcome {
    ReadOutcome::Readings(vec![reading(p, 0, "a", 40.0), reading(p, 1, "b", 50.0)])
}

/// The two readings of `r1` with other values: `a` at 41 and `b` at 51.
pub fn r2(p: Provider) -> ReadOutcome {
    ReadOutcome::Readings(vec![reading(p, 0, "a", 41.0), reading(p, 1, "b", 51.0)])
}

/// What one `start` call of a [`Fake`] does. `Available` and
/// `Unavailable` return that outcome at once; `Panics` panics.
#[derive(Clone, Debug)]
pub enum StartStep {
    Available,
    Unavailable,
    /// Returns `Available` after the given number of seconds of clock
    /// time.
    Takes(f64),
    /// Waits until the test calls `Fake::release_start`, then returns
    /// `Available`.
    Hangs,
    Panics,
}

/// What one `configure` call of a [`Fake`] does: `Records` keeps the
/// settings it was given (see `Fake::configured`); `Panics` keeps them
/// too, then panics.
#[derive(Clone, Debug)]
pub enum ConfigureStep {
    Records,
    Panics,
}

/// What one `read` call of a [`Fake`] does.
///
/// A provider of kind `ProviderKind::ChildRun` watches the stop
/// descriptor while a read waits: when it becomes readable, the read
/// returns `ReadOutcome::Failed` at once and the call records
/// `stop_seen`. An in-process provider does not watch it.
#[derive(Clone, Debug)]
pub enum ReadStep {
    /// Returns the outcome at once.
    Gives(ReadOutcome),
    /// Returns the outcome after the given number of seconds of clock
    /// time.
    Takes(f64, ReadOutcome),
    /// Waits until the test calls `Fake::release_read`, then returns the
    /// outcome given there.
    Hangs,
    /// Panics with the message `fake read panics`.
    Panics,
    /// Panics with this text as the payload (`&'static str`).
    PanicsText(&'static str),
    /// Panics with a payload that is not text (a `u32`).
    PanicsValue,
}

/// What one `stop` call of a [`Fake`] does: return at once, or panic.
#[derive(Clone, Debug)]
pub enum StopStep {
    AtOnce,
    Panics,
}

/// Which method of `Source` a recorded call was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Start,
    Read,
    Stop,
}

/// One call of the engine to a [`Fake`], as recorded. Times are in
/// seconds of clock time.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub provider: Provider,
    pub call: CallKind,
    /// The number of `start` calls so far, this one included; it tells
    /// which start of the provider a read belongs to.
    pub generation: u32,
    /// When the call began.
    pub start: f64,
    /// When the call returned, or was about to panic; `None` while it
    /// runs.
    pub end: Option<f64>,
    /// When a waiting read saw the stop descriptor become readable (see
    /// [`ReadStep`]); `None` if it did not.
    pub stop_seen: Option<f64>,
}

/// A fake provider adapter (`Source`) that follows four scripts, one
/// each for `configure`, `start`, `read` and `stop`, and records every
/// call. Each
/// call takes the next step of its script; the last step repeats for
/// every later call. Clones share the scripts and the record, so the
/// test keeps one clone and gives another to the engine (see
/// [`adapters`]).
#[derive(Clone)]
pub struct Fake(Arc<FakeInner>);

struct FakeInner {
    clock: Arc<dyn Clock>,
    provider: Provider,
    kind: ProviderKind,
    s: Mutex<FakeState>,
}

/// The scripts, the record, and the two release signals of a [`Fake`].
struct FakeState {
    configures: VecDeque<ConfigureStep>,
    starts: VecDeque<StartStep>,
    reads: VecDeque<ReadStep>,
    stops: VecDeque<StopStep>,
    calls: Vec<Call>,
    /// One entry per `configure` call: the number of `start` calls before
    /// it, and the settings it was given.
    configured: Vec<(u32, puron_core::settings::Settings)>,
    generation: u32,
    start_gate: bool,
    read_gate: Option<ReadOutcome>,
}

/// The next step of a script; the last step stays to repeat. Panics on
/// an empty script.
fn step<T: Clone>(q: &mut VecDeque<T>) -> T {
    if q.len() > 1 {
        q.pop_front().unwrap()
    } else {
        q.front().cloned().expect("a script step")
    }
}

impl Fake {
    /// A fake for `provider` that waits on `clock`. Every `start` returns
    /// `Available`, every `read` returns an empty list of readings, and
    /// every `stop` returns at once, until a script is set. The provider's
    /// kind comes from `PROVIDERS`. Every `configure` records its
    /// settings until `configures` sets a script.
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
                configures: [ConfigureStep::Records].into(),
                starts: [StartStep::Available].into(),
                reads: [ReadStep::Gives(ReadOutcome::Readings(vec![]))].into(),
                stops: [StopStep::AtOnce].into(),
                calls: vec![],
                configured: vec![],
                generation: 0,
                start_gate: false,
                read_gate: None,
            }),
        }))
    }

    /// Sets the `start` script; `reads` and `stops` set the other two.
    pub fn starts(self, steps: impl IntoIterator<Item = StartStep>) -> Fake {
        self.0.s.lock().unwrap().starts = steps.into_iter().collect();
        self
    }

    pub fn configures(self, steps: impl IntoIterator<Item = ConfigureStep>) -> Fake {
        self.0.s.lock().unwrap().configures = steps.into_iter().collect();
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

    /// Replaces the rest of the `read` script, while the fake is in use;
    /// `script_starts` and `script_stops` do the same for the other two.
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

    /// Lets a hanging `start` return `Available`. If no `start` hangs
    /// yet, the next one that hangs returns at once.
    pub fn release_start(&self) {
        self.0.s.lock().unwrap().start_gate = true;
        self.0.clock.wake();
    }

    /// Lets a hanging `read` return `outcome`. If no `read` hangs yet, the
    /// next one that hangs returns it at once.
    pub fn release_read(&self, outcome: ReadOutcome) {
        self.0.s.lock().unwrap().read_gate = Some(outcome);
        self.0.clock.wake();
    }

    /// Every call so far, in the order the calls began.
    pub fn calls(&self) -> Vec<Call> {
        self.0.s.lock().unwrap().calls.clone()
    }

    /// Every `configure` call so far, in order: the number of `start`
    /// calls that came before it, and the settings it was given. They are
    /// kept apart from `calls`, which holds `start`, `read` and `stop`
    /// only.
    pub fn configured(&self) -> Vec<(u32, puron_core::settings::Settings)> {
        self.0.s.lock().unwrap().configured.clone()
    }

    /// The times at which the calls of `kind` began, in order.
    pub fn times(&self, kind: CallKind) -> Vec<f64> {
        self.calls()
            .iter()
            .filter(|c| c.call == kind)
            .map(|c| c.start)
            .collect()
    }

    /// The number of calls of `kind` so far.
    pub fn count(&self, kind: CallKind) -> usize {
        self.times(kind).len()
    }

    fn now(&self) -> f64 {
        self.0.clock.now().as_secs_f64()
    }

    /// Records the start of a call and returns its index in the record.
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

    /// Waits on the clock until `gate` gives a value, the clock reaches
    /// `until`, or, for a child-run provider, `stop` becomes readable,
    /// which is recorded in the call `i`. Returns the value, or `None` in
    /// the two other cases.
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

/// Whether `fd` can be read now, without waiting.
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
    fn configure(&mut self, settings: &puron_core::settings::Settings) {
        let mut s = self.0.s.lock().unwrap();
        let starts = s.generation;
        s.configured.push((starts, settings.clone()));
        if let ConfigureStep::Panics = step(&mut s.configures) {
            // Unlock first, so that the panic does not poison the state.
            drop(s);
            panic!("fake configure panics");
        }
    }

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

/// The `Adapters` for an engine under test: for each provider, a clone of
/// its fake from `fakes`. Panics when the engine asks for a provider that
/// has no fake.
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

/// One recorded call on a [`Handle`].
#[derive(Clone, Debug, PartialEq)]
pub enum HandleCall {
    Start(puron_core::settings::Settings),
    PollNow,
    SetInterval(u32),
    SetProviders(puron_core::settings::Settings),
    Shutdown,
    /// A `next` call began to wait for a snapshot.
    WaitBegin,
    /// A `next` call got a snapshot and returns it.
    WaitEnd,
    /// A `try_next` call.
    TryNext,
}

/// A stand-in for the engine's handle, for the tests of the code that
/// drives the engine. It has the handle's methods, and each records the
/// call. It makes no snapshot of its own: the test hands one over with
/// `deliver`, and the next `next` or `try_next` takes it. `shutdown`
/// blocks for the real time given to `new`.
pub struct Handle {
    m: Mutex<(Vec<HandleCall>, Option<puron_engine::engine::Snapshot>)>,
    cv: Condvar,
    shutdown: Duration,
}

impl Handle {
    /// A handle with an empty record whose `shutdown` takes `shutdown`.
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

    /// Records the call. `poll_now`, `set_interval` and `set_providers`
    /// likewise only record theirs.
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
    /// Records the call, then sleeps for the time given to `new`.
    pub fn shutdown(&self) {
        self.rec(HandleCall::Shutdown);
        std::thread::sleep(self.shutdown);
    }
    /// Records `WaitBegin`, waits without a limit until a snapshot is
    /// delivered, records `WaitEnd` and returns the snapshot.
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
    /// Records `TryNext` and returns the delivered snapshot not yet
    /// taken, if there is one.
    pub fn try_next(&self) -> Option<puron_engine::engine::Snapshot> {
        let mut g = self.m.lock().unwrap();
        g.0.push(HandleCall::TryNext);
        g.1.take()
    }
    /// Hands over a snapshot for `next` or `try_next` to take. It replaces
    /// a delivered snapshot that was not taken yet, as the engine keeps
    /// only its latest snapshot.
    pub fn deliver(&self, s: puron_engine::engine::Snapshot) {
        self.m.lock().unwrap().1 = Some(s);
        self.cv.notify_all();
    }
    /// The record so far, in order.
    pub fn calls(&self) -> Vec<HandleCall> {
        self.m.lock().unwrap().0.clone()
    }
    /// Waits, in real time and at most `max`, until `f` holds for the
    /// record; returns whether it did.
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
