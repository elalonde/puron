//! The engine: poll cycle, workers and snapshots.
//!
//! Threads: the scheduler, which also runs the aggregator, and one
//! worker per provider from its first start. They share one lock; every
//! wait is a wait on the engine's clock, and nothing calls the clock
//! while it holds the lock.

use std::collections::VecDeque;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use puron_core::rows::{Provider, RawReading};
use puron_core::settings::Settings;

use crate::clock::{self, Clock};
use crate::diag;
use crate::registry::{self, Action, ProviderKind, ReadOutcome, Source, StartOutcome, PROVIDERS};

/// The adapters' configuration: builds the adapter of a
/// provider, once, when its worker is created at its first start.
pub type Adapters = Box<dyn FnMut(Provider) -> Box<dyn Source> + Send>;

/// Read deadline of an in-process read.
pub const READ_DEADLINE: Duration = Duration::from_secs(10);
/// How long `shutdown` waits at most for the workers to end.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// The lm-sensors report of a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LmReport {
    NoneYet,
    Unavailable,
    Answered,
}

/// What the subscription returns: the readings of the providers that are
/// on, and the lm-sensors report.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub readings: Vec<RawReading>,
    pub lm_sensors: LmReport,
}

/// The engine handle. No command waits for a provider.
pub struct Engine {
    sh: Arc<Shared>,
}

struct Shared {
    clock: Arc<dyn Clock>,
    st: Mutex<State>,
}

struct State {
    /// Commands and reports for the scheduler, in arrival order.
    inbox: VecDeque<In>,
    /// One slot per provider, in provider order, from its first start.
    workers: Vec<Option<Slot>>,
    created: usize,
    alive: usize,
    /// The latest-value channel: it holds one snapshot.
    latest: Option<Snapshot>,
    fresh: bool,
    /// The scheduler has told every worker to stop for shutdown.
    shut: bool,
}

struct Slot {
    queue: VecDeque<Msg>,
    stop: Arc<OwnedFd>,
}

enum In {
    Start(Settings, Adapters),
    PollNow,
    SetInterval(u32),
    SetProviders(Settings),
    Shutdown,
    /// A worker's report: provider index, generation, outcome. A report of
    /// an earlier generation, or of a provider now off, has no effect.
    Report(usize, u64, ReadOutcome),
    /// An in-process read started at this time; its read deadline counts
    /// from here.
    ReadStarted(usize, u64, Duration),
}

/// Commands to a worker, run in the order sent.
enum Msg {
    Start(u64),
    Stop,
    Shutdown,
    /// The pending request: at most one in a queue.
    Read,
}

fn index(p: Provider) -> usize {
    PROVIDERS.iter().position(|e| e.provider == p).unwrap()
}

impl Engine {
    /// The handle, with its scheduler; no worker yet.
    pub fn new(clock: Arc<dyn Clock>) -> Engine {
        let sh = Arc::new(Shared {
            clock,
            st: Mutex::new(State {
                inbox: VecDeque::new(),
                workers: PROVIDERS.iter().map(|_| None).collect(),
                created: 0,
                alive: 0,
                latest: None,
                fresh: false,
                shut: false,
            }),
        });
        let s = Arc::clone(&sh);
        clock::spawn(&sh.clock, "puron-scheduler".into(), move || {
            Scheduler::new(s).run()
        });
        Engine { sh }
    }

    fn send(&self, m: In) {
        self.sh.st.lock().unwrap().inbox.push_back(m);
        self.sh.clock.wake();
    }

    pub fn start(&self, settings: &Settings, adapters: Adapters) {
        self.send(In::Start(settings.clone(), adapters));
    }

    pub fn poll_now(&self) {
        self.send(In::PollNow);
    }

    pub fn set_interval(&self, secs: u32) {
        self.send(In::SetInterval(secs));
    }

    pub fn set_providers(&self, settings: &Settings) {
        self.send(In::SetProviders(settings.clone()));
    }

    /// Returns when every worker has ended, or after the shutdown wait.
    pub fn shutdown(&self) {
        self.send(In::Shutdown);
        let until = self.sh.clock.now() + SHUTDOWN_WAIT;
        self.sh.clock.wait_until(Some(until), &mut || {
            let s = self.sh.st.lock().unwrap();
            s.shut && s.alive == 0
        });
    }

    /// The subscription: waits until a snapshot is new, returns the
    /// latest unread one.
    pub fn next(&self) -> Snapshot {
        let mut out = None;
        self.sh.clock.wait_until(None, &mut || {
            out = self.take();
            out.is_some()
        });
        out.unwrap()
    }

    /// The latest unread snapshot, if one is new; does not wait.
    pub fn try_next(&self) -> Option<Snapshot> {
        self.take()
    }

