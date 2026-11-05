//! The applet's state machine, the launch of the two helper programs,
//! and the event loop that feeds the state machine. It runs without a
//! display: events in, state and commands out. The engine handle, the
//! config and the program places are parameters.

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use puron_config::{Config, Delivery, ProblemKind, StoreResult, Value, PINNED};
use puron_core::menu::{self, Key, Menu, Report, Shown, Update};
use puron_core::panel::{self, Panel, State};
use puron_core::pins;
use puron_core::rows::{Provider, ShownClasses};
use puron_core::settings::Settings;
use puron_engine::diag;
use puron_engine::engine::{Engine, LmReport, Snapshot};

/// The engine handle as the applet uses it.
pub trait Handle: Send + Sync {
    fn start(&self, settings: &Settings);
    fn poll_now(&self);
    fn set_interval(&self, secs: u32);
    fn set_providers(&self, settings: &Settings);
    /// Returns when the engine has stopped, after at most 1 s.
    fn shutdown(&self);
    /// The subscription: waits until a snapshot is new.
    fn next(&self) -> Snapshot;
    /// The latest unread snapshot, without waiting.
    fn try_next(&self) -> Option<Snapshot>;
}

/// The production handle: the engine with the production sensor
/// adapters.
impl Handle for Engine {
    fn start(&self, settings: &Settings) {
        use puron_engine::registry;
        Engine::start(
            self,
            settings,
            Box::new(|p| -> Box<dyn puron_engine::registry::Source> {
                match p {
                    Provider::LmSensors => Box::new(registry::lm_sensors()),
                    Provider::Liquidctl => Box::new(registry::liquidctl()),
                    Provider::Nvidia => Box::new(registry::nvidia()),
                    Provider::Udisks2 => Box::new(registry::udisks2()),
                    Provider::NvmeCli => Box::new(registry::nvme()),
                }
            }),
        );
    }
    fn poll_now(&self) {
        Engine::poll_now(self)
    }
    fn set_interval(&self, secs: u32) {
        Engine::set_interval(self, secs)
    }
    fn set_providers(&self, settings: &Settings) {
        Engine::set_providers(self, settings)
    }
    fn shutdown(&self) {
        Engine::shutdown(self)
    }
    fn next(&self) -> Snapshot {
        Engine::next(self)
    }
    fn try_next(&self) -> Option<Snapshot> {
        Engine::try_next(self)
    }
}

/// An event the state machine handles.
#[derive(Clone, Debug)]
pub enum Event {
    Start,
    Snapshot(Snapshot),
    Delivery(Delivery),
    PinClick(String),
    SettingsLink,
    HomepageLink,
    Stop,
}

/// An engine command.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Start(Settings),
    PollNow,
    SetInterval(u32),
    SetProviders(Settings),
    Shutdown,
}

/// The result of a launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launched {
    Started,
    Failed(i32),
}

/// What one event did, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Engine(Command),
    Store(Vec<String>, StoreResult),
    /// `diag` Switch.
    Switch(bool),
    /// A `diag` event, by its ID.
    Diag(&'static str),
    Layout {
        menu: Menu,
        forced: bool,
    },
    Panel(Panel),
    Launch(PathBuf, Launched),
    /// The program exits (after the engine shutdown has returned).
    Exit,
}

/// The places of the two programs (parameters, so tests can set them).
#[derive(Clone, Debug)]
pub struct Programs {
    /// The directory of `puron-settings`.
    pub settings_dir: PathBuf,
    /// The search path for `xdg-open`.
    pub search_path: OsString,
}

impl Programs {
    /// The directory of `/proc/self/exe` and the applet's `PATH`.
    pub fn production() -> Programs {
        let exe = std::fs::read_link("/proc/self/exe").unwrap_or_default();
        Programs {
            settings_dir: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
            search_path: std::env::var_os("PATH").unwrap_or_default(),
        }
    }
}

/// The settings program.
pub const SETTINGS_PROGRAM: &str = "puron-settings";
/// The browser opener.
pub const OPENER: &str = "xdg-open";

/// The applet's state with its parameters.
pub struct App {
    handle: Arc<dyn Handle>,
    config: Config,
    programs: Programs,
    settings: Settings,
    last: Option<Snapshot>,
    shown: Shown,
    last_key: Option<Key>,
    forced: bool,
    panel: Panel,
    menu: Option<Menu>,
    /// The shown classes of the settings that the current layout was
    /// built with.
    layout_classes: ShownClasses,
    pinned: Vec<String>,
    store_pending: bool,
    stopped: bool,
}

fn classes(s: &Settings) -> ShownClasses {
    ShownClasses {
        temperature: s.show_temperature,
        fan: s.show_rotation_rate,
        voltage: s.show_voltage,
        power: s.show_power,
    }
}

