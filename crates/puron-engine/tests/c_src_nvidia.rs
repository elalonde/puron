//! Tests of the NVIDIA source, through `start`, `read` and `stop` with a fake
//! PCI sysfs tree as the sysfs root and a fake NVML interface, except
//! the static checks in `c_src_nvidia_11` and `c_src_nvidia_12`.

use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::nvidia::Nvidia;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::nvml::{self, Call, Fake, Script};
use puron_testkit::{sys, umd, workspace_root};

/// Test time limits: 5 s; the static checks 120 s.
const LIMIT: u64 = 5;
const STATIC_LIMIT: u64 = 120;

const GOOD_01: &str = "NVIDIA GeForce RTX 3060";

/// One expected reading: name, value, key; the Position is its index.
type Exp<'a> = (&'a str, Option<f64>, &'a str);

fn adapter(root: &Path, script: Script) -> (Nvidia, Fake) {
    let fake = Fake::new(script);
    (Nvidia::new(root, Box::new(fake.clone())), fake)
}

fn outcome(src: &mut Nvidia) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

fn read(src: &mut Nvidia) -> Vec<RawReading> {
    match outcome(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Every field of the readings: NVIDIA, Positions 0, 1, ..., kind
/// temperature, no chip, device, item or unit, and the expected name,
/// value and key.
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

fn status(t: &sys::Sys, pci: &str, text: &str) {
    let path = t.path(&format!("bus/pci/devices/{pci}/power/runtime_status"));
    std::fs::write(path, text).expect("write runtime_status");
}

fn open() -> Call {
    Call::Open
}

/// The calls of one queried GPU.
fn queried(pci: &str) -> [Call; 3] {
    [
        Call::Device(pci.into()),
        Call::Name(pci.into()),
        Call::Temperature(pci.into()),
    ]
}

fn devices(log: &[Call]) -> Vec<String> {
    log.iter()
        .filter_map(|c| match c {
            Call::Device(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

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

/// The good NVML fake names `0000:01:00.0`.
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

#[test]
fn c_src_nvidia_11() {
    puron_testkit::limit("C-src.nvidia-11", STATIC_LIMIT, || {
        let src = registry::nvidia();
        assert_eq!(src.root(), Path::new("/sys"));
        assert_eq!(src.interface().kind(), "nvml-wrapper");
        let file = workspace_root().join("crates/puron-engine/src/nvml.rs");
        // Exactly one NVML start, the plain init (no flag), and no
        // other start.
        assert_eq!(grep_count("Nvml::init()", &file), 1, "plain init");
        for start in ["init_with_flags", "Nvml::builder", "nvmlInit"] {
            assert_eq!(grep_count(start, &file), 0, "{start}");
        }
        // The one handle lookup is by PCI bus ID; no other lookup.
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
