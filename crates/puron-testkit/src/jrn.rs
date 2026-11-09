//! Fixtures for the tests of the diagnostic events that Puron sends to
//! the systemd journal, over the journal's native protocol on a datagram
//! socket.
//!
//! Three stand-ins for the journal's socket: [`Journal`], a socket that a
//! thread of the test reads; [`full`], a socket that takes no more data;
//! and [`none`], a path where no socket exists. Also a parser of the
//! native protocol, and [`one_of_each`], which builds one event of each
//! kind with fixed field values.

use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use puron_engine::diag::{self, Event};

/// One parsed journal entry: its fields as `(name, value)`, in the
/// order of the datagram.
pub type Entry = Vec<(String, String)>;

/// A journal socket, `<tmp>/journal.socket`, bound by the test. A reader
/// thread records each datagram, unparsed, in the order it arrives. The
/// thread runs until the process ends.
pub struct Journal {
    /// The socket's path, for the code under test to send to.
    pub path: PathBuf,
    got: Arc<(Mutex<Vec<Vec<u8>>>, Condvar)>,
}

impl Journal {
    /// Binds a new socket in a fresh temporary directory named after `tag`
    /// and starts its reader.
    pub fn new(tag: &str) -> Journal {
        let path = crate::temp_dir(tag).join("journal.socket");
        let sock = UnixDatagram::bind(&path).expect("bind X-jrn-sock");
        let got = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let g = got.clone();
        sock.set_nonblocking(true).expect("nonblocking reader");
        // The reader receives every queued datagram without taking the
        // lock, and hands the batch over only once the socket's queue is
        // empty, so that it keeps up with senders that send without pause.
        // Then it waits in poll for the next datagram.
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 1 << 20];
            let mut batch = Vec::new();
            loop {
                match sock.recv(&mut buf) {
                    Ok(n) => batch.push(buf[..n].to_vec()),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if !batch.is_empty() {
                            g.0.lock().unwrap().append(&mut batch);
                            g.1.notify_all();
                        }
                        let mut p = libc::pollfd {
                            fd: sock.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        // SAFETY: one valid pollfd.
                        unsafe { libc::poll(&mut p, 1, -1) };
                    }
                    Err(_) => return,
                }
            }
        });
        Journal { path, got }
    }

    /// The datagrams received so far, unparsed.
    pub fn raw(&self) -> Vec<Vec<u8>> {
        self.got.0.lock().unwrap().clone()
    }

    /// Waits until at least `n` datagrams have arrived or `max` has
    /// passed, then returns all that have arrived, unparsed. The caller
    /// checks the count.
    pub fn wait_raw(&self, n: usize, max: Duration) -> Vec<Vec<u8>> {
        let end = Instant::now() + max;
        let mut g = self.got.0.lock().unwrap();
        while g.len() < n {
            let now = Instant::now();
            if now >= end {
                break;
            }
            g = self.got.1.wait_timeout(g, end - now).unwrap().0;
        }
        g.clone()
    }

    /// As `wait_raw`, with each datagram parsed; a datagram that is not
    /// one whole entry panics.
    pub fn wait(&self, n: usize, max: Duration) -> Vec<Entry> {
        self.wait_raw(n, max)
            .iter()
            .map(|d| parse(d).expect("one whole entry"))
            .collect()
    }
}

/// A journal that takes nothing more: a bound socket with the smallest
/// receive buffer (`SO_RCVBUF` 1) and no reader, so that a send to it
/// soon finds it full. Returns its path and the socket; the caller keeps
/// the socket alive for the whole test.
pub fn full(tag: &str) -> (PathBuf, UnixDatagram) {
    let path = crate::temp_dir(tag).join("journal.socket");
    let sock = UnixDatagram::bind(&path).expect("bind X-jrn-full");
    let one: libc::c_int = 1;
    // SAFETY: a valid socket descriptor and an int option of its size.
    let rc = unsafe {
        libc::setsockopt(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &one as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    assert_eq!(rc, 0, "SO_RCVBUF");
    (path, sock)
}

/// No journal: the path `<tmp>/nojournal/socket`, in an empty
/// directory, where no socket exists.
pub fn none(tag: &str) -> PathBuf {
    let dir = crate::temp_dir(tag).join("nojournal");
    std::fs::create_dir(&dir).expect("nojournal");
    dir.join("socket")
}

/// Parses one datagram of the journal's native protocol into an entry.
/// Each field is either `NAME=value\n`, or, for a value that may hold a
/// newline, `NAME\n`, the value's length as 64-bit little-endian, the
/// value and `\n`. Returns an error, naming the place, for anything else,
/// such as a field cut short or a value that is not UTF-8.
pub fn parse(d: &[u8]) -> Result<Entry, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let rest = &d[i..];
        let stop = rest
            .iter()
            .position(|&b| b == b'=' || b == b'\n')
            .ok_or_else(|| format!("no end of field at {i}"))?;
        let name = String::from_utf8(rest[..stop].to_vec()).map_err(|e| e.to_string())?;
        if name.is_empty() {
            return Err(format!("empty field name at {i}"));
        }
        if rest[stop] == b'=' {
            let v = &rest[stop + 1..];
            let nl = v
                .iter()
                .position(|&b| b == b'\n')
                .ok_or_else(|| format!("{name}: no newline"))?;
            let value = String::from_utf8(v[..nl].to_vec()).map_err(|e| e.to_string())?;
            out.push((name, value));
            i += stop + 1 + nl + 1;
        } else {
            let v = &rest[stop + 1..];
            if v.len() < 8 {
                return Err(format!("{name}: short length"));
            }
            let n = u64::from_le_bytes(v[..8].try_into().unwrap()) as usize;
            if v.len() < 8 + n + 1 || v[8 + n] != b'\n' {
                return Err(format!("{name}: bad binary value"));
            }
            let value = String::from_utf8(v[8..8 + n].to_vec()).map_err(|e| e.to_string())?;
            out.push((name, value));
            i += stop + 1 + 8 + n + 1;
        }
    }
    Ok(out)
}