fn kind_word(k: ProblemKind) -> &'static str {
    match k {
        ProblemKind::Unreadable => "unreadable",
        ProblemKind::WrongType => "wrong-type",
        ProblemKind::Clamped => "clamped",
    }
}

impl App {
    pub fn new(handle: Arc<dyn Handle>, config: Config, programs: Programs) -> App {
        let settings = Settings::default();
        App {
            handle,
            config,
            programs,
            shown: menu::shown(&[], &settings),
            layout_classes: classes(&settings),
            settings,
            last: None,
            last_key: None,
            forced: false,
            panel: Panel {
                items: Vec::new(),
                padding: 0,
                state: State::default(),
            },
            menu: None,
            pinned: Vec::new(),
            store_pending: false,
            stopped: false,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }
    pub fn panel(&self) -> &Panel {
        &self.panel
    }
    pub fn menu(&self) -> Option<&Menu> {
        self.menu.as_ref()
    }
    pub fn store_pending(&self) -> bool {
        self.store_pending
    }
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Handles one event; returns what it did, in order.
    pub fn handle(&mut self, event: Event) -> Vec<Output> {
        let mut out = Vec::new();
        match event {
            Event::Start => self.start(&mut out),
            Event::Snapshot(s) => {
                self.last = Some(s);
                self.shown = self.compute_shown();
                let forced = self.forced;
                self.forced = false;
                self.layout(forced, &mut out);
            }
            Event::Delivery(d) => self.delivery(d, &mut out),
            Event::PinClick(id) => self.pin_click(&id, &mut out),
            Event::SettingsLink => {
                let path = self.programs.settings_dir.join(SETTINGS_PROGRAM);
                let r = launch(&path, &[]);
                self.launched(path, r, &mut out);
            }
            Event::HomepageLink => {
                // An empty or unset PATH has no directories, so nothing
                // is found. split_paths would give one empty entry, which
                // means the current directory.
                let path = &self.programs.search_path;
                let found = (!path.is_empty())
                    .then(|| std::env::split_paths(path))
                    .into_iter()
                    .flatten()
                    .map(|d| d.join(OPENER))
                    .find(|p| is_executable(p));
                match found {
                    Some(path) => {
                        let r = launch(&path, &[menu::HOMEPAGE_URL]);
                        self.launched(path, r, &mut out);
                    }
                    None => self.launched(
                        PathBuf::from(OPENER),
                        Launched::Failed(libc::ENOENT),
                        &mut out,
                    ),
                }
            }
            Event::Stop => {
                if !self.stopped {
                    self.stopped = true;
                    self.handle.shutdown();
                    out.push(Output::Engine(Command::Shutdown));
                    out.push(Output::Exit);
                }
            }
        }
        out
    }

    fn diag(&self, e: diag::Event<'_>, out: &mut Vec<Output>) {
        out.push(Output::Diag(e.id));
        diag::event(e);
    }

    fn problems(&self, problems: &[puron_config::Problem], out: &mut Vec<Output>) {
        for p in problems {
            self.diag(
                diag::applet::config_problem(p.key.unwrap_or(""), kind_word(p.kind)),
                out,
            );
        }
    }

    /// Start: load the config, set diag logging, build the first layout
    /// and panel, and start the engine.
    fn start(&mut self, out: &mut Vec<Output>) {
        let loaded = self.config.load();
        self.settings = loaded.settings;
        diag::switch(self.settings.debug_logging);
        out.push(Output::Switch(self.settings.debug_logging));
        self.problems(&loaded.problems, out);
        self.pinned = pins::start(&self.settings.pinned);
        self.shown = self.compute_shown();
        let m = menu::menu(&self.shown, &self.settings, Report::NoneYet, None, false);
        self.set_menu(m, false, out);
        self.set_panel(State::default(), out);
        self.handle.start(&self.settings);
        out.push(Output::Engine(Command::Start(self.settings.clone())));
    }

    fn compute_shown(&self) -> Shown {
        let raw = self.last.as_ref().map(|s| &s.readings[..]).unwrap_or(&[]);
        menu::shown(raw, &self.settings)
    }

    fn report(&self) -> Report {
        match self.last.as_ref().map(|s| s.lm_sensors) {
            None | Some(LmReport::NoneYet) => Report::NoneYet,
            Some(LmReport::Unavailable) => Report::Unavailable,
            Some(LmReport::Answered) => Report::Answered,
        }
    }

    fn set_menu(&mut self, m: Menu, forced: bool, out: &mut Vec<Output>) {
        self.last_key = Some(m.key.clone());
        self.layout_classes = classes(&self.settings);
        self.menu = Some(m.clone());
        out.push(Output::Layout { menu: m, forced });
    }

    fn set_panel(&mut self, state: State, out: &mut Vec<Output>) {
        let p = panel::panel(&self.shown, &self.pinned, &self.settings, &state);
        self.panel = p.clone();
        out.push(Output::Panel(p));
    }

    /// A layout after start (a snapshot's, or one rebuilt at once).
    fn layout(&mut self, forced: bool, out: &mut Vec<Output>) {
        let m = menu::menu(
            &self.shown,
            &self.settings,
            self.report(),
            self.last_key.as_ref(),
            forced,
        );
        if m.update == Update::Rebuild {
            self.diag(diag::applet::menu_rebuilt(), out);
        }
        self.set_menu(m, forced, out);
        self.set_panel(self.panel.state.clone(), out);
        self.diag(diag::applet::display_updated(), out);
    }

    /// A config delivery: apply the changed settings at once.
    fn delivery(&mut self, d: Delivery, out: &mut Vec<Output>) {
        let old = std::mem::replace(&mut self.settings, d.loaded.settings);
        let (o, n) = (&old, &self.settings);
        if o.debug_logging != n.debug_logging {
            diag::switch(n.debug_logging);
            out.push(Output::Switch(n.debug_logging));
        }
        self.problems(&d.loaded.problems, out);
        let units = o.show_temperature_unit != n.show_temperature_unit
            || o.show_rotation_rate_unit != n.show_rotation_rate_unit
            || o.show_voltage_unit != n.show_voltage_unit
            || o.show_power_unit != n.show_power_unit;
        let shown_groups = o.show_temperature != n.show_temperature
            || o.show_rotation_rate != n.show_rotation_rate
            || o.show_voltage != n.show_voltage
            || o.show_power != n.show_power
            || o.group_temperature != n.group_temperature
            || o.group_rotation_rate != n.group_rotation_rate
            || o.group_voltage != n.group_voltage;
        let poll = shown_groups
            || o.temperature_unit != n.temperature_unit
            || o.show_decimals != n.show_decimals
            || o.pinned != n.pinned;
        let providers = o.provider_lm_sensors != n.provider_lm_sensors
            || o.provider_liquidctl != n.provider_liquidctl
            || o.provider_nvidia != n.provider_nvidia
            || o.provider_udisks2 != n.provider_udisks2
            || o.provider_nvme != n.provider_nvme;
        let interval = o.poll_interval_secs != n.poll_interval_secs;
        let icons = o.show_panel_icons != n.show_panel_icons;
        if interval {
            self.handle.set_interval(n.poll_interval_secs);
            out.push(Output::Engine(Command::SetInterval(n.poll_interval_secs)));
        }
        if providers {
            self.handle.set_providers(&self.settings);
            out.push(Output::Engine(Command::SetProviders(self.settings.clone())));
        } else if poll {
            self.handle.poll_now();
            out.push(Output::Engine(Command::PollNow));
        }
        if shown_groups {
            self.forced = true;
        }
        if units {
            self.shown = self.compute_shown();
            self.layout(false, out);
        } else if icons {
            // Only the panel items change: no new layout, so no `display-updated` event.
            self.set_panel(self.panel.state.clone(), out);
        }
    }

    /// A pin click: update the pinned list, store it, and refresh the
    /// panel.
    fn pin_click(&mut self, id: &str, out: &mut Vec<Output>) {
        let ids = self
            .menu
            .as_ref()
            .map(|m| m.identities.clone())
            .unwrap_or_default();
        self.pinned = pins::click(&self.pinned, id, &ids, &self.layout_classes);
        let r = self.config.store(PINNED, &Value::List(self.pinned.clone()));
        out.push(Output::Store(self.pinned.clone(), r));
        match r {
            StoreResult::Stored => {
                self.settings.pinned = self.pinned.clone();
                self.store_pending = false;
            }
            StoreResult::Failed => {
                self.store_pending = true;
                self.diag(diag::applet::pinned_store_failed(), out);
            }
        }
        self.set_panel(self.panel.state.clone(), out);
        self.handle.poll_now();
        out.push(Output::Engine(Command::PollNow));
    }

    fn launched(&self, path: PathBuf, r: Launched, out: &mut Vec<Output>) {
        if let Launched::Failed(errno) = r {
            let program = path.to_string_lossy().into_owned();
            let errno = errno.to_string();
            self.diag(diag::applet::launch_failed(&program, &errno), out);
        }
        out.push(Output::Launch(path, r));
    }
}

fn is_executable(p: &Path) -> bool {
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    p.is_file() && unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Starts a program detached from the applet: a close-on-exec
/// pipe, fork, `setsid` and a second fork in the intermediate child,
/// `execve` in the grandchild; the applet reaps the intermediate child,
/// then reads the pipe (end of file: started; four bytes: `errno`).
pub fn launch(path: &Path, args: &[&str]) -> Launched {
    // Everything the children use is built before the first fork.
    let Ok(prog) = CString::new(path.as_os_str().as_bytes()) else {
        return Launched::Failed(libc::EINVAL);
    };
    let mut argv = vec![prog.clone()];
    for a in args {
        match CString::new(*a) {
            Ok(c) => argv.push(c),
            Err(_) => return Launched::Failed(libc::EINVAL),
        }
    }
    let env: Vec<CString> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let mut b = k.as_bytes().to_vec();
            b.push(b'=');
            b.extend_from_slice(v.as_bytes());
            CString::new(b).ok()
        })
        .collect();
    let mut argp: Vec<*const libc::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
    argp.push(std::ptr::null());
    let mut envp: Vec<*const libc::c_char> = env.iter().map(|c| c.as_ptr()).collect();
    envp.push(std::ptr::null());
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Launched::Failed(errno());
    }
    let (rd, wr) = (fds[0], fds[1]);
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let e = errno();
        unsafe {
            libc::close(rd);
            libc::close(wr);
        }
        return Launched::Failed(e);
    }
    if pid == 0 {
        // Intermediate child: fork, setsid, write and _exit only.
        unsafe {
            libc::setsid();
            let g = libc::fork();
            if g == 0 {
                libc::execve(prog.as_ptr(), argp.as_ptr(), envp.as_ptr());
                let e = *libc::__errno_location();
                libc::write(wr, (&e as *const i32).cast(), 4);
                libc::_exit(127);
            }
            if g < 0 {
                let e = *libc::__errno_location();
                libc::write(wr, (&e as *const i32).cast(), 4);
                libc::_exit(1);
            }
            libc::_exit(0);
        }
    }
    unsafe { libc::close(wr) };
    let mut status = 0;
    while unsafe { libc::waitpid(pid, &mut status, 0) } < 0 && errno() == libc::EINTR {}
    let mut buf = [0u8; 4];
    let mut got = 0usize;
    while got < 4 {
        let n = unsafe { libc::read(rd, buf[got..].as_mut_ptr().cast(), 4 - got) };
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        if n <= 0 {
            break;
        }
        got += n as usize;
    }
    unsafe { libc::close(rd) };
    if got == 4 {
        Launched::Failed(i32::from_ne_bytes(buf))
    } else {
        Launched::Started
    }
}

