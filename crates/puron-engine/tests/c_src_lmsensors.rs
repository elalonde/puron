//! Tests of the lm-sensors source, each in its own `umockdev-run`
//! through the re-executed child.

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use puron_core::order;
use puron_core::rows::{self, Class, Kind, Provider, RawReading, ShownClasses};
use puron_engine::diag;
use puron_engine::lmsensors::LmSensors;
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::{fixture, jrn, temp_dir, umd, workspace_root};

const LIMIT: u64 = 10;

/// The nvme hwmon directory of the captured testbed, under
/// `$UMOCKDEV_DIR`.
const NVME: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0/nvme/nvme0/hwmon2";
/// The device that the nvme hwmon's `device` link names, and its parent.
const NVME_DEVICE: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0/nvme/nvme0";
const NVME_PARENT: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0";
/// The device of the battery's hwmon.
const BATTERY: &str = "sys/devices/LNXSYSTM:00/LNXSYBUS:00/PNP0C0A:00/power_supply/BAT0";
/// The device of the CPU's hwmon, and the hwmon.
const CORETEMP: &str = "sys/devices/platform/coretemp.0";
const CORETEMP_HWMON: &str = "sys/devices/platform/coretemp.0/hwmon/hwmon5";

const CHIP_NVME: &str = "nvme-pci-0100";
const CHIP_CORE: &str = "coretemp-isa-0000";
const CHIP_BOARD: &str = "example_acpi-isa-0000";
const CHIP_BATTERY: &str = "BAT0-acpi-0";

/// The CPU's name in the `laptop/cpuinfo` fixture, cleaned.
const CPU: &str = "Example Core E7-1000";

/// One expected reading: chip, feature, kind, name, value.
type Exp = (&'static str, &'static str, Kind, &'static str, Option<f64>);

use Kind::{Fan, Power, Temperature as T, Voltage};

/// The 12 readings of the captured testbed, each chip in feature order.
const CAPTURE: [Exp; 12] = [
    ("coretemp-isa-0000", "temp1", T, "CPU", Some(73.0)),
    (
        "coretemp-isa-0000",
        "temp2",
        T,
        "Example Core E7-1000 Core 0",
        Some(65.0),
    ),
    (
        "coretemp-isa-0000",
        "temp3",
        T,
        "Example Core E7-1000 Core 1",
        Some(71.0),
    ),
    (
        "coretemp-isa-0000",
        "temp4",
        T,
        "Example Core E7-1000 Core 2",
        Some(64.0),
    ),
    (
        "coretemp-isa-0000",
        "temp5",
        T,
        "Example Core E7-1000 Core 3",
        Some(72.0),
    ),
    ("nvme-pci-0100", "temp1", T, "SSD", Some(37.85)),
    ("nvme-pci-0100", "temp2", T, "nvme Sensor 1", Some(37.85)),
    ("nvme-pci-0100", "temp3", T, "nvme Sensor 2", Some(43.85)),
    ("iwlwifi_1-virtual-0", "temp1", T, "WiFi", Some(33.0)),
    (
        "example_acpi-isa-0000",
        "fan1",
        Fan,
        "example_acpi CPU fan",
        Some(0.0),
    ),
    (
        "example_acpi-isa-0000",
        "temp1",
        T,
        "example_acpi CPU temp",
        Some(67.0),
    ),
    ("BAT0-acpi-0", "in0", Voltage, "BAT0 in0", Some(8.494)),
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

/// The CPU list of the `laptop` fixture.
fn cpuinfo() -> PathBuf {
    dir("laptop").join("cpuinfo")
}

/// An adapter on the child's configuration, started.
fn started() -> LmSensors {
    with_cpuinfo(cpuinfo())
}

/// The same with its CPU list in `file`.
fn with_cpuinfo(file: impl Into<PathBuf>) -> LmSensors {
    let mut s = LmSensors::new(Some(umd::conf()), file);
    assert_eq!(s.start(), StartOutcome::Available);
    s
}

/// Stops and starts `src`, which finds the device names again.
fn restart(src: &mut LmSensors) {
    src.stop();
    assert_eq!(src.start(), StartOutcome::Available);
}

/// The names of a read's readings of chip `chip`, in feature order.
fn names(src: &mut LmSensors, chip: &str) -> Vec<String> {
    readings(src)
        .iter()
        .filter(|r| r.chip.as_deref() == Some(chip))
        .map(|r| r.name.clone().unwrap_or_default())
        .collect()
}

/// `path` under the testbed.
fn tb(path: &str) -> PathBuf {
    umd::dir().join(path)
}

/// Child: gives the machine the name file holding `bytes`.
fn machine(bytes: &[u8]) {
    std::fs::create_dir_all(tb("sys/class/dmi/id")).expect("dmi directory");
    umd::write("sys/class/dmi/id/product_name", bytes);
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
            let mut s = LmSensors::new(Some(umd::conf()), cpuinfo());
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
            let mut s = started();
            let mut exp = CAPTURE;
            for e in exp.iter_mut() {
                e.3 = match e.3 {
                    "CPU" => "Example Core E7-1000 Package id 0",
                    "SSD" => "nvme Composite",
                    "WiFi" => "iwlwifi temp1",
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
            let mut s = started();
            let got = readings(&mut s);
            let c = "puron_kinds-isa-0000";
            let exp = [
                (c, "in0", Voltage, "puron_kinds in0", Some(12.0)),
                (c, "fan1", Fan, "puron_kinds fan1", Some(1200.0)),
                (c, "temp1", T, "puron_kinds temp1", Some(41.0)),
                (c, "power1", Power, "puron_kinds power1", Some(12.5)),
            ];
            same(&got, &exp);
        })
    });
}

