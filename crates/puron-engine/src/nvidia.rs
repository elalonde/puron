//! The NVIDIA source: the temperatures of NVIDIA GPUs, read through NVML
//! (NVIDIA's management library).
//!
//! The NVIDIA notice of the NVML header, as the header of
//! nvml-wrapper-sys carries it:
//!
//! Copyright 1993-2025 NVIDIA Corporation.  All rights reserved.
//!
//! NOTICE TO USER:
//!
//! This source code is subject to NVIDIA ownership rights under U.S. and
//! international Copyright laws.  Users and possessors of this source code
//! are hereby granted a nonexclusive, royalty-free license to use this code
//! in individual and commercial software.
//!
//! NVIDIA MAKES NO REPRESENTATION ABOUT THE SUITABILITY OF THIS SOURCE
//! CODE FOR ANY PURPOSE.  IT IS PROVIDED "AS IS" WITHOUT EXPRESS OR
//! IMPLIED WARRANTY OF ANY KIND.  NVIDIA DISCLAIMS ALL WARRANTIES WITH
//! REGARD TO THIS SOURCE CODE, INCLUDING ALL IMPLIED WARRANTIES OF
//! MERCHANTABILITY, NONINFRINGEMENT, AND FITNESS FOR A PARTICULAR PURPOSE.
//! IN NO EVENT SHALL NVIDIA BE LIABLE FOR ANY SPECIAL, INDIRECT, INCIDENTAL,
//! OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
//! OF USE, DATA OR PROFITS,  WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE
//! OR OTHER TORTIOUS ACTION,  ARISING OUT OF OR IN CONNECTION WITH THE USE
//! OR PERFORMANCE OF THIS SOURCE CODE.
//!
//! U.S. Government End Users.   This source code is a "commercial item" as
//! that term is defined at  48 C.F.R. 2.101 (OCT 1995), consisting  of
//! "commercial computer  software"  and "commercial computer software
//! documentation" as such terms are  used in 48 C.F.R. 12.212 (SEPT 1995)
//! and is provided to the U.S. Government only as a commercial end item.
//! Consistent with 48 C.F.R.12.212 and 48 C.F.R. 227.7202-1 through
//! 227.7202-4 (JUNE 1995), all U.S. Government End Users acquire the
//! source code with only those rights set forth herein.
//!
//! Any use of this source code in individual and commercial software must
//! include, in the user documentation and internal comments to the code,
//! the above Disclaimer and U.S. Government End Users Notice.
//!
//! The adapter must not wake a GPU that the kernel has suspended. At each
//! `read` it lists the NVIDIA GPUs under sysfs `bus/pci/devices/` and
//! reads each one's runtime power status. Only if at least one GPU is
//! awake does it open NVML; it then queries the awake GPUs by PCI
//! address and closes NVML before it returns. A suspended GPU is listed
//! without a value.
//!
//! From sysfs the adapter reads only the device list and each device's
//! `vendor`, `class` and `power/runtime_status` files, which the kernel
//! answers without touching the device, and it opens every file for
//! reading only. It never opens a device's `config` or `resource`
//! files, because an access to the PCI configuration space resumes a
//! suspended GPU.

use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use puron_core::rows::{Kind, Provider, RawReading};

use crate::diag;
use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The PCI device directory, relative to the sysfs root.
const DEVICES: &str = "bus/pci/devices";
/// NVIDIA's PCI vendor ID.
const NVIDIA: u32 = 0x10de;
/// The PCI base class of a display controller, which every GPU has.
const GPU_CLASS: u32 = 0x03;
/// The values of `power/runtime_status` that count as awake.
/// `unsupported` means the device has no runtime power management, so it
/// is never suspended.
const AWAKE: [&[u8]; 2] = [b"active", b"unsupported"];
/// The name shown for a suspended GPU whose name is unknown because it
/// has not been read awake since the adapter was created: "NVIDIA GPU"
/// for the first NVIDIA GPU by PCI address, "NVIDIA GPU 2" for the
/// second, and so on.
const DEFAULT_NAME: &str = "NVIDIA GPU";

/// Why `Interface::open` failed. Either way the read reports the
/// provider unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The NVML library, or a symbol in it, could not be loaded.
    LibraryMissing,
    /// NVML loaded but its init failed. Holds the library's error text;
    /// the production interface leaves it empty unless diagnostics are
    /// on.
    InitFailed(String),
}

