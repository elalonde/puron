//! The NVIDIA source's access to NVML (NVIDIA's management library) in
//! production, through the `nvml-wrapper` crate, which loads
//! `libnvidia-ml.so` at each `open`.
//!
//! This file holds only thin calls into NVML; the logic is in `nvidia`,
//! behind the `Interface` trait, where the tests replace NVML with a
//! fake. The test suite cannot run the calls in this file without an
//! NVIDIA GPU.

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{Device, Nvml};

use crate::diag;
use crate::nvidia::{Gpu, Interface, OpenError, Session};

/// The `Interface` that calls the real NVML through `nvml-wrapper`.
#[derive(Debug, Default)]
pub struct NvmlWrapper;

impl Interface for NvmlWrapper {
    fn kind(&self) -> &'static str {
        "nvml-wrapper"
    }

    /// Loads and initializes NVML with the plain init call, without
    /// flags. The init itself sets up no GPU; NVML sets up a GPU only when
    /// `Session::device` looks it up by PCI address. A missing library or
    /// symbol becomes `LibraryMissing`; any other error `InitFailed`.
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

/// The library's error text, for the diagnostic log. It is built only
/// when diagnostics are switched on (`diag::on`); otherwise the text is
/// empty.
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

    // `text` returns the library's message while diagnostics are on and
    // an empty string once they are off. No other test in this binary
    // switches diagnostics, because the switch is process-wide.
    #[test]
    fn text_only_with_diag() {
        puron_testkit::limit("text_only_with_diag", 2, || {
            diag::switch(true);
            let on = text(NvmlError::Uninitialized);
            diag::switch(false);
            assert_eq!(on, NvmlError::Uninitialized.to_string());
            assert_eq!(text(NvmlError::Uninitialized), "");
        });
    }
}
