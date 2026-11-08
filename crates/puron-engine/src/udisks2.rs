//! The udisks2 source: the temperatures of ATA drives, as the udisks2
//! service reports them over D-Bus.
//!
//! `start` connects to the bus, lists the service's objects once, and
//! keeps the drives that have both the `Drive` and the `Drive.Ata`
//! interface and a string `Model`, sorted by object path. The list stays
//! fixed until the next start. Each `read` asks for each kept drive's
//! `SmartTemperature` property, the value udisks2 has stored, with one
//! property call per drive. The adapter makes no other call to the
//! service. zbus is asynchronous; every call runs to completion on the
//! worker thread through `zbus::block_on`.

use std::collections::HashMap;
use std::future::Future;
use std::os::fd::BorrowedFd;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use puron_core::rows::{Kind, Provider, RawReading};
use udisks2::zbus;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::diag::{self, udisks2 as ev};
use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The udisks2 service's bus name; every call goes to it.
pub const SERVICE: &str = "org.freedesktop.UDisks2";
/// The path of udisks2's object manager, whose `GetManagedObjects` lists
/// the drives at start.
pub const MANAGER_PATH: &str = "/org/freedesktop/UDisks2";
/// The interfaces a drive must have to be kept. `SmartTemperature` is a
/// property of `ATA`.
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
const ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
/// 0 °C in kelvin. udisks2 reports temperatures in kelvin.
const KELVIN: f64 = 273.15;

/// Which D-Bus bus the adapter connects to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bus {
    /// The system bus, where udisks2 runs.
    System,
    /// The bus at a D-Bus address, such as a test's private bus.
    Address(String),
}

/// A drive kept at start.
#[derive(Debug)]
struct Drive {
    /// The drive's object path, which keys its reading.
    path: String,
    /// The drive's `Model` property, which names its reading.
    model: String,
}

/// The udisks2 adapter.
pub struct Udisks2 {
    bus: Bus,
    /// The longest wait for connecting and for each call on the bus.
    limit: Duration,
    /// The connection, or `None` before the first start and after it was
    /// closed or lost.
    conn: Option<zbus::Connection>,
    drives: Vec<Drive>,
}

impl Udisks2 {
    pub fn new(bus: Bus, limit: Duration) -> Self {
        Udisks2 {
            bus,
            limit,
            conn: None,
            drives: Vec::new(),
        }
    }

    /// The bus the adapter was created with.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// The time limit the adapter was created with.
    pub fn limit(&self) -> Duration {
        self.limit
    }

    /// Connects to the bus within `limit`, and sets the same limit on
    /// every method call made through the connection. The error is
    /// `None` when the time limit passed before the connection was made.
    fn connect(&self) -> Result<zbus::Connection, Option<zbus::Error>> {
        let builder = match &self.bus {
            Bus::System => zbus::connection::Builder::system(),
            Bus::Address(a) => zbus::connection::Builder::address(a.as_str()),
        }
        .map_err(Some)?
        .method_timeout(self.limit);
        match zbus::block_on(Within::new(self.limit, builder.build())) {
            Some(r) => r.map_err(Some),
            None => Err(None),
        }
    }

    /// Closes the connection, if there is one, waiting at most `limit`.
    /// An error while closing is ignored.
    fn close(&mut self) {
        if let Some(c) = self.conn.take() {
            let _ = zbus::block_on(Within::new(self.limit, c.close()));
        }
    }
}

/// A future that waits for `fut` for at most a time limit. It yields
/// `Some` with the result, or `None` when the limit passes first.
///
/// The first time `fut` is pending, a timer thread is started that wakes
/// the task when the limit passes. The thread waits on a condition
/// variable, so it ends as soon as the `Within` is dropped instead of
/// sleeping until the limit.
struct Within<F> {
    fut: Pin<Box<F>>,
    shared: Arc<(Mutex<Timer>, Condvar)>,
    /// The time limit. It is taken when the timer thread starts, so the
    /// thread starts only once.
    limit: Option<Duration>,
}

/// The state that `Within` shares with its timer thread.
#[derive(Default)]
struct Timer {
    /// Set by the thread when the limit has passed.
    passed: bool,
    /// Set when the `Within` is dropped; the thread then ends.
    done: bool,
    /// The waker of the last poll, woken when the limit passes.
    waker: Option<Waker>,
}