    fn take(&self) -> Option<Snapshot> {
        let mut s = self.sh.st.lock().unwrap();
        if !s.fresh {
            return None;
        }
        s.fresh = false;
        s.latest.clone()
    }

    /// The number of workers created, for tests.
    pub fn workers(&self) -> usize {
        self.sh.st.lock().unwrap().created
    }
}

/// A handle dropped without a `shutdown` call sends the shutdown command
/// without waiting: the scheduler raises every worker's stop signal and
/// the threads end on their own. `shutdown` remains the way to wait for
/// them.
impl Drop for Engine {
    fn drop(&mut self) {
        let mut s = self.sh.st.lock().unwrap_or_else(|e| e.into_inner());
        if !s.shut {
            s.inbox.push_back(In::Shutdown);
            drop(s);
            self.sh.clock.wake();
        }
    }
}

/// What the aggregator keeps of a provider.
#[derive(Default)]
struct Prov {
    on: bool,
    generation: u64,
    /// Its latest report of the current generation.
    outcome: Option<ReadOutcome>,
    /// Its last *readings* report of the current generation.
    last: Option<Vec<RawReading>>,
    /// The read deadline passed and the read has not ended.
    stale: bool,
}

impl Prov {
    fn reset(&mut self) {
        self.outcome = None;
        self.last = None;
        self.stale = false;
    }
}

struct Scheduler {
    sh: Arc<Shared>,
    adapters: Option<Adapters>,
    settings: Option<Settings>,
    interval: Duration,
    next_poll: Option<Duration>,
    provs: Vec<Prov>,
    /// Armed read deadlines: (provider, generation of the read, time).
    /// One of an earlier generation passes with no effect.
    deadlines: Vec<(usize, u64, Duration)>,
    /// Worker queues changed: wake after the lock is released.
    sent: bool,
    publish: bool,
}

fn no_value(v: &[RawReading]) -> Vec<RawReading> {
    v.iter()
        .map(|r| RawReading {
            value: None,
            ..r.clone()
        })
        .collect()
}

fn secs(n: u32) -> Duration {
    Duration::from_secs(u64::from(n))
}

impl Scheduler {
    fn new(sh: Arc<Shared>) -> Scheduler {
        Scheduler {
            sh,
            adapters: None,
            settings: None,
            interval: Duration::ZERO,
            next_poll: None,
            provs: PROVIDERS.iter().map(|_| Prov::default()).collect(),
            deadlines: vec![],
            sent: false,
            publish: false,
        }
    }

    fn due(&self) -> Option<Duration> {
        self.deadlines
            .iter()
            .map(|d| d.2)
            .chain(self.next_poll)
            .min()
    }

    fn run(mut self) {
        loop {
            let mut batch = VecDeque::new();
            let due = self.due();
            self.sh.clock.wait_until(due, &mut || {
                batch.extend(self.sh.st.lock().unwrap().inbox.drain(..));
                !batch.is_empty()
            });
            let now = self.sh.clock.now();
            let mut down = false;
            for m in batch {
                match m {
                    In::Start(settings, adapters) => {
                        self.adapters = Some(adapters);
                        self.interval = secs(settings.poll_interval_secs);
                        self.switch(None, &settings);
                        self.settings = Some(settings);
                        self.poll(now);
                    }
                    In::PollNow if self.settings.is_some() => self.poll(now),
                    In::SetInterval(n) if self.settings.is_some() => {
                        self.interval = secs(n);
                        self.next_poll = Some(now + self.interval);
                    }
                    In::SetProviders(new) if self.settings.is_some() => {
                        let old = self.settings.take();
                        self.switch(old.as_ref(), &new);
                        self.settings = Some(new);
                        self.poll(now);
                    }
                    In::Shutdown => down = true,
                    In::Report(i, g, outcome) => self.report(i, g, outcome),
                    // A read of a stopped provider or an earlier
                    // generation arms nothing.
                    In::ReadStarted(i, g, at) => {
                        let p = &self.provs[i];
                        if p.on && g == p.generation {
                            self.deadlines.push((i, g, at + READ_DEADLINE));
                        }
                    }
                    _ => {}
                }
            }
            if down {
                self.shutdown();
                return;
            }
            let (passed, armed) = self.deadlines.iter().partition(|d| d.2 <= now);
            self.deadlines = armed;
            for (i, g, _) in passed {
                let p = &mut self.provs[i];
                if p.on && g == p.generation {
                    p.stale = true;
                    self.publish = true;
                    diag::event(diag::engine::read_deadline(PROVIDERS[i].provider.word()));
                }
            }
            if let Some(t) = self.next_poll {
                if t <= now {
                    self.poll(now);
                }
            }
            self.flush();
        }
    }