/// Where an event of the loop came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Start,
    Subscription,
    Delivery,
    Stop,
    Ui,
}

static STOP_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_sigterm(_: libc::c_int) {
    let fd = STOP_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let b = 1u8;
        unsafe { libc::write(fd, (&b as *const u8).cast(), 1) };
    }
}

/// SIGTERM only writes one byte to a pipe; returns the pipe's read end.
fn stop_on_sigterm() -> libc::c_int {
    let mut fds = [0 as libc::c_int; 2];
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return -1;
        }
        STOP_FD.store(fds[1], Ordering::SeqCst);
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigterm as extern "C" fn(libc::c_int) as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
    }
    fds[0]
}

/// The event loop: Start, then the config Watch and the snapshot
/// subscription; each event from `rx` (subscription, deliveries, the
/// stop pipe, the UI through `tx`) is handled in turn and `view` gets
/// the state and its outputs. At Exit the process exits with status 0.
/// The end of the channel counts as a Stop (the event loop ended).
pub fn run(
    mut app: App,
    tx: Sender<(Source, Event)>,
    rx: Receiver<(Source, Event)>,
    mut view: impl FnMut(Source, &App, &[Output]),
) -> ! {
    let stop = stop_on_sigterm();
    if stop >= 0 {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut b = 0u8;
            loop {
                let n = unsafe { libc::read(stop, (&mut b as *mut u8).cast(), 1) };
                if n == 1 {
                    let _ = tx.send((Source::Stop, Event::Stop));
                } else if n == 0 || errno() != libc::EINTR {
                    break;
                }
            }
        });
    }
    let out = app.handle(Event::Start);
    view(Source::Start, &app, &out);
    let dtx = std::sync::Mutex::new(tx.clone());
    let _watch = app.config().watch(move |d| {
        let _ = dtx
            .lock()
            .unwrap()
            .send((Source::Delivery, Event::Delivery(d)));
    });
    let handle = Arc::clone(&app.handle);
    let stx = tx.clone();
    std::thread::spawn(move || loop {
        let s = handle.next();
        if stx
            .send((Source::Subscription, Event::Snapshot(s)))
            .is_err()
        {
            break;
        }
    });
    drop(tx);
    loop {
        let (source, event) = rx.recv().unwrap_or((Source::Stop, Event::Stop));
        let out = app.handle(event);
        view(source, &app, &out);
        if out.contains(&Output::Exit) {
            std::process::exit(0);
        }
    }
}
