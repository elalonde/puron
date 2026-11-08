//! The engine runs the sensor providers on their own threads and
//! publishes their readings as snapshots.
//!
//! A provider is one source of readings (lm-sensors, liquidctl, NVIDIA,
//! udisks2 or NVMe), listed in `registry::PROVIDERS`. The settings switch
//! each provider on or off. The engine has one scheduler thread and one
//! worker thread per provider. A provider's worker thread is created the
//! first time the provider is switched on and lives until shutdown, so a
//! provider that is switched off and on again reuses it.
//!
//! The scheduler handles the commands sent through the `Engine` handle,
//! asks each worker for a read once per poll interval, collects the
//! results, and stores a new `Snapshot` after every result it accepts,
//! every provider switched off, and every read that runs past its
//! deadline. It accepts a worker's result only if the provider is still
//! switched on and the result belongs to the provider's current start
//! (see `Prov::generation`); any other result is dropped and stores
//! nothing. The scheduler never waits for a worker, so a slow provider
//! delays only itself.
//!
//! All threads share one `Mutex<State>`. Every blocking wait goes through
//! `Clock::wait_until`, so that tests can run the engine on a virtual
//! clock. `wait_until` runs its `ready` check under the clock's own lock,
//! and those checks take the state lock. For that reason no thread calls
//! the clock while it holds the state lock: doing so could deadlock.

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

/// Builds the adapter for a provider: the `Source` that reads that
/// provider's sensors. The engine calls it once per provider, when the
/// provider is first switched on and its worker thread is created.
pub type Adapters = Box<dyn FnMut(Provider) -> Box<dyn Source> + Send>;

/// How long an in-process read may run before the snapshot shows that
/// provider's readings without values. The read is not interrupted: if
/// it finishes later, its result is still used.
pub const READ_DEADLINE: Duration = Duration::from_secs(10);
/// The longest time `Engine::shutdown` waits for the worker threads to
/// end. After it, `shutdown` returns even if a worker is still reading.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// The state of the lm-sensors provider in a snapshot. Callers use it
/// to tell "lm-sensors is unavailable" apart from "lm-sensors works
/// but has found nothing to show".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LmReport {
    /// lm-sensors has not reported since it was last switched on, or it
    /// is switched off.
    NoneYet,
    /// lm-sensors could not start, or a read reported it unavailable.
    Unavailable,
    /// lm-sensors has completed a read, whether it succeeded or failed.
    Answered,
}

/// What the engine publishes: the current readings of every provider
/// that is switched on, in provider order, and the state of lm-sensors.
/// `Scheduler::snapshot` decides which readings a provider contributes.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub readings: Vec<RawReading>,
    pub lm_sensors: LmReport,
}

/// The caller's handle to the engine. `start`, `poll_now`,
/// `set_interval` and `set_providers` only queue a command for the
/// scheduler thread and return at once; none of them waits for a
/// provider.
pub struct Engine {
    sh: Arc<Shared>,
}

struct Shared {
    clock: Arc<dyn Clock>,
    st: Mutex<State>,
}

struct State {
    /// Messages for the scheduler thread, in arrival order: commands
    /// from the handle and results from the workers.
    inbox: VecDeque<In>,
    /// One slot per entry of `PROVIDERS`, in the same order. A slot is
    /// `None` until that provider is first switched on.
    workers: Vec<Option<Slot>>,
    created: usize,
    /// Worker threads still running; `shutdown` waits for this to reach
    /// zero.
    alive: usize,
    /// The most recent snapshot. Only one is kept: a new snapshot
    /// replaces it whether or not it was read. `fresh` is true while it
    /// has not yet been returned by `next` or `try_next`.
    latest: Option<Snapshot>,
    fresh: bool,
    /// Set once the scheduler has told every worker to shut down. A
    /// worker exits when this is set and its adapter is stopped.
    shut: bool,
}

/// The scheduler's side of a worker: its message queue and its stop
/// signal.
struct Slot {
    queue: VecDeque<Msg>,
    /// The worker's stop signal: an eventfd that `raise` makes readable.
    /// A child-run read (one that runs an external program) waits on it,
    /// so that a stop or shutdown can end the run early.
    stop: Arc<OwnedFd>,
}

enum In {
    Start(Settings, Adapters),
    PollNow,
    SetInterval(u32),
    SetProviders(Settings),
    Shutdown,
    /// A worker's result: provider index, generation (see
    /// `Prov::generation`), outcome. The scheduler ignores it if the
    /// provider has since been switched off or restarted.
    Report(usize, u64, ReadOutcome),
    /// Sent by a worker when an in-process read begins, with the clock
    /// time; that read's `READ_DEADLINE` counts from this time.
    ReadStarted(usize, u64, Duration),
}

