//! Every part that logs diagnostics reaches the real systemd journal
//! through the journal's own socket. The test runs itself again under
//! `umockdev-run`, with a libsensors configuration file that libsensors
//! rejects. That child makes six parts log one event each: the engine,
//! the four sensor adapters and `App`. It then polls
//! `journalctl _PID=<its pid>` every 0.2 s for up to 10 s, and all six
//! entries must be found. The host must keep debug-priority entries and
//! have `jq`. The test has its own binary because the diagnostics switch
//! and socket are process-wide.

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

/// The six entries the child must cause, as
/// `<PURON_SOURCE> <PURON_EVENT>`.
const EXPECTED: [&str; 6] = [
    "engine panic",
    "src.lmsensors config-error",
    "src.nvidia init-failed",
    "src.udisks2 connect-failed",
    "src.liquidctl run-failed",
    "applet.app launch-failed",
];

/// A shell check of the host: journald keeps an entry of debug priority,
/// and the test user can read it back.
const JRN_HOST: &str = "T=chk$$; echo x | systemd-cat -t $T -p debug; sleep 1; \
    journalctl -t $T -o json --no-pager | jq -er .MESSAGE";

fn read(src: &mut dyn Source) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// How many of the six expected entries the journal holds for process
/// `pid`, as the text that `grep -c` prints.
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

/// The child's work, run under `umockdev-run`.
fn body() {
    // Log to the journal's own socket, with diagnostics on.
    diag::set_socket(diag::JOURNAL_SOCKET);
    diag::switch(true);

    // engine panic: a fake lm-sensors provider that panics in its first
    // read.
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

    // src.lmsensors config-error: the configuration file that libsensors
    // rejects makes the read report lm-sensors unavailable.
    let mut s = LmAdapter::new(Some(umd::conf()), "/proc/cpuinfo");
    s.start();
    assert_eq!(read(&mut s), ReadOutcome::Unavailable, "lm-sensors");
    s.stop();

    // src.nvidia init-failed: a fake NVML whose initialization fails at
    // every read.
    let t = sys::build("pci-hybrid");
    let mut s = Nvidia::new(&t.root, Box::new(nvml::Fake::new(nvml::initfail())));
    assert_eq!(s.start(), StartOutcome::Available);
    assert_eq!(read(&mut s), ReadOutcome::Unavailable, "nvidia");
    s.stop();

    // src.udisks2 connect-failed: a D-Bus address where no bus daemon
    // listens.
    let nobus = format!(
        "unix:path={}",
        temp_dir("c-diag-10").join("nobus").display()
    );
    let mut s = Udisks2::new(Bus::Address(nobus), Duration::from_secs(1));
    assert_eq!(s.start(), StartOutcome::Unavailable, "udisks2");
    s.stop();

    // src.liquidctl run-failed: a fake liquidctl (fixture `invalid-2`)
    // whose output is not JSON.
    let log = temp_dir("c-diag-10").join("lqc.log");
    std::env::set_var("PURON_TEST_LQC_LOG", &log);
    let mut s = Liquidctl::new(fixture("lqc", "invalid-2"), Duration::from_secs(1));
    assert_eq!(s.start(), StartOutcome::Available);
    assert_eq!(read(&mut s), ReadOutcome::Failed, "liquidctl");
    s.stop();

    // applet.app launch-failed: the settings link when no settings
    // program can be found.
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

    // Poll the journal every 0.2 s for up to 10 s until all six entries
    // are there, then print what was found.
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
