//! The production NVML interface of the NVIDIA source, over
//! `nvml-wrapper`, which loads `libnvidia-ml.so` at each Open.
//! This file holds only the thin calls into NVML. The test suite cannot
//! run them without an NVIDIA GPU, so they are checked by hand on a
//! machine that has one.

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{Device, Nvml};

use crate::diag;
use crate::nvidia::{Gpu, Interface, OpenError, Session};

/// The `nvml-wrapper` interface.
#[derive(Debug, Default)]
pub struct NvmlWrapper;

impl Interface for NvmlWrapper {
    fn kind(&self) -> &'static str {
        "nvml-wrapper"
    }

    /// The source's one NVML start, the plain init with no flag: it
    /// initializes no GPU; the lookup by PCI address of an awake GPU
    /// initializes that GPU.
    fn open(&mut self) -> Result<Box<dyn Session>, OpenError> {
        match Nvml::init() {
            Ok(nvml) => Ok(Box::new(Open(nvml))),
            Err(
                NvmlError::LibloadingError(_)
                | NvmlError::FailedToLoadSymbol(_)
                | NvmlError::LibraryNotFound,
            ) => Err(OpenError::LibraryMissing),
            Err(e) => Err(OpenError::InitFailed(text(e))),
        }
    }
}

struct Open(Nvml);

impl Session for Open {
    fn device<'a>(&'a self, pci: &str) -> Result<Box<dyn Gpu + 'a>, String> {
        self.0
            .device_by_pci_bus_id(pci)
            .map(|d| Box::new(Handle(d)) as Box<dyn Gpu + 'a>)
            .map_err(text)
    }

    fn close(self: Box<Self>) -> Result<(), String> {
        self.0.shutdown().map_err(text)
    }
}

struct Handle<'a>(Device<'a>);

impl Gpu for Handle<'_> {
    fn name(&self) -> Result<String, String> {
        self.0.name().map_err(text)
    }

    fn temperature(&self) -> Result<u32, String> {
        self.0.temperature(TemperatureSensor::Gpu).map_err(text)
    }
}

/// The library's error text for the init-failed and call-failed events,
/// formed only when diag is on; else empty.
fn text(e: NvmlError) -> String {
    if diag::on() {
        e.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The library's text only when diag is on. This is the only test in
    // this binary that sets the diag switch.
    #[test]
    fn text_only_with_diag() {
        puron_testkit::limit("p4z1-nvml-text", 2, || {
            diag::switch(true);
            let on = text(NvmlError::Uninitialized);
            diag::switch(false);
            assert_eq!(on, NvmlError::Uninitialized.to_string());
            assert_eq!(text(NvmlError::Uninitialized), "");
        });
    }
}
