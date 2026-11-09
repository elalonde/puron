//! Tests of the NVIDIA adapter (`puron_engine::nvidia`), through its
//! `start`, `read` and `stop`.
//!
//! Each test builds a fake sysfs PCI device tree from a fixture in
//! `fixtures/sys/` (see `sys::build`) and gives the adapter a fake NVML
//! (`puron_testkit::nvml::Fake`) that answers from a script and logs each
//! call. A GPU counts as awake when its `power/runtime_status` is
//! `active` or `unsupported`; the adapter opens NVML only while a GPU is
//! awake. In the fixtures, each device's `config` file is a named pipe,
//! which the adapter must never open. `c_src_nvidia_11` and
//! `c_src_nvidia_12` check source files instead.

use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::nvidia::Nvidia;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::nvml::{self, Call, Fake, Script};
use puron_testkit::{sys, umd, workspace_root};

/// The time limit of the tests that read, in seconds.
const LIMIT: u64 = 5;
/// The time limit of the tests that check source files, in seconds.
const STATIC_LIMIT: u64 = 120;

/// The name that `nvml::good` gives the GPU at `0000:01:00.0`; it gives
/// every other GPU the name `GPU <address>`.
const GOOD_01: &str = "NVIDIA GeForce RTX 3060";

/// One expected reading: its name, value and key (the GPU's PCI address).
/// Its position is its index in the list of expected readings.
type Exp<'a> = (&'a str, Option<f64>, &'a str);

/// An adapter on the sysfs root `root` with a fake NVML that plays
/// `script`, and a handle on that fake to read its call log.
fn adapter(root: &Path, script: Script) -> (Nvidia, Fake) {
    let fake = Fake::new(script);
    (Nvidia::new(root, Box::new(fake.clone())), fake)
}