/// Access to NVML, so that tests can replace the library with a fake.
/// The production implementation is `crate::nvml::NvmlWrapper`.
pub trait Interface: Send {
    /// A name for the implementation: `"nvml-wrapper"` in production.
    fn kind(&self) -> &'static str;
    /// Loads the library and initializes NVML with its plain init call,
    /// without flags or arguments.
    fn open(&mut self) -> Result<Box<dyn Session>, OpenError>;
}

/// An initialized NVML, from `Interface::open` until `close`.
pub trait Session {
    /// Returns the GPU at PCI address `pci`, written as its sysfs
    /// directory name (`DDDD:BB:DD.F`).
    fn device<'a>(&'a self, pci: &str) -> Result<Box<dyn Gpu + 'a>, String>;
    /// Shuts NVML down and releases the library.
    fn close(self: Box<Self>) -> Result<(), String>;
}

/// One GPU, as NVML reports it.
pub trait Gpu {
    /// The GPU's product name.
    fn name(&self) -> Result<String, String>;
    /// The GPU core temperature in whole degrees Celsius.
    fn temperature(&self) -> Result<u32, String>;
}

/// The NVIDIA adapter. `root` is the sysfs root, `/sys` in production.
pub struct Nvidia {
    root: PathBuf,
    nvml: Box<dyn Interface>,
    /// The last non-empty name NVML gave for each PCI address, trimmed.
    /// It names a GPU while it is suspended. It is kept for the adapter's
    /// whole life, through `stop` and `start`.
    names: HashMap<String, String>,
}

impl Nvidia {
    pub fn new(root: impl Into<PathBuf>, nvml: Box<dyn Interface>) -> Self {
        Nvidia {
            root: root.into(),
            nvml,
            names: HashMap::new(),
        }
    }

    /// The sysfs root the adapter was created with.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The NVML interface the adapter was created with.
    pub fn interface(&self) -> &dyn Interface {
        self.nvml.as_ref()
    }
}

/// Whether `name` is a PCI address as sysfs writes it: `DDDD:BB:DD.F`
/// in lower-case hexadecimal, with a function number from 0 to 7.
fn is_address(name: &str) -> bool {
    let b = name.as_bytes();
    let hex = |r: std::ops::Range<usize>| {
        b[r].iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
    };
    b.len() == 12
        && hex(0..4)
        && b[4] == b':'
        && hex(5..7)
        && b[7] == b':'
        && hex(8..10)
        && b[10] == b'.'
        && (b'0'..=b'7').contains(&b[11])
}

/// Reads a sysfs attribute of the form `0x` followed by exactly `digits`
/// hexadecimal digits and an optional final newline, and returns the
/// digits. Returns `None` when the file cannot be read or has any other
/// form.
fn hex_attribute(path: &Path, digits: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let hex = bytes.strip_prefix(b"0x")?;
    if hex.len() != digits || !hex.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    String::from_utf8(hex.to_vec()).ok()
}

/// Whether the PCI device in `dir` is an NVIDIA GPU: NVIDIA's vendor ID
/// and the display-controller base class, which is the first two of the
/// six digits of `class`. `None` when either file is unreadable or
/// malformed.
fn is_gpu(dir: &Path) -> Option<bool> {
    let vendor = u32::from_str_radix(&hex_attribute(&dir.join("vendor"), 4)?, 16).ok()?;
    let class = hex_attribute(&dir.join("class"), 6)?;
    let base = u32::from_str_radix(class.get(..2)?, 16).ok()?;
    Some(vendor == NVIDIA && base == GPU_CLASS)
}

/// The NVIDIA GPUs in the PCI device directory, as (PCI address,
/// directory), sorted by address. Entries that are not PCI addresses, or
/// whose vendor or class cannot be read, are skipped. An unreadable
/// directory gives no GPUs and logs an event.
fn gpus(devices: &Path) -> Vec<(String, PathBuf)> {
    let list = match std::fs::read_dir(devices) {
        Ok(list) => list,
        Err(_) => {
            diag::event(diag::nvidia::pci_list());
            return Vec::new();
        }
    };
    let mut out: Vec<(String, PathBuf)> = list
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !is_address(&name) {
                return None;
            }
            let dir = e.path();
            is_gpu(&dir)?.then_some((name, dir))
        })
        .collect();
    out.sort();
    out
}

