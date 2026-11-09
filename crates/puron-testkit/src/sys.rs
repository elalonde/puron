//! Builds fake sysfs trees, such as `class/nvme` or `bus/pci/devices`
//! with their files, for the tests of the code that reads sysfs.
//!
//! A fixture is built at test time, into a fresh temporary directory
//! `<tmp>`, from its description `fixtures/sys/<name>.txt`, because git
//! cannot keep a read-only directory or a named pipe. The tree's root is
//! `<tmp>/root`. A description has one item per line, created in file
//! order; blank lines and lines that start with `#` are skipped, and
//! missing parent directories are created. A path is relative to the
//! root and cannot hold a space.
//! - `f <path> "<bytes>"`: a file holding the bytes between the quotes,
//!   which may use the escapes `\n`, `\t`, `\\`, `\"` and `\xHH`;
//! - `d <path>`: a directory;
//! - `p <path>`: a named pipe;
//! - `l <path> <target>`: a symbolic link to `<target>`, written as it
//!   is;
//! - `m <path> <octal mode>`: the mode of the path, set after every other
//!   line is done, so that a read-only directory can still be filled.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A built tree. Dropping it makes everything under `tmp` writable again
/// and removes it.
pub struct Sys {
    /// The temporary directory that holds the tree and its copies.
    pub tmp: PathBuf,
    /// The root of the tree, `<tmp>/root`; it stands for `/sys`.
    pub root: PathBuf,
}

impl Sys {
    /// The path of `rel` inside the tree.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Makes two copies of the tree with `cp -a`: `<tmp>/readonly`, made
    /// read-only with `chmod -R a-w`, for a test that must not be able
    /// to write, and `<tmp>/copy`, left as it is, to compare with after
    /// the test (see `compare`). Returns `(readonly, copy)`. Call it once
    /// per tree.
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

/// Runs a program and panics unless it exits 0.
fn run(program: &str, args: &[&std::ffi::OsStr]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("run {program}: {e}"));
    assert!(status.success(), "{program} {args:?}: {status}");
}

/// Compares a tree with its copy, to check that the code under test
/// changed nothing. Returns two exit codes, both 0 when nothing changed:
/// that of `diff -r -x config <tree> <copy>` (files and their contents;
/// any path named `config` is skipped), and that of a `cmp` of the two
/// sorted lists of named pipes, which `diff` does not compare. Their
/// output goes to standard error.
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

/// Gives every directory under `path` mode 700 and every file mode 600,
/// without following symbolic links, so that the tree can be removed.
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

/// The bytes of a quoted string of a description line, with its escapes
/// decoded; panics, naming `line`, on a string without quotes or a bad
/// escape.
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

/// Creates the missing parent directories of `path`.
fn parent(path: &Path) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("create parent");
    }
}

/// Builds the tree described by `fixtures/sys/<name>.txt` into a new
/// temporary directory. Panics on a malformed line.
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
