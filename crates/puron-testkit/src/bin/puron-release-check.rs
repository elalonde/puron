//! The engine's read-after-panic case, built with the release profile:
//! a Read that panics gives no value for that poll, the next Read works
//! again, and the other provider is not affected. Prints a success line
//! and exits 0 when the checks hold; prints the failed check and exits 1
//! otherwise.

use puron_core::rows::Provider::{self, LmSensors, Nvidia};
use puron_core::settings::Settings;
use puron_engine::engine::{Engine, Snapshot};
use puron_testkit::eng::{adapters, r1, r2, Fake, ReadStep, VirtualClock};

fn values(s: &Snapshot, p: Provider) -> Vec<Option<f64>> {
    s.readings
        .iter()
        .filter(|r| r.provider == p)
        .map(|r| r.value)
        .collect()
}

fn check(what: &str, ok: bool) {
    if !ok {
        println!("C-engine-15 failed: {what}");
        std::process::exit(1);
    }
}

fn main() {
    // A panic of the fake is expected; keep its message short.
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
