//! Each source reaches the real journal through `diag`'s production
//! socket. One test process, re-run under `umockdev-run` with a
//! libsensors configuration file that initialization rejects. The child
//! drives the engine, the four adapters and `app`, then polls
//! `journalctl _PID=<its pid>` every 0.2 s for up to 10 s. In its own
//! test binary.

mod common;

use std::os::fd::AsFd;
use std::process::Command;
use std::time::{Duration, Instant};

use common::*;
use puron_applet::app::{Event, Launched, Output, Programs};
use puron_core::rows::Provider::LmSensors;
use puron_core::settings::Settings;
use puron_engine::clock::SystemClock;
use puron_engine::diag;
use puron_engine::engine::Engine;
use puron_engine::liquidctl::Liquidctl;
use puron_engine::lmsensors::LmSensors as LmAdapter;
use puron_engine::nvidia::Nvidia;
use puron_engine::registry::{ReadOutcome, Source, StartOutcome};
use puron_engine::udisks2::{Bus, Udisks2};
use puron_testkit::eng::{adapters, Fake, ReadStep};
use puron_testkit::{cfg, fixture, nvml, sys, temp_dir, umd};

/// The six entries, as `<PURON_SOURCE> <PURON_EVENT>`.
const EXPECTED: [&str; 6] = [
    "engine panic",
    "src.lmsensors config-error",
    "src.nvidia init-failed",
    "src.udisks2 connect-failed",
    "src.liquidctl run-failed",
    "applet.app launch-failed",
];

/// Checks that journald stores debug priority and that the test user
/// can read its own entries.
const JRN_HOST: &str = "T=chk$$; echo x | systemd-cat -t $T -p debug; sleep 1; \
    journalctl -t $T -o json --no-pager | jq -er .MESSAGE";

fn read(src: &mut dyn Source) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// The number of the six entries in the journal for this process.
fn found(pid: u32) -> String {
    let mut cmd = format!(
        "journalctl _PID={pid} -o json --no-pager \
         | jq -r '.PURON_SOURCE + \" \" + .PURON_EVENT' | sort -u | grep -cxF"
    );
    for e in EXPECTED {
        cmd.push_str(&format!(" -e '{e}'"));
    }
    let out = Command::new("sh")
        .args(["-c", &cmd])
        .output()
        .expect("run journalctl");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn body() {
    // The production socket, switch on.
    diag::set_socket(diag::JOURNAL_SOCKET);
    diag::switch(true);

    // engine: a fake provider that panics in a read.
    let lm = Fake::new(SystemClock::new(), LmSensors).reads([ReadStep::Panics]);
    let e = Engine::new(SystemClock::new());
    let only_lm = Settings {
        provider_lm_sensors: true,
        provider_liquidctl: false,
        provider_nvidia: false,
        provider_udisks2: false,
        provider_nvme: false,
        ..Settings::default()
    };
    e.start(&only_lm, adapters(&[lm]));
    e.next();
    e.shutdown();

    // src.lmsensors: the configuration file that libsensors rejects.
    let mut s = LmAdapter::new(Some(umd::conf()), "/proc/cpuinfo");
    s.start();
    assert_eq!(read(&mut s), ReadOutcome::Unavailable, "lm-sensors");
    s.stop();

    // src.nvidia: NVML init fails at every read.
    let t = sys::build("pci-hybrid");
    let mut s = Nvidia::new(&t.root, Box::new(nvml::Fake::new(nvml::initfail())));
    assert_eq!(s.start(), StartOutcome::Available);
    assert_eq!(read(&mut s), ReadOutcome::Unavailable, "nvidia");
    s.stop();

    // src.udisks2: an address with no bus daemon.
    let nobus = format!(
        "unix:path={}",
        temp_dir("c-diag-10").join("nobus").display()
    );
    let mut s = Udisks2::new(Bus::Address(nobus), Duration::from_secs(1));
    assert_eq!(s.start(), StartOutcome::Unavailable, "udisks2");
    s.stop();

    // src.liquidctl: a fake liquidctl whose output is not JSON
    // (`invalid-2`).
    let log = temp_dir("c-diag-10").join("lqc.log");
    std::env::set_var("PURON_TEST_LQC_LOG", &log);
    let mut s = Liquidctl::new(fixture("lqc", "invalid-2"), Duration::from_secs(1));
    assert_eq!(s.start(), StartOutcome::Available);
    assert_eq!(read(&mut s), ReadOutcome::Failed, "liquidctl");
    s.stop();

    // applet.app: the settings link with no settings program.
    let c = cfg::build("good");
    let noprog = temp_dir("c-diag-10").join("noprog");
    std::fs::create_dir_all(&noprog).unwrap();
    let programs = Programs {
        settings_dir: noprog.clone(),
        search_path: noprog.into_os_string(),
    };
    let mut a = app_with(&c.root, &handle(0), programs);
    a.handle(Event::Start);
    let out = a.handle(Event::SettingsLink);
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Launch(_, Launched::Failed(_)))),
        "{out:?}"
    );

    // The journal, every 0.2 s for up to 10 s.
    let pid = std::process::id();
    let t0 = Instant::now();
    let mut n = found(pid);
    while n != "6" && t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
        n = found(pid);
    }
    let got = Command::new("sh")
        .args([
            "-c",
            &format!(
                "journalctl _PID={pid} -o json --no-pager \
                 | jq -r '.PURON_SOURCE + \" \" + .PURON_EVENT' | sort -u"
            ),
        ])
        .output()
        .expect("run journalctl");
    eprintln!(
        "C-diag-10: pid {pid}, after {:.1} s:\n{}",
        t0.elapsed().as_secs_f64(),
        String::from_utf8_lossy(&got.stdout)
    );
    assert_eq!(n, "6", "entries found of the six");
}

#[test]
fn c_diag_10() {
    puron_testkit::limit("C-diag-10", 30, || {
        if !umd::is_child() {
            puron_testkit::host_need("journald storing priority 7", JRN_HOST);
            puron_testkit::host_need("jq", "command -v jq");
        }
        let d = fixture("umd", "badconf");
        umd::run(
            "c_diag_10",
            &d.join("testbed.umockdev"),
            &d.join("sensors.conf"),
            body,
        );
    });
}
