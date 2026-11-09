//! Builds settings directories for the tests of `puron_config::Config`
//! and of the programs that use it.
//!
//! The settings are stored the way cosmic-config stores them: one file
//! per key (an entry), holding the value in RON, in the directory
//! `<root>/cosmic/io.github.elalonde.Puron/v1`. A fixture is built at
//! test time, into a fresh temporary directory `<tmp>`, from its
//! description `fixtures/cfg/<name>.txt`, because git cannot keep a file
//! of mode 000 or a read-only directory. The configuration root is
//! `<tmp>/config`; the system data directory, which holds the system
//! defaults (the `sysdefault-*` fixtures use it), is `<tmp>/data`.
//!
//! A description has one item per line; blank lines and lines that start
//! with `#` are skipped:
//! - `<key> <value>`: the entry `<key>` holds the rest of the line as it
//!   is (a RON value), without a newline;
//! - `@base <name>`: the lines of the description `<name>`, read at this
//!   point;
//! - `@dir <path>`: the entries are written under `<root>/<path>`. Each
//!   `@dir` adds one more directory that gets every entry; without any,
//!   the one directory is `ID_DIR`;
//! - `@hex <key> <hex bytes>`: the entry holds these bytes, for content
//!   that is not text;
//! - `@mode <key> <octal>`: the entry's mode, set after all entries are
//!   written;
//! - `@mkdir <path>`: an empty directory `<root>/<path>`;
//! - `@data <key> <value>`: an entry in the system data directory, under
//!   `ID_DIR`;
//! - `@readonly`: `chmod a-w` on `<root>/cosmic/io.github.elalonde.Puron`,
//!   on `<root>/ID_DIR` and on each entry in it.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The directory of Puron's entries, relative to a configuration root or
/// to the system data directory.
pub const ID_DIR: &str = "cosmic/io.github.elalonde.Puron/v1";

/// A built fixture. Dropping it makes its whole tree writable again and
/// removes it.
pub struct Cfg {
    /// The temporary directory that holds the fixture.
    pub tmp: PathBuf,
    /// The configuration root, `<tmp>/config`.
    pub root: PathBuf,
    /// The system data directory, `<tmp>/data`.
    pub data: PathBuf,
}

impl Cfg {
    /// The directory of the entries under the configuration root,
    /// `<root>/ID_DIR`.
    pub fn id_dir(&self) -> PathBuf {
        self.root.join(ID_DIR)
    }

