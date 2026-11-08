//! The sensor sources (one adapter per provider, such as lm-sensors or
//! NVIDIA) and the engine that runs them on their own threads and
//! publishes their readings.

pub mod clock;
pub mod diag;
pub mod engine;
pub mod liquidctl;
pub mod lmsensors;
pub mod nvidia;
pub mod nvme;
pub mod nvml;
pub mod registry;
pub mod udisks2;
