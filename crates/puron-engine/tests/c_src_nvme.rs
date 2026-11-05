//! Tests of the nvme source, through Start, Read and Stop with a fake
//! sysfs tree as the sysfs root.

use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::nvme::Nvme;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::{cfg, fixture, sys, temp_dir, umd};

/// Test time limit: 5 s.
const LIMIT: u64 = 5;

/// One expected reading: name, value, key; the Position is its index.
type Exp = (&'static str, Option<f64>, &'static str);

const CAPTURE: [Exp; 2] = [
    (
        "Samsung SSD 980 PRO 1TB S1",
        Some(37.85),
        "0000:01:00.0/temp2",
    ),
    (
        "Samsung SSD 980 PRO 1TB S2",
        Some(43.85),
        "0000:01:00.0/temp3",
    ),
];

fn read(src: &mut Nvme) -> Vec<RawReading> {
    let stop = umd::stop_fd();
    match src.read(stop.as_fd()) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Every field of the readings: nvme-cli, Positions 0, 1, ..., kind
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

fn started(root: &Path) -> Nvme {
    let mut src = Nvme::new(root);
    assert_eq!(src.start(), StartOutcome::Available);
    src
}

/// The exit code of `cmp a b`.
fn cmp(a: &Path, b: &Path) -> i32 {
    Command::new("cmp")
        .arg(a)
        .arg(b)
        .status()
        .expect("run cmp")
        .code()
        .unwrap_or(-1)
}

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
        // The S1 name without " S1" against nvme-cli's ModelNumber.
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
                (
                    "Samsung SSD 980 PRO 1TB S1",
                    Some(37.85),
                    "0000:01:00.0/temp2",
                ),
                ("Samsung SSD 980 PRO 1TB S2", None, "0000:01:00.0/temp3"),
            ],
        );
        src.stop();
    });
}

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

#[test]
fn c_src_nvme_11() {
    puron_testkit::limit("C-src.nvme-11", LIMIT, || {
        assert_eq!(registry::nvme().root(), Path::new("/sys"));
    });
}
