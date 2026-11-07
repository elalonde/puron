//! The udisks2 source: ATA drive temperatures from udisks2.
//!
//! Start connects to the bus, lists the service's objects once and keeps
//! the drives with both the Drive and the Drive.Ata interface and a
//! string `Model`, in object path order; the list stays fixed until the
//! next start. Each Read asks for each kept drive's stored
//! `SmartTemperature` with one property call. No other call is made to
//! the service. The crate's async calls run on the worker thread through
//! `zbus::block_on`.

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

/// Service name.
pub const SERVICE: &str = "org.freedesktop.UDisks2";
/// Object manager path.
pub const MANAGER_PATH: &str = "/org/freedesktop/UDisks2";
/// The interfaces of a listed drive.
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
const ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
/// Kelvin offset.
const KELVIN: f64 = 273.15;

/// The bus address parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bus {
    System,
    Address(String),
}

/// One kept drive.
#[derive(Debug)]
struct Drive {
    path: String,
    model: String,
}

/// The udisks2 adapter. `limit` is the bus time limit.
pub struct Udisks2 {
    bus: Bus,
    limit: Duration,
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

    /// The bus address parameter.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// The bus time limit parameter.
    pub fn limit(&self) -> Duration {
        self.limit
    }

    /// Connects within the bus time limit; the connection's calls wait at
    /// most the same limit.
    /// The error is `None` when the time limit passed first; its text is
    /// formed only for `diag`.
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

    /// Closes the connection, if any.
    fn close(&mut self) {
        if let Some(c) = self.conn.take() {
            let _ = zbus::block_on(Within::new(self.limit, c.close()));
        }
    }
}

/// Waits for a future at most a time limit: `None` when the limit
/// passes first. The limit is kept by a thread that wakes the future;
/// it waits on a condition variable, so it ends as soon as the future is
/// done or dropped, not when the limit passes.
struct Within<F> {
    fut: Pin<Box<F>>,
    shared: Arc<(Mutex<Timer>, Condvar)>,
    /// The limit, until the timer thread is started.
    limit: Option<Duration>,
}

#[derive(Default)]
struct Timer {
    /// The limit passed.
    passed: bool,
    /// The future is done or dropped: the thread ends.
    done: bool,
    /// The waker to wake when the limit passes.
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

type Managed = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

/// The drive listing: the objects with both interfaces and a string
/// `Model`, in code point order of their paths.
fn drives(objects: Managed) -> Vec<Drive> {
    let mut objects: Vec<(String, HashMap<String, HashMap<String, OwnedValue>>)> = objects
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

/// Why a temperature call gave no value.
enum Fault {
    /// This drive only: an error reply, the time limit, another type.
    Drive(Reason),
    /// The connection is lost.
    Lost,
}

/// A drive fault, kept unformatted until `diag` takes it.
enum Reason {
    /// The call's error: an error reply or the time limit.
    Call(zbus::Error),
    /// The reply's body could not be read.
    Body(zbus::Error),
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

/// The stored temperature of `path`, in kelvin.
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
    /// Connects and lists the drives; *unavailable* when the bus cannot
    /// be reached, the listing fails or no drive is kept.
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

    /// One reading per kept drive at more than 0 K, in list order; a call
    /// that fails gives that drive no value; a lost connection fails the
    /// read, and the next read connects again once.
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

    /// Closes the connection and drops the list.
    fn stop(&mut self) {
        self.close();
        self.drives.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The timer still ends a future that never completes.
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

    // The read-failed texts, formed only for diag.
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
