//! Builder of the Config fixtures.
//!
//! A fixture is built at test time into a fresh temporary directory
//! `<tmp>` from its description `fixtures/cfg/<name>.txt`, because git
//! keeps no mode 000 and no read-only directory. The configuration root
//! is `<tmp>/config`, the system data directory of the `sysdefault`
//! fixtures is `<tmp>/data`.
//!
//! Description lines, in order (`#` starts a comment):
//! - `<key> <RON value>`: the entry `<key>` holds the value, no newline;
//! - `@base <name>`: the lines of another description, here;
//! - `@dir <path>`: entries go under `<root>/<path>` (each `@dir` adds
//!   one; without one, `cosmic/io.github.elalonde.Puron/v1`);
//! - `@hex <key> <hex bytes>`: the entry holds these bytes;
//! - `@mode <key> <octal>`: the entry's mode, set after writing;
//! - `@mkdir <path>`: an empty directory `<root>/<path>`;
//! - `@data <key> <RON value>`: the entry in
//!   `<tmp>/data/cosmic/io.github.elalonde.Puron/v1/`;
//! - `@readonly`: `chmod a-w` on the ID directory, the version
//!   directory and each entry.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The entry directory, relative to a root.
pub const ID_DIR: &str = "cosmic/io.github.elalonde.Puron/v1";

/// A built fixture; dropping it makes its tree writable and removes it.
pub struct Cfg {
    pub tmp: PathBuf,
    /// The configuration root, `<tmp>/config`.
    pub root: PathBuf,
    /// The system data directory, `<tmp>/data`.
    pub data: PathBuf,
}

impl Cfg {
    /// `<root>/cosmic/io.github.elalonde.Puron/v1`.
    pub fn id_dir(&self) -> PathBuf {
        self.root.join(ID_DIR)
    }

    /// The bytes of the entry `key` under the ID directory.
    pub fn entry(&self, key: &str) -> Vec<u8> {
        let path = self.id_dir().join(key);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Every path under the root with its kind and content (`d` for a
    /// directory, `f <bytes>` for a file); entries that cannot be read
    /// are listed with their mode only.
    pub fn tree(&self) -> BTreeMap<PathBuf, String> {
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

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

#[derive(Default)]
struct Desc {
    dirs: Vec<String>,
    entries: BTreeMap<String, Vec<u8>>,
    modes: Vec<(String, u32)>,
    mkdirs: Vec<String>,
    data: BTreeMap<String, Vec<u8>>,
    readonly: bool,
}

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

/// Builds the fixture described by `fixtures/cfg/<name>.txt`.
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

/// Writes `bytes` to `path` (for a test that changes an entry by hand).
pub fn write(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("create dir");
    }
    std::fs::write(path, bytes).expect("write");
}

/// The `puron-cfg-writer` helper: stores `key` = `ron` (the value in
/// RON, as a description line holds it) into the configuration root
/// `root` through `Config::store`. Returns the exit code: 0 on *stored*,
/// 1 otherwise (a value this helper cannot read included).
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