#[test]
fn c_src_lmsensors_4() {
    puron_testkit::limit("C-src.lmsensors-4", LIMIT, || {
        on("c_src_lmsensors_4", "samelabel", || {
            let mut s = started();
            let got = readings(&mut s);
            let c = "puron_same-isa-0000";
            same(
                &got,
                &[
                    (c, "temp1", T, "puron_same Core 0", Some(50.0)),
                    (c, "temp2", T, "puron_same Core 0", Some(60.0)),
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
            let mut s = started();
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
            let mut s = started();
            let c = "puron_compute-isa-0000";
            same(
                &readings(&mut s),
                &[
                    (c, "temp1", T, "puron_compute temp1", Some(0.0625)),
                    (c, "temp2", T, "puron_compute temp2", Some(0.1875)),
                    (c, "temp3", T, "puron_compute temp3", Some(1.0 / 3.0)),
                    (c, "temp4", T, "puron_compute temp4", Some(-0.0625)),
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
            let mut s = LmSensors::new(Some(conf.clone()), cpuinfo());
            assert_eq!(s.start(), StartOutcome::Available);
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
            exp.push((
                "puron_new-isa-0000",
                "temp1",
                T,
                "puron_new temp1",
                Some(30.0),
            ));
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
            let mut s = LmSensors::new(Some(conf.clone()), cpuinfo());
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
            let mut s = started();
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

            let mut s = started();
            assert_eq!(s.start(), StartOutcome::Available);
            for _ in 0..3 {
                let c = "puron_set-isa-0000";
                same(
                    &readings(&mut s),
                    &[(c, "temp1", T, "puron_set temp1", Some(40.0))],
                );
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
            let mut s = started();
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
                [
                    "CPU",
                    "Example Core E7-1000 Core 0",
                    "Example Core E7-1000 Core 1",
                    "Example Core E7-1000 Core 2",
                    "Example Core E7-1000 Core 3"
                ]
            );
            assert_eq!(
                names("nvme-pci-0100"),
                ["SSD", "nvme Sensor 1", "nvme Sensor 2"]
            );
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
            let mut s = started();
            let c = "puron_bytes-isa-0000";
            same(
                &readings(&mut s),
                &[(c, "temp1", T, "puron_bytes Core\u{FFFD} 0", Some(45.0))],
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
        let list = Path::new("/nonexistent/cpuinfo");
        let s = LmSensors::new(Some(test.into()), list);
        assert_eq!((s.config(), s.cpuinfo()), (Some(test), list));
        let production = registry::lm_sensors();
        assert_eq!(production.config(), None);
        assert_eq!(production.cpuinfo(), Path::new("/proc/cpuinfo"));
    });
}

#[test]
fn c_src_lmsensors_14() {
    puron_testkit::limit("C-src.lmsensors-14", LIMIT, || {
        on("c_src_lmsensors_14", "nolabel", || {
            let model = format!("{NVME_DEVICE}/model");
            let product = format!("{NVME_DEVICE}/product");
            let core_model = format!("{CORETEMP}/model");
            // Never a parent, and never the hwmon directory itself.
            for file in ["model", "product"] {
                umd::write(&format!("{NVME_PARENT}/{file}"), b"PCI bridge\n");
                umd::write(&format!("{NVME}/{file}"), b"The hwmon\n");
            }
            machine(b"Some Machine\n");
            let mut s = started();
            // The chip name, as neither a drive nor a battery nor a chip
            // with no device takes the machine's name.
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(names(&mut s, CHIP_BATTERY), ["BAT0 in0"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["iwlwifi temp1"]);
            // The CPU's name before the machine's; a platform chip that
            // measures no CPU has the machine's.
            assert_eq!(names(&mut s, CHIP_CORE)[1], format!("{CPU} Core 0"));
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "Some Machine CPU fan");
            // The own device's model before all of them, and before the
            // product.
            umd::write(&model, b"Some Drive\n");
            umd::write(&product, b"Some Product\n");
            umd::write(&core_model, b"Own Model\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Some Drive Sensor 1");
            assert_eq!(names(&mut s, CHIP_CORE)[1], "Own Model Core 0");
            // Then each next rule as the one before it has nothing.
            std::fs::remove_file(tb(&model)).unwrap();
            std::fs::remove_file(tb(&core_model)).unwrap();
            let mut s = with_cpuinfo("/nonexistent/cpuinfo");
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Some Product Sensor 1");
            assert_eq!(names(&mut s, CHIP_CORE)[1], "Some Machine Core 0");
            std::fs::remove_file(tb("sys/class/dmi/id/product_name")).unwrap();
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_CORE)[1], "coretemp Core 0");
        })
    });
}

#[test]
fn device_model_text() {
    puron_testkit::limit("device_model_text", LIMIT, || {
        on("device_model_text", "capture", || {
            let model = format!("{NVME_DEVICE}/model");
            let product = format!("{NVME_DEVICE}/product");
            let drive = |s: &mut LmSensors| names(s, CHIP_NVME);

            umd::write(&model, b"Example SSD 1TB\n");
            let mut s = started();
            assert_eq!(
                drive(&mut s),
                [
                    "SSD",
                    "Example SSD 1TB Sensor 1",
                    "Example SSD 1TB Sensor 2"
                ]
            );
            // Trimmed on both sides, and nothing else.
            umd::write(&model, b"  Disk  Model\t \n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Disk  Model Sensor 1");
            // Bytes that are not UTF-8 are data too.
            umd::write(&model, b"Disk\xff\n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Disk\u{FFFD} Sensor 1");
            // The model comes before the product.
            umd::write(&model, b"Model\n");
            umd::write(&product, b"Product\n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Model Sensor 1");
            // The product when there is no model.
            std::fs::remove_file(tb(&model)).unwrap();
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Product Sensor 1");
        })
    });
}

#[test]
fn device_cpu_chips() {
    puron_testkit::limit("device_cpu_chips", LIMIT, || {
        on("device_cpu_chips", "cpus", || {
            let mut s = started();
            let got = readings(&mut s);
            let c = |chip, feature, name, v| (chip, feature, T, name, Some(v));
            let cpu = [
                c(
                    "k10temp-pci-00c3",
                    "temp1",
                    "Example Core E7-1000 Tctl",
                    50.0,
                ),
                c(
                    "zenpower-pci-00cb",
                    "temp1",
                    "Example Core E7-1000 Tdie",
                    51.0,
                ),
                c(CHIP_CORE, "temp2", "Example Core E7-1000 Core 0", 52.0),
            ];
            let other = c("puron_other-isa-0000", "temp1", "puron_other temp1", 53.0);
            same(&got, &[cpu[0], cpu[1], cpu[2], other]);
            // A CPU chip keeps the CPU's name on a machine with a name; a
            // chip of the same subsystem that is no CPU chip takes the
            // machine's.
            machine(b"Some Machine\n");
            restart(&mut s);
            let other = c("puron_other-isa-0000", "temp1", "Some Machine temp1", 53.0);
            same(&readings(&mut s), &[cpu[0], cpu[1], cpu[2], other]);
            // Its own device's model comes first.
            umd::write("sys/devices/platform/coretemp.0/model", b"Own Model\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_CORE), ["Own Model Core 0"]);
        })
    });
}

#[test]
fn device_machine_name() {
    puron_testkit::limit("device_machine_name", LIMIT, || {
        on("device_machine_name", "nolabel", || {
            machine(b"  Example Laptop \n");
            let mut s = started();
            assert_eq!(
                names(&mut s, CHIP_BOARD),
                ["Example Laptop CPU fan", "Example Laptop CPU temp"]
            );
            // Only chips of subsystem platform: the drive, the battery and
            // a chip without a device do not take it.
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(names(&mut s, CHIP_BATTERY), ["BAT0 in0"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["iwlwifi temp1"]);
        })
    });
}

#[test]
fn c_src_lmsensors_17() {
    puron_testkit::limit("C-src.lmsensors-17", LIMIT, || {
        on("c_src_lmsensors_17", "chipnames", || {
            let mut s = started();
            let row = |chip, name| (chip, "temp1", T, name, Some(40.0));
            same(
                &readings(&mut s),
                &[
                    row("puron_x_12-isa-0000", "puron_x temp1"),
                    row("puron_2_34-isa-0000", "puron_2 temp1"),
                    row("puron_y_-isa-0000", "puron_y_ temp1"),
                    row("puron0-isa-0000", "puron0 temp1"),
                    row("puron_z9-isa-0000", "puron_z9 temp1"),
                ],
            );
        })
    });
}

#[test]
fn c_src_lmsensors_15() {
    puron_testkit::limit("C-src.lmsensors-15", LIMIT, || {
        on("c_src_lmsensors_15", "capture", || {
            let list = temp_dir("c-src-lmsensors-15").join("cpuinfo");
            let cases = [
                (
                    "model name\t: Example(R) Core(TM) E7-1000 @ 2.80GHz\n",
                    "Example Core E7-1000",
                ),
                (
                    "model name\t: Example(R) Server(R) CPU           S1000  @ 2.53GHz\n",
                    "Example Server S1000",
                ),
                (
                    "model name\t: Intel(R) Core(TM) i5-2500 CPU @ 3.30GHz\n",
                    "Intel Core i5-2500",
                ),
                (
                    "model name\t: AMD Ryzen 7 5800X 8-Core Processor\n",
                    "AMD Ryzen 7 5800X 8-Core Processor",
                ),
                // Only the word "CPU" goes, and "@" only with a frequency.
                (
                    "model name\t: CPUMaker Turbo CPUs @ home\n",
                    "CPUMaker Turbo CPUs @ home",
                ),
                ("model name\t: Maker X @ 800MHz\n", "Maker X"),
                ("model name\t: Maker X @ 2.80GHz rev\n", "Maker X rev"),
                ("model name\t: Maker X @ fastHz\n", "Maker X @ fastHz"),
                // The marks go in capitals only, as written.
                ("model name\t: Maker(tm) X\n", "Maker(tm) X"),
                // The first line of that name, and no other line.
                (
                    "model\t\t: 140\nmodel name\t: First\nmodel name\t: Second\n",
                    "First",
                ),
            ];
            let mut s = with_cpuinfo(&list);
            for (text, device) in cases {
                std::fs::write(&list, text).unwrap();
                restart(&mut s);
                let row = names(&mut s, CHIP_CORE)[1].clone();
                assert_eq!(row, format!("{device} Core 0"), "{text:?}");
            }
        })
    });
}

#[test]
fn device_machine_placeholders() {
    puron_testkit::limit("device_machine_placeholders", LIMIT, || {
        on("device_machine_placeholders", "nolabel", || {
            let placeholders = [
                "To be filled by O.E.M.",
                "To Be Filled By O.E.M.",
                "System Product Name",
                "Default string",
                "Not Specified",
                "Not Applicable",
                "Product Name",
                "System Name",
                "O.E.M.",
                "None",
                "N/A",
                "Unknown",
                "UNKNOWN",
                " Default string \n",
            ];
            let mut s = started();
            for p in placeholders {
                machine(p.as_bytes());
                restart(&mut s);
                assert_eq!(
                    names(&mut s, CHIP_BOARD)[0],
                    "example_acpi CPU fan",
                    "{p:?}"
                );
            }
            // A whole name only: one that holds such a word is a name.
            machine(b"Unknown Box\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "Unknown Box CPU fan");
        })
    });
}

#[test]
fn device_read_errors() {
    puron_testkit::limit("device_read_errors", LIMIT, || {
        on("device_read_errors", "capture", || {
            let model = format!("{NVME_DEVICE}/model");
            let product = format!("{NVME_DEVICE}/product");
            let core = format!("{CORETEMP}/model");
            // A directory opens but does not read.
            std::fs::create_dir(tb(&model)).unwrap();
            umd::write(&product, b"Product\n");
            let mut s = started();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Product Sensor 1");
            // Empty or blank text is none.
            std::fs::remove_dir(tb(&model)).unwrap();
            umd::write(&model, b"");
            umd::write(&product, b" \t\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            // The same for the next rules: the model of the CPU chip's
            // device, then the CPU's name, then the machine's name.
            std::fs::create_dir(tb(&core)).unwrap();
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_CORE)[1], format!("{CPU} Core 0"));
            machine(b"Some Machine\n");
            let list = temp_dir("device_read_errors").join("cpuinfo");
            for text in [
                None,
                Some(""),
                Some("vendor_id\t: x\n"),
                Some("model name\t: CPU\n"),
            ] {
                let mut s = match text {
                    None => with_cpuinfo("/nonexistent/cpuinfo"),
                    Some(t) => {
                        std::fs::write(&list, t).unwrap();
                        with_cpuinfo(&list)
                    }
                };
                assert_eq!(
                    names(&mut s, CHIP_CORE)[1],
                    "Some Machine Core 0",
                    "{text:?}"
                );
                s.stop();
            }
            // No name can be read (no cpuinfo, product_name a directory):
            // each chip keeps its chip name.
            std::fs::remove_file(tb("sys/class/dmi/id/product_name")).unwrap();
            std::fs::create_dir(tb("sys/class/dmi/id/product_name")).unwrap();
            let mut s = with_cpuinfo("/nonexistent/cpuinfo");
            assert_eq!(names(&mut s, CHIP_CORE)[1], "coretemp Core 0");
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "example_acpi CPU fan");
        })
    });
}

#[test]
fn c_src_lmsensors_16() {
    puron_testkit::limit("C-src.lmsensors-16", LIMIT, || {
        on("c_src_lmsensors_16", "capture", || {
            umd::write(&format!("{NVME_DEVICE}/model"), b"Some Drive\n");
            // Feature temp5 lost its final newline: the library drops the
            // last byte whatever it is.
            umd::write(&format!("{CORETEMP_HWMON}/temp5_label"), b"Core 3");
            let conf = copy(&umd::conf(), "c-src-lmsensors-16");
            let text = std::fs::read_to_string(&conf).unwrap();
            std::fs::write(
                &conf,
                format!(
                    "{text}
chip \"coretemp-isa-*\"
    label temp2 \"Core 0\"
    label temp3 \"  My  Core \"
chip \"nvme-pci-*\"
    label temp2 \"temp2\"
    label temp3 \"sensor 2\"
chip \"BAT0-*\"
    label in0 \"Battery\"
"
                ),
            )
            .unwrap();
            let mut s = LmSensors::new(Some(conf), cpuinfo());
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(
                names(&mut s, CHIP_CORE),
                [
                    // Written in the configuration: as written.
                    "CPU",
                    // The limit: equal to the kernel label, so seen as it.
                    "Example Core E7-1000 Core 0",
                    "  My  Core ",
                    // Not configured: follows the device.
                    "Example Core E7-1000 Core 2",
                    "Example Core E7-1000 Core ",
                ]
            );
            assert_eq!(
                names(&mut s, CHIP_NVME),
                [
                    "SSD",
                    // The limit: equal to the raw name, so seen as it.
                    "Some Drive temp2",
                    "sensor 2",
                ]
            );
            assert_eq!(names(&mut s, CHIP_BATTERY), ["Battery"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["WiFi"]);
        })
    });
}

#[test]
fn device_power_supply() {
    puron_testkit::limit("device_power_supply", LIMIT, || {
        on("device_power_supply", "capture", || {
            for file in ["model", "product", "model_name", "manufacturer"] {
                umd::write(&format!("{BATTERY}/{file}"), b"BAT\n");
            }
            let mut s = started();
            assert_eq!(names(&mut s, CHIP_BATTERY), ["BAT0 in0"]);
        })
    });
}

#[test]
fn c_src_lmsensors_18() {
    puron_testkit::limit("C-src.lmsensors-18", LIMIT, || {
        on("c_src_lmsensors_18", "capture", || {
            let ids = |raw: &[RawReading]| {
                let mut v = rows::identities(&rows::rows(raw, &ShownClasses::ALL));
                v.sort();
                v
            };
            // The identities of the version with no device in a name.
            let raw = include_str!("../../puron-testkit/fixtures/raw/capture.json");
            let plain: Vec<RawReading> = serde_json::from_str(raw).unwrap();
            let want = ids(&plain);
            assert_eq!(want.len(), 12);
            assert!(
                want.iter()
                    .any(|i| i == "temperature/lm-sensors/nvme-pci-0100/temp2"),
                "{want:?}"
            );

            let mut s = started();
            let before = readings(&mut s);
            assert_eq!(ids(&before), want);
            // A device name appears: every name of the drive changes, no
            // identity does, so a pin stays on its sensor.
            umd::write(&format!("{NVME_DEVICE}/model"), b"Some Drive\n");
            restart(&mut s);
            let after = readings(&mut s);
            assert_eq!(ids(&after), want);
            assert_ne!(before, after);
        })
    });
}

#[test]
fn device_found_once() {
    puron_testkit::limit("device_found_once", LIMIT, || {
        on("device_found_once", "capture", || {
            let model = format!("{NVME_DEVICE}/model");
            umd::write(&model, b"First Drive\n");
            machine(b"Some Machine\n");
            // Before a start nothing has been found.
            let mut s = LmSensors::new(Some(umd::conf()), cpuinfo());
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            // Reads do not look again, not even when the file is gone.
            umd::write(&model, b"Second Drive\n");
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            std::fs::remove_file(tb(&model)).unwrap();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            // A chip that appears later has only its chip name.
            let st = Command::new("sh")
                .arg(fixture("umd", "addchip.sh"))
                .status()
                .expect("run addchip.sh");
            assert!(st.success(), "addchip.sh: {st}");
            assert_eq!(names(&mut s, "puron_new-isa-0000"), ["puron_new temp1"]);
            // Another chip in that hwmon directory is not this device.
            umd::write(&format!("{NVME}/name"), b"puron_reused\n");
            assert_eq!(
                names(&mut s, "puron_reused-pci-0100")[1],
                "puron_reused Sensor 1"
            );
            umd::write(&format!("{NVME}/name"), b"nvme\n");
            // Stop forgets; the next start looks again.
            umd::write(&model, b"Second Drive\n");
            s.stop();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Second Drive Sensor 1");
            assert_eq!(names(&mut s, "puron_new-isa-0000"), ["Some Machine temp1"]);
        })
    });
}

#[test]
fn device_two_chips_one_name() {
    puron_testkit::limit("device_two_chips_one_name", LIMIT, || {
        on("device_two_chips_one_name", "twin", || {
            let mut s = started();
            let mut got: Vec<(String, Option<f64>)> = readings(&mut s)
                .into_iter()
                .map(|r| {
                    assert_eq!(r.key, "puron_twin-acpi-0/temp1");
                    (r.name.unwrap(), r.value)
                })
                .collect();
            got.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(
                got,
                [
                    ("Drive A temp1".to_string(), Some(61.0)),
                    ("Drive B temp1".to_string(), Some(62.0)),
                ]
            );
        })
    });
}

/// The rows of a read in shown order, by class, as the popup lists them.
fn shown(raw: &[RawReading], class: Class) -> Vec<String> {
    order::order(rows::rows(raw, &ShownClasses::ALL))
        .into_iter()
        .filter(|r| r.class == class)
        .map(|r| r.name)
        .collect()
}

/// Writes the rows of a read when `PURON_SHOW_ROWS` is set.
fn show(raw: &[RawReading]) {
    if std::env::var_os("PURON_SHOW_ROWS").is_some() {
        for class in [Class::Temperature, Class::Fan, Class::Voltage, Class::Power] {
            for name in shown(raw, class) {
                println!("{}\t{name}", class.word());
            }
        }
    }
}

#[test]
fn device_rows_laptop() {
    puron_testkit::limit("device_rows_laptop", LIMIT, || {
        on("device_rows_laptop", "laptop", || {
            let mut s = started();
            let got = readings(&mut s);
            show(&got);
            assert_eq!(
                shown(&got, Class::Temperature),
                [
                    "Example Core E7-1000 Core 0",
                    "Example Core E7-1000 Core 1",
                    "Example Core E7-1000 Core 2",
                    "Example Core E7-1000 Core 3",
                    "Example Core E7-1000 Package id 0",
                    "Example Laptop CPU temp",
                    "iwlwifi temp1",
                    "Example SSD 1TB Composite",
                    "Example SSD 1TB Sensor 1",
                    "Example SSD 1TB Sensor 2",
                ]
            );
            assert_eq!(shown(&got, Class::Fan), ["Example Laptop CPU fan"]);
            assert_eq!(shown(&got, Class::Voltage), ["BAT0 in0"]);
            assert_eq!(shown(&got, Class::Power), [""; 0]);
        })
    });
}

#[test]
fn device_rows_server() {
    puron_testkit::limit("device_rows_server", LIMIT, || {
        on("device_rows_server", "server", || {
            let mut s = with_cpuinfo(dir("server").join("cpuinfo"));
            let got = readings(&mut s);
            show(&got);
            assert_eq!(
                shown(&got, Class::Temperature),
                [
                    "Example Server S1000 Core 0",
                    "Example Server S1000 Core 1",
                    "Example Server S1000 Core 2",
                    "Example Server S1000 Core 3",
                ]
            );
        })
    });
}

#[test]
fn c_src_lmsensors_29() {
    puron_testkit::limit("C-src.lmsensors-29", LIMIT, || {
        // The capture's configuration calls `Package id 0` "CPU".
        on("c_src_lmsensors_29", "capture", || {
            let mut s = started();
            let got = readings(&mut s);
            let of = |key: &str| got.iter().find(|r| r.key == key).expect(key);
            let package = of("coretemp-isa-0000/temp1");
            assert_eq!(package.name.as_deref(), Some("CPU"));
            assert_eq!(package.kernel_label.as_deref(), Some("Package id 0"));
            assert_eq!(package.device_chars, 0, "a configured label has no device");
            assert!(package.platform);
            // A driver's label passes through, with the device beside it.
            let core = of("coretemp-isa-0000/temp2");
            assert_eq!(core.kernel_label.as_deref(), Some("Core 0"));
            assert_eq!(core.device_chars, "Example Core E7-1000".len());
            // No label file: none. A chip on another bus is no platform chip.
            let nvme = of("nvme-pci-0100/temp1");
            assert!(!nvme.platform);
            // The CPU's group and its line follow from that.
            let settings = puron_core::settings::Settings::default();
            let shown = puron_core::menu::shown(&got, &settings);
            let cpu = shown
                .devices
                .iter()
                .find(|d| d.heading == puron_core::menu::Heading::Cpu)
                .expect("the CPU's group");
            let line = cpu
                .lines
                .iter()
                .find(|l| l.name == "coretemp: CPU")
                .expect("the configured package reading's line");
            assert_eq!(
                line.identity,
                "temperature/lm-sensors/coretemp-isa-0000/temp1"
            );
        })
    });
}

/// The library's chip names against the name that puron-core derives from
/// an NVMe controller's address: the same reading through both real
/// adapters.
#[test]
fn nvme_repeats_through_the_library() {
    puron_testkit::limit("nvme_repeats_through_the_library", LIMIT, || {
        on("nvme_repeats_through_the_library", "capture", || {
            let mut s = started();
            let mut got = readings(&mut s);
            let t = puron_testkit::sys::build("nvme-capture");
            let mut nvme = puron_engine::nvme::Nvme::new(&t.root);
            assert_eq!(nvme.start(), StartOutcome::Available);
            let stop = puron_testkit::umd::stop_fd();
            match nvme.read(std::os::fd::AsFd::as_fd(&stop)) {
                ReadOutcome::Readings(r) => got.extend(r),
                other => panic!("read gave {other:?}"),
            }
            let settings = puron_core::settings::Settings::default();
            let shown = puron_core::menu::shown(&got, &settings);
            assert!(!shown.lines().any(|l| l.identity.contains("nvme-cli")));
            assert_eq!(shown.repeats.len(), 2);
            let drive = shown
                .line("temperature/lm-sensors/nvme-pci-0100/temp1")
                .expect("the drive's line");
            assert_eq!(drive.name, "SSD");
        });
    });
}

/// How the library names the chip of an NVMe controller, for device,
/// function and domain.
#[test]
fn nvme_chip_names_of_the_library() {
    puron_testkit::limit("nvme_chip_names_of_the_library", LIMIT, || {
        on("nvme_chip_names_of_the_library", "nvmeaddr", || {
            let mut s = started();
            let mut got = readings(&mut s);
            let mut chips: Vec<&str> = got.iter().filter_map(|r| r.chip.as_deref()).collect();
            chips.sort_unstable();
            assert_eq!(chips, ["nvme-pci-01ff", "nvme-pci-1021c", "nvme-pci-e100"]);
            for (i, address) in ["0000:01:1f.7", "0001:02:03.4", "0000:e1:00.0"]
                .iter()
                .enumerate()
            {
                got.push(RawReading {
                    provider: Provider::NvmeCli,
                    position: i as u32,
                    kind: Some(Kind::Temperature),
                    chip: None,
                    name: Some("M".to_string()),
                    device: None,
                    item: None,
                    unit: None,
                    key: format!("{address}/temp1"),
                    value: Some(40.0),
                    kernel_label: None,
                    platform: false,
                    device_chars: 0,
                });
            }
            assert_eq!(puron_core::rows::repeats(&got).len(), 3);
        });
    });
}
