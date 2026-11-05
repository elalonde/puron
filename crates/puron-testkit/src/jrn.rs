//! Journal fixtures: a socket with a reader, a full socket and no
//! journal; the native-protocol parser; "one of each" (one event of each
//! kind, in a fixed order, with fixed field values).

use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use puron_engine::diag::{self, Event};

/// One parsed entry: (field name, value) in datagram order.
pub type Entry = Vec<(String, String)>;

/// A journal socket `<tmp>/journal.socket`, bound by the test; a reader
/// thread records each datagram, raw, in arrival order.
pub struct Journal {
    pub path: PathBuf,
    got: Arc<(Mutex<Vec<Vec<u8>>>, Condvar)>,
}

impl Journal {
    pub fn new(tag: &str) -> Journal {
        let path = crate::temp_dir(tag).join("journal.socket");
        let sock = UnixDatagram::bind(&path).expect("bind X-jrn-sock");
        let got = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let g = got.clone();
        sock.set_nonblocking(true).expect("nonblocking reader");
        // The reader drains the queue without taking the lock and
        // publishes a batch only when the queue is empty, so that it
        // keeps up with senders that never wait.
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

    /// The raw datagrams received so far.
    pub fn raw(&self) -> Vec<Vec<u8>> {
        self.got.0.lock().unwrap().clone()
    }

    /// Waits until `n` datagrams are in or `max` passes; returns them all.
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

    /// As `wait_raw`, each datagram parsed (a parse error panics).
    pub fn wait(&self, n: usize, max: Duration) -> Vec<Entry> {
        self.wait_raw(n, max)
            .iter()
            .map(|d| parse(d).expect("one whole entry"))
            .collect()
    }
}

/// A full journal: a bound socket with `SO_RCVBUF` 1 and no reader.
/// Keep the returned socket alive for the test.
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

/// No journal: `<tmp>/nojournal/socket` in an empty directory.
pub fn none(tag: &str) -> PathBuf {
    let dir = crate::temp_dir(tag).join("nojournal");
    std::fs::create_dir(&dir).expect("nojournal");
    dir.join("socket")
}

/// Parses one datagram of the native protocol: `NAME=value\n`, or
/// `NAME\n`, a 64-bit little-endian length, the bytes and `\n`.
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

/// The value of `name` in `e` (the first one).
pub fn field<'a>(e: &'a Entry, name: &str) -> Option<&'a str> {
    e.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// `FIELD=value` per line, one entry after another, a blank line after
/// each entry.
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

/// The fixed value of field `name` in event number `n` (1 to 29).
fn v(name: &str, n: usize) -> String {
    format!("{}-{n}", name.to_lowercase())
}

/// Calls `f` with each of the 29 diagnostic events, in the order of the
/// event list, with field values `<field in lower case>-<event number>`.
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
