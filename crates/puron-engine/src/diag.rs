//! Diagnostic logging to the systemd journal: a process-wide on/off
//! switch, the events the engine, the sources and the applet can log,
//! and the writer.
//!
//! Each event is sent as one datagram in the journal's native protocol,
//! without waiting. Any error drops the event silently, so logging can
//! never block or fail the caller.

use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// The journal's socket, used unless `set_socket` names another one.
pub const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";
/// The syslog priority of every entry: 7, debug.
pub const PRIORITY: &str = "7";
/// The `SYSLOG_IDENTIFIER` of every entry.
pub const IDENTIFIER: &str = "puron-applet";
/// The prefix of the journal field name of each event field.
pub const FIELD_PREFIX: &str = "PURON_";
/// The longest field value, in bytes; a longer value is cut.
pub const VALUE_CAP: usize = 1024;

static ON: AtomicBool = AtomicBool::new(false);
static SOCKET: RwLock<Option<PathBuf>> = RwLock::new(None);

/// One diagnostic event, built by the functions of the submodules below.
/// `source` names the part of the program that logs it, `id` is a short
/// stable name for the event, and `text` is its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event<'a> {
    pub source: &'static str,
    pub id: &'static str,
    pub text: &'static str,
    /// The event's fields in the order they are written: (name, value).
    /// Each name is upper case and without `FIELD_PREFIX`.
    pub fields: Vec<(&'static str, &'a str)>,
}

/// Switches diagnostics on or off for the whole process. They are off
/// until the first call.
pub fn switch(on: bool) {
    ON.store(on, Ordering::SeqCst);
}

/// Whether diagnostics are on. A caller whose event fields must be
/// formatted checks this first, so that nothing is formatted while
/// diagnostics are off.
pub fn on() -> bool {
    ON.load(Ordering::SeqCst)
}

/// Sets the socket that events are sent to, instead of
/// `JOURNAL_SOCKET`. Tests use it to receive the events themselves.
pub fn set_socket(path: impl Into<PathBuf>) {
    let mut g = SOCKET.write().unwrap_or_else(|e| e.into_inner());
    *g = Some(path.into());
}

/// Sends `e` to the journal. While diagnostics are off, the event is
/// dropped before anything is formatted.
pub fn event(e: Event<'_>) {
    if !ON.load(Ordering::SeqCst) {
        return;
    }
    write(&e);
}

/// Builds the journal entry for `e` and sends it as one datagram. The
/// `MESSAGE` field repeats the source, the text and every field, after
/// which each field is also written as a journal field of its own.
fn write(e: &Event<'_>) {
    let values: Vec<&str> = e.fields.iter().map(|(_, v)| cut(v)).collect();
    let mut message = format!("{}: {}", e.source, e.text);
    for ((name, _), v) in e.fields.iter().zip(&values) {
        message.push_str("; ");
        message.push_str(&name.to_lowercase());
        message.push(' ');
        message.push_str(v);
    }
    let mut buf = Vec::with_capacity(256);
    put(&mut buf, "MESSAGE", &message);
    put(&mut buf, "PRIORITY", PRIORITY);
    put(&mut buf, "SYSLOG_IDENTIFIER", IDENTIFIER);
    put(&mut buf, "PURON_SOURCE", e.source);
    put(&mut buf, "PURON_EVENT", e.id);
    for ((name, _), v) in e.fields.iter().zip(&values) {
        put(&mut buf, &format!("{FIELD_PREFIX}{name}"), v);
    }
    send(&buf);
}

/// Returns `v` cut to at most `VALUE_CAP` bytes, at a character
/// boundary so that the result stays valid UTF-8.
fn cut(v: &str) -> &str {
    if v.len() <= VALUE_CAP {
        return v;
    }
    let mut end = VALUE_CAP;
    while !v.is_char_boundary(end) {
        end -= 1;
    }
    &v[..end]
}

