//! Checks that a test run through `puron_testkit::umd` sees the testbed
//! and not the machine. The testbed, `fixtures/umd/capture`, holds five
//! sensor chips. In the child, `/sys` must
//! report the sysfs file-system type, `class/hwmon/hwmon5/name` must read
//! `coretemp`, and libsensors, with the testbed's `sensors.conf`, must
//! list exactly the chips of `CHIPS`.

use puron_testkit::{fixture, umd};

/// The names of the chips that libsensors finds in the testbed, sorted.
const CHIPS: [&str; 5] = [
    "BAT0-acpi-0",
    "coretemp-isa-0000",
    "example_acpi-isa-0000",
    "iwlwifi_1-virtual-0",
    "nvme-pci-0100",
];

/// The file-system type that `statfs` reports for sysfs (`SYSFS_MAGIC`
/// in `linux/magic.h`).
const SYSFS_MAGIC: i64 = 0x6265_6572;

/// The checks, run in the child under umockdev.
fn probe() {
    let path = std::ffi::CString::new("/sys").unwrap();
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statfs(path.as_ptr(), &mut st) };
    assert_eq!(rc, 0, "statfs(/sys)");
    assert_eq!(st.f_type as i64, SYSFS_MAGIC, "f_type of /sys");

    let name = std::fs::read_to_string("/sys/class/hwmon/hwmon5/name");
    assert_eq!(name.ok().as_deref(), Some("coretemp\n"), "hwmon5/name");

    let sensors = lm_sensors::Initializer::default()
        .config_path(umd::conf())
        .initialize()
        .expect("libsensors initialized");
    let mut chips: Vec<String> = sensors
        .chip_iter(None)
        .map(|c| c.name().expect("chip name"))
        .collect();
    chips.sort();
    assert_eq!(chips, CHIPS);
}

#[test]
fn c_tests_8() {
    puron_testkit::limit("C-tests-8", 10, || {
        let dir = fixture("umd", "capture");
        umd::run(
            "c_tests_8",
            &dir.join("testbed.umockdev"),
            &dir.join("sensors.conf"),
            probe,
        );
    });
}
