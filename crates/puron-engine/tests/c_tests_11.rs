//! A hung liquidctl run does not hold up another provider. Real clock:
//! the liquidctl worker waits in a real poll(2) during its run, so the
//! virtual clock cannot advance. Time limit 15 s.

use std::path::Path;
use std::time::{Duration, Instant};

use puron_core::rows::Provider;
use puron_core::settings::Settings;
use puron_engine::clock::SystemClock;
use puron_engine::engine::{Adapters, Engine};
use puron_engine::liquidctl::{self, Liquidctl};
use puron_engine::nvme::Nvme;
use puron_engine::registry::Source;
use puron_testkit::{fixture, sys, temp_dir};

const LOG_ENV: &str = "PURON_TEST_LQC_LOG";
/// The nvme readings that `c_src_nvme_1` expects: name, value, key.
/// The nvme readings of the captured sysfs tree: name, value, key.
const CAPTURE: [(&str, f64, &str); 2] = [
    ("Example SSD 1TB S1", 37.85, "0000:01:00.0/temp2"),
    ("Example SSD 1TB S2", 43.85, "0000:01:00.0/temp3"),
];

/// The processes whose parent is this process, every state counted
/// (`/proc/<n>/stat` field 4, after the command's closing parenthesis).
/// Starts no process.
fn children() -> Vec<i32> {
    let me = std::process::id() as i32;
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in dir.flatten() {
        let name = e.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else {
            continue;
        };
        let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
            continue;
        };
        let ppid = rest
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<i32>().ok());
        if ppid == Some(me) {
            out.push(pid);
        }
    }
    out
}

fn holds_nvme(s: &puron_engine::engine::Snapshot) -> bool {
    let got: Vec<(&str, Option<f64>, &str)> = s
        .readings
        .iter()
        .filter(|r| r.provider == Provider::NvmeCli)
        .map(|r| (r.name.as_deref().unwrap_or(""), r.value, r.key.as_str()))
        .collect();
    let want: Vec<(&str, Option<f64>, &str)> =
        CAPTURE.iter().map(|&(n, v, k)| (n, Some(v), k)).collect();
    got == want
}

#[test]
fn c_tests_11() {
    puron_testkit::limit("C-tests-11", 15, || {
        puron_testkit::host_need("/proc", "test -r /proc/self/stat");
        let tmp = temp_dir("c-tests-11");
        let log = tmp.join("lqc.log");
        // One test in this binary: no other thread reads the environment.
        std::env::set_var(LOG_ENV, &log);
        let t = sys::build("nvme-capture");
        let root = t.root.clone();
        let slow = fixture("lqc", "slow");
        let adapters: Adapters = Box::new(move |p| -> Box<dyn Source> {
            match p {
                Provider::NvmeCli => Box::new(Nvme::new(&root)),
                Provider::Liquidctl => Box::new(Liquidctl::new(slow.as_os_str(), liquidctl::LIMIT)),
                other => panic!("no adapter for {other:?}"),
            }
        });
        let settings = Settings {
            provider_lm_sensors: false,
            provider_liquidctl: true,
            provider_nvidia: false,
            provider_udisks2: false,
            provider_nvme: true,
            poll_interval_secs: 1,
            ..Settings::default()
        };
        let e = Engine::new(SystemClock::new());
        let t0 = Instant::now();
        e.start(&settings, adapters);
        let (mut nvme_snaps, mut lqc_snaps, mut max_children) = (0, 0, 0);
        let mut listings = Vec::new();
        let mut k = 1u32;
        loop {
            while let Some(s) = e.try_next() {
                if holds_nvme(&s) {
                    nvme_snaps += 1;
                }
                if s.readings.iter().any(|r| r.provider == Provider::Liquidctl) {
                    lqc_snaps += 1;
                }
            }
            let c = children();
            max_children = max_children.max(c.len());
            if c.len() > 1 {
                listings.push(c);
            }
            let next = Duration::from_millis(100 * u64::from(k));
            if next > Duration::from_millis(5500) {
                break;
            }
            if let Some(d) = next.checked_sub(t0.elapsed()) {
                std::thread::sleep(d);
            }
            k += 1;
        }
        let runs = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count();
        e.shutdown();
        let after = children();
        eprintln!(
            "C-tests-11: nvme snapshots {nvme_snaps}, liquidctl snapshots {lqc_snaps}, \
             fake runs {runs}, max children {max_children}, after shutdown {after:?}"
        );
        assert!(
            nvme_snaps >= 3,
            "nvme snapshots before Shut down: {nvme_snaps}"
        );
        assert_eq!(lqc_snaps, 0, "a snapshot holds a liquidctl reading");
        assert_eq!(runs, 1, "fake runs (log lines)");
        assert!(
            listings.is_empty(),
            "listings with more than one child: {listings:?}"
        );
        assert!(after.is_empty(), "children after Shut down: {after:?}");
        drop(t);
        let _ = std::fs::remove_dir_all(Path::new(&tmp));
    });
}
