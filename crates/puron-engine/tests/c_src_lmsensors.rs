//! Tests of the lm-sensors adapter (`puron_engine::lmsensors`), which reads
//! sensors through libsensors.
//!
//! libsensors reads the machine's sysfs, so each test runs under umockdev on
//! a fake sysfs tree: `on` runs the test body in a child process that
//! `umockdev-run` starts (see `puron_testkit::umd`). The tree and the
//! libsensors configuration come from one fixture directory of
//! `fixtures/umd/`: its `testbed.umockdev` and its `sensors.conf`. The
//! `capture` fixture is a recorded tree with five chips (the CPU, an NVMe
//! drive, a Wi-Fi card, the board and the battery) and a configuration that
//! labels three of their inputs; the `nolabel`, `change` and `badconf`
//! fixtures use the same tree.
//!
//! A reading's name starts with its chip's device name, a readable name for
//! the hardware such as a drive's model or the CPU's name, unless the
//! configuration gives the reading a label. The adapter finds the device
//! names at `start` (see its module documentation), so the tests that change
//! the files those names come from restart the adapter before they read. A
//! chip with no device name goes by its short name: the prefix of its chip
//! name without a final `_<digits>`, such as `nvme` for `nvme-pci-0100`.
//!
//! The last tests follow the readings further, into the popup: the window
//! of the applet that lists every sensor's row. `puron_core::rows` turns
//! readings into rows, each with an identity that the list of pinned
//! sensors uses, and `puron_core::menu::shown` gives the popup's lines.

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

/// One reading in the form that `same` compares: key, kind, name and value.
type Reading = (String, Option<Kind>, Option<String>, Option<f64>);

/// The time limit of each test here, in seconds.
const LIMIT: u64 = 10;

/// The NVMe drive's hwmon directory in the `capture` tree, relative to the
/// tree's root (`umd::dir`). The other paths here are relative to it too.
const NVME: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0/nvme/nvme0/hwmon2";
/// The drive's own device, which the `device` link of its hwmon directory
/// names. The adapter takes a device name from its `model` or `product` file.
const NVME_DEVICE: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0/nvme/nvme0";
/// The parent of the drive's device, a PCI device. The adapter never takes a
/// device name from it.
const NVME_PARENT: &str = "sys/devices/pci0000:00/0000:00:1d.0/0000:01:00.0";
/// The battery's device, a power supply, whose name files the adapter does
/// not use.
const BATTERY: &str = "sys/devices/LNXSYSTM:00/LNXSYBUS:00/PNP0C0A:00/power_supply/BAT0";
/// The device of the CPU temperature chip.
const CORETEMP: &str = "sys/devices/platform/coretemp.0";
/// The hwmon directory of the CPU temperature chip.
const CORETEMP_HWMON: &str = "sys/devices/platform/coretemp.0/hwmon/hwmon5";

/// The libsensors chip names of the drive, the CPU, the board and the battery
/// in the `capture` tree. A reading's key is `<chip>/<feature>`.
const CHIP_NVME: &str = "nvme-pci-0100";
const CHIP_CORE: &str = "coretemp-isa-0000";
const CHIP_BOARD: &str = "example_acpi-isa-0000";
const CHIP_BATTERY: &str = "BAT0-acpi-0";

/// The CPU's name that the adapter makes from the `model name` line of the
/// `laptop` CPU list, which reads "Example(R) Core(TM) E7-1000 @ 2.80GHz".
const CPU: &str = "Example Core E7-1000";