/// Reads once, with a stop descriptor that is never raised.
fn outcome(src: &mut Nvidia) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// Reads once and returns the readings; panics if the read gives
/// anything else.
fn read(src: &mut Nvidia) -> Vec<RawReading> {
    match outcome(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Checks every field of `readings`: name, value and key as in `exp`, in
/// that order; provider `Nvidia`; positions 0, 1, 2 and so on; kind
/// temperature; and no chip, device, item or unit.
fn check(readings: &[RawReading], exp: &[Exp]) {
    let got: Vec<(String, Option<f64>, String)> = readings
        .iter()
        .map(|r| (r.name.clone().unwrap_or_default(), r.value, r.key.clone()))
        .collect();
    let want: Vec<(String, Option<f64>, String)> = exp
        .iter()
        .map(|(n, v, k)| (n.to_string(), *v, k.to_string()))
        .collect();
    assert_eq!(got, want);
    for (i, r) in readings.iter().enumerate() {
        assert_eq!(r.provider, Provider::Nvidia);
        assert_eq!(r.position, i as u32, "position of {:?}", r.name);
        assert_eq!(r.kind, Some(Kind::Temperature));
        assert!(r.name.is_some());
        assert_eq!(
            (&r.chip, &r.device, &r.item, &r.unit),
            (&None, &None, &None, &None)
        );
    }
}

/// Writes `text` as the `power/runtime_status` of the device `pci`.
fn status(t: &sys::Sys, pci: &str, text: &str) {
    let path = t.path(&format!("bus/pci/devices/{pci}/power/runtime_status"));
    std::fs::write(path, text).expect("write runtime_status");
}

/// The fake's log entry for opening NVML.
fn open() -> Call {
    Call::Open
}

/// The calls the adapter makes for one awake GPU, in order: look it up
/// by PCI address, ask its name, ask its temperature.
fn queried(pci: &str) -> [Call; 3] {
    [
        Call::Device(pci.into()),
        Call::Name(pci.into()),
        Call::Temperature(pci.into()),
    ]
}

/// The PCI addresses looked up in `log`, in order.
fn devices(log: &[Call]) -> Vec<String> {
    log.iter()
        .filter_map(|c| match c {
            Call::Device(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

/// A hybrid-graphics tree: an Intel GPU, an NVIDIA 3D controller, its
/// audio function and an Intel audio device, all awake. Expects one
/// reading, for the NVIDIA GPU only, and NVML calls of exactly: open,
/// the three calls for that GPU, close.
#[test]
fn c_src_nvidia_1() {
    puron_testkit::limit("C-src.nvidia-1", LIMIT, || {
        let t = sys::build("pci-hybrid");
        let (mut src, fake) = adapter(&t.root, nvml::good());
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &[(GOOD_01, Some(45.0), "0000:01:00.0")]);
        let mut want = vec![open()];
        want.extend(queried("0000:01:00.0"));
        want.push(Call::Close);
        assert_eq!(fake.log(), want);
    });
}

/// The same tree with the NVIDIA GPU suspended. Expects its reading with
/// the default name "NVIDIA GPU" and no value, and no NVML call at all,
/// so that the GPU is not woken.
#[test]
fn c_src_nvidia_2() {
    puron_testkit::limit("C-src.nvidia-2", LIMIT, || {
        let t = sys::build("pci-hybrid");
        status(&t, "0000:01:00.0", "suspended\n");
        let (mut src, fake) = adapter(&t.root, nvml::good());
        assert_eq!(src.start(), StartOutcome::Available);
        check(&read(&mut src), &[("NVIDIA GPU", None, "0000:01:00.0")]);
        assert_eq!(fake.log(), []);
    });
}

/// Ten GPUs whose `runtime_status` is "active", "unsupported",
/// "suspended", "suspending", "resuming", "error", missing, "Active",
/// "active" without a newline, and "active " with a space. Expects only
/// GPUs 1, 2 and 9 to count as awake: they are looked up in NVML and
/// read; every other one is shown with its default name, numbered by
/// its place among the GPUs, and no value. No `start` is needed first.
#[test]
fn c_src_nvidia_3() {
    puron_testkit::limit("C-src.nvidia-3", LIMIT, || {
        let t = sys::build("pci-status");
        let (mut src, fake) = adapter(&t.root, nvml::good());
        let r = read(&mut src);
        let exp: Vec<(String, Option<f64>, String)> = (1..=10)
            .map(|i| {
                let pci = format!("0000:{i:02x}:00.0");
                match i {
                    1 => (GOOD_01.to_string(), Some(45.0), pci),
                    2 | 9 => (format!("GPU {pci}"), Some(50.0), pci),
                    _ => (format!("NVIDIA GPU {i}"), None, pci),
                }
            })
            .collect();
        let exp: Vec<Exp> = exp
            .iter()
            .map(|(n, v, k)| (n.as_str(), *v, k.as_str()))
            .collect();
        check(&r, &exp);
        assert_eq!(
            devices(&fake.log()),
            ["0000:01:00.0", "0000:02:00.0", "0000:09:00.0"]
        );
    });
}

/// GPUs created out of address order, one of them in PCI domain 0001.
/// Expects the readings, and the NVML lookups, in PCI address order.
#[test]
fn c_src_nvidia_4() {
    puron_testkit::limit("C-src.nvidia-4", LIMIT, || {
        let t = sys::build("pci-order");
        let (mut src, fake) = adapter(&t.root, nvml::good());
        assert_eq!(src.start(), StartOutcome::Available);
        check(
            &read(&mut src),
            &[
                (GOOD_01, Some(45.0), "0000:01:00.0"),
                ("GPU 0000:02:00.0", Some(50.0), "0000:02:00.0"),
                ("GPU 0000:0a:00.0", Some(50.0), "0000:0a:00.0"),
                ("GPU 0001:00:00.0", Some(50.0), "0001:00:00.0"),
            ],
        );
        assert_eq!(
            devices(&fake.log()),
            [
                "0000:01:00.0",
                "0000:02:00.0",
                "0000:0a:00.0",
                "0001:00:00.0"
            ]
        );
    });
}

/// Ten awake GPUs, with the script `nvml::left`: for the first six, NVML
/// gives an empty name, a name of spaces only, 0 °C, or an error from
/// the lookup, the name or the temperature call. Expects those six to be
/// left out, the seventh kept with its name trimmed and 1 °C, the last
/// three read normally, and NVML opened once at the start of the read,
/// closed once at its end, and asked for every GPU.
#[test]
fn c_src_nvidia_5() {
    puron_testkit::limit("C-src.nvidia-5", LIMIT, || {
        let t = sys::build("pci-status");
        for i in 1..=10 {
            status(&t, &format!("0000:{i:02x}:00.0"), "active\n");
        }
        let (mut src, fake) = adapter(&t.root, nvml::left());
        check(
            &read(&mut src),
            &[
                ("NVIDIA RTX A2000", Some(1.0), "0000:07:00.0"),
                ("GPU 0000:08:00.0", Some(50.0), "0000:08:00.0"),
                ("GPU 0000:09:00.0", Some(50.0), "0000:09:00.0"),
                ("GPU 0000:0a:00.0", Some(50.0), "0000:0a:00.0"),
            ],
        );
        let log = fake.log();
        assert_eq!(log.first(), Some(&open()), "log {log:?}");
        assert_eq!(log.last(), Some(&Call::Close), "log {log:?}");
        let count = |c: &Call| log.iter().filter(|x| *x == c).count();
        assert_eq!((count(&open()), count(&Call::Close)), (1, 1), "{log:?}");
        assert_eq!(devices(&log).len(), 10, "every GPU queried: {log:?}");
    });
}

/// The names shown for suspended GPUs, with two GPUs `A` (0 °C in NVML)
/// and `B` (50 °C). Both suspended: the default names "NVIDIA GPU" and
/// "NVIDIA GPU 2". A awake: A is left out for its 0 °C, but its name is
/// stored. A suspended and B awake: A shown as "A" without a value, B at
/// 50 °C. After `stop` and `start`, with both suspended: both shown by
/// their stored names, since the names last as long as the adapter.
#[test]
fn c_src_nvidia_6() {
    puron_testkit::limit("C-src.nvidia-6", LIMIT, || {
        let t = sys::build("pci-two");
        let (mut src, _fake) = adapter(&t.root, nvml::names());
        let (a, b) = ("0000:01:00.0", "0000:02:00.0");
        assert_eq!(src.start(), StartOutcome::Available);
        check(
            &read(&mut src),
            &[("NVIDIA GPU", None, a), ("NVIDIA GPU 2", None, b)],
        );
        status(&t, a, "active\n");
        check(&read(&mut src), &[("NVIDIA GPU 2", None, b)]);
        status(&t, a, "suspended\n");
        status(&t, b, "active\n");
        check(&read(&mut src), &[("A", None, a), ("B", Some(50.0), b)]);
        src.stop();
        assert_eq!(src.start(), StartOutcome::Available);
        status(&t, b, "suspended\n");
        check(&read(&mut src), &[("A", None, a), ("B", None, b)]);
    });
}

/// NVML that cannot be loaded or whose init fails. Expects `start` to
/// report available, since it does not open NVML, and the read with an
/// awake GPU to report unavailable. With the GPU suspended and NVML
/// missing, expects the GPU without a value and no NVML call. With NVML
/// missing at the first open only, expects the first read unavailable
/// and the second read to succeed: each read opens NVML anew.
#[test]
fn c_src_nvidia_7() {
    puron_testkit::limit("C-src.nvidia-7", LIMIT, || {
        let t = sys::build("pci-hybrid");
        for script in [nvml::missing(), nvml::initfail()] {
            let (mut src, _) = adapter(&t.root, script);
            assert_eq!(src.start(), StartOutcome::Available);
            assert_eq!(outcome(&mut src), ReadOutcome::Unavailable);
        }
        status(&t, "0000:01:00.0", "suspended\n");
        let (mut src, fake) = adapter(&t.root, nvml::missing());
        check(&read(&mut src), &[("NVIDIA GPU", None, "0000:01:00.0")]);
        assert_eq!(fake.log(), []);
        status(&t, "0000:01:00.0", "active\n");
        let (mut src, _) = adapter(&t.root, nvml::flaky());
        assert_eq!(outcome(&mut src), ReadOutcome::Unavailable);
        check(&read(&mut src), &[(GOOD_01, Some(45.0), "0000:01:00.0")]);
    });
}

/// A GPU added to the tree after the first read, then the first GPU
/// removed. Expects each read to list the devices again: the new GPU
/// appears in the second read, and the removed one is gone in the third.
#[test]
fn c_src_nvidia_8() {
    puron_testkit::limit("C-src.nvidia-8", LIMIT, || {
        let t = sys::build("pci-hybrid");
        let (mut src, _) = adapter(&t.root, nvml::good());
        assert_eq!(src.start(), StartOutcome::Available);
        let first = (GOOD_01, Some(45.0), "0000:01:00.0");
        let second = ("GPU 0000:02:00.0", Some(50.0), "0000:02:00.0");
        check(&read(&mut src), &[first]);
        let dir = t.path("bus/pci/devices/0000:02:00.0");
        std::fs::create_dir_all(dir.join("power")).unwrap();
        std::fs::write(dir.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(dir.join("class"), "0x030000\n").unwrap();
        std::fs::write(dir.join("power/runtime_status"), "active\n").unwrap();
        check(&read(&mut src), &[first, second]);
        std::fs::remove_dir_all(t.path("bus/pci/devices/0000:01:00.0")).unwrap();
        check(&read(&mut src), &[second]);
    });
}

/// The hybrid-graphics tree made read-only, so that any write fails.
/// Expects
/// three reads to give the NVIDIA reading, the tree afterwards to equal
/// an untouched copy, named pipes included (so no `config` was opened),
/// and each read's NVML calls to be exactly open, the three calls for
/// the GPU, close. Root may write to read-only files, so the test needs a
/// user who is not root.
#[test]
fn c_src_nvidia_9() {
    puron_testkit::limit("C-src.nvidia-9", LIMIT, || {
        puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
        let t = sys::build("pci-hybrid");
        let (ro, copy) = t.copies();
        let (mut src, fake) = adapter(&ro, nvml::good());
        assert_eq!(src.start(), StartOutcome::Available);
        for _ in 0..3 {
            check(&read(&mut src), &[(GOOD_01, Some(45.0), "0000:01:00.0")]);
        }
        src.stop();
        assert_eq!(sys::compare(&ro, &copy), (0, 0), "diff_rc, pipes_rc");
        let mut one = vec![open()];
        one.extend(queried("0000:01:00.0"));
        one.push(Call::Close);
        assert_eq!(fake.log(), [one.clone(), one.clone(), one].concat());
    });
}

/// Devices that must be skipped: an unreadable `vendor`, a malformed
/// `vendor`, no `class`, a `class` with too few digits, and directory
/// names that are not PCI addresses (`foo`, `0000:05:00`). Expects only
/// the one sound GPU, `0000:06:00.0`. Then three trees without an NVIDIA
/// GPU: no device directory, only Intel devices, and an unreadable
/// device directory; expects an empty list of readings and no NVML call
/// for each. Root may read unreadable files, so the test needs a user
/// who is not root.
#[test]
fn c_src_nvidia_10() {
    puron_testkit::limit("C-src.nvidia-10", LIMIT, || {
        puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
        let t = sys::build("pci-bad");
        let (mut src, _) = adapter(&t.root, nvml::good());
        check(
            &read(&mut src),
            &[("GPU 0000:06:00.0", Some(50.0), "0000:06:00.0")],
        );
        for name in ["pci-none-a", "pci-none-b", "pci-none-c"] {
            let t = sys::build(name);
            let (mut src, fake) = adapter(&t.root, nvml::good());
            assert_eq!(outcome(&mut src), ReadOutcome::Readings(vec![]), "{name}");
            assert_eq!(fake.log(), [], "{name}");
        }
    });
}

/// The number of lines of `file` that hold `pattern` (`grep -c`, basic
/// regular expression).
fn grep_count(pattern: &str, file: &Path) -> usize {
    let out = Command::new("grep")
        .args(["-c", "--", pattern])
        .arg(file)
        .output()
        .expect("run grep");
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse()
        .unwrap_or_else(|_| panic!("grep -c {pattern}: {text:?}"))
}

/// Expects the adapter that the registry builds for production to use
/// `/sys` and the nvml-wrapper interface. Then checks the source of
/// `nvml.rs` with `grep`: NVML is started only by one plain
/// `Nvml::init()`, a GPU is looked up only by PCI bus ID, and the only
/// temperature call asks for the GPU core sensor.
#[test]
fn c_src_nvidia_11() {
    puron_testkit::limit("C-src.nvidia-11", STATIC_LIMIT, || {
        let src = registry::nvidia();
        assert_eq!(src.root(), Path::new("/sys"));
        assert_eq!(src.interface().kind(), "nvml-wrapper");
        let file = workspace_root().join("crates/puron-engine/src/nvml.rs");
        // Exactly one NVML start, the plain init without flags, and no
        // other way to start it.
        assert_eq!(grep_count("Nvml::init()", &file), 1, "plain init");
        for start in ["init_with_flags", "Nvml::builder", "nvmlInit"] {
            assert_eq!(grep_count(start, &file), 0, "{start}");
        }
        // The one lookup of a GPU is by PCI bus ID; no other lookup.
        assert_eq!(grep_count("device_by_pci_bus_id", &file), 1, "lookup");
        for lookup in [
            "device_by_index",
            "device_by_uuid",
            "device_by_serial",
            "device_count",
        ] {
            assert_eq!(grep_count(lookup, &file), 0, "{lookup}");
        }
        assert_eq!(grep_count("\\.temperature(", &file), 1, "temperature calls");
        assert_eq!(
            grep_count("\\.temperature(TemperatureSensor::Gpu)", &file),
            1,
            "core sensor"
        );
    });
}

/// The NVML header's notice asks that it be included in the internal
/// comments of code that uses it. Expects each of the 27 non-empty lines
/// of `fixtures/nvml/notice.txt` to appear as written in `nvidia.rs`.
#[test]
fn c_src_nvidia_12() {
    puron_testkit::limit("C-src.nvidia-12", STATIC_LIMIT, || {
        let notice = puron_testkit::fixture("nvml", "notice.txt");
        let text = std::fs::read_to_string(&notice).expect("read notice.txt");
        let source = workspace_root().join("crates/puron-engine/src/nvidia.rs");
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 27, "non-empty lines of the notice");
        for line in lines {
            let status = Command::new("grep")
                .args(["-q", "-F", "--", line])
                .arg(&source)
                .status()
                .expect("run grep");
            assert!(status.success(), "not in nvidia.rs: {line:?}");
        }
    });
}