    /// Publishes a snapshot if one is due, then wakes the waits.
    fn flush(&mut self) {
        if self.publish {
            let snap = self.snapshot();
            let mut s = self.sh.st.lock().unwrap();
            s.latest = Some(snap);
            s.fresh = true;
        }
        if self.publish || self.sent {
            self.publish = false;
            self.sent = false;
            self.sh.clock.wake();
        }
    }

    /// A poll: a request to each enabled worker; the timer restarts.
    fn poll(&mut self, now: Duration) {
        let settings = self.settings.as_ref().unwrap();
        let mut s = self.sh.st.lock().unwrap();
        for (i, e) in PROVIDERS.iter().enumerate() {
            if !registry::switch(settings, e.provider) {
                continue;
            }
            if let Some(w) = s.workers[i].as_mut() {
                if !w.queue.iter().any(|m| matches!(m, Msg::Read)) {
                    w.queue.push_back(Msg::Read);
                    self.sent = true;
                }
            }
        }
        self.next_poll = Some(now + self.interval);
    }

    /// The registry's actions from `old` to `new`.
    fn switch(&mut self, old: Option<&Settings>, new: &Settings) {
        for a in registry::actions(old, new) {
            match a {
                Action::Start(p) => self.start_provider(index(p)),
                Action::Stop(p) => {
                    let i = index(p);
                    let pr = &mut self.provs[i];
                    pr.on = false;
                    pr.reset();
                    // A stop drops the provider's read deadline.
                    self.deadlines.retain(|d| d.0 != i);
                    self.publish = true;
                    self.to_worker(i, Msg::Stop);
                }
            }
        }
    }

    fn start_provider(&mut self, i: usize) {
        let pr = &mut self.provs[i];
        pr.on = true;
        pr.generation += 1;
        pr.reset();
        let g = pr.generation;
        let exists = self.sh.st.lock().unwrap().workers[i].is_some();
        if !exists {
            let src = (self.adapters.as_mut().unwrap())(PROVIDERS[i].provider);
            spawn_worker(&self.sh, i, src);
        }
        self.to_worker(i, Msg::Start(g));
    }

    /// Queues `m`; a stop or shutdown also raises the stop signal.
    // "to" names the receiver here ("send to worker"), not a conversion.
    #[allow(clippy::wrong_self_convention)]
    fn to_worker(&mut self, i: usize, m: Msg) {
        let mut s = self.sh.st.lock().unwrap();
        let Some(w) = s.workers[i].as_mut() else {
            return;
        };
        if matches!(m, Msg::Stop | Msg::Shutdown) {
            // The pending request of the old generation goes too.
            w.queue.retain(|q| !matches!(q, Msg::Read));
            raise(&w.stop);
        }
        w.queue.push_back(m);
        self.sent = true;
    }

    fn report(&mut self, i: usize, g: u64, outcome: ReadOutcome) {
        let p = &mut self.provs[i];
        if !p.on || p.generation != g {
            return;
        }
        self.deadlines.retain(|d| (d.0, d.1) != (i, g));
        p.stale = false;
        if let ReadOutcome::Readings(v) = &outcome {
            p.last = Some(v.clone());
            diag::event(diag::engine::provider_updated(PROVIDERS[i].provider.word()));
        }
        p.outcome = Some(outcome);
        self.publish = true;
    }

    fn snapshot(&self) -> Snapshot {
        let mut readings = vec![];
        for (p, e) in self.provs.iter().zip(PROVIDERS) {
            if !p.on {
                continue;
            }
            let last = || no_value(p.last.as_deref().unwrap_or(&[]));
            match &p.outcome {
                _ if p.stale => readings.extend(last()),
                Some(ReadOutcome::Readings(v)) => readings.extend(v.iter().cloned()),
                Some(ReadOutcome::Failed) if e.kind == ProviderKind::InProcess => {
                    readings.extend(last())
                }
                _ => {}
            }
        }
        let lm = &self.provs[0];
        let lm_sensors = match &lm.outcome {
            None => LmReport::NoneYet,
            Some(ReadOutcome::Unavailable) => LmReport::Unavailable,
            Some(_) => LmReport::Answered,
        };
        Snapshot {
            readings,
            lm_sensors,
        }
    }

    /// On `In::Shutdown`: the timer stops; each worker is told to stop.
    fn shutdown(&mut self) {
        self.next_poll = None;
        {
            let mut s = self.sh.st.lock().unwrap();
            for w in s.workers.iter_mut().flatten() {
                w.queue.clear();
                raise(&w.stop);
                w.queue.push_back(Msg::Shutdown);
            }
            s.shut = true;
        }
        self.sh.clock.wake();
    }
}