/// One expected reading: chip, feature, kind, name and value. Its key is
/// `<chip>/<feature>`.
type Exp = (&'static str, &'static str, Kind, &'static str, Option<f64>);

use Kind::{Fan, Power, Temperature as T, Voltage};

/// The 12 readings of the `capture` tree with its own configuration, each
/// chip's readings in the order of the read. "CPU", "SSD" and "WiFi" are
/// labels from the configuration; the other names are the device name, or the
/// short chip name, followed by the kernel's label or the feature's name.
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

/// The fixture directory `fixtures/umd/<name>`.
fn dir(name: &str) -> PathBuf {
    fixture("umd", name)
}

/// Runs `body` under umockdev on the fixture `name`: on its tree, with its
/// `sensors.conf` as the configuration that `umd::conf` returns in the child.
/// `test` is the calling test's name, which the child runs again.
fn on(test: &str, name: &str, body: impl FnOnce()) {
    let d = dir(name);
    umd::run(
        test,
        &d.join("testbed.umockdev"),
        &d.join("sensors.conf"),
        body,
    );
}

/// The CPU list of the `laptop` fixture, in the format of `/proc/cpuinfo`.
fn cpuinfo() -> PathBuf {
    dir("laptop").join("cpuinfo")
}

/// A started adapter on the child's configuration and the `laptop` CPU list.
fn started() -> LmSensors {
    with_cpuinfo(cpuinfo())
}

/// A started adapter on the child's configuration that reads the CPU's name
/// from `file`. Panics unless `start` gives `Available`.
fn with_cpuinfo(file: impl Into<PathBuf>) -> LmSensors {
    let mut s = LmSensors::new(Some(umd::conf()), file);
    assert_eq!(s.start(), StartOutcome::Available);
    s
}

/// Stops and starts `src`, so that it finds the device names again.
fn restart(src: &mut LmSensors) {
    src.stop();
    assert_eq!(src.start(), StartOutcome::Available);
}

/// Reads once and returns the names of the readings of chip `chip`, in the
/// order of the read.
fn names(src: &mut LmSensors, chip: &str) -> Vec<String> {
    readings(src)
        .iter()
        .filter(|r| r.chip.as_deref() == Some(chip))
        .map(|r| r.name.clone().unwrap_or_default())
        .collect()
}

/// The path `path` in the fake sysfs tree; only in the child.
fn tb(path: &str) -> PathBuf {
    umd::dir().join(path)
}

/// In the child, writes `bytes` to the machine's product name file
/// (`sys/class/dmi/id/product_name`), which the adapter reads for the
/// machine's name.
fn machine(bytes: &[u8]) {
    std::fs::create_dir_all(tb("sys/class/dmi/id")).expect("dmi directory");
    umd::write("sys/class/dmi/id/product_name", bytes);
}

/// Reads once with a stop descriptor that is never raised.
fn read(src: &mut LmSensors) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// Reads once and returns the readings. Panics if the read gives anything
/// else.
fn readings(src: &mut LmSensors) -> Vec<RawReading> {
    match read(src) {
        ReadOutcome::Readings(v) => v,
        other => panic!("read: {other:?}"),
    }
}

/// Checks that `got` holds exactly the readings of `exp`: for each chip, the
/// same keys, kinds, names and values in the same order. The order of the
/// chips does not matter. Also checks that every reading has provider
/// `LmSensors` and no device, item or unit.
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
        let g: Vec<Reading> = got
            .iter()
            .filter(|r| r.chip.as_deref() == Some(chip))
            .map(|r| (r.key.clone(), r.kind, r.name.clone(), r.value))
            .collect();
        let e: Vec<Reading> = exp
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

/// Checks that the positions of `got` are 0, 1, 2 and so on, in the order of
/// the read.
fn positions_in_order(got: &[RawReading]) {
    let p: Vec<u32> = got.iter().map(|r| r.position).collect();
    let want: Vec<u32> = (0..got.len() as u32).collect();
    assert_eq!(p, want, "positions in report order");
}

/// Copies `file` to `sensors.conf` in a new temporary directory named after
/// `tag`, so that a test can change it, and returns the copy's path.
fn copy(file: &Path, tag: &str) -> PathBuf {
    let to = temp_dir(tag).join("sensors.conf");
    std::fs::copy(file, &to).expect("copy configuration");
    to
}

/// With the `capture` tree and its own configuration, `start` gives
/// `Available` and one read gives exactly the 12 readings of `CAPTURE`.
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

/// With the `capture` tree and an empty configuration, the three readings
/// that the configuration labels are named like the others: after the device
/// and the kernel's label (`Package id 0`, `Composite`), or the feature's
/// name where the kernel gives no label (`temp1` of the Wi-Fi chip). Every
/// other reading is as in `CAPTURE`.
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

/// A chip with one input of each kind that is read: voltage, fan, temperature
/// and power. Each gives one reading with its kind and its value as
/// libsensors gives it (12 V, 1200 RPM, 41 °C, 12.5 W).
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

/// Two temperatures of one chip with the same kernel label, `Core 0`. Both
/// are kept, with the same name and different keys, and their positions are 0
/// and 1 in the order of the read.
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

/// After a good read, the drive's `temp2_input` file is changed to text that
/// is no number. The next read still gives all 12 readings; only that one has
/// no value.
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

/// The configuration has `compute` statements for four temperatures. The
/// readings carry the values after the statements' formulas, also when they
/// are fractions or negative.
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

/// Between two reads, a value changes, a label in the configuration file
/// changes, and a new chip appears (`addchip.sh`). The second read, with no
/// restart, shows all three: the new value, the new label `Disk`, and the new
/// chip's reading.
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

/// The `badconf` configuration does not parse: it holds the five bytes
/// `bogus`. `start` still gives `Available`, the first read gives
/// `Unavailable`, and a `config-error` event with a `PURON_FILE` field has
/// reached the diagnostic journal when that read returns. Once the file is
/// replaced with a good one, the next read gives the 12 readings of
/// `CAPTURE`.
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
            // Diagnostic events go to a test journal socket, switched on
            // before `start`. The adapter always sets `PURON_FILE`, empty
            // when the library gives no file name.
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

/// A second `start` on a started adapter gives `Available` again; three reads
/// in a row give the same 12 readings; and after `stop` and `start`, a read
/// gives them again.
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

/// Runs `script` with `sh -c`, with `args` as `$1`, `$2` and so on, and
/// returns its exit code.
fn sh(script: &str, args: &[&Path]) -> Option<i32> {
    Command::new("sh")
        .args(["-c", script, "sh"])
        .args(args)
        .status()
        .expect("run sh")
        .code()
}

/// The `set` configuration holds a `set temp1_max 99` statement, which would
/// write to the sensor if it were applied. Three reads and a stop write
/// nothing: `diff -r` against a copy of the tree made before the reads finds
/// no change, apart from umockdev's own `config` and `ioctl` files. The
/// adapter's source also calls none of the library's writing functions
/// (`set_value`, `set_raw_value`, `do_chip_sets`).
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

            // The reads wrote nothing: the tree matches its copy from before,
            // apart from umockdev's own `config` and `ioctl` files.
            let diff = r#"diff -r --no-dereference -x config -x ioctl "$1" "$2""#;
            assert_eq!(sh(diff, &[&tree, &copy]), Some(0), "diff_rc");
            let src = workspace_root().join("crates/puron-engine/src/lmsensors.rs");
            let grep = r#"grep -E 'set_(raw_)?value|do_chip_sets' "$1""#;
            assert_eq!(sh(grep, &[&src]), Some(1), "grep of the adapter source");
        })
    });
}

