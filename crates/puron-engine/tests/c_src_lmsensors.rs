//! Tests of the lm-sensors source, each in its own `umockdev-run`
//! through the re-executed child.

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use puron_core::rows::{Kind, Provider, RawReading};
use puron_engine::diag;
use puron_engine::lmsensors::LmSensors;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::{fixture, jrn, temp_dir, umd, workspace_root};

const LIMIT: u64 = 10;

/// The nvme hwmon directory of the captured testbed, under
/// `$UMOCKDEV_DIR`.
const NVME: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0/nvme/nvme0/hwmon2";

/// One expected reading: chip, feature, kind, name, value.
type Exp = (&'static str, &'static str, Kind, &'static str, Option<f64>);

use Kind::{Fan, Power, Temperature as T, Voltage};

/// The 12 readings of the captured testbed, each chip in feature order.
const CAPTURE: [Exp; 12] = [
    ("coretemp-isa-0000", "temp1", T, "CPU", Some(73.0)),
    ("coretemp-isa-0000", "temp2", T, "Core 0", Some(65.0)),
    ("coretemp-isa-0000", "temp3", T, "Core 1", Some(71.0)),
    ("coretemp-isa-0000", "temp4", T, "Core 2", Some(64.0)),
    ("coretemp-isa-0000", "temp5", T, "Core 3", Some(72.0)),
    ("nvme-pci-0100", "temp1", T, "SSD", Some(37.85)),
    ("nvme-pci-0100", "temp2", T, "Sensor 1", Some(37.85)),
    ("nvme-pci-0100", "temp3", T, "Sensor 2", Some(43.85)),
    ("iwlwifi_1-virtual-0", "temp1", T, "WiFi", Some(33.0)),
    ("system76_acpi-isa-0000", "fan1", Fan, "CPU fan", Some(0.0)),
    ("system76_acpi-isa-0000", "temp1", T, "CPU temp", Some(67.0)),
    ("BAT0-acpi-0", "in0", Voltage, "in0", Some(8.494)),
];

fn dir(name: &str) -> PathBuf {
    fixture("umd", name)
}

/// Runs `body` in the child on fixture `name` (its testbed and
/// `sensors.conf`).
fn on(test: &str, name: &str, body: impl FnOnce()) {
    let d = dir(name);
    umd::run(
        test,
        &d.join("testbed.umockdev"),
        &d.join("sensors.conf"),
        body,
    );
}

fn read(src: &mut LmSensors) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

fn readings(src: &mut LmSensors) -> Vec<RawReading> {
    match read(src) {
        ReadOutcome::Readings(v) => v,
        other => panic!("read: {other:?}"),
    }
}

/// Compares readings with `exp` as a multiset, order checked only within
/// one chip, and checks the fields every reading has.
fn same(got: &[RawReading], exp: &[Exp]) {
    for r in got {
        assert_eq!(r.provider, Provider::LmSensors, "{r:?}");
        assert_eq!(
            (&r.device, &r.item, &r.unit),
            (&None, &None, &None),
            "{r:?}"
        );
    }
    let mut chips: Vec<&str> = exp.iter().map(|e| e.0).collect();
    chips.extend(got.iter().filter_map(|r| r.chip.as_deref()));
    chips.sort();
    chips.dedup();
    for chip in chips {
        let g: Vec<(String, Option<Kind>, Option<String>, Option<f64>)> = got
            .iter()
            .filter(|r| r.chip.as_deref() == Some(chip))
            .map(|r| (r.key.clone(), r.kind, r.name.clone(), r.value))
            .collect();
        let e: Vec<(String, Option<Kind>, Option<String>, Option<f64>)> = exp
            .iter()
            .filter(|e| e.0 == chip)
            .map(|e| {
                (
                    format!("{}/{}", e.0, e.1),
                    Some(e.2),
                    Some(e.3.to_string()),
                    e.4,
                )
            })
            .collect();
        assert_eq!(g, e, "chip {chip}");
    }
    assert_eq!(got.len(), exp.len(), "number of readings");
}

fn positions_in_order(got: &[RawReading]) {
    let p: Vec<u32> = got.iter().map(|r| r.position).collect();
    let want: Vec<u32> = (0..got.len() as u32).collect();
    assert_eq!(p, want, "positions in report order");
}

/// A fresh copy of `file` in a temporary directory, writable.
fn copy(file: &Path, tag: &str) -> PathBuf {
    let to = temp_dir(tag).join("sensors.conf");
    std::fs::copy(file, &to).expect("copy configuration");
    to
}

#[test]
fn c_src_lmsensors_1() {
    puron_testkit::limit("C-src.lmsensors-1", LIMIT, || {
        on("c_src_lmsensors_1", "capture", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            assert_eq!(s.start(), StartOutcome::Available);
            same(&readings(&mut s), &CAPTURE);
            s.stop();
        })
    });
}

