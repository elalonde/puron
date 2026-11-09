//! Tests of the NVMe adapter (`puron_engine::nvme`), through its `start`,
//! `read` and `stop`. Each test builds a fake sysfs tree from a fixture
//! in `fixtures/sys/` (see `sys::build`) and gives it to the adapter as
//! its sysfs root.

use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::nvme::Nvme;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::{cfg, fixture, sys, temp_dir, umd};

/// The time limit of each test here, in seconds.
const LIMIT: u64 = 5;

/// One expected reading: its name, value and key. Its position is its
/// index in the list of expected readings.
type Exp = (&'static str, Option<f64>, &'static str);

/// The readings of the `nvme-capture` tree, which holds the files of one
/// real drive: a controller with `temp2` and `temp3`, so two readings.
const CAPTURE: [Exp; 2] = [
    ("Example SSD 1TB S1", Some(37.85), "0000:01:00.0/temp2"),
    ("Example SSD 1TB S2", Some(43.85), "0000:01:00.0/temp3"),
];

/// Reads once with a stop descriptor that is never raised, and returns
/// the readings. Panics if the read gives anything else.
fn read(src: &mut Nvme) -> Vec<RawReading> {
    let stop = umd::stop_fd();
    match src.read(stop.as_fd()) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Checks every field of `readings`: name, value and key as in `exp`, in
/// that order; provider `NvmeCli`; positions 0, 1, 2 and so on; kind
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
        assert_eq!(r.provider, Provider::NvmeCli);
        assert_eq!(r.position, i as u32, "position of {:?}", r.name);
        assert_eq!(r.kind, Some(Kind::Temperature));
        assert!(r.name.is_some());
        assert_eq!(
            (&r.chip, &r.device, &r.item, &r.unit),
            (&None, &None, &None, &None)
        );
    }
}

/// An adapter on the sysfs root `root`, started; panics unless the start
/// reports it available.
fn started(root: &Path) -> Nvme {
    let mut src = Nvme::new(root);
    assert_eq!(src.start(), StartOutcome::Available);
    src
}

/// The exit code of `cmp a b`: 0 when the two files are equal.
fn cmp(a: &Path, b: &Path) -> i32 {
    Command::new("cmp")
        .arg(a)
        .arg(b)
        .status()
        .expect("run cmp")
        .code()
        .unwrap_or(-1)
}

/// The tree of a real drive, with the attribute files and `nvme list`
/// output captured from it. Expects the tree's `model` file to equal the
/// captured one byte for byte, the read to give `CAPTURE`, and the first
/// reading's name without its " S1" to equal the `ModelNumber` that
/// `nvme list` reported for the drive. Needs `jq` on the host.
#[test]
fn c_src_nvme_1() {
    puron_testkit::limit("C-src.nvme-1", LIMIT, || {
        puron_testkit::host_need("jq", "jq --version");
        let t = sys::build("nvme-capture");
        let capture = fixture("capture", "nvme");
        let model = t.path("class/nvme/nvme0/model");
        assert_eq!(
            cmp(&model, &capture.join("nvme0-model.raw")),
            0,
            "cmp model"
        );
        let mut src = started(&t.root);
        let r = read(&mut src);
        check(&r, &CAPTURE);
        // Compare the first name, without " S1", with the ModelNumber in
        // the captured `nvme list` output (`nvme-list.json`), each written
        // as one line.
        let out = Command::new("jq")
            .args(["-r", ".Devices[0].ModelNumber"])
            .arg(capture.join("nvme-list.json"))
            .output()
            .expect("run jq");
        assert!(out.status.success(), "jq");
        let dir = temp_dir("nvme-1");
        let name = r[0].name.as_deref().unwrap();
        let cut = &name.as_bytes()[..name.len() - 3];
        std::fs::write(dir.join("name"), [cut, b"\n"].concat()).unwrap();
        std::fs::write(dir.join("jq"), &out.stdout).unwrap();
        assert_eq!(cmp(&dir.join("name"), &dir.join("jq")), 0, "cmp jq");
        let _ = std::fs::remove_dir_all(&dir);
        src.stop();
    });
}

