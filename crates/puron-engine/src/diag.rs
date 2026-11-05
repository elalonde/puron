//! Diagnostics, `puron-engine` side: the switch, the events and the
//! journal writer. One datagram per event in the journal's native
//! protocol, sent without waiting; any error drops it.

use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// The journal socket, priority, identifier, field prefix and value cap.
pub const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";
pub const PRIORITY: &str = "7";
pub const IDENTIFIER: &str = "puron-applet";
pub const FIELD_PREFIX: &str = "PURON_";
pub const VALUE_CAP: usize = 1024;

static ON: AtomicBool = AtomicBool::new(false);
static SOCKET: RwLock<Option<PathBuf>> = RwLock::new(None);

/// One event: its source, ID, text and fields in a fixed order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event<'a> {
    pub source: &'static str,
    pub id: &'static str,
    pub text: &'static str,
    /// (name without `PURON_`, upper case; value)
    pub fields: Vec<(&'static str, &'a str)>,
}

/// Sets the diagnostics switch; it is off until the first call.
pub fn switch(on: bool) {
    ON.store(on, Ordering::SeqCst);
}

/// Whether the switch is on: a caller whose event fields need
/// formatting tests it first, so nothing is formatted when off.
pub fn on() -> bool {
    ON.load(Ordering::SeqCst)
}

/// Sets the journal socket (the production path, or a test's socket).
pub fn set_socket(path: impl Into<PathBuf>) {
    let mut g = SOCKET.write().unwrap_or_else(|e| e.into_inner());
    *g = Some(path.into());
}

/// Sends an event; when the switch is off it is dropped and nothing is
/// formatted.
pub fn event(e: Event<'_>) {
    if !ON.load(Ordering::SeqCst) {
        return;
    }
    write(&e);
}

/// Builds the journal entry and sends it as one datagram.
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

/// `v` cut to at most `VALUE_CAP` bytes at a character boundary.
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

/// One field: `NAME=value\n`, or with a newline in the value the binary
/// form `NAME\n`, the length (u64 little-endian), the bytes, `\n`.
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

/// Sends without waiting; any error drops the entry.
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

/// The events of source `engine`.
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

/// The events of source `src.lmsensors`.
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

/// The events of source `src.nvidia`.
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

/// The events of source `src.udisks2`.
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

/// The events of source `src.liquidctl`.
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

    /// `text` is empty unless `reason` is `not-json` or `wait-error`;
    /// `position` is empty unless `reason` is `not-json`.
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

/// The events of source `applet.app`.
pub mod applet {
    use super::{ev, Event};
    const S: &str = "applet.app";

    /// `key` is empty for the directory.
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
