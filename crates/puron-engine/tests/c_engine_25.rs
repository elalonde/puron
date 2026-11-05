//! The aggregator logs `provider-updated` at each accepted report with
//! readings, to a test journal socket with diag switched on; fake
//! providers run on the virtual clock. Also checks the
//! start-unavailable and panic entries.
//! Own test binary: diag's switch and socket are per process.

use std::time::Duration;

use puron_core::rows::Provider::{LmSensors, Nvidia};
use puron_core::settings::Settings;
use puron_engine::diag;
use puron_engine::engine::Engine;
use puron_engine::registry::ReadOutcome;
use puron_testkit::eng::{adapters, r1, Fake, ReadStep, StartStep, VirtualClock};
use puron_testkit::jrn::{self, Journal};

#[test]
fn c_engine_25() {
    puron_testkit::limit("C-engine-25", 5, || {
        let j = Journal::new("c-engine-25");
        diag::set_socket(&j.path);
        diag::switch(true);
        let clock = VirtualClock::new();
        let lm = Fake::new(clock.dyn_clock(), LmSensors).reads([
            ReadStep::Gives(r1(LmSensors)),
            ReadStep::Gives(ReadOutcome::Failed),
            ReadStep::Gives(ReadOutcome::Readings(vec![])),
            ReadStep::Gives(ReadOutcome::Unavailable),
            ReadStep::PanicsText("boom"),
            ReadStep::PanicsValue,
        ]);
        let nv = Fake::new(clock.dyn_clock(), Nvidia).starts([StartStep::Unavailable]);
        let settings = Settings {
            provider_lm_sensors: true,
            provider_liquidctl: false,
            provider_nvidia: true,
            provider_udisks2: false,
            provider_nvme: false,
            ..Settings::default()
        };
        let e = Engine::new(clock.dyn_clock());
        e.start(&settings, adapters(&[lm.clone(), nv]));
        clock.settle();
        clock.advance_to(26.0);
        // Every send is done when advance_to returns; the reader may lag.
        let all = j.wait(usize::MAX, Duration::from_millis(500));
        let updated: Vec<_> = all
            .iter()
            .filter(|x| jrn::field(x, "PURON_EVENT") == Some("provider-updated"))
            .map(|x| jrn::field(x, "PURON_PROVIDER").unwrap_or(""))
            .collect();
        assert_eq!(updated, ["lm-sensors", "lm-sensors"]);
        let of = |event: &str, names: &[&str]| -> Vec<Vec<String>> {
            all.iter()
                .filter(|x| jrn::field(x, "PURON_EVENT") == Some(event))
                .map(|x| {
                    names
                        .iter()
                        .map(|n| jrn::field(x, n).unwrap_or("<none>").to_string())
                        .collect()
                })
                .collect()
        };
        assert_eq!(
            of("start-unavailable", &["PURON_SOURCE", "PURON_PROVIDER"]),
            [["engine", "NVIDIA"]]
        );
        assert_eq!(
            of("panic", &["PURON_PROVIDER", "PURON_PHASE", "PURON_TEXT"]),
            [["lm-sensors", "read", "boom"], ["lm-sensors", "read", ""]]
        );
        assert_eq!(lm.count(puron_testkit::eng::CallKind::Read), 6);
    });
}