/// Whether the GPU in `dir` is awake: its `power/runtime_status` is one
/// of `AWAKE`, with an optional final newline. Any other value, a
/// missing file or a read error counts as suspended.
fn awake(dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(dir.join("power/runtime_status")) else {
        return false;
    };
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    AWAKE.contains(&bytes)
}

/// Asks NVML for the name and temperature of the awake GPU at `pci`.
/// Returns the trimmed name and the temperature, or `None` to leave the
/// GPU out of the readings: when a call fails (each failure is logged),
/// the name is empty, or the temperature is 0 °C. A non-empty name is
/// stored in `names` even when the GPU is then left out.
fn query(
    session: &dyn Session,
    pci: &str,
    names: &mut HashMap<String, String>,
) -> Option<(String, u32)> {
    let failed = |call: &str, text: &str| diag::event(diag::nvidia::call_failed(pci, call, text));
    let gpu = session.device(pci).map_err(|e| failed("Device", &e)).ok()?;
    let name = gpu.name().map_err(|e| failed("Name", &e)).ok()?;
    let name = name.trim();
    if !name.is_empty() {
        names.insert(pci.to_string(), name.to_string());
    }
    let t = gpu
        .temperature()
        .map_err(|e| failed("Temperature", &e))
        .ok()?;
    (!name.is_empty() && t != 0).then(|| (name.to_string(), t))
}

impl Source for Nvidia {
    /// Does nothing: NVML is opened only during a read, and only while a
    /// GPU is awake. Always returns `Available`.
    fn start(&mut self) -> StartOutcome {
        StartOutcome::Available
    }

    /// Returns one reading per NVIDIA GPU, in PCI address order: an awake
    /// GPU with its name and temperature, unless `query` leaves it out; a
    /// suspended GPU with its stored or default name and no value. The
    /// result is `Unavailable` only when NVML cannot be opened.
    fn read(&mut self, _stop: BorrowedFd<'_>) -> ReadOutcome {
        let gpus: Vec<(String, bool)> = gpus(&self.root.join(DEVICES))
            .into_iter()
            .map(|(pci, dir)| {
                let awake = awake(&dir);
                (pci, awake)
            })
            .collect();
        let mut values: HashMap<&str, Option<(String, u32)>> = HashMap::new();
        if gpus.iter().any(|(_, awake)| *awake) {
            let session = match self.nvml.open() {
                Ok(s) => s,
                Err(OpenError::LibraryMissing) => {
                    diag::event(diag::nvidia::library_missing());
                    return ReadOutcome::Unavailable;
                }
                Err(OpenError::InitFailed(text)) => {
                    diag::event(diag::nvidia::init_failed(&text));
                    return ReadOutcome::Unavailable;
                }
            };
            for (pci, _) in gpus.iter().filter(|(_, awake)| *awake) {
                let got = query(session.as_ref(), pci, &mut self.names);
                values.insert(pci, got);
            }
            if session.close().is_err() {
                diag::event(diag::nvidia::close_failed());
            }
        }
        let mut out = Vec::new();
        for (place, (pci, awake)) in gpus.iter().enumerate() {
            let (name, value) = if *awake {
                match values.remove(pci.as_str()).flatten() {
                    Some((name, t)) => (name, Some(f64::from(t))),
                    None => continue,
                }
            } else {
                let name = match self.names.get(pci) {
                    Some(name) => name.clone(),
                    None if place == 0 => DEFAULT_NAME.to_string(),
                    None => format!("{DEFAULT_NAME} {}", place + 1),
                };
                (name, None)
            };
            out.push(RawReading {
                provider: Provider::Nvidia,
                position: out.len() as u32,
                kind: Some(Kind::Temperature),
                chip: None,
                name: Some(name),
                device: None,
                item: None,
                unit: None,
                key: pci.clone(),
                value,
                kernel_label: None,
                platform: false,
                device_chars: 0,
            });
        }
        ReadOutcome::Readings(out)
    }

    /// Does nothing: NVML is never left open between reads, and the
    /// stored names are kept on purpose.
    fn stop(&mut self) {}
}