/// Messages to a worker, handled in the order queued.
enum Msg {
    /// Start the adapter for the given generation.
    Start(u64),
    Stop,
    Shutdown,
    /// Read once. The scheduler keeps at most one in a queue, so polls
    /// that pile up behind a slow read result in a single read.
    Read,
}

fn index(p: Provider) -> usize {
    PROVIDERS.iter().position(|e| e.provider == p).unwrap()
}

impl Engine {
    /// Creates the engine and starts its scheduler thread. No provider
    /// runs until `start`.
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

    /// Starts the providers that `settings` switches on, and the poll
    /// timer. Call it once: a second call starts the adapters of those
    /// providers again without stopping them first.
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

    /// Tells the scheduler to stop every worker, then waits until all
    /// worker threads have ended or `SHUTDOWN_WAIT` has passed, whichever
    /// comes first.
    pub fn shutdown(&self) {
        self.send(In::Shutdown);
        let until = self.sh.clock.now() + SHUTDOWN_WAIT;
        self.sh.clock.wait_until(Some(until), &mut || {
            let s = self.sh.st.lock().unwrap();
            s.shut && s.alive == 0
        });
    }

    /// Blocks until there is a snapshot that has not been returned yet,
    /// then returns it. Only the most recent snapshot is kept, so a
    /// caller that reads slowly skips the ones in between. After shutdown
    /// no new snapshot comes, so a call then blocks for ever unless one
    /// is still unread.
    pub fn next(&self) -> Snapshot {
        let mut out = None;
        self.sh.clock.wait_until(None, &mut || {
            out = self.take();
            out.is_some()
        });
        out.unwrap()
    }

    /// Like `next`, but returns `None` at once instead of waiting.
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

    /// How many worker threads have been created so far. Switching a
    /// provider off does not remove its worker. Used by tests.
    pub fn workers(&self) -> usize {
        self.sh.st.lock().unwrap().created
    }
}

/// Dropping the handle without calling `shutdown` still shuts the engine
/// down, but does not wait: it queues the shutdown command, and the
/// scheduler and the workers end on their own. Call `shutdown` to wait
/// for them.
impl Drop for Engine {
    fn drop(&mut self) {
        // Use the state even if another thread panicked while holding the
        // lock: a panic in `drop` while the thread is already unwinding
        // would abort the process.
        let mut s = self.sh.st.lock().unwrap_or_else(|e| e.into_inner());
        if !s.shut {
            s.inbox.push_back(In::Shutdown);
            drop(s);
            self.sh.clock.wake();
        }
    }
}

/// The scheduler's record of one provider, from which it builds
/// snapshots.
#[derive(Default)]
struct Prov {
    on: bool,
    /// Counts the provider's starts. Every message about a read carries
    /// the generation it was started under, so that the result of a read
    /// from before the provider was switched off (and maybe on again) is
    /// recognised and ignored.
    generation: u64,
    /// The latest result from the worker since the provider was last
    /// started: a read's outcome, or `Unavailable` if the start failed.
    outcome: Option<ReadOutcome>,
    /// The readings of the latest successful read since the last start.
    last: Option<Vec<RawReading>>,
    /// An in-process read has run past `READ_DEADLINE` and has not
    /// reported yet.
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
    /// Read deadlines that have not passed yet: (provider index,
    /// generation of the read, time it passes). One whose generation is
    /// no longer current has no effect when it passes.
    deadlines: Vec<(usize, u64, Duration)>,
    /// A worker queue has changed. `flush` wakes the workers once the
    /// state lock is released.
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
                // `PollNow`, `SetInterval` and `SetProviders` are ignored
                // until the first `Start`: before it there are no settings
                // to poll or compare against, and no adapters to build.
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
                    // Arm a deadline only if the provider is still on and
                    // the read belongs to its current generation (see
                    // `Prov::generation`).
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

    /// Stores a new snapshot if a worker result or a provider change asked
    /// for one (`publish`). Then, if a snapshot was stored or a worker
    /// queue changed (`sent`), wakes every thread that waits on the
    /// clock: the workers waiting for messages and any caller blocked in
    /// `Engine::next`. The wake comes after the state lock is released.
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

    /// Asks the worker of every switched-on provider for a read, unless
    /// one is already queued, and restarts the poll timer from `now`.
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