/// Five controllers whose `model` files hold "A  B" padded with spaces,
/// " A" padded, only spaces, "A" with no newline, and "A", byte 0xFF,
/// "B" padded. Expects the names "A  B", " A", "", "A" and "A\u{FFFD}B":
/// trailing spaces and the newline are removed, inner and leading spaces
/// kept, and an invalid byte becomes U+FFFD.
#[test]
fn c_src_nvme_2() {
    puron_testkit::limit("C-src.nvme-2", LIMIT, || {
        let t = sys::build("nvme-trim");
        let mut src = started(&t.root);
        let r = read(&mut src);
        let names: Vec<&str> = r.iter().map(|r| r.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["A  B", " A", "", "A", "A\u{FFFD}B"]);
        src.stop();
    });
}

/// Two controllers without `temp3_input`, the second with a `temp2_input`.
/// Expects one reading each, named by the model alone and read from
/// `temp1`: `temp2` alone does not make two readings.
#[test]
fn c_src_nvme_3() {
    puron_testkit::limit("C-src.nvme-3", LIMIT, || {
        let t = sys::build("nvme-single");
        let mut src = started(&t.root);
        check(
            &read(&mut src),
            &[
                ("Model One", Some(40.85), "0000:00:00.0/temp1"),
                ("Model Two", Some(41.0), "0000:01:00.0/temp1"),
            ],
        );
        src.stop();
    });
}

/// A controller with `temp1_input` and `temp3_input` but no
/// `temp2_input`. Expects two readings, "S1" without a value and "S2"
/// from `temp3`: the presence of `temp3` decides.
#[test]
fn c_src_nvme_4() {
    puron_testkit::limit("C-src.nvme-4", LIMIT, || {
        let t = sys::build("nvme-s2only");
        let mut src = started(&t.root);
        check(
            &read(&mut src),
            &[
                ("Model S S1", None, "0000:00:00.0/temp2"),
                ("Model S S2", Some(45.0), "0000:00:00.0/temp3"),
            ],
        );
        src.stop();
    });
}

/// Controllers created out of order (`nvme10`, `nvme2`, `nvme0`, ...),
/// some with namespace directories such as `nvme2n1`, together with
/// controllers that lack a hwmon directory (`nvme3`), a model (`nvme4`)
/// or an address (`nvme5`), and the entry `nvme-fabrics`. Expects one
/// reading for each complete controller, in the order of the controller
/// number: 0, 1, 2, 10.
#[test]
fn c_src_nvme_5() {
    puron_testkit::limit("C-src.nvme-5", LIMIT, || {
        let t = sys::build("nvme-multi");
        let mut src = started(&t.root);
        check(
            &read(&mut src),
            &[
                ("M0", Some(40.0), "0000:00:00.0/temp1"),
                ("M1", Some(40.0), "0000:01:00.0/temp1"),
                ("M2", Some(40.0), "0000:02:00.0/temp1"),
                ("M10", Some(40.0), "0000:10:00.0/temp1"),
            ],
        );
        src.stop();
    });
}

/// Two trees without a usable controller: one without `class/nvme/` at
/// all, and one with only a directory whose name has no number
/// (`nvme-fabrics`) and a controller without a hwmon directory. Expects
/// `start` to report unavailable for both.
#[test]
fn c_src_nvme_6() {
    puron_testkit::limit("C-src.nvme-6", LIMIT, || {
        for name in ["nvme-none-a", "nvme-none-b"] {
            let t = sys::build(name);
            let mut src = Nvme::new(&t.root);
            assert_eq!(src.start(), StartOutcome::Unavailable, "{name}");
            src.stop();
        }
    });
}

