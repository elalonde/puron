//! The list of providers and the interface their adapters implement.
//!
//! A provider is one source of sensor readings, such as lm-sensors or
//! NVIDIA. Its adapter is the type that implements `Source` for it. The
//! settings switch each provider on or off, and the engine starts and
//! stops the adapters to match.

use std::os::fd::BorrowedFd;

use puron_core::rows::{Provider, RawReading};
use puron_core::settings::Settings;

/// How a provider reads its sensors, which decides how the engine
/// treats a slow or failed read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    /// The adapter reads inside this process, through a library, sysfs or
    /// D-Bus. The engine marks such a read late when it runs past its
    /// deadline, and after a failed read it keeps showing the provider's
    /// last readings without values.
    InProcess,
    /// The adapter runs an external program for each read and enforces
    /// its own time limit. The program is ended early when the provider
    /// is stopped, and read requests that queued up while it ran are
    /// dropped.
    ChildRun,
}

/// One provider in `PROVIDERS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub provider: Provider,
    /// The name of the provider's on/off switch in the settings.
    pub key: &'static str,
    pub kind: ProviderKind,
}

/// Every provider, in a fixed order. The engine keeps its per-provider
/// state in this order and lists readings in it. lm-sensors must stay
/// first: the engine looks it up by index 0.
pub const PROVIDERS: &[Entry] = &[
    Entry {
        provider: Provider::LmSensors,
        key: "provider_lm_sensors",
        kind: ProviderKind::InProcess,
    },
    Entry {
        provider: Provider::Liquidctl,
        key: "provider_liquidctl",
        kind: ProviderKind::ChildRun,
    },
    Entry {
        provider: Provider::Nvidia,
        key: "provider_nvidia",
        kind: ProviderKind::InProcess,
    },
    Entry {
        provider: Provider::Udisks2,
        key: "provider_udisks2",
        kind: ProviderKind::InProcess,
    },
    Entry {
        provider: Provider::NvmeCli,
        key: "provider_nvme",
        kind: ProviderKind::InProcess,
    },
];

/// What `Source::start` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    /// The source is ready to be read.
    Available,
    /// The source cannot run. It carries no message for the user. The
    /// worker reads it no further until the provider is started again.
    Unavailable,
}

/// What `Source::read` returns, and what a worker reports to the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum ReadOutcome {
    /// The read succeeded with these readings, which may be none.
    Readings(Vec<RawReading>),
    /// The read failed. A panic in `read` is also reported as `Failed`.
    Failed,
    /// The source cannot run. Only the lm-sensors and NVIDIA sources
    /// return it from a read; the worker also reports it when `start`
    /// returned `Unavailable` or panicked.
    Unavailable,
}

/// The interface every adapter implements. The engine calls these methods
/// only on the provider's own worker thread, never on the UI thread or
/// the scheduler thread, so a slow call delays only that provider.
pub trait Source: Send {
    /// Gives the source the settings in effect, just before each `start`.
    /// A source that takes a choice from the settings beyond its on/off
    /// switch keeps it here and applies it from that start on; the others
    /// keep nothing, which is the default. When such a choice changes
    /// while the provider is on, the engine stops the provider and starts
    /// it again (see `restarts`), so a source never has to change its
    /// choice between two reads.
    fn configure(&mut self, _settings: &Settings) {}
    /// Prepares the source for reading. Called each time the provider is
    /// switched on.
    fn start(&mut self) -> StartOutcome;
    /// Reads the sensors once. `stop` is the worker's stop signal, an
    /// eventfd that becomes readable when the provider is stopped or the
    /// engine shuts down. Only a child-run source waits on it, to end its
    /// program early; the in-process sources ignore it.
    fn read(&mut self, stop: BorrowedFd<'_>) -> ReadOutcome;
    /// Releases everything the source holds. The worker calls it when the
    /// provider is stopped or the engine shuts down, if the provider was
    /// started, whether that start returned `Available` or not.
    fn stop(&mut self);
}

/// What `actions` asks the engine to do for one provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start(Provider),
    Stop(Provider),
}

/// Whether `settings` switches `provider` on.
pub fn switch(settings: &Settings, provider: Provider) -> bool {
    match provider {
        Provider::LmSensors => settings.provider_lm_sensors,
        Provider::Liquidctl => settings.provider_liquidctl,
        Provider::Nvidia => settings.provider_nvidia,
        Provider::Udisks2 => settings.provider_udisks2,
        Provider::NvmeCli => settings.provider_nvme,
    }
}

/// Whether a change of the settings from `previous` to `new` changes a
/// choice that the source of `provider` takes in `Source::configure`. A
/// provider that stays on through such a change is started again, so that
/// its source gets the new choice. Only lm-sensors has one: whether it
/// reads the chips of drives.
pub fn restarts(previous: &Settings, new: &Settings, provider: Provider) -> bool {
    match provider {
        Provider::LmSensors => {
            previous.provider_lm_sensors_drives != new.provider_lm_sensors_drives
        }
        Provider::Liquidctl | Provider::Nvidia | Provider::Udisks2 | Provider::NvmeCli => false,
    }
}

/// The providers to start and to stop when the settings change from
/// `previous` to `new`, in `PROVIDERS` order. `previous` is `None` at
/// engine start, when every provider counts as off. A provider that stays
/// on is stopped and then started when `restarts` says that its source's
/// choice changed. Only the provider switches and those choices are
/// compared; other settings produce no action.
pub fn actions(previous: Option<&Settings>, new: &Settings) -> Vec<Action> {
    PROVIDERS
        .iter()
        .flat_map(|e| {
            let p = e.provider;
            let was = previous.is_some_and(|s| switch(s, p));
            let restart = previous.is_some_and(|s| restarts(s, new, p));
            match (was, switch(new, p)) {
                (false, true) => vec![Action::Start(p)],
                (true, false) => vec![Action::Stop(p)],
                (true, true) if restart => vec![Action::Stop(p), Action::Start(p)],
                _ => vec![],
            }
        })
        .collect()
}

/// The lm-sensors adapter as used in production: the library's default
/// configuration files, and `/proc/cpuinfo` for the CPU's name.
pub fn lm_sensors() -> crate::lmsensors::LmSensors {
    crate::lmsensors::LmSensors::new(None, "/proc/cpuinfo")
}

/// The NVMe adapter as used in production, reading sysfs at `/sys`.
pub fn nvme() -> crate::nvme::Nvme {
    crate::nvme::Nvme::new("/sys")
}

/// The NVIDIA adapter as used in production: sysfs at `/sys`, and NVML
/// through the `nvml-wrapper` crate.
pub fn nvidia() -> crate::nvidia::Nvidia {
    crate::nvidia::Nvidia::new("/sys", Box::new(crate::nvml::NvmlWrapper))
}

/// The udisks2 adapter as used in production: the system D-Bus, with a
/// time limit of 5 s for each bus call.
pub fn udisks2() -> crate::udisks2::Udisks2 {
    crate::udisks2::Udisks2::new(
        crate::udisks2::Bus::System,
        std::time::Duration::from_secs(5),
    )
}

/// The liquidctl adapter as used in production. It searches for the
/// program in this process's `PATH`, or in `DEFAULT_SEARCH_PATH` when
/// `PATH` is not set, with the time limit `liquidctl::LIMIT`.
pub fn liquidctl() -> crate::liquidctl::Liquidctl {
    crate::liquidctl::Liquidctl::new(
        std::env::var_os("PATH").unwrap_or_else(|| crate::liquidctl::DEFAULT_SEARCH_PATH.into()),
        crate::liquidctl::LIMIT,
    )
}
