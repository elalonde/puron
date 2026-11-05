//! The NVIDIA source: NVIDIA GPU temperatures through NVML.
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
//! At each Read the adapter lists the NVIDIA GPUs of
//! `bus/pci/devices/`, reads each one's runtime status without waking
//! it, and only when one is awake opens NVML, queries the awake GPUs by
//! PCI address and closes NVML before it returns. Every file is opened
//! for reading only. The adapter reads only the device list and each
//! device's vendor, class and runtime status, which the kernel keeps
//! without touching the device; it never opens a device's config or
//! resource files, since an access to the PCI configuration space
//! resumes a suspended GPU.

use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use puron_core::rows::{Kind, Provider, RawReading};

use crate::diag;
use crate::registry::{ReadOutcome, Source, StartOutcome};

/// The device directory.
const DEVICES: &str = "bus/pci/devices";
/// The NVIDIA vendor.
const NVIDIA: u32 = 0x10de;
/// The GPU base class, display controller.
const GPU_CLASS: u32 = 0x03;
/// The runtime statuses that count as awake.
const AWAKE: [&[u8]; 2] = [b"active", b"unsupported"];
/// The default name.
const DEFAULT_NAME: &str = "NVIDIA GPU";

/// Why Open gave no session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    LibraryMissing,
    /// The library's error text.
    InitFailed(String),
}

/// The NVML interface: production over `nvml-wrapper` (`crate::nvml`),
/// a fake in tests.
pub trait Interface: Send {
    /// `"nvml-wrapper"` for the production interface.
    fn kind(&self) -> &'static str;
    /// Open: loads the library and initializes NVML with its plain
    /// init, no flag and no argument.
    fn open(&mut self) -> Result<Box<dyn Session>, OpenError>;
}

/// An open NVML, until Close.
pub trait Session {
    /// Device: a handle by PCI address, the sysfs entry name.
    fn device<'a>(&'a self, pci: &str) -> Result<Box<dyn Gpu + 'a>, String>;
    /// Close: shuts NVML down and releases the library.
    fn close(self: Box<Self>) -> Result<(), String>;
}

/// A handle of one GPU.
pub trait Gpu {
    /// Name: the product name.
    fn name(&self) -> Result<String, String>;
    /// Temperature: the GPU core temperature, whole °C.
    fn temperature(&self) -> Result<u32, String>;
}

/// The NVIDIA adapter. `root` is the sysfs root.
pub struct Nvidia {
    root: PathBuf,
    nvml: Box<dyn Interface>,
    /// The last trimmed non-empty name per PCI address; kept
    /// for the adapter's life, through Stop and Start.
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

    /// The sysfs root parameter.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The NVML interface parameter.
    pub fn interface(&self) -> &dyn Interface {
        self.nvml.as_ref()
    }
}

/// A PCI address `DDDD:BB:DD.F`, lower-case hexadecimal.
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

/// The digits of an attribute `0x` and exactly `digits` hexadecimal
/// digits, with one optional final newline; `None` otherwise or when
/// the file cannot be read.
fn hex_attribute(path: &Path, digits: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let hex = bytes.strip_prefix(b"0x")?;
    if hex.len() != digits || !hex.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    String::from_utf8(hex.to_vec()).ok()
}

/// An NVIDIA GPU: vendor 0x10de and base class 0x03, the first two of
/// the six class digits.
fn is_gpu(dir: &Path) -> Option<bool> {
    let vendor = u32::from_str_radix(&hex_attribute(&dir.join("vendor"), 4)?, 16).ok()?;
    let class = hex_attribute(&dir.join("class"), 6)?;
    let base = u32::from_str_radix(class.get(..2)?, 16).ok()?;
    Some(vendor == NVIDIA && base == GPU_CLASS)
}

/// The NVIDIA GPUs of the device directory, by PCI address; an
/// unreadable directory gives none.
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

/// The runtime status: `active` or `unsupported`, with one optional final
/// newline; anything else, a missing file or a read error is asleep.
fn awake(dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(dir.join("power/runtime_status")) else {
        return false;
    };
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    AWAKE.contains(&bytes)
}

/// Device, Name and Temperature of one awake GPU: the trimmed name and
/// the temperature, or `None` when the GPU is left out (a failed call,
/// an empty name, 0 °C). A non-empty name is remembered, also at 0 °C.
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
    /// Nothing; always *available*.
    fn start(&mut self) -> StartOutcome {
        StartOutcome::Available
    }

    /// Lists the GPUs and queries the awake ones; *unavailable* only when
    /// Open fails.
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
            });
        }
        ReadOutcome::Readings(out)
    }

    /// Nothing to release; the name memory is kept.
    fn stop(&mut self) {}
}