    /// The bytes of the entry `key` in `id_dir`; panics if it cannot be
    /// read.
    pub fn entry(&self, key: &str) -> Vec<u8> {
        let path = self.id_dir().join(key);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Every path under the configuration root, relative to it, with a
    /// description: `d <mode>` for a directory, `f <mode> <bytes>` for
    /// anything else (mode in octal, bytes as a debug-printed list). Tests
    /// compare the trees before and after an operation. A file that cannot
    /// be read is listed with an empty content; the contents of a
    /// directory that cannot be read are not listed.
    pub fn tree(&self) -> BTreeMap<PathBuf, String> {
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

/// Adds every path under `dir` to `out`, for `Cfg::tree`.
fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) {
    let Ok(list) = std::fs::read_dir(dir) else {
        return;
    };
    for item in list {
        let path = item.expect("dir entry").path();
        let meta = std::fs::symlink_metadata(&path).expect("metadata");
        let rel = path.strip_prefix(base).expect("under base").to_path_buf();
        let mode = meta.permissions().mode() & 0o7777;
        if meta.is_dir() {
            out.insert(rel, format!("d {mode:o}"));
            walk(base, &path, out);
        } else {
            let content = std::fs::read(&path)
                .map(|b| format!("{b:?}"))
                .unwrap_or_default();
            out.insert(rel, format!("f {mode:o} {content}"));
        }
    }
}

impl Drop for Cfg {
    fn drop(&mut self) {
        make_writable(&self.tmp);
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

/// Gives every directory under `path` mode 700 and every file mode 600,
/// so that a fixture with read-only or unreadable parts can be removed.
fn make_writable(path: &Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    if let Ok(list) = std::fs::read_dir(path) {
        for item in list.flatten() {
            let p = item.path();
            if p.is_dir() {
                make_writable(&p);
            } else {
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

/// A parsed description, with every `@base` included.
#[derive(Default)]
struct Desc {
    dirs: Vec<String>,
    entries: BTreeMap<String, Vec<u8>>,
    modes: Vec<(String, u32)>,
    mkdirs: Vec<String>,
    data: BTreeMap<String, Vec<u8>>,
    readonly: bool,
}

/// Adds the lines of the description `name` to `desc`; a later line for
/// the same entry replaces an earlier one.
fn parse(name: &str, desc: &mut Desc) {
    let path = crate::fixture("cfg", &format!("{name}.txt"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "@base" => parse(rest, desc),
            "@dir" => desc.dirs.push(rest.to_string()),
            "@hex" => {
                let (key, hex) = rest.split_once(' ').expect("@hex <key> <bytes>");
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
                    .collect();
                desc.entries.insert(key.to_string(), bytes);
            }
            "@mode" => {
                let (key, mode) = rest.split_once(' ').expect("@mode <key> <octal>");
                let mode = u32::from_str_radix(mode, 8).expect("octal mode");
                desc.modes.push((key.to_string(), mode));
            }
            "@mkdir" => desc.mkdirs.push(rest.to_string()),
            "@data" => {
                let (key, value) = rest.split_once(' ').expect("@data <key> <value>");
                desc.data.insert(key.to_string(), value.as_bytes().to_vec());
            }
            "@readonly" => desc.readonly = true,
            key => {
                desc.entries
                    .insert(key.to_string(), rest.as_bytes().to_vec());
            }
        }
    }
}

/// Builds the fixture described by `fixtures/cfg/<name>.txt` into a new
/// temporary directory. Panics on a malformed description.
pub fn build(name: &str) -> Cfg {
    let mut desc = Desc::default();
    parse(name, &mut desc);
    let tmp = crate::temp_dir(&format!("cfg-{name}"));
    let cfg = Cfg {
        root: tmp.join("config"),
        data: tmp.join("data"),
        tmp,
    };
    std::fs::create_dir_all(&cfg.root).expect("create root");
    for dir in &desc.mkdirs {
        std::fs::create_dir_all(cfg.root.join(dir)).expect("mkdir");
    }
    let dirs = if desc.dirs.is_empty() {
        vec![ID_DIR.to_string()]
    } else {
        desc.dirs.clone()
    };
    if !desc.entries.is_empty() {
        for dir in &dirs {
            let dir = cfg.root.join(dir);
            std::fs::create_dir_all(&dir).expect("create entry dir");
            for (key, bytes) in &desc.entries {
                std::fs::write(dir.join(key), bytes).expect("write entry");
            }
            for (key, mode) in &desc.modes {
                std::fs::set_permissions(dir.join(key), std::fs::Permissions::from_mode(*mode))
                    .expect("chmod entry");
            }
        }
    }
    if !desc.data.is_empty() {
        let dir = cfg.data.join(ID_DIR);
        std::fs::create_dir_all(&dir).expect("create data dir");
        for (key, bytes) in &desc.data {
            std::fs::write(dir.join(key), bytes).expect("write data entry");
        }
    }
    if desc.readonly {
        let id = cfg.root.join("cosmic/io.github.elalonde.Puron");
        let v1 = cfg.id_dir();
        for key in desc.entries.keys() {
            set_readonly(&v1.join(key));
        }
        set_readonly(&v1);
        set_readonly(&id);
    }
    cfg
}

fn set_readonly(path: &Path) {
    let mut perms = std::fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(perms.mode() & !0o222);
    std::fs::set_permissions(path, perms).expect("chmod a-w");
}

/// Writes `bytes` to `path`, creating its parent directories, for a test
/// that changes an entry by hand.
pub fn write(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("create dir");
    }
    std::fs::write(path, bytes).expect("write");
}

/// The work of the `puron-cfg-writer` program: stores the value `ron`
/// (written as a description line writes it) for `key` into the
/// configuration root `root` through `Config::store`, as another process
/// would. Returns the exit code: 0 when `store` returns
/// `StoreResult::Stored`, 1 otherwise. A value it cannot parse also gives
/// 1, and so does the key `PINNED`, whose values it does not handle.
#[cfg(feature = "cfg-writer")]
pub fn writer_main(root: &Path, key: &str, ron: &str) -> i32 {
    use puron_config::{Config, StoreResult, Value};
    use puron_core::settings::TemperatureUnit;
    let value = match key {
        puron_config::POLL_INTERVAL_SECS => ron.parse().ok().map(Value::U32),
        puron_config::TEMPERATURE_UNIT => match ron {
            "Celsius" => Some(Value::TemperatureUnit(TemperatureUnit::Celsius)),
            "Fahrenheit" => Some(Value::TemperatureUnit(TemperatureUnit::Fahrenheit)),
            _ => None,
        },
        puron_config::PINNED => None,
        _ => ron.parse().ok().map(Value::Bool),
    };
    match value.map(|v| Config::new(root).store(key, &v)) {
        Some(StoreResult::Stored) => 0,
        _ => 1,
    }
}