/// Appends one field in the journal's native format: `NAME=value\n`. A
/// value that contains a newline needs the binary form instead: `NAME\n`,
/// the value's length as a little-endian u64, the value, and `\n`.
fn put(buf: &mut Vec<u8>, name: &str, value: &str) {
    buf.extend_from_slice(name.as_bytes());
    if value.contains('\n') {
        buf.push(b'\n');
        buf.extend_from_slice(&(value.len() as u64).to_le_bytes());
    } else {
        buf.push(b'=');
    }
    buf.extend_from_slice(value.as_bytes());
    buf.push(b'\n');
}

/// Sends one entry from a non-blocking socket. Any error, including a
/// full socket buffer, drops the entry.
fn send(buf: &[u8]) {
    let path = match SOCKET.read() {
        Ok(g) => g.clone(),
        Err(e) => e.into_inner().clone(),
    };
    let path = path.unwrap_or_else(|| PathBuf::from(JOURNAL_SOCKET));
    if let Ok(sock) = UnixDatagram::unbound() {
        if sock.set_nonblocking(true).is_ok() {
            let _ = sock.send_to(buf, &path);
        }
    }
}

fn ev<'a>(
    source: &'static str,
    id: &'static str,
    text: &'static str,
    fields: Vec<(&'static str, &'a str)>,
) -> Event<'a> {
    Event {
        source,
        id,
        text,
        fields,
    }
}

/// The events the engine logs (source `engine`).
pub mod engine {
    use super::{ev, Event};
    const S: &str = "engine";

    pub fn panic<'a>(provider: &'a str, phase: &'a str, text: &'a str) -> Event<'a> {
        let f = vec![("PROVIDER", provider), ("PHASE", phase), ("TEXT", text)];
        ev(S, "panic", "caught a panic", f)
    }

    pub fn read_deadline(provider: &str) -> Event<'_> {
        ev(
            S,
            "read-deadline",
            "read deadline passed",
            vec![("PROVIDER", provider)],
        )
    }

    pub fn start_unavailable(provider: &str) -> Event<'_> {
        let f = vec![("PROVIDER", provider)];
        ev(S, "start-unavailable", "start found nothing to read", f)
    }

    pub fn provider_updated(provider: &str) -> Event<'_> {
        let f = vec![("PROVIDER", provider)];
        ev(S, "provider-updated", "provider has new readings", f)
    }
}

/// The events the lm-sensors source logs (source `src.lmsensors`).
pub mod lmsensors {
    use super::{ev, Event};
    const S: &str = "src.lmsensors";

    pub fn init_failed(text: &str) -> Event<'_> {
        ev(
            S,
            "init-failed",
            "initialization failed",
            vec![("TEXT", text)],
        )
    }

    pub fn config_error<'a>(file: &'a str, line: &'a str, text: &'a str) -> Event<'a> {
        let f = vec![("FILE", file), ("LINE", line), ("TEXT", text)];
        ev(S, "config-error", "configuration error", f)
    }

    pub fn value_failed<'a>(chip: &'a str, subfeature: &'a str, text: &'a str) -> Event<'a> {
        let f = vec![("CHIP", chip), ("SUBFEATURE", subfeature), ("TEXT", text)];
        ev(S, "value-failed", "value read failed", f)
    }

    pub fn chip_name() -> Event<'static> {
        ev(S, "chip-name", "chip name not available", vec![])
    }
}

/// The events the NVIDIA source logs (source `src.nvidia`).
pub mod nvidia {
    use super::{ev, Event};
    const S: &str = "src.nvidia";

    pub fn library_missing() -> Event<'static> {
        ev(S, "library-missing", "NVML library missing", vec![])
    }

    pub fn init_failed(text: &str) -> Event<'_> {
        ev(S, "init-failed", "NVML init failed", vec![("TEXT", text)])
    }

    pub fn call_failed<'a>(pci: &'a str, call: &'a str, text: &'a str) -> Event<'a> {
        let f = vec![("PCI", pci), ("CALL", call), ("TEXT", text)];
        ev(S, "call-failed", "GPU call failed", f)
    }

    pub fn close_failed() -> Event<'static> {
        ev(S, "close-failed", "NVML close failed", vec![])
    }

    pub fn pci_list() -> Event<'static> {
        ev(S, "pci-list", "PCI device list not readable", vec![])
    }
}

