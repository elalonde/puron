//! Diagnostics: the entries the engine logs for its providers.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use std::time::Duration;

use puron_core::rows::Provider::{LmSensors, Nvidia};
use puron_core::settings::Settings;
use puron_engine::diag;
use puron_engine::engine::Engine;
use puron_engine::registry::ReadOutcome;
use puron_testkit::eng::{adapters, r1, Fake, ReadStep, StartStep, VirtualClock};
use puron_testkit::jrn::{self, Journal};

/// Runs the engine on the virtual clock with two fake providers switched
/// on and diagnostics sent to a test journal socket. At its successive
/// reads, lm-sensors gives readings, a failed read, an empty list of
/// readings, unavailable, a panic with the text `boom`, and a panic with
/// a payload that is not text; NVIDIA reports unavailable at start.
/// After 26 s of clock time, lm-sensors has been read 6 times, and the
/// journal holds: one `provider-updated` entry for lm-sensors for each
/// result with readings that the engine accepted (the first read and the
/// empty list, not the failed or unavailable read), one engine
/// `start-unavailable` entry for NVIDIA, and one `panic` entry for each
/// read that panicked, with the text `boom` and with an empty text.
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
        // The engine has sent every event when `advance_to` returns, but
        // the journal's reader thread may still be receiving them, so
        // take whatever has arrived within 500 ms.
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
