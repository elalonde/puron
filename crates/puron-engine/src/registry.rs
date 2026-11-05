//! The provider registry: the providers that exist.

use std::os::fd::BorrowedFd;

use puron_core::rows::{Provider, RawReading};
use puron_core::settings::Settings;

/// What a provider's kind decides in the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    InProcess,
    ChildRun,
}

/// One entry of the provider table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub provider: Provider,
    /// The switch key in the settings.
    pub key: &'static str,
    pub kind: ProviderKind,
}

/// The provider table, in provider order.
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

/// The outcome of a Start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    Available,
    /// Carries no text for the user.
    Unavailable,
}

/// The outcome of a Read.
#[derive(Clone, Debug, PartialEq)]
pub enum ReadOutcome {
    Readings(Vec<RawReading>),
    Failed,
    /// Returned only by the lm-sensors and NVIDIA sources.
    Unavailable,
}

/// The provider contract: what every adapter offers to its
/// worker. None is called on the UI or the scheduler thread.
pub trait Source: Send {
    fn start(&mut self) -> StartOutcome;
    /// `stop` is the worker's stop signal; only a child-run read waits
    /// on it.
    fn read(&mut self, stop: BorrowedFd<'_>) -> ReadOutcome;
    /// Releases everything; called after any start.
    fn stop(&mut self);
}

/// One action of a switch change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start(Provider),
    Stop(Provider),
}

/// The value of a provider's switch in a settings record.
pub fn switch(settings: &Settings, provider: Provider) -> bool {
    match provider {
        Provider::LmSensors => settings.provider_lm_sensors,
        Provider::Liquidctl => settings.provider_liquidctl,
        Provider::Nvidia => settings.provider_nvidia,
        Provider::Udisks2 => settings.provider_udisks2,
        Provider::NvmeCli => settings.provider_nvme,
    }
}

/// The actions from `previous` (`None` at engine start) to `new`, in
/// provider order. Only the five switch keys are read.
pub fn actions(previous: Option<&Settings>, new: &Settings) -> Vec<Action> {
    PROVIDERS
        .iter()
        .filter_map(|e| {
            let was = previous.is_some_and(|p| switch(p, e.provider));
            match (was, switch(new, e.provider)) {
                (false, true) => Some(Action::Start(e.provider)),
                (true, false) => Some(Action::Stop(e.provider)),
                _ => None,
            }
        })
        .collect()
}

/// The lm-sensors adapter with its production values: the library's
/// default configuration files.
pub fn lm_sensors() -> crate::lmsensors::LmSensors {
    crate::lmsensors::LmSensors::new(None)
}

/// The nvme-cli adapter with its production values: the sysfs root
/// `/sys`.
pub fn nvme() -> crate::nvme::Nvme {
    crate::nvme::Nvme::new("/sys")
}

/// The NVIDIA adapter with its production values: the sysfs root `/sys`
/// and the `nvml-wrapper` interface.
pub fn nvidia() -> crate::nvidia::Nvidia {
    crate::nvidia::Nvidia::new("/sys", Box::new(crate::nvml::NvmlWrapper))
}

/// The udisks2 adapter with its production values: the system bus and
/// a bus time limit of 5 s.
pub fn udisks2() -> crate::udisks2::Udisks2 {
    crate::udisks2::Udisks2::new(
        crate::udisks2::Bus::System,
        std::time::Duration::from_secs(5),
    )
}

/// The liquidctl adapter with its production values: the applet's
/// `PATH`, or `/bin:/usr/bin` when it is not set, and a time limit of
/// 10 s.
pub fn liquidctl() -> crate::liquidctl::Liquidctl {
    crate::liquidctl::Liquidctl::new(
        std::env::var_os("PATH").unwrap_or_else(|| crate::liquidctl::DEFAULT_SEARCH_PATH.into()),
        crate::liquidctl::LIMIT,
    )
}