fn eventfd() -> OwnedFd {
    // SAFETY: eventfd returns a new descriptor or -1.
    let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    assert!(fd >= 0, "eventfd: {}", std::io::Error::last_os_error());
    // SAFETY: fd is a new descriptor that nothing else owns.
    unsafe { OwnedFd::from_raw_fd(fd) }
}

/// Makes the stop signal readable.
fn raise(fd: &OwnedFd) {
    let one: u64 = 1;
    // SAFETY: writes 8 bytes from a valid u64.
    unsafe { libc::write(fd.as_raw_fd(), (&one as *const u64).cast(), 8) };
}

/// Makes the stop signal not readable.
fn drain(fd: &OwnedFd) {
    let mut n: u64 = 0;
    // SAFETY: reads at most 8 bytes into a valid u64.
    unsafe { libc::read(fd.as_raw_fd(), (&mut n as *mut u64).cast(), 8) };
}

fn spawn_worker(sh: &Arc<Shared>, i: usize, src: Box<dyn Source>) {
    let stop = Arc::new(eventfd());
    {
        let mut s = sh.st.lock().unwrap();
        s.workers[i] = Some(Slot {
            queue: VecDeque::new(),
            stop: Arc::clone(&stop),
        });
        s.created += 1;
        s.alive += 1;
    }
    let w = Worker {
        sh: Arc::clone(sh),
        i,
        src,
        stop,
    };
    let name = format!("puron-{}", PROVIDERS[i].provider.word());
    clock::spawn(&sh.clock, name, move || w.run());
}

struct Worker {
    sh: Arc<Shared>,
    i: usize,
    src: Box<dyn Source>,
    stop: Arc<OwnedFd>,
}

/// The panic message when it is text, else empty.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

impl Worker {
    fn word(&self) -> &'static str {
        PROVIDERS[self.i].provider.word()
    }

    fn send(&self, m: In) {
        self.sh.st.lock().unwrap().inbox.push_back(m);
        self.sh.clock.wake();
    }

    fn caught(&self, phase: &'static str, p: &(dyn std::any::Any + Send)) {
        diag::event(diag::engine::panic(self.word(), phase, &panic_text(p)));
    }

    fn run(mut self) {
        let kind = PROVIDERS[self.i].kind;
        let mut generation = 0;
        let mut started = false;
        let mut available = false;
        loop {
            let mut msg = None;
            self.sh.clock.wait_until(None, &mut || {
                let mut s = self.sh.st.lock().unwrap();
                msg = s.workers[self.i].as_mut().unwrap().queue.pop_front();
                msg.is_some()
            });
            match msg.unwrap() {
                Msg::Start(g) => {
                    drain(&self.stop);
                    generation = g;
                    started = true;
                    let src = &mut self.src;
                    available = match catch_unwind(AssertUnwindSafe(|| src.start())) {
                        Ok(StartOutcome::Available) => true,
                        Ok(StartOutcome::Unavailable) => {
                            diag::event(diag::engine::start_unavailable(self.word()));
                            false
                        }
                        Err(p) => {
                            self.caught("start", &*p);
                            false
                        }
                    };
                    if !available {
                        self.send(In::Report(self.i, g, ReadOutcome::Unavailable));
                    }
                }
                Msg::Stop | Msg::Shutdown if started => {
                    started = false;
                    available = false;
                    let src = &mut self.src;
                    if let Err(p) = catch_unwind(AssertUnwindSafe(|| src.stop())) {
                        self.caught("stop", &*p);
                    }
                }
                Msg::Read if available => {
                    if kind == ProviderKind::InProcess {
                        let now = self.sh.clock.now();
                        self.send(In::ReadStarted(self.i, generation, now));
                    }
                    let (src, stop) = (&mut self.src, self.stop.as_fd());
                    let out = match catch_unwind(AssertUnwindSafe(|| src.read(stop))) {
                        Ok(o) => o,
                        Err(p) => {
                            self.caught("read", &*p);
                            ReadOutcome::Failed
                        }
                    };
                    if kind == ProviderKind::ChildRun {
                        // A poll during a run is dropped.
                        let mut s = self.sh.st.lock().unwrap();
                        let q = &mut s.workers[self.i].as_mut().unwrap().queue;
                        while matches!(q.front(), Some(Msg::Read)) {
                            q.pop_front();
                        }
                    }
                    self.send(In::Report(self.i, generation, out));
                }
                _ => {}
            }
            let shut = self.sh.st.lock().unwrap().shut;
            if shut && !started {
                break;
            }
        }
        self.sh.st.lock().unwrap().alive -= 1;
        self.sh.clock.wake();
    }
}