/// After a start on the `nvme-capture` tree, a new controller appears,
/// the drive's model changes, its `temp3_input` is removed and its
/// `temp1_input` is written. Expects the read to give the controllers,
/// names and sensors found at start: the same two readings, with "S2"
/// now without a value, and no reading for the new controller.
#[test]
fn c_src_nvme_7() {
    puron_testkit::limit("C-src.nvme-7", LIMIT, || {
        let t = sys::build("nvme-capture");
        let mut src = started(&t.root);
        let n1 = t.path("class/nvme/nvme1");
        cfg::write(&n1.join("model"), b"New\n");
        cfg::write(&n1.join("address"), b"0000:02:00.0\n");
        cfg::write(&n1.join("hwmon3/name"), b"nvme\n");
        cfg::write(&n1.join("hwmon3/temp1_input"), b"30000\n");
        cfg::write(&t.path("class/nvme/nvme0/model"), b"Other\n");
        let h = t.path("class/nvme/nvme0/hwmon2");
        std::fs::remove_file(h.join("temp3_input")).unwrap();
        cfg::write(&h.join("temp1_input"), b"50000\n");
        check(
            &read(&mut src),
            &[
                ("Example SSD 1TB S1", Some(37.85), "0000:01:00.0/temp2"),
                ("Example SSD 1TB S2", None, "0000:01:00.0/temp3"),
            ],
        );
        src.stop();
    });
}

/// Temperatures that do not parse. Expects -5 and 0 to be read first.
/// Then, with `temp2_input` removed and `temp3_input` holding in turn
/// "garbage", nothing, a trailing space, a leading "+" and an exponent,
/// expects both readings without values, and the read itself still to
/// succeed. With the files restored, the values are read again.
#[test]
fn c_src_nvme_8() {
    puron_testkit::limit("C-src.nvme-8", LIMIT, || {
        let t = sys::build("nvme-values");
        let mut src = started(&t.root);
        let good = [
            ("V S1", Some(-5.0), "0000:00:00.0/temp2"),
            ("V S2", Some(0.0), "0000:00:00.0/temp3"),
        ];
        let none = [
            ("V S1", None, "0000:00:00.0/temp2"),
            ("V S2", None, "0000:00:00.0/temp3"),
        ];
        check(&read(&mut src), &good);
        let h = t.path("class/nvme/nvme0/hwmon1");
        let (t2, t3) = (h.join("temp2_input"), h.join("temp3_input"));
        let keep2 = std::fs::read(&t2).unwrap();
        let keep3 = std::fs::read(&t3).unwrap();
        std::fs::remove_file(&t2).unwrap();
        for bad in [&b"garbage"[..], b"", b"37850 ", b"+37850", b"3.7e4"] {
            cfg::write(&t3, bad);
            check(&read(&mut src), &none);
        }
        cfg::write(&t2, &keep2);
        cfg::write(&t3, &keep3);
        check(&read(&mut src), &good);
        src.stop();
    });
}

/// Expects millidegrees to be divided by 1000: 37850 and 43850 give
/// 37.85 and 43.85, -5000 and 0 give -5 and 0.
#[test]
fn c_src_nvme_9() {
    puron_testkit::limit("C-src.nvme-9", LIMIT, || {
        let values = |name: &str| -> Vec<Option<f64>> {
            let t = sys::build(name);
            let mut src = started(&t.root);
            let v = read(&mut src).iter().map(|r| r.value).collect();
            src.stop();
            v
        };
        assert_eq!(values("nvme-capture"), [Some(37.85), Some(43.85)]);
        assert_eq!(values("nvme-values"), [Some(-5.0), Some(0.0)]);
    });
}

/// The `nvme-capture` tree made read-only, so that any write fails.
/// Expects three reads to give `CAPTURE`, and the tree afterwards to
/// equal an untouched copy. Root may write to read-only files, so the
/// test needs a user who is not root.
#[test]
fn c_src_nvme_10() {
    puron_testkit::limit("C-src.nvme-10", LIMIT, || {
        puron_testkit::host_need("a test user who is not root", "test \"$(id -u)\" -ne 0");
        let t = sys::build("nvme-capture");
        let (ro, copy) = t.copies();
        let mut src = started(&ro);
        for _ in 0..3 {
            check(&read(&mut src), &CAPTURE);
        }
        src.stop();
        assert_eq!(sys::compare(&ro, &copy), (0, 0), "diff_rc, pipes_rc");
    });
}

/// Expects the adapter that the registry builds for production to use
/// `/sys` as its sysfs root.
#[test]
fn c_src_nvme_11() {
    puron_testkit::limit("C-src.nvme-11", LIMIT, || {
        assert_eq!(registry::nvme().root(), Path::new("/sys"));
    });
}