/// The value of the first field `name` in `e`, if any.
pub fn field<'a>(e: &'a Entry, name: &str) -> Option<&'a str> {
    e.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// The entries as text, for comparing with an expected file: one line
/// `NAME=value` per field, and a blank line after each entry.
pub fn dump(entries: &[Entry]) -> String {
    let mut s = String::new();
    for e in entries {
        for (n, v) in e {
            s.push_str(&format!("{n}={v}\n"));
        }
        s.push('\n');
    }
    s
}

/// The value that `one_of_each` gives the field `name` of its `n`-th
/// event: `<name in lower case>-<n>`, so that each value shows where it
/// came from.
fn v(name: &str, n: usize) -> String {
    format!("{}-{n}", name.to_lowercase())
}

/// Calls `f` once for each of the 29 kinds of diagnostic event of
/// `puron_engine::diag`, in a fixed order grouped by source (engine,
/// lm-sensors, NVIDIA, udisks2, liquidctl, applet). Each field value is
/// made by `v` from the event's place in that order, 1 to 29; events
/// without fields still take their place.
pub fn one_of_each(mut f: impl FnMut(Event<'_>)) {
    use diag::{applet, engine, liquidctl, lmsensors, nvidia, udisks2};
    f(engine::panic(
        &v("PROVIDER", 1),
        &v("PHASE", 1),
        &v("TEXT", 1),
    ));
    f(engine::read_deadline(&v("PROVIDER", 2)));
    f(engine::start_unavailable(&v("PROVIDER", 3)));
    f(engine::provider_updated(&v("PROVIDER", 4)));
    f(lmsensors::init_failed(&v("TEXT", 5)));
    f(lmsensors::config_error(
        &v("FILE", 6),
        &v("LINE", 6),
        &v("TEXT", 6),
    ));
    f(lmsensors::value_failed(
        &v("CHIP", 7),
        &v("SUBFEATURE", 7),
        &v("TEXT", 7),
    ));
    f(lmsensors::chip_name());
    f(nvidia::library_missing());
    f(nvidia::init_failed(&v("TEXT", 10)));
    f(nvidia::call_failed(
        &v("PCI", 11),
        &v("CALL", 11),
        &v("TEXT", 11),
    ));
    f(nvidia::close_failed());
    f(nvidia::pci_list());
    f(udisks2::connect_failed(&v("TEXT", 14)));
    f(udisks2::list_failed());
    f(udisks2::no_ata_drive());
    f(udisks2::drive_left_out(&v("OBJECT", 17), &v("REASON", 17)));
    f(udisks2::read_failed(&v("OBJECT", 18), &v("TEXT", 18)));
    f(udisks2::connection_lost());
    f(udisks2::reconnect_failed());
    f(liquidctl::not_found(&v("SEARCH_PATH", 21)));
    f(liquidctl::not_started(&v("TEXT", 22)));
    f(liquidctl::run_failed(
        &v("REASON", 23),
        &v("TEXT", 23),
        &v("POSITION", 23),
    ));
    f(liquidctl::exit_status(&v("STATUS", 24)));
    f(applet::config_problem(&v("KEY", 25), &v("KIND", 25)));
    f(applet::pinned_store_failed());
    f(applet::launch_failed(&v("PROGRAM", 27), &v("ERRNO", 27)));
    f(applet::display_updated());
    f(applet::menu_rebuilt());
}