#[test]
fn c_src_lmsensors_2() {
    puron_testkit::limit("C-src.lmsensors-2", LIMIT, || {
        on("c_src_lmsensors_2", "nolabel", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            let mut exp = CAPTURE;
            for e in exp.iter_mut() {
                e.3 = match e.3 {
                    "CPU" => "Package id 0",
                    "SSD" => "Composite",
                    "WiFi" => "temp1",
                    n => n,
                };
            }
            same(&readings(&mut s), &exp);
        })
    });
}

#[test]
fn c_src_lmsensors_3() {
    puron_testkit::limit("C-src.lmsensors-3", LIMIT, || {
        on("c_src_lmsensors_3", "kinds", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            let got = readings(&mut s);
            let c = "puron_kinds-isa-0000";
            let exp = [
                (c, "in0", Voltage, "in0", Some(12.0)),
                (c, "fan1", Fan, "fan1", Some(1200.0)),
                (c, "temp1", T, "temp1", Some(41.0)),
                (c, "power1", Power, "power1", Some(12.5)),
            ];
            same(&got, &exp);
        })
    });
}

#[test]
fn c_src_lmsensors_4() {
    puron_testkit::limit("C-src.lmsensors-4", LIMIT, || {
        on("c_src_lmsensors_4", "samelabel", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            let got = readings(&mut s);
            let c = "puron_same-isa-0000";
            same(
                &got,
                &[
                    (c, "temp1", T, "Core 0", Some(50.0)),
                    (c, "temp2", T, "Core 0", Some(60.0)),
                ],
            );
            positions_in_order(&got);
        })
    });
}

#[test]
fn c_src_lmsensors_5() {
    puron_testkit::limit("C-src.lmsensors-5", LIMIT, || {
        on("c_src_lmsensors_5", "capture", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            same(&readings(&mut s), &CAPTURE);
            umd::write(&format!("{NVME}/temp2_input"), b"garbage\n");
            let mut exp = CAPTURE;
            exp[6].4 = None;
            same(&readings(&mut s), &exp);
        })
    });
}

#[test]
fn c_src_lmsensors_6() {
    puron_testkit::limit("C-src.lmsensors-6", LIMIT, || {
        on("c_src_lmsensors_6", "compute", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            let c = "puron_compute-isa-0000";
            same(
                &readings(&mut s),
                &[
                    (c, "temp1", T, "temp1", Some(0.0625)),
                    (c, "temp2", T, "temp2", Some(0.1875)),
                    (c, "temp3", T, "temp3", Some(1.0 / 3.0)),
                    (c, "temp4", T, "temp4", Some(-0.0625)),
                ],
            );
        })
    });
}

#[test]
fn c_src_lmsensors_7() {
    puron_testkit::limit("C-src.lmsensors-7", LIMIT, || {
        on("c_src_lmsensors_7", "change", || {
            let conf = copy(&umd::conf(), "c-src-lmsensors-7");
            let mut s = LmSensors::new(Some(conf.clone()));
            same(&readings(&mut s), &CAPTURE);

            umd::write(&format!("{NVME}/temp1_input"), b"52345\n");
            let text = std::fs::read_to_string(&conf).unwrap();
            let changed = text.replace(r#"label temp1 "SSD""#, r#"label temp1 "Disk""#);
            assert_ne!(text, changed, "label SSD in the configuration");
            std::fs::write(&conf, changed).unwrap();
            let st = Command::new("sh")
                .arg(fixture("umd", "addchip.sh"))
                .status()
                .expect("run addchip.sh");
            assert!(st.success(), "addchip.sh: {st}");

            let mut exp: Vec<Exp> = CAPTURE.to_vec();
            exp[5] = ("nvme-pci-0100", "temp1", T, "Disk", Some(52.345));
            exp.push(("puron_new-isa-0000", "temp1", T, "temp1", Some(30.0)));
            same(&readings(&mut s), &exp);
        })
    });
}

#[test]
fn c_src_lmsensors_8() {
    puron_testkit::limit("C-src.lmsensors-8", LIMIT, || {
        let bad = std::fs::read(dir("badconf").join("sensors.conf")).unwrap();
        assert_eq!(
            bad, b"bogus",
            "X-umd-badconf: the 5 bytes `bogus`, no newline"
        );
        on("c_src_lmsensors_8", "badconf", || {
            let conf = copy(&umd::conf(), "c-src-lmsensors-8");
            // The test journal socket is bound and diag is on from before
            // Start: the config-error event carries PURON_FILE, present
            // and empty with a stream configuration.
            let jrn = jrn::Journal::new("c-src-lmsensors-8-jrn");
            diag::set_socket(&jrn.path);
            diag::switch(true);
            let mut s = LmSensors::new(Some(conf.clone()));
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(read(&mut s), ReadOutcome::Unavailable);
            let got = jrn.wait(1, Duration::from_secs(5));
            let hit = got.iter().any(|e| {
                jrn::field(e, "PURON_SOURCE") == Some("src.lmsensors")
                    && jrn::field(e, "PURON_EVENT") == Some("config-error")
                    && jrn::field(e, "PURON_FILE").is_some()
            });
            assert!(
                hit,
                "no config-error entry with PURON_FILE by the first read's return:\n{}",
                jrn::dump(&got)
            );
            std::fs::copy(dir("badconf").join("sensors.conf.good"), &conf).unwrap();
            same(&readings(&mut s), &CAPTURE);
        })
    });
}

#[test]
fn c_src_lmsensors_9() {
    puron_testkit::limit("C-src.lmsensors-9", LIMIT, || {
        on("c_src_lmsensors_9", "capture", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            assert_eq!(s.start(), StartOutcome::Available);
            for _ in 0..3 {
                same(&readings(&mut s), &CAPTURE);
            }
            s.stop();
            assert_eq!(s.start(), StartOutcome::Available);
            same(&readings(&mut s), &CAPTURE);
        })
    });
}