impl<F: Future> Within<F> {
    fn new(limit: Duration, fut: F) -> Self {
        Within {
            fut: Box::pin(fut),
            shared: Arc::new((Mutex::new(Timer::default()), Condvar::new())),
            limit: Some(limit),
        }
    }
}

impl<F> Drop for Within<F> {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).done = true;
        cv.notify_all();
    }
}

impl<F: Future> Future for Within<F> {
    type Output = Option<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(v) = self.fut.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        let mut s = self.shared.0.lock().unwrap();
        if s.passed {
            return Poll::Ready(None);
        }
        s.waker = Some(cx.waker().clone());
        drop(s);
        if let Some(limit) = self.limit.take() {
            let shared = self.shared.clone();
            std::thread::spawn(move || {
                let (lock, cv) = &*shared;
                let end = Instant::now() + limit;
                let mut s = lock.lock().unwrap();
                while !s.done {
                    let now = Instant::now();
                    if now >= end {
                        s.passed = true;
                        if let Some(w) = s.waker.take() {
                            w.wake();
                        }
                        break;
                    }
                    s = cv.wait_timeout(s, end - now).unwrap().0;
                }
            });
        }
        Poll::Pending
    }
}

type Interfaces = HashMap<String, HashMap<String, OwnedValue>>;
type Managed = HashMap<OwnedObjectPath, Interfaces>;

/// The drives to keep from the listing of the service's objects: those
/// with both the `Drive` and the `Drive.Ata` interface and a string
/// `Model`, sorted by object path. An object with `Drive` that is left
/// out is logged with the reason.
fn drives(objects: Managed) -> Vec<Drive> {
    let mut objects: Vec<(String, Interfaces)> = objects
        .into_iter()
        .map(|(p, i)| (p.as_str().to_string(), i))
        .collect();
    objects.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (path, ifaces) in objects {
        let Some(drive) = ifaces.get(DRIVE) else {
            continue;
        };
        if !ifaces.contains_key(ATA) {
            diag::event(ev::drive_left_out(&path, "no Drive.Ata interface"));
            continue;
        }
        match drive.get("Model").map(|v| &**v) {
            Some(Value::Str(s)) => out.push(Drive {
                model: s.as_str().to_string(),
                path,
            }),
            Some(_) => diag::event(ev::drive_left_out(&path, "Model not a string")),
            None => diag::event(ev::drive_left_out(&path, "no Model")),
        }
    }
    out
}

/// Why `temperature` gave no value.
enum Fault {
    /// A problem with this drive only: the reading has no value and the
    /// read goes on with the next drive.
    Drive(Reason),
    /// Any other call error, taken to mean that the connection is lost:
    /// the read stops and fails.
    Lost,
}

/// The cause of a `Fault::Drive`. It is turned into text by `text` only
/// when diagnostics are on.
enum Reason {
    /// The call failed with an error reply or reached the time limit.
    Call(zbus::Error),
    /// The reply's body could not be decoded.
    Body(zbus::Error),
    /// The property's value is not a double.
    NotDouble,
}

impl Reason {
    fn text(&self) -> String {
        match self {
            Reason::Call(zbus::Error::MethodError(name, text, _)) => {
                format!("{name}: {}", text.as_deref().unwrap_or_default())
            }
            Reason::Call(zbus::Error::InputOutput(e)) => e.to_string(),
            Reason::Call(e) | Reason::Body(e) => e.to_string(),
            Reason::NotDouble => "not a double".to_string(),
        }
    }
}