/// On the `capture` tree, the positions are 0, 1, 2 and so on in the order of
/// the read, and each chip's readings come in the library's feature order:
/// the CPU's package reading `CPU` before its four cores, and the drive's
/// `SSD` before its two sensors.
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

/// A kernel label that is not valid UTF-8 (`Core\xff 0`) gives a name with
/// U+FFFD in place of the invalid byte.
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

/// `config` and `cpuinfo` return the paths the adapter was built with. The
/// adapter that `registry::lm_sensors` builds for production has no
/// configuration file (`None`, for the library's default files) and reads
/// `/proc/cpuinfo`.
#[test]
fn c_src_lmsensors_13() {
    puron_testkit::limit("C-src.lmsensors-13", LIMIT, || {
        // `config` returns the path it was given, so the `None` of the
        // production adapter below is its real setting and not a fixed
        // answer.
        let test = Path::new("/nonexistent/sensors.conf");
        let list = Path::new("/nonexistent/cpuinfo");
        let s = LmSensors::new(Some(test.into()), list);
        assert_eq!((s.config(), s.cpuinfo()), (Some(test), list));
        let production = registry::lm_sensors();
        assert_eq!(production.config(), None);
        assert_eq!(production.cpuinfo(), Path::new("/proc/cpuinfo"));
    });
}

