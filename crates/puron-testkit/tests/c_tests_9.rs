//! Checks that a chip added to the testbed while `umockdev-run` runs is
//! seen by `sensors` within the same run.
//!
//! In one `umockdev-run` on the captured testbed, `sensors -j` runs, then
//! `fixtures/umd/addchip.sh` adds the chip `puron_new` with one
//! temperature of 30 °C, then `sensors -j` runs again. Expected: the
//! first output lacks `puron_new-isa-0000`, and the second lists it with
//! `temp1_input` 30.

use std::process::Command;

use puron_testkit::{fixture, host_need, temp_dir};

/// The exit code of `jq -e <filter> <file>`: 0 when the filter's last
/// output is true, 1 when it is false or null.
fn jq(filter: &str, file: &std::path::Path) -> Option<i32> {
    Command::new("jq")
        .args(["-e", filter])
        .arg(file)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run jq")
        .code()
}

#[test]
fn c_tests_9() {
    puron_testkit::limit("C-tests-9", 10, || {
        host_need(
            "libsensors5 and sensors",
            "dpkg-query -W libsensors5 lm-sensors",
        );
        host_need(
            "umockdev",
            "umockdev-run --version && test -e /usr/lib/x86_64-linux-gnu/libumockdev-preload.so.0",
        );
        host_need("jq", "jq --version");
        let out = temp_dir("c-tests-9");
        let status = Command::new("umockdev-run")
            .arg("-d")
            .arg(fixture("umd", "capture/testbed.umockdev"))
            .args(["--", "sh", "-c"])
            .arg(r#"sensors -j -c "$1" > "$2/first.json" && sh "$3" && sensors -j -c "$1" > "$2/second.json""#)
            .arg("sh")
            .arg(fixture("umd", "capture/sensors.conf"))
            .arg(&out)
            .arg(fixture("umd", "addchip.sh"))
            .status()
            .expect("run umockdev-run");
        assert!(status.success(), "umockdev-run: {status}");
        let has = r#"has("puron_new-isa-0000")"#;
        assert_eq!(
            jq(has, &out.join("first.json")),
            Some(1),
            "first run lists puron_new"
        );
        assert_eq!(
            jq(has, &out.join("second.json")),
            Some(0),
            "second run lacks puron_new"
        );
        let value = r#".["puron_new-isa-0000"].temp1.temp1_input == 30"#;
        assert_eq!(
            jq(value, &out.join("second.json")),
            Some(0),
            "temp1_input 30"
        );
        let _ = std::fs::remove_dir_all(&out);
    });
}