    /// Starts the providers that `new` switches on and stops the ones it
    /// switches off, compared with `old` (`None` at engine start).
    fn switch(&mut self, old: Option<&Settings>, new: &Settings) {
        for a in registry::actions(old, new) {
            match a {
                Action::Start(p) => self.start_provider(index(p)),
                Action::Stop(p) => {
                    let i = index(p);
                    let pr = &mut self.provs[i];
                    pr.on = false;
                    pr.reset();
                    // The provider's pending read deadline no longer
                    // applies.
                    self.deadlines.retain(|d| d.0 != i);
                    self.publish = true;
                    self.to_worker(i, Msg::Stop);
                }
            }
        }
    }

    /// Each start begins a new generation. The worker thread is created
    /// on the provider's first start only.
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

    /// Queues `m` for worker `i`. For a stop or shutdown it also raises
    /// the stop signal, so that a child-run read in progress ends early.
    // The name means "send to worker", not a conversion, so clippy's
    // naming rule for `to_*` methods does not apply.
    #[allow(clippy::wrong_self_convention)]
    fn to_worker(&mut self, i: usize, m: Msg) {
        let mut s = self.sh.st.lock().unwrap();
        let Some(w) = s.workers[i].as_mut() else {
            return;
        };
        if matches!(m, Msg::Stop | Msg::Shutdown) {
            // A queued read was asked for before this stop: drop it.
            w.queue.retain(|q| !matches!(q, Msg::Read));
            raise(&w.stop);
        }
        w.queue.push_back(m);
        self.sent = true;
    }

    /// Records a worker's result, unless the provider has been switched
    /// off since or the result belongs to an earlier generation.
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

    /// Builds a snapshot from the providers that are switched on, in
    /// provider order. A provider contributes, checked in this order:
    /// - while an in-process read is past its deadline: the readings of
    ///   its last successful read, with the values removed;
    /// - after a successful read: that read's readings;
    /// - after a failed in-process read: the readings of its last
    ///   successful read, with the values removed;
    /// - otherwise (before its first result, after an `Unavailable`
    ///   result, or after a failed child-run read): nothing.
    ///
    /// So a failed child-run read removes the provider's lines from the
    /// snapshot until a later read succeeds, while a failed in-process
    /// read keeps them without values.
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
        // lm-sensors is the first entry of `PROVIDERS`.
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

    /// Handles the shutdown command: stops the poll timer, replaces each
    /// worker's queue with a single shutdown message, and raises its stop
    /// signal. The scheduler thread returns right after this.
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

/// Raises the stop signal by making the eventfd readable.
fn raise(fd: &OwnedFd) {
    // The result is not checked: the write fails only if the counter
    // would overflow, and it is lowered at every start.
    let one: u64 = 1;
    // SAFETY: writes 8 bytes from a valid u64.
    unsafe { libc::write(fd.as_raw_fd(), (&one as *const u64).cast(), 8) };
}

/// Lowers the stop signal by reading the eventfd's counter back to zero.
fn drain(fd: &OwnedFd) {
    // The result is not checked: when the signal is already lowered, the
    // read fails with EAGAIN, which is the state wanted.
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

/// The message of a caught panic, or an empty string when the panic's
/// payload is not a string.
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

    /// The worker's loop: it handles the messages of its queue in order,
    /// and exits after shutdown once its adapter is stopped.
    ///
    /// Every call into the adapter is wrapped in `catch_unwind`, so that
    /// a faulty adapter does not end the worker thread. The panic is
    /// logged; a panic in `start` is reported as `Unavailable`, one in
    /// `read` as `Failed`, and one in `stop` has no other effect.
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
                    // The stop signal may still be raised from the
                    // previous stop.
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
                    // Only an in-process read has a read deadline. A
                    // child-run read has its own time limit in the
                    // adapter, and it also ends when the stop signal is
                    // raised.
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
                        // Drop the read requests that arrived while the
                        // program ran, so that the next read waits for the
                        // next poll instead of starting the program again
                        // at once.
                        let mut s = self.sh.st.lock().unwrap();
                        let q = &mut s.workers[self.i].as_mut().unwrap().queue;
                        while matches!(q.front(), Some(Msg::Read)) {
                            q.pop_front();
                        }
                    }
                    self.send(In::Report(self.i, generation, out));
                }
                // A read for an adapter that is not available, and a stop
                // for one that was never started, have nothing to do.
                _ => {}
            }
            // After shutdown, exit only once the adapter has been stopped.
            let shut = self.sh.st.lock().unwrap().shut;
            if shut && !started {
                break;
            }
        }
        self.sh.st.lock().unwrap().alive -= 1;
        self.sh.clock.wake();
    }
}
