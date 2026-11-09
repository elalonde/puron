//! A long-running load for finding leaks: it runs the engine with the
//! real providers, polling every second, and builds from each snapshot
//! what the applet would show, without a GUI. By default liquidctl is a
//! fake (see `FAKE`) that hangs on every tenth run.
//!
//! `puron-soak [--on <keys>] [--seconds <n>] [--liquidctl fake|real]`
//!
//! `<keys>` is a comma-separated list of `lmsensors`, `liquidctl`,
//! `nvidia`, `udisks2` and `nvme` (default: all five). It runs until
//! SIGTERM or for `<n>` seconds, then shuts the engine down, prints the
//! number of snapshots it handled and exits 0.
//!
//! `puron-soak --dump <root>` instead prints the text dump of what Puron
//! would show, with the settings stored under the configuration root
//! `<root>` (see `puron_testkit::d5`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use puron_core::menu::{self, Report};
use puron_core::panel::{self, State};
use puron_core::rows::Provider;
use puron_core::settings::Settings;
use puron_engine::clock::SystemClock;
use puron_engine::engine::{Engine, LmReport};
use puron_engine::registry::{self, Source};

/// A fake `liquidctl` program. It counts its runs in the file `count`
/// beside it. Every tenth run prints the start of a JSON list, `[{`, and
/// then hangs for 100 s; every other run prints `G.json` and exits 0.
const FAKE: &str = "#!/bin/sh
# Fake liquidctl of puron-soak.
d=$(dirname \"$0\")
n=$(( $(cat \"$d/count\" 2>/dev/null || echo 0) + 1 ))
echo \"$n\" > \"$d/count\"
if [ $(( n % 10 )) -eq 0 ]; then printf '[{'; exec sleep 100; fi
cat \"$d/G.json\"; exit 0
";
const G_JSON: &str = include_str!("../../fixtures/lqc/G.json");

/// Set by SIGTERM; the main loop then ends.
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn usage() -> ! {
    eprintln!("usage: puron-soak [--on <keys>] [--seconds <n>] [--liquidctl fake|real]");
    eprintln!("       puron-soak --dump <settings root>");
    std::process::exit(2);
}

/// Writes the fake and its `G.json` into a new temporary directory, puts
/// that directory first on `PATH`, and returns it. The liquidctl provider
/// looks up `PATH` when it starts, so it finds the fake.
fn install_fake() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("puron-soak-{}", std::process::id()));
    std::fs::create_dir(&dir).expect("puron-soak: fake directory");
    let script = dir.join("liquidctl");
    std::fs::write(&script, FAKE).expect("puron-soak: fake script");
    std::fs::write(dir.join("G.json"), G_JSON).expect("puron-soak: G.json");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("puron-soak: fake mode");
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![dir.clone()];
    paths.extend(std::env::split_paths(&old));
    std::env::set_var(
        "PATH",
        std::env::join_paths(paths).expect("puron-soak: PATH"),
    );
    dir
}

/// The settings of a run: a poll interval of 1 s, the providers in `on`
/// switched on and every other provider switched off; the remaining
/// settings keep their defaults.
fn settings(on: &[Provider]) -> Settings {
    Settings {
        poll_interval_secs: 1,
        provider_lm_sensors: on.contains(&Provider::LmSensors),
        provider_liquidctl: on.contains(&Provider::Liquidctl),
        provider_nvidia: on.contains(&Provider::Nvidia),
        provider_udisks2: on.contains(&Provider::Udisks2),
        provider_nvme: on.contains(&Provider::NvmeCli),
        ..Settings::default()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let [_, root] = &args[..] else { usage() };
        use puron_testkit::d5;
        let settings = d5::settings(Path::new(root));
        let text = d5::dump(
            SystemClock::new(),
            &settings,
            d5::registry_adapters(),
            d5::WAIT,
        );
        print!("{text}");
        std::process::exit(0);
    }
    let mut on = vec![
        Provider::LmSensors,
        Provider::Liquidctl,
        Provider::Nvidia,
        Provider::Udisks2,
        Provider::NvmeCli,
    ];
    let mut seconds: Option<u64> = None;
    let mut fake = true;
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).unwrap_or_else(|| usage());
        match args[i].as_str() {
            "--on" => {
                on = v
                    .split(',')
                    .map(|k| match k {
                        "lmsensors" => Provider::LmSensors,
                        "liquidctl" => Provider::Liquidctl,
                        "nvidia" => Provider::Nvidia,
                        "udisks2" => Provider::Udisks2,
                        "nvme" => Provider::NvmeCli,
                        _ => usage(),
                    })
                    .collect()
            }
            "--seconds" => seconds = Some(v.parse().unwrap_or_else(|_| usage())),
            "--liquidctl" => {
                fake = match v.as_str() {
                    "fake" => true,
                    "real" => false,
                    _ => usage(),
                }
            }
            _ => usage(),
        }
        i += 2;
    }
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(
            libc::SIGTERM,
            on_sigterm as extern "C" fn(libc::c_int) as libc::sighandler_t,
        )
    };
    let dir = fake.then(install_fake);

    let settings = settings(&on);
    let engine = Engine::new(SystemClock::new());
    engine.start(
        &settings,
        Box::new(|p| -> Box<dyn Source> {
            match p {
                Provider::LmSensors => Box::new(registry::lm_sensors()),
                Provider::Liquidctl => Box::new(registry::liquidctl()),
                Provider::Nvidia => Box::new(registry::nvidia()),
                Provider::Udisks2 => Box::new(registry::udisks2()),
                Provider::NvmeCli => Box::new(registry::nvme()),
            }
        }),
    );

    // From each new snapshot, build what the applet would show (the shown
    // rows, the menu and the panel items) and drop it at once, so that a
    // leak in that code grows with the length of the run. A snapshot is
    // looked for every 100 ms.
    let start = Instant::now();
    let mut last_key = None;
    let mut snapshots = 0u64;
    while !STOP.load(Ordering::SeqCst) && seconds.is_none_or(|n| start.elapsed().as_secs() < n) {
        let Some(s) = engine.try_next() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        snapshots += 1;
        let shown = menu::shown(&s.readings, &settings);
        let report = match s.lm_sensors {
            LmReport::NoneYet => Report::NoneYet,
            LmReport::Unavailable => Report::Unavailable,
            LmReport::Answered => Report::Answered,
        };
        let m = menu::menu(&shown, &settings, report, last_key.as_ref(), false);
        let pinned = settings.pinned.as_deref().unwrap_or_default();
        let p = panel::panel(&shown, pinned, &settings, &State::default());
        last_key = Some(m.key.clone());
        drop((shown, m, p));
    }
    engine.shutdown();
    if let Some(d) = dir {
        remove(&d);
    }
    println!("puron-soak: {snapshots} snapshots");
}

fn remove(d: &Path) {
    let _ = std::fs::remove_dir_all(d);
}