/// The rules for a chip's device name, in the order in which the adapter
/// tries them: the `model` file, then the `product` file, of the chip's own
/// device; for a CPU chip, the CPU's name from the CPU list; for a chip on
/// the platform bus, the machine's name. A chip that none of them names keeps
/// its short name. The test takes away one source of names after another and
/// restarts the adapter each time.
#[test]
fn c_src_lmsensors_14() {
    puron_testkit::limit("C-src.lmsensors-14", LIMIT, || {
        on("c_src_lmsensors_14", "nolabel", || {
            let model = format!("{NVME_DEVICE}/model");
            let product = format!("{NVME_DEVICE}/product");
            let core_model = format!("{CORETEMP}/model");
            // Name files in the drive's parent device and in the hwmon
            // directory itself must never be used.
            for file in ["model", "product"] {
                umd::write(&format!("{NVME_PARENT}/{file}"), b"PCI bridge\n");
                umd::write(&format!("{NVME}/{file}"), b"The hwmon\n");
            }
            machine(b"Some Machine\n");
            let mut s = started();
            // The drive, whose own device has no name file yet, the battery,
            // and the Wi-Fi chip, which has no device, keep their short
            // names: the machine's name is not for them.
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(names(&mut s, CHIP_BATTERY), ["BAT0 in0"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["iwlwifi temp1"]);
            // The CPU chip takes the CPU's name rather than the machine's;
            // the board chip, on the platform bus but no CPU chip, takes the
            // machine's.
            assert_eq!(names(&mut s, CHIP_CORE)[1], format!("{CPU} Core 0"));
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "Some Machine CPU fan");
            // A `model` file in a chip's own device comes before every other
            // rule, and before a `product` file.
            umd::write(&model, b"Some Drive\n");
            umd::write(&product, b"Some Product\n");
            umd::write(&core_model, b"Own Model\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Some Drive Sensor 1");
            assert_eq!(names(&mut s, CHIP_CORE)[1], "Own Model Core 0");
            // Without the `model` files and the CPU list, the next rules give
            // the names: the drive's `product`, and the machine's name for
            // the CPU chip. Without the machine's name too, the CPU chip
            // keeps its short name.
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

/// How the text of a drive's `model` file becomes its device name. Whitespace
/// is removed at both ends only, an invalid UTF-8 byte becomes U+FFFD, and
/// the `product` file is used only when there is no `model` file. The reading
/// `SSD` keeps its configured label throughout.
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
            // Whitespace at both ends is removed; the double space inside
            // stays.
            umd::write(&model, b"  Disk  Model\t \n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Disk  Model Sensor 1");
            // An invalid UTF-8 byte becomes U+FFFD; the rest of the model is
            // kept.
            umd::write(&model, b"Disk\xff\n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Disk\u{FFFD} Sensor 1");
            // With both files, the model wins.
            umd::write(&model, b"Model\n");
            umd::write(&product, b"Product\n");
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Model Sensor 1");
            // Without the model, the product is used.
            std::fs::remove_file(tb(&model)).unwrap();
            restart(&mut s);
            assert_eq!(drive(&mut s)[1], "Product Sensor 1");
        })
    });
}

/// The `cpus` tree has three CPU temperature chips of different drivers
/// (`k10temp`, `zenpower` and `coretemp`) and one chip, `puron_other`, of the
/// same subsystem that measures no CPU. The three CPU chips take the CPU's
/// name; `puron_other` keeps its short name while the machine has no name,
/// and takes the machine's name once it has one.
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
            // Once the machine has a name, the CPU chips keep the CPU's name,
            // and `puron_other` takes the machine's.
            machine(b"Some Machine\n");
            restart(&mut s);
            let other = c("puron_other-isa-0000", "temp1", "Some Machine temp1", 53.0);
            same(&readings(&mut s), &[cpu[0], cpu[1], cpu[2], other]);
            // A `model` file in the CPU chip's own device comes before the
            // CPU's name.
            umd::write("sys/devices/platform/coretemp.0/model", b"Own Model\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_CORE), ["Own Model Core 0"]);
        })
    });
}

/// The machine's product name, without its surrounding whitespace, starts the
/// names of the board chip, which is on the platform bus. The drive, the
/// battery and the Wi-Fi chip, which has no device, do not take it.
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
            // Chips that are not on the platform bus keep their short names.
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(names(&mut s, CHIP_BATTERY), ["BAT0 in0"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["iwlwifi temp1"]);
        })
    });
}

/// A chip with no device name goes by its prefix without a final `_<digits>`.
/// `puron_x_12` and `puron_2_34` lose that part; `puron_y_`, `puron0` and
/// `puron_z9` do not end in one and stay as they are.
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