fn sh(script: &str, args: &[&Path]) -> Option<i32> {
    Command::new("sh")
        .args(["-c", script, "sh"])
        .args(args)
        .status()
        .expect("run sh")
        .code()
}

#[test]
fn c_src_lmsensors_10() {
    puron_testkit::limit("C-src.lmsensors-10", LIMIT, || {
        on("c_src_lmsensors_10", "set", || {
            let tree = umd::dir();
            let copy = temp_dir("c-src-lmsensors-10").join("copy");
            assert_eq!(sh(r#"cp -a "$1" "$2""#, &[&tree, &copy]), Some(0), "cp -a");

            let mut s = LmSensors::new(Some(umd::conf()));
            assert_eq!(s.start(), StartOutcome::Available);
            for _ in 0..3 {
                let c = "puron_set-isa-0000";
                same(&readings(&mut s), &[(c, "temp1", T, "temp1", Some(40.0))]);
            }
            s.stop();

            // The reads wrote nothing: the testbed matches its copy from
            // before, apart from umockdev's own config and ioctl files.
            let diff = r#"diff -r --no-dereference -x config -x ioctl "$1" "$2""#;
            assert_eq!(sh(diff, &[&tree, &copy]), Some(0), "diff_rc");
            let src = workspace_root().join("crates/puron-engine/src/lmsensors.rs");
            let grep = r#"grep -E 'set_(raw_)?value|do_chip_sets' "$1""#;
            assert_eq!(sh(grep, &[&src]), Some(1), "grep of the adapter source");
        })
    });
}

#[test]
fn c_src_lmsensors_11() {
    puron_testkit::limit("C-src.lmsensors-11", LIMIT, || {
        on("c_src_lmsensors_11", "capture", || {
            let mut s = LmSensors::new(Some(umd::conf()));
            let got = readings(&mut s);
            positions_in_order(&got);
            let names = |chip: &str| -> Vec<String> {
                got.iter()
                    .filter(|r| r.chip.as_deref() == Some(chip))
                    .map(|r| r.name.clone().unwrap_or_default())
                    .collect()
            };
            assert_eq!(
                names("coretemp-isa-0000"),
                ["CPU", "Core 0", "Core 1", "Core 2", "Core 3"]
            );
            assert_eq!(names("nvme-pci-0100"), ["SSD", "Sensor 1", "Sensor 2"]);
        })
    });
}

#[test]
fn c_src_lmsensors_12() {
    puron_testkit::limit("C-src.lmsensors-12", LIMIT, || {
        on("c_src_lmsensors_12", "bytes", || {
            umd::write(
                "sys/devices/platform/puron_bytes/hwmon/hwmon0/temp1_label",
                b"Core\xff 0\n",
            );
            let mut s = LmSensors::new(Some(umd::conf()));
            let c = "puron_bytes-isa-0000";
            same(
                &readings(&mut s),
                &[(c, "temp1", T, "Core\u{FFFD} 0", Some(45.0))],
            );
        })
    });
}

#[test]
fn c_src_lmsensors_13() {
    puron_testkit::limit("C-src.lmsensors-13", LIMIT, || {
        // The accessor shows a test configuration, so None below is the
        // production value and not the accessor's.
        let test = Path::new("/nonexistent/sensors.conf");
        assert_eq!(LmSensors::new(Some(test.into())).config(), Some(test));
        assert_eq!(registry::lm_sensors().config(), None);
    });
}
