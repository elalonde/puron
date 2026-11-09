//! D-Bus fixtures for the udisks2 provider's tests: a private bus run by
//! `dbus-daemon`, and a fake udisks2 service on it that answers from a
//! script, logs the calls it receives, and can be changed while it runs.
//!
//! The fake also exists as the program `puron-fake-udisks2 --address
//! <addr> --script <file> --log <file>`. The tests do not use that
//! program: [`Fake::start`] runs the current test binary again (see
//! `rerun_self`) with the fake's arguments in `PURON_DBUS_FAKE`, because
//! a test run of one package alone does not build the testkit's
//! programs. Each udisks2 test therefore calls [`child`] first, which
//! runs the fake when it finds that variable.
//!
//! A script, `fixtures/dbus/<name>.json`, holds:
//! - `objects`: a list of `{"path", "interfaces": {<interface>:
//!   {<property>: <value>}}}`, each value tagged with its D-Bus type,
//!   `{"s": "text"}`, `{"d": 1.5}` or `{"i": 3}`;
//! - `faults` (optional): for an object path, the action for its first,
//!   second, ... `Get` of `SmartTemperature`: `"error"` (a D-Bus error
//!   reply), `"string"` (a string instead of a double), `"hang"` (no
//!   reply), or null (the normal reply). Calls past the end of the list
//!   get the normal reply;
//! - `hang_list` (optional): when true, `GetManagedObjects` gets no
//!   reply.
//!
//! The log holds one line per method call received, `<interface>
//! <member> <object path>`; the line of a `Properties.Get` call adds the
//! interface and the property asked for.
//!
//! The control socket, `ctl` in the bus directory, takes one command per
//! connection, one line, and answers `ok` once the script is changed:
//! - `set <path> <property> <value>`: sets the property in every
//!   interface of the object that has it, to the rest of the line, read
//!   as a double for `SmartTemperature` and as a string otherwise;
//! - `add <path> <model> <kelvin>`: adds a drive object with both
//!   interfaces, `Drive.Model` and `Drive.Ata.SmartTemperature`;
//! - `remove <path>`: removes the object.
//!
//! An unknown command, or `set` on an unknown object, panics the fake's
//! control thread; no `ok` comes back, so the test's `Fake::ctl` fails.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use udisks2::zbus;
use zbus::zvariant::{ObjectPath, Value};

/// The variable that makes a re-run test binary run the fake. It holds
/// the bus address and the paths of the script, the log and the control
/// socket, one per line.
pub const FAKE_ENV: &str = "PURON_DBUS_FAKE";

pub const SERVICE: &str = "org.freedesktop.UDisks2";
pub const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
pub const ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
/// The object path of the udisks2 drive `n`:
/// `/org/freedesktop/UDisks2/drives/<n>`.
pub fn drive(n: &str) -> String {
    format!("/org/freedesktop/UDisks2/drives/{n}")
}

fn need() {
    crate::host_need("dbus-daemon", "dbus-daemon --version");
}