/// How the CPU's name is made from the `model name` line of the CPU list: the
/// "(R)" and "(TM)" marks, the word "CPU", and an "@" followed by a frequency
/// are removed, and runs of spaces become one. For each case, the test writes
/// the CPU list, restarts the adapter, and expects the CPU chip's `Core 0`
/// reading to start with the name.
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
                // Only "CPU" as a whole word is removed, and "@" only when a
                // frequency follows it.
                (
                    "model name\t: CPUMaker Turbo CPUs @ home\n",
                    "CPUMaker Turbo CPUs @ home",
                ),
                ("model name\t: Maker X @ 800MHz\n", "Maker X"),
                ("model name\t: Maker X @ 2.80GHz rev\n", "Maker X rev"),
                ("model name\t: Maker X @ fastHz\n", "Maker X @ fastHz"),
                // Only "(R)" and "(TM)" in capitals are removed.
                ("model name\t: Maker(tm) X\n", "Maker(tm) X"),
                // Only the first `model name` line counts; a `model` line is
                // not one.
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

/// Text that firmware leaves in the product name when nobody filled it in is
/// no machine name, in any ASCII case and with whitespace around it: the
/// board chip keeps its short name `example_acpi`. A name that only contains
/// such a word, such as `Unknown Box`, is used.
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
            // Only a whole name is a placeholder; a name that contains one is
            // used.
            machine(b"Unknown Box\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "Unknown Box CPU fan");
        })
    });
}

/// A name file that cannot be read, is empty, or holds only whitespace gives
/// no name, and the next rule for the device name applies. This holds for the
/// drive's `model` and `product`, the CPU chip's `model`, the CPU list and
/// the machine's name. When none of them gives a name, each chip keeps its
/// short name.
#[test]
fn device_read_errors() {
    puron_testkit::limit("device_read_errors", LIMIT, || {
        on("device_read_errors", "capture", || {
            let model = format!("{NVME_DEVICE}/model");
            let product = format!("{NVME_DEVICE}/product");
            let core = format!("{CORETEMP}/model");
            // A directory in place of `model` can be opened but not read, so
            // the `product` file is used.
            std::fs::create_dir(tb(&model)).unwrap();
            umd::write(&product, b"Product\n");
            let mut s = started();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Product Sensor 1");
            // An empty `model` and a `product` of whitespace give no name, so
            // the drive keeps its short name.
            std::fs::remove_dir(tb(&model)).unwrap();
            umd::write(&model, b"");
            umd::write(&product, b" \t\n");
            restart(&mut s);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            // The same for the next rules. An unreadable `model` of the CPU
            // chip's device leaves the CPU's name. With a machine name, a CPU
            // list that is missing, empty, without a `model name` line, or
            // naming only "CPU", leaves the machine's name.
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
            // With no CPU list and a directory in place of the product name
            // file, nothing gives a name, so the CPU chip and the board chip
            // keep their short names.
            std::fs::remove_file(tb("sys/class/dmi/id/product_name")).unwrap();
            std::fs::create_dir(tb("sys/class/dmi/id/product_name")).unwrap();
            let mut s = with_cpuinfo("/nonexistent/cpuinfo");
            assert_eq!(names(&mut s, CHIP_CORE)[1], "coretemp Core 0");
            assert_eq!(names(&mut s, CHIP_BOARD)[0], "example_acpi CPU fan");
        })
    });
}

/// Which configured labels are a reading's whole name. A label set in the
/// configuration file is the whole name, spaces included, unless it equals
/// the kernel's label or the feature's raw name: such a label cannot be told
/// apart from those, so it counts as not configured, and the name is the
/// device name followed by the label. The test also drops the final newline
/// from the kernel label of `temp5`: the label then loses its last character.
#[test]
fn c_src_lmsensors_16() {
    puron_testkit::limit("C-src.lmsensors-16", LIMIT, || {
        on("c_src_lmsensors_16", "capture", || {
            umd::write(&format!("{NVME_DEVICE}/model"), b"Some Drive\n");
            // The label file of `temp5` loses its final newline. libsensors
            // drops the last byte of a label file whatever it is, so the
            // label becomes `Core ` with a trailing space.
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
                    // From the configuration: the whole name.
                    "CPU",
                    // Configured as `Core 0`, equal to the kernel's label, so
                    // it counts as not configured.
                    "Example Core E7-1000 Core 0",
                    // From the configuration: the whole name, spaces
                    // included.
                    "  My  Core ",
                    // Not configured: the device name and the kernel's label.
                    "Example Core E7-1000 Core 2",
                    "Example Core E7-1000 Core ",
                ]
            );
            assert_eq!(
                names(&mut s, CHIP_NVME),
                [
                    "SSD",
                    // Configured as `temp2`, equal to the feature's raw name,
                    // so it counts as not configured: the device name and
                    // that label.
                    "Some Drive temp2",
                    "sensor 2",
                ]
            );
            assert_eq!(names(&mut s, CHIP_BATTERY), ["Battery"]);
            assert_eq!(names(&mut s, "iwlwifi_1-virtual-0"), ["WiFi"]);
        })
    });
}