/// The events the udisks2 source logs (source `src.udisks2`).
pub mod udisks2 {
    use super::{ev, Event};
    const S: &str = "src.udisks2";

    pub fn connect_failed(text: &str) -> Event<'_> {
        ev(
            S,
            "connect-failed",
            "connection failed",
            vec![("TEXT", text)],
        )
    }

    pub fn list_failed() -> Event<'static> {
        ev(S, "list-failed", "listing failed", vec![])
    }

    pub fn no_ata_drive() -> Event<'static> {
        ev(S, "no-ata-drive", "no ATA drive", vec![])
    }

    pub fn drive_left_out<'a>(object: &'a str, reason: &'a str) -> Event<'a> {
        let f = vec![("OBJECT", object), ("REASON", reason)];
        ev(S, "drive-left-out", "drive left out at start", f)
    }

    pub fn read_failed<'a>(object: &'a str, text: &'a str) -> Event<'a> {
        let f = vec![("OBJECT", object), ("TEXT", text)];
        ev(S, "read-failed", "temperature read failed", f)
    }

    pub fn connection_lost() -> Event<'static> {
        ev(S, "connection-lost", "connection lost", vec![])
    }

    pub fn reconnect_failed() -> Event<'static> {
        ev(S, "reconnect-failed", "connecting again failed", vec![])
    }
}

/// The events the liquidctl source logs (source `src.liquidctl`).
pub mod liquidctl {
    use super::{ev, Event};
    const S: &str = "src.liquidctl";

    pub fn not_found(search_path: &str) -> Event<'_> {
        ev(
            S,
            "not-found",
            "program not found",
            vec![("SEARCH_PATH", search_path)],
        )
    }

    pub fn not_started(text: &str) -> Event<'_> {
        ev(S, "not-started", "run not started", vec![("TEXT", text)])
    }

    /// `text` is empty unless `reason` is `not-json` or `wait-error`, and
    /// `position` is empty unless `reason` is `not-json`. The caller
    /// passes them that way; this function does not check it.
    pub fn run_failed<'a>(reason: &'a str, text: &'a str, position: &'a str) -> Event<'a> {
        let f = vec![("REASON", reason), ("TEXT", text), ("POSITION", position)];
        ev(S, "run-failed", "run failed", f)
    }

    pub fn exit_status(status: &str) -> Event<'_> {
        ev(
            S,
            "exit-status",
            "exit status not 0",
            vec![("STATUS", status)],
        )
    }
}

/// The events the applet logs (source `applet.app`).
pub mod applet {
    use super::{ev, Event};
    const S: &str = "applet.app";

    /// `key` names the setting that could not be used, or is empty when
    /// the problem is with the settings directory itself.
    pub fn config_problem<'a>(key: &'a str, kind: &'a str) -> Event<'a> {
        let f = vec![("KEY", key), ("KIND", kind)];
        ev(S, "config-problem", "stored setting not usable", f)
    }

    pub fn pinned_store_failed() -> Event<'static> {
        ev(S, "pinned-store-failed", "pinned list not stored", vec![])
    }

    pub fn launch_failed<'a>(program: &'a str, errno: &'a str) -> Event<'a> {
        let f = vec![("PROGRAM", program), ("ERRNO", errno)];
        ev(S, "launch-failed", "launch failed", f)
    }

    pub fn display_updated() -> Event<'static> {
        ev(S, "display-updated", "display updated", vec![])
    }

    pub fn menu_rebuilt() -> Event<'static> {
        ev(S, "menu-rebuilt", "menu rebuilt", vec![])
    }
}
