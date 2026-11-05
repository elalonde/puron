//! Builder of the sysfs Sensor tree fixtures.
//!
//! A fixture is built at test time into a fresh temporary directory
//! from its description `fixtures/sys/<name>.txt`, because git keeps no
//! read-only directory and no named pipe. The sysfs root
//! is `<tmp>/root`. Description lines, created in file order (`#`
//! starts a comment; parents are made as needed):
//! - `f <path> "<bytes>"`: a file holding the bytes, written with the
//!   escapes `\n`, `\t`, `\\`, `\"` and `\xHH`;
//! - `d <path>`: a directory;
//! - `p <path>`: a named pipe;
//! - `l <path> <target>`: a symbolic link;
//! - `m <path> <octal mode>`: the mode, set after every other line.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A built tree; dropping it makes everything under `tmp` writable and
/// removes it.
pub struct Sys {
    pub tmp: PathBuf,
    /// The sysfs root, `<tmp>/root`.
    pub root: PathBuf,
}

impl Sys {
    /// `<root>/<rel>`.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// The read-only copy and the untouched copy of the tree:
    /// `cp -a` of the root to `<tmp>/copy`, then
    /// `cp -a` to `<tmp>/readonly` and `chmod -R a-w` on it. Returns
    /// (read-only copy, untouched copy).
    pub fn copies(&self) -> (PathBuf, PathBuf) {
        let copy = self.tmp.join("copy");
        let ro = self.tmp.join("readonly");
        run(
            "cp",
            &["-a".as_ref(), self.root.as_os_str(), copy.as_os_str()],
        );
        run(
            "cp",
            &["-a".as_ref(), self.root.as_os_str(), ro.as_os_str()],
        );
        run("chmod", &["-R".as_ref(), "a-w".as_ref(), ro.as_os_str()]);
        (ro, copy)
    }
}

fn run(program: &str, args: &[&std::ffi::OsStr]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("run {program}: {e}"));
    assert!(status.success(), "{program} {args:?}: {status}");
}

/// The check that a sysfs tree is unchanged: the exit
/// codes of `diff -r -x config "$TREE" "$COPY"` and of the `cmp` of the
/// two lists of named pipes. Both are 0 when the tree is unchanged.
pub fn compare(tree: &Path, copy: &Path) -> (i32, i32) {
    let script = r#"diff -r -x config "$1" "$2" >&2; echo diff_rc=$?
cmp <(cd "$1" && find . -type p | sort) <(cd "$2" && find . -type p | sort) >&2; echo pipes_rc=$?"#;
    let out = Command::new("bash")
        .args(["-c", script, "compare"])
        .arg(tree)
        .arg(copy)
        .output()
        .expect("run bash");
    let text = String::from_utf8_lossy(&out.stdout);
    let rc = |key: &str| -> i32 {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(|| panic!("no {key} in {text:?}"))
    };
    (rc("diff_rc="), rc("pipes_rc="))
}

impl Drop for Sys {
    fn drop(&mut self) {
        make_writable(&self.tmp);
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

fn make_writable(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    let mode = if meta.is_dir() { 0o700 } else { 0o600 };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    if meta.is_dir() {
        if let Ok(list) = std::fs::read_dir(path) {
            for item in list.flatten() {
                make_writable(&item.path());
            }
        }
    }
}

/// The bytes of a quoted string of a description line.
fn unquote(text: &str, line: &str) -> Vec<u8> {
    let inner = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not a quoted string: {line}"));
    let mut out = Vec::new();
    let mut bytes = inner.bytes();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match bytes.next() {
            Some(b'n') => out.push(b'\n'),
            Some(b't') => out.push(b'\t'),
            Some(b'\\') => out.push(b'\\'),
            Some(b'"') => out.push(b'"'),
            Some(b'x') => {
                let hex: Vec<u8> = bytes.by_ref().take(2).collect();
                let hex = std::str::from_utf8(&hex).expect("hex escape");
                out.push(
                    u8::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("\\x{hex}: {line}")),
                );
            }
            other => panic!("escape {other:?}: {line}"),
        }
    }
    out
}

fn parent(path: &Path) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("create parent");
    }
}

/// Builds the tree described by `fixtures/sys/<name>.txt`.
pub fn build(name: &str) -> Sys {
    let desc = crate::fixture("sys", &format!("{name}.txt"));
    let text =
        std::fs::read_to_string(&desc).unwrap_or_else(|e| panic!("read {}: {e}", desc.display()));
    let tmp = crate::temp_dir(&format!("sys-{name}"));
    let root = tmp.join("root");
    std::fs::create_dir_all(&root).expect("create root");
    let mut modes = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let kind = parts.next().unwrap_or_default();
        let rel = parts.next().unwrap_or_else(|| panic!("no path: {line}"));
        let rest = parts.next();
        let path = root.join(rel);
        match (kind, rest) {
            ("f", Some(bytes)) => {
                parent(&path);
                std::fs::write(&path, unquote(bytes, line)).expect("write file");
            }
            ("d", None) => std::fs::create_dir_all(&path).expect("create dir"),
            ("p", None) => {
                parent(&path);
                let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                    .expect("path without NUL");
                // SAFETY: `c` is a valid NUL-terminated path.
                let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o644) };
                assert_eq!(rc, 0, "mkfifo {}", path.display());
            }
            ("l", Some(target)) => {
                parent(&path);
                std::os::unix::fs::symlink(target, &path).expect("symlink");
            }
            ("m", Some(mode)) => {
                let mode = u32::from_str_radix(mode, 8).unwrap_or_else(|_| panic!("mode: {line}"));
                modes.push((path, mode));
            }
            _ => panic!("bad line in {name}: {line}"),
        }
    }
    for (path, mode) in modes {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
    Sys { tmp, root }
}