/// A battery's device is a power supply. Its `model`, `product`, `model_name`
/// and `manufacturer` files, all `BAT` here, give no device name, so the
/// battery's reading keeps the short chip name `BAT0`.
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

/// A reading's identity (the text that names its row in the pinned list, from
/// `puron_core::rows`) does not depend on the device name. The identities of
/// a read equal those of the saved readings `fixtures/raw/capture.json`,
/// whose names have no device part. After the drive gets a `model` file and
/// the adapter restarts, the readings change but the identities stay the
/// same.
#[test]
fn c_src_lmsensors_18() {
    puron_testkit::limit("C-src.lmsensors-18", LIMIT, || {
        on("c_src_lmsensors_18", "capture", || {
            let ids = |raw: &[RawReading]| {
                let mut v = rows::identities(&rows::rows(raw, &ShownClasses::ALL));
                v.sort();
                v
            };
            // The expected identities, from saved readings whose names have
            // no device part.
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
            // Once the drive has a model, the names of its readings change
            // but no identity does, so a pinned sensor stays pinned.
            umd::write(&format!("{NVME_DEVICE}/model"), b"Some Drive\n");
            restart(&mut s);
            let after = readings(&mut s);
            assert_eq!(ids(&after), want);
            assert_ne!(before, after);
        })
    });
}

/// Device names are found at `start` and kept until `stop`. Before the first
/// start, reads work and every chip has its short name. After it, reads keep
/// the names found then, even when a name file changes or is removed, and a
/// chip that appears later has its short name. A different chip in the
/// drive's hwmon directory does not take the drive's name. After `stop` and
/// `start`, the names are found again.
#[test]
fn device_found_once() {
    puron_testkit::limit("device_found_once", LIMIT, || {
        on("device_found_once", "capture", || {
            let model = format!("{NVME_DEVICE}/model");
            umd::write(&model, b"First Drive\n");
            machine(b"Some Machine\n");
            // Before `start`, reads work and every chip has its short name.
            let mut s = LmSensors::new(Some(umd::conf()), cpuinfo());
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            // Reads do not look at the name files again, even after the file
            // is changed or removed.
            umd::write(&model, b"Second Drive\n");
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            std::fs::remove_file(tb(&model)).unwrap();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "First Drive Sensor 1");
            // A chip added after `start` has its short name, although it is
            // on the platform bus and the machine has a name.
            let st = Command::new("sh")
                .arg(fixture("umd", "addchip.sh"))
                .status()
                .expect("run addchip.sh");
            assert!(st.success(), "addchip.sh: {st}");
            assert_eq!(names(&mut s, "puron_new-isa-0000"), ["puron_new temp1"]);
            // A device name belongs to one chip name in one hwmon directory:
            // a chip of another name in the drive's hwmon directory goes by
            // its short name.
            umd::write(&format!("{NVME}/name"), b"puron_reused\n");
            assert_eq!(
                names(&mut s, "puron_reused-pci-0100")[1],
                "puron_reused Sensor 1"
            );
            umd::write(&format!("{NVME}/name"), b"nvme\n");
            // `stop` forgets the names, and the next `start` finds them
            // again, also for the chip added since.
            umd::write(&model, b"Second Drive\n");
            s.stop();
            assert_eq!(names(&mut s, CHIP_NVME)[1], "nvme Sensor 1");
            assert_eq!(s.start(), StartOutcome::Available);
            assert_eq!(names(&mut s, CHIP_NVME)[1], "Second Drive Sensor 1");
            assert_eq!(names(&mut s, "puron_new-isa-0000"), ["Some Machine temp1"]);
        })
    });
}