/// Ends a child process: SIGTERM, then SIGKILL if it has not exited
/// within about 1 s. Returns at once if it has already exited.
fn end(child: &mut Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    // SAFETY: kill only sends a signal to the child's process ID.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    for _ in 0..20 {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Makes the kernel kill the child with SIGKILL when the test process
/// ends, so that no bus or fake outlives a test that fails or times out.
fn die_with_parent(cmd: &mut Command) {
    // SAFETY: prctl is async-signal-safe; it only sets a flag of the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

/// A private bus: a `dbus-daemon` with the configuration
/// `fixtures/dbus/session.conf`, listening on `unix:path=<dir>/bus`.
/// Dropping it stops the daemon.
pub struct Bus {
    /// The directory of the bus socket; the fake's log and control
    /// socket go here too.
    pub dir: PathBuf,
    /// The bus address, `unix:path=<dir>/bus`.
    pub address: String,
    daemon: Child,
}

impl Bus {
    /// Starts the daemon in `dir` and returns once it has printed its
    /// address. A socket left in `dir` by an earlier daemon is removed
    /// first. Ends the process with `HOST NEED dbus-daemon` when the
    /// daemon is not installed.
    pub fn start(dir: &Path) -> Bus {
        need();
        let _ = std::fs::remove_file(dir.join("bus"));
        let address = format!("unix:path={}", dir.join("bus").display());
        let mut cmd = Command::new("dbus-daemon");
        cmd.arg("--nofork")
            .arg("--print-address")
            .arg(format!(
                "--config-file={}",
                crate::fixture("dbus", "session.conf").display()
            ))
            .arg(format!("--address={address}"))
            .stdout(Stdio::piped());
        die_with_parent(&mut cmd);
        let mut daemon = cmd.spawn().expect("start dbus-daemon");
        let mut line = String::new();
        BufReader::new(daemon.stdout.take().expect("daemon stdout"))
            .read_line(&mut line)
            .expect("daemon address");
        // The daemon prints the address followed by its guid. The tests
        // keep the address without the guid, so that it also reaches a
        // daemon started again in the same directory.
        assert!(line.starts_with(&address), "daemon printed {line:?}");
        Bus {
            dir: dir.to_path_buf(),
            address,
            daemon,
        }
    }

    /// Stops the daemon (see `end`). The bus can be started again with
    /// `Bus::start` on the same directory.
    pub fn stop(&mut self) {
        end(&mut self.daemon);
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The fake udisks2 service on a [`Bus`], run in a child process.
/// Dropping it stops the child.
pub struct Fake {
    child: Child,
    /// The call log, `<bus dir>/<script>.log`.
    pub log: PathBuf,
    /// The control socket, `<bus dir>/ctl`.
    pub ctl: PathBuf,
}

impl Fake {
    /// Starts the fake with the script `fixtures/dbus/<script>.json`, by
    /// running the test `test_name` of the current binary again, and
    /// returns once the fake owns the udisks2 bus name. Panics if the
    /// child ends before that.
    pub fn start(test_name: &str, bus: &Bus, script: &str) -> Fake {
        let log = bus.dir.join(format!("{script}.log"));
        let ctl = bus.dir.join("ctl");
        let script = crate::fixture("dbus", &format!("{script}.json"));
        let args = format!(
            "{}\n{}\n{}\n{}",
            bus.address,
            script.display(),
            log.display(),
            ctl.display()
        );
        let mut cmd = crate::rerun_self(test_name);
        cmd.env(FAKE_ENV, args).stdout(Stdio::piped());
        die_with_parent(&mut cmd);
        // The child is not waited for on every path. If it ends before
        // `ready`, the panic below fails the test and the child has
        // already exited; a child still running is killed by
        // die_with_parent when the test process ends.
        #[allow(clippy::zombie_processes)]
        let mut child = cmd.spawn().expect("start the fake");
        let out = BufReader::new(child.stdout.take().expect("fake stdout"));
        for line in out.lines() {
            // The test harness prints "test <name> ... " before `ready`, on
            // the same line, so only the end of the line is compared.
            if line.expect("fake output").ends_with("ready") {
                return Fake { child, log, ctl };
            }
        }
        panic!("the fake ended before ready");
    }

    /// The lines of the call log so far; none if the log does not exist
    /// yet.
    pub fn log(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Sends one control command (see the module documentation) and
    /// panics unless the fake answers `ok`. When it returns, the change is
    /// in place for the next call the fake answers.
    pub fn ctl(&self, line: &str) {
        let mut s = UnixStream::connect(&self.ctl).expect("control socket");
        writeln!(s, "{line}").expect("send command");
        let mut answer = String::new();
        BufReader::new(s).read_line(&mut answer).expect("answer");
        assert_eq!(answer, "ok\n", "command {line:?}");
    }

    /// Stops the fake (see `end`).
    pub fn stop(&mut self) {
        end(&mut self.child);
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether a log line is a call the udisks2 provider is expected to
/// make: `GetManagedObjects` on `/org/freedesktop/UDisks2`, or a
/// `Properties.Get` of `Drive.Ata.SmartTemperature` on an object under
/// it. Tests check each log line with it to show that the provider made
/// no other call.
pub fn allowed(line: &str) -> bool {
    line == "org.freedesktop.DBus.ObjectManager GetManagedObjects /org/freedesktop/UDisks2"
        || (line.starts_with("org.freedesktop.DBus.Properties Get /org/freedesktop/UDisks2/")
            && line.ends_with(" org.freedesktop.UDisks2.Drive.Ata SmartTemperature"))
}

/// Collects the `NameOwnerChanged` signals of a bus, each as
/// `(name, old owner, new owner)`; an empty owner means none. Tests use
/// it to see when a name is taken or released.
pub struct Owners {
    rx: mpsc::Receiver<(String, String, String)>,
    _conn: zbus::blocking::Connection,
}

impl Owners {
    /// Opens a connection of its own to `bus` and starts collecting. Only
    /// changes after this call are seen.
    pub fn watch(bus: &Bus) -> Owners {
        let conn = zbus::blocking::connection::Builder::address(bus.address.as_str())
            .and_then(|b| b.build())
            .expect("watch connection");
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus")
            .and_then(|r| r.member("NameOwnerChanged"))
            .expect("match rule")
            .build();
        let it =
            zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None).expect("subscribe");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for m in it.flatten() {
                if let Ok(v) = m.body().deserialize::<(String, String, String)>() {
                    if tx.send(v).is_err() {
                        return;
                    }
                }
            }
        });
        Owners { rx, _conn: conn }
    }

    /// The next change not yet returned, waiting at most `wait` for one;
    /// `None` if none comes.
    pub fn next(&self, wait: Duration) -> Option<(String, String, String)> {
        self.rx.recv_timeout(wait).ok()
    }
}

// ---- The fake service; this code runs in the fake's own process ----

/// A property value of the script, tagged with its D-Bus type: `s`
/// (string), `d` (double) or `i` (32-bit integer).
#[derive(Clone, Debug, Deserialize)]
enum Val {
    #[serde(rename = "s")]
    S(String),
    #[serde(rename = "d")]
    D(f64),
    #[serde(rename = "i")]
    I(i32),
}

impl Val {
    fn value(&self) -> Value<'static> {
        match self {
            Val::S(s) => Value::from(s.clone()),
            Val::D(d) => Value::from(*d),
            Val::I(i) => Value::from(*i),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Object {
    path: String,
    /// Interfaces in name order, each with its properties.
    interfaces: BTreeMap<String, BTreeMap<String, Val>>,
}

#[derive(Debug, Deserialize)]
struct Script {
    objects: Vec<Object>,
    #[serde(default)]
    faults: HashMap<String, Vec<Option<String>>>,
    #[serde(default)]
    hang_list: bool,
}

struct State {
    script: Script,
    /// `SmartTemperature` calls so far, per object path.
    calls: HashMap<String, usize>,
}

/// Every udisks2 test calls this first. In the process started by
/// `Fake::start` (`PURON_DBUS_FAKE` set) it runs the fake until its
/// connection to the bus ends, then exits the process with code 0. In
/// any other process it returns at once.
pub fn child() {
    let Some(args) = std::env::var_os(FAKE_ENV) else {
        return;
    };
    let args = args.into_string().expect("fake arguments");
    let a: Vec<&str> = args.lines().collect();
    serve(a[0], Path::new(a[1]), Path::new(a[2]), Path::new(a[3]));
    std::process::exit(0);
}

/// The `main` of `puron-fake-udisks2 --address <addr> --script <file>
/// --log <file>`: runs the fake; the control socket is `ctl` next to the
/// log. Panics if a flag is missing.
pub fn main_args(args: &[String]) {
    let get = |flag: &str| {
        let i = args.iter().position(|a| a == flag).expect(flag);
        args[i + 1].clone()
    };
    let log = PathBuf::from(get("--log"));
    let ctl = log.with_file_name("ctl");
    serve(&get("--address"), Path::new(&get("--script")), &log, &ctl);
}

/// Listens on the control socket `ctl` on a thread of its own and
/// applies each command to the script.
fn control(state: Arc<Mutex<State>>, ctl: &Path) {
    let _ = std::fs::remove_file(ctl);
    let listener = UnixListener::bind(ctl).expect("bind control socket");
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let mut line = String::new();
            let mut r = BufReader::new(&s);
            if r.read_line(&mut line).is_err() {
                continue;
            }
            command(
                &mut state.lock().unwrap().script,
                line.trim_end_matches('\n'),
            );
            let _ = (&s).write_all(b"ok\n");
        }
    });
}

fn command(script: &mut Script, line: &str) {
    let mut w = line.splitn(4, ' ');
    match (w.next(), w.next(), w.next(), w.next()) {
        (Some("set"), Some(path), Some(prop), Some(v)) => {
            let val = if prop == "SmartTemperature" {
                Val::D(v.parse().expect("a double"))
            } else {
                Val::S(v.to_string())
            };
            let o = script
                .objects
                .iter_mut()
                .find(|o| o.path == path)
                .expect("object");
            for props in o.interfaces.values_mut() {
                if let Some(p) = props.get_mut(prop) {
                    *p = val.clone();
                }
            }
        }
        (Some("add"), Some(path), Some(model), Some(k)) => {
            let mut interfaces = BTreeMap::new();
            interfaces.insert(
                DRIVE.to_string(),
                BTreeMap::from([("Model".to_string(), Val::S(model.to_string()))]),
            );
            interfaces.insert(
                ATA.to_string(),
                BTreeMap::from([(
                    "SmartTemperature".to_string(),
                    Val::D(k.parse().expect("kelvin")),
                )]),
            );
            script.objects.push(Object {
                path: path.to_string(),
                interfaces,
            });
        }
        (Some("remove"), Some(path), None, None) => script.objects.retain(|o| o.path != path),
        _ => panic!("control command {line:?}"),
    }
}

type Managed = HashMap<ObjectPath<'static>, HashMap<String, HashMap<String, Value<'static>>>>;

fn managed(script: &Script) -> Managed {
    script
        .objects
        .iter()
        .map(|o| {
            let ifaces = o
                .interfaces
                .iter()
                .map(|(i, props)| {
                    let p = props.iter().map(|(k, v)| (k.clone(), v.value())).collect();
                    (i.clone(), p)
                })
                .collect();
            (ObjectPath::try_from(o.path.clone()).expect("path"), ifaces)
        })
        .collect()
}

/// Runs the fake: takes the udisks2 name on the bus at `address`, prints
/// `ready`, then logs and answers each method call until the connection
/// ends. A call the script does not cover gets a D-Bus error reply.
fn serve(address: &str, script: &Path, log: &Path, ctl: &Path) {
    let text = std::fs::read_to_string(script).expect("read script");
    let script: Script = serde_json::from_str(&text).expect("parse script");
    let state = Arc::new(Mutex::new(State {
        script,
        calls: HashMap::new(),
    }));
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("open log");
    let conn = zbus::blocking::connection::Builder::address(address)
        .and_then(|b| b.build())
        .expect("fake connection");
    // The iterator is made before the name is requested, so that it
    // already receives a call sent as soon as the name is owned.
    let calls = zbus::blocking::MessageIterator::from(&conn);
    let reply: u32 = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "RequestName",
            // Flag 4 is DO_NOT_QUEUE; reply 1 means the fake is the
            // primary owner.
            &(SERVICE, 4u32),
        )
        .and_then(|m| m.body().deserialize())
        .expect("RequestName");
    assert_eq!(reply, 1, "primary owner");
    control(state.clone(), ctl);
    println!("ready");
    let _ = std::io::stdout().flush();
    for msg in calls {
        let Ok(msg) = msg else { break };
        let h = msg.header();
        if h.message_type() != zbus::message::Type::MethodCall {
            continue;
        }
        let iface = h.interface().map(|i| i.to_string()).unwrap_or_default();
        let member = h.member().map(|m| m.to_string()).unwrap_or_default();
        let path = h.path().map(|p| p.to_string()).unwrap_or_default();
        let get = (iface == "org.freedesktop.DBus.Properties" && member == "Get")
            .then(|| msg.body().deserialize::<(String, String)>().ok())
            .flatten();
        match &get {
            Some((i, p)) => writeln!(log, "{iface} {member} {path} {i} {p}"),
            None => writeln!(log, "{iface} {member} {path}"),
        }
        .expect("write log");
        let _ = log.flush();
        let mut st = state.lock().unwrap();
        let r = match (iface.as_str(), member.as_str(), get) {
            ("org.freedesktop.DBus.ObjectManager", "GetManagedObjects", _) => {
                if st.script.hang_list {
                    continue;
                }
                conn.reply(&h, &managed(&st.script))
            }
            (_, _, Some((i, p))) => {
                let mut action = None;
                if p == "SmartTemperature" {
                    let n = st.calls.get(&path).copied().unwrap_or(0);
                    action = st
                        .script
                        .faults
                        .get(&path)
                        .and_then(|f| f.get(n).cloned().flatten());
                    st.calls.insert(path.clone(), n + 1);
                }
                let value = st
                    .script
                    .objects
                    .iter()
                    .find(|o| o.path == path)
                    .and_then(|o| o.interfaces.get(&i))
                    .and_then(|props| props.get(&p))
                    .map(Val::value);
                match (action.as_deref(), value) {
                    (Some("hang"), _) => continue,
                    (Some("error"), _) => conn.reply_error(
                        &h,
                        "org.freedesktop.UDisks2.Error.Failed",
                        &"fake failure",
                    ),
                    (Some("string"), _) => conn.reply(&h, &Value::from("not a double")),
                    (_, Some(v)) => conn.reply(&h, &v),
                    (_, None) => conn.reply_error(
                        &h,
                        "org.freedesktop.DBus.Error.UnknownObject",
                        &"no such object or property",
                    ),
                }
            }
            _ => conn.reply_error(
                &h,
                "org.freedesktop.DBus.Error.UnknownMethod",
                &"not offered by the fake",
            ),
        };
        drop(st);
        let _ = r;
    }
}