/// The `SmartTemperature` property of the drive at `path`, in kelvin.
/// An error reply or a timeout is a fault of this drive; any other call
/// error counts as a lost connection.
fn temperature(conn: &zbus::Connection, path: &str) -> Result<f64, Fault> {
    let reply = zbus::block_on(conn.call_method(
        Some(SERVICE),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(ATA, "SmartTemperature"),
    ));
    let msg = match reply {
        Ok(m) => m,
        Err(e @ (zbus::Error::MethodError(..) | zbus::Error::FDO(_))) => {
            return Err(Fault::Drive(Reason::Call(e)))
        }
        Err(zbus::Error::InputOutput(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Err(Fault::Drive(Reason::Call(zbus::Error::InputOutput(e))))
        }
        Err(_) => return Err(Fault::Lost),
    };
    let value: OwnedValue = msg
        .body()
        .deserialize()
        .map_err(|e| Fault::Drive(Reason::Body(e)))?;
    match &*value {
        Value::F64(k) => Ok(*k),
        _ => Err(Fault::Drive(Reason::NotDouble)),
    }
}

impl Source for Udisks2 {
    /// Connects and lists the drives. Returns `Unavailable` when the bus
    /// cannot be reached, the listing fails, or no drive is kept.
    fn start(&mut self) -> StartOutcome {
        self.close();
        self.drives.clear();
        let conn = match self.connect() {
            Ok(c) => c,
            Err(e) => {
                if diag::on() {
                    let text = match e {
                        Some(e) => e.to_string(),
                        None => "time limit reached".to_string(),
                    };
                    diag::event(ev::connect_failed(&text));
                }
                return StartOutcome::Unavailable;
            }
        };
        self.conn = Some(conn.clone());
        let listed: Result<Managed, zbus::Error> = zbus::block_on(conn.call_method(
            Some(SERVICE),
            MANAGER_PATH,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        ))
        .and_then(|m| m.body().deserialize());
        let Ok(objects) = listed else {
            diag::event(ev::list_failed());
            self.close();
            return StartOutcome::Unavailable;
        };
        self.drives = drives(objects);
        if self.drives.is_empty() {
            diag::event(ev::no_ata_drive());
            self.close();
            return StartOutcome::Unavailable;
        }
        StartOutcome::Available
    }

    /// Returns one reading per kept drive, in list order. A drive whose
    /// temperature is 0 K or less is left out, and a drive whose call
    /// fails gets a reading without a value. A lost connection fails the
    /// whole read and closes the connection; each later read then tries
    /// once to connect again, and fails if it cannot. The drive list is
    /// not refreshed when the adapter connects again.
    fn read(&mut self, _stop: BorrowedFd<'_>) -> ReadOutcome {
        if self.conn.is_none() {
            match self.connect() {
                Ok(c) => self.conn = Some(c),
                Err(_) => {
                    diag::event(ev::reconnect_failed());
                    return ReadOutcome::Failed;
                }
            }
        }
        let conn = self.conn.clone().expect("connected");
        let mut out = Vec::new();
        let mut lost = false;
        for d in &self.drives {
            let value = match temperature(&conn, &d.path) {
                Ok(k) if k > 0.0 => Some(k - KELVIN),
                Ok(_) => continue,
                Err(Fault::Drive(r)) => {
                    if diag::on() {
                        diag::event(ev::read_failed(&d.path, &r.text()));
                    }
                    None
                }
                Err(Fault::Lost) => {
                    lost = true;
                    break;
                }
            };
            out.push(RawReading {
                provider: Provider::Udisks2,
                position: out.len() as u32,
                kind: Some(Kind::Temperature),
                chip: None,
                name: Some(d.model.clone()),
                device: None,
                item: None,
                unit: None,
                key: d.path.clone(),
                value,
                kernel_label: None,
                platform: false,
                device_chars: 0,
            });
        }
        if lost {
            diag::event(ev::connection_lost());
            self.close();
            return ReadOutcome::Failed;
        }
        ReadOutcome::Readings(out)
    }

    /// Closes the connection and forgets the drives.
    fn stop(&mut self) {
        self.close();
        self.drives.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A future that never completes: `Within` gives `None` once the
    // limit has passed.
    #[test]
    fn within_limit_passes() {
        puron_testkit::limit("p4f3-within", 2, || {
            let r = zbus::block_on(Within::new(
                Duration::from_millis(50),
                std::future::pending::<()>(),
            ));
            assert_eq!(r, None);
        });
    }

    // `Reason::text` gives "name: message" for an error reply and a fixed
    // text for a value that is not a double.
    #[test]
    fn reason_texts() {
        puron_testkit::limit("p4f3-reason", 2, || {
            let name = zbus::names::OwnedErrorName::try_from("org.example.Error").unwrap();
            let call = zbus::message::Message::method_call("/", "M")
                .unwrap()
                .build(&())
                .unwrap();
            let e = zbus::Error::MethodError(name, Some("no data".into()), call);
            assert_eq!(Reason::Call(e).text(), "org.example.Error: no data");
            assert_eq!(Reason::NotDouble.text(), "not a double");
        });
    }
}
