//! Checks that the engine recovers from a provider's panic in a release
//! build; a script builds this program with the release profile and runs
//! it.
//!
//! On a virtual clock, with the default poll interval of 5 s, a fake
//! lm-sensors reads the two temperatures of `puron_testkit::eng::r1`,
//! then panics, then reads `r1` again, while a fake NVIDIA reads `r2`
//! every time. Expected: lm-sensors shows the values of `r1` at 0 s, its
//! readings without values at 5 s, and the values of `r1` again at 10 s;
//! NVIDIA shows the values of `r2` at 5 s. Prints a success line and exits 0 when every
//! check holds; otherwise prints the failed check and exits 1.

use puron_core::rows::Provider::{self, LmSensors, Nvidia};
use puron_core::settings::Settings;
use puron_engine::engine::{Engine, Snapshot};
use puron_testkit::eng::{adapters, r1, r2, Fake, ReadStep, VirtualClock};

/// The values of the readings of `p` in `s`, in order.
fn values(s: &Snapshot, p: Provider) -> Vec<Option<f64>> {
    s.readings
        .iter()
        .filter(|r| r.provider == p)
        .map(|r| r.value)
        .collect()
}

/// Unless `ok`, prints which check failed and exits 1.
fn check(what: &str, ok: bool) {
    if !ok {
        println!("C-engine-15 failed: {what}");
        std::process::exit(1);
    }
}

fn main() {
    // The fake's panic is expected; print it as one short line instead
    // of the default panic message.
    std::panic::set_hook(Box::new(|i| eprintln!("caught: {i}")));
    let clock = VirtualClock::new();
    let lm = Fake::new(clock.dyn_clock(), LmSensors).reads([
        ReadStep::Gives(r1(LmSensors)),
        ReadStep::Panics,
        ReadStep::Gives(r1(LmSensors)),
    ]);
    let nv = Fake::new(clock.dyn_clock(), Nvidia).reads([ReadStep::Gives(r2(Nvidia))]);
    let e = Engine::new(clock.dyn_clock());
    let settings = Settings {
        provider_nvidia: true,
        ..Settings::default()
    };
    e.start(&settings, adapters(&[lm, nv]));
    clock.settle();
    let mut last = e.try_next();
    let mut at = |t: f64| {
        clock.advance_to(t);
        if let Some(s) = e.try_next() {
            last = Some(s);
        }
        last.clone().unwrap_or(Snapshot {
            readings: vec![],
            lm_sensors: puron_engine::engine::LmReport::NoneYet,
        })
    };
    let r1v = vec![Some(40.0), Some(50.0)];
    let s = at(0.0);
    check("R1 at 0", values(&s, LmSensors) == r1v);
    let s = at(5.0);
    check("no value at 5", values(&s, LmSensors) == [None, None]);
    check(
        "NVIDIA's R2 at 5",
        values(&s, Nvidia) == [Some(41.0), Some(51.0)],
    );
    let s = at(10.0);
    check("R1 at 10", values(&s, LmSensors) == r1v);
    println!("C-engine-15 read-after-panic");
}