/// Two chips with the same chip name, `puron_twin-acpi-0`, in two hwmon
/// directories, each with its own device model. Each reading takes the model
/// of its own chip's device (`Drive A`, `Drive B`), although both readings
/// have the same key.
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

/// The display names of the rows of class `class`, in the order in which the
/// popup lists them.
fn shown(raw: &[RawReading], class: Class) -> Vec<String> {
    order::order(rows::rows(raw, &ShownClasses::ALL))
        .into_iter()
        .filter(|r| r.class == class)
        .map(|r| r.name)
        .collect()
}

/// Prints one `<class>\t<name>` line for each row of a read when
/// `PURON_SHOW_ROWS` is set, and nothing otherwise.
fn show(raw: &[RawReading]) {
    if std::env::var_os("PURON_SHOW_ROWS").is_some() {
        for class in [Class::Temperature, Class::Fan, Class::Voltage, Class::Power] {
            for name in shown(raw, class) {
                println!("{}\t{name}", class.word());
            }
        }
    }
}

/// The rows that the popup lists for the `laptop` tree, by class and in
/// display order: the CPU's cores and package under the CPU's name, the
/// board's temperature under the machine's name, the Wi-Fi chip, and the
/// drive under its model; one fan, one voltage, and no power reading.
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

/// On the `server` tree, the CPU chip's four cores are named after the CPU
/// from the server's CPU list (`Example Server S1000`), not after the
/// machine's product name (`Example Server 110`), and they are the only
/// temperature rows.
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

/// The fields of a reading that the popup needs besides its name, on the
/// `capture` tree. `kernel_label` keeps the kernel's label also under a
/// configured label, `device_chars` is the length of the device part of the
/// name (0 for a configured label), and `platform` tells whether the chip's
/// device is on the platform bus. From them, `menu::shown` puts the CPU
/// package reading, which the configuration labels "CPU", into the CPU's
/// group as the line `coretemp: CPU`.
#[test]
fn c_src_lmsensors_29() {
    puron_testkit::limit("C-src.lmsensors-29", LIMIT, || {
        // The `capture` configuration labels the CPU package reading, whose
        // kernel label is `Package id 0`, "CPU".
        on("c_src_lmsensors_29", "capture", || {
            let mut s = started();
            let got = readings(&mut s);
            let of = |key: &str| got.iter().find(|r| r.key == key).expect(key);
            let package = of("coretemp-isa-0000/temp1");
            assert_eq!(package.name.as_deref(), Some("CPU"));
            assert_eq!(package.kernel_label.as_deref(), Some("Package id 0"));
            assert_eq!(package.device_chars, 0, "a configured label has no device");
            assert!(package.platform);
            // A reading with no configured label has the kernel's label too,
            // and its name starts with the device name.
            let core = of("coretemp-isa-0000/temp2");
            assert_eq!(core.kernel_label.as_deref(), Some("Core 0"));
            assert_eq!(core.device_chars, "Example Core E7-1000".len());
            // The drive is on the PCI bus, not the platform bus.
            let nvme = of("nvme-pci-0100/temp1");
            assert!(!nvme.platform);
            // With the default settings, the popup's CPU group holds the
            // package reading as the line `coretemp: CPU`.
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

/// The drive of the `capture` tree, read through both the lm-sensors adapter
/// and the NVMe adapter (on the `nvme-capture` sysfs fixture, whose
/// controller has the address of chip `nvme-pci-0100`).
/// `puron_core::rows::repeats` matches each NVMe reading to the lm-sensors
/// reading of the same sensor by the chip name that libsensors makes from the
/// controller's PCI address. So the popup shows no NVMe reading, lists 2
/// repeats, and keeps the lm-sensors line `SSD`.
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

/// How libsensors names the chip of an NVMe controller, on the `nvmeaddr`
/// tree: `nvme-pci-<hex>`, where the number holds the PCI domain, the bus,
/// and the device and function in one byte (device × 8 + function). The tree
/// has controllers with a function number, in domain 1, and on bus `e1`.
/// `rows::repeats` must match an NVMe reading at each address to the
/// lm-sensors reading of that chip.
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
