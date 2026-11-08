//! The NVML fake of the NVIDIA provider: scripts in code. Per PCI
//! address a name and a temperature, or an error for one call; per `open` call
//! (the n-th `open` of the fake, which is the n-th read that opens NVML)
//! an outcome. Every call is logged in order with its address; Open
//! takes no argument.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use puron_engine::nvidia::{Gpu, Interface, OpenError, Session};

/// One entry of the call log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Open,
    Device(String),
    Name(String),
    Temperature(String),
    Close,
}

/// What one `open` call gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opens {
    Ok,
    Missing,
    InitFail,
}

/// What one address gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Good(String, u32),
    DeviceError,
    NameError,
    TemperatureError,
}

/// A script of the fake.
#[derive(Clone, Debug)]
pub struct Script {
    /// The outcome of the n-th `open`; the last one repeats.
    pub opens: Vec<Opens>,
    /// Per address; any other address as [`good`] gives it.
    pub gpus: HashMap<String, Answer>,
}

/// The [`good`] answer for `pci`.
fn good_answer(pci: &str) -> Answer {
    if pci == "0000:01:00.0" {
        Answer::Good("NVIDIA GeForce RTX 3060".into(), 45)
    } else {
        Answer::Good(format!("GPU {pci}"), 50)
    }
}

impl Script {
    fn answer(&self, pci: &str) -> Answer {
        self.gpus
            .get(pci)
            .cloned()
            .unwrap_or_else(|| good_answer(pci))
    }
}

fn script(opens: &[Opens], gpus: &[(&str, Answer)]) -> Script {
    Script {
        opens: opens.to_vec(),
        gpus: gpus
            .iter()
            .map(|(a, g)| (a.to_string(), g.clone()))
            .collect(),
    }
}

fn good_name(name: &str, temperature: u32) -> Answer {
    Answer::Good(name.into(), temperature)
}

/// Every `open` succeeds; every address gives a name and a temperature.
pub fn good() -> Script {
    script(&[Opens::Ok], &[])
}

/// NVML is missing at every `open`.
pub fn missing() -> Script {
    script(&[Opens::Missing], &[])
}

/// NVML fails to initialize at every `open`.
pub fn initfail() -> Script {
    script(&[Opens::InitFail], &[])
}

/// Missing at the first `open`, then as [`good`].
pub fn flaky() -> Script {
    script(&[Opens::Missing, Opens::Ok], &[])
}

/// NVML results for the GPUs that are left out (an empty or blank name,
/// a temperature of 0, a device, name or temperature error) and for the
/// edge case kept (a name with spaces, read untrimmed, at 1 degree), for
/// the first seven addresses of the sysfs fixture `pci-status`, in its
/// address order.
pub fn left() -> Script {
    script(
        &[Opens::Ok],
        &[
            ("0000:01:00.0", good_name("", 45)),
            ("0000:02:00.0", good_name("   ", 45)),
            ("0000:03:00.0", good_name("GPU 0000:03:00.0", 0)),
            ("0000:04:00.0", Answer::DeviceError),
            ("0000:05:00.0", Answer::NameError),
            ("0000:06:00.0", Answer::TemperatureError),
            ("0000:07:00.0", good_name(" NVIDIA RTX A2000 ", 1)),
        ],
    )
}

/// Two named GPUs, for the sysfs fixture `pci-two`.
pub fn names() -> Script {
    script(
        &[Opens::Ok],
        &[
            ("0000:01:00.0", good_name("A", 0)),
            ("0000:02:00.0", good_name("B", 50)),
        ],
    )
}

/// The fake interface; clones share the script position and the log.
#[derive(Clone)]
pub struct Fake {
    script: Arc<Script>,
    opened: Arc<Mutex<usize>>,
    log: Arc<Mutex<Vec<Call>>>,
}

impl Fake {
    pub fn new(script: Script) -> Self {
        Fake {
            script: Arc::new(script),
            opened: Arc::new(Mutex::new(0)),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The calls so far.
    pub fn log(&self) -> Vec<Call> {
        self.log.lock().unwrap().clone()
    }

    /// Empties the log.
    pub fn clear(&self) {
        self.log.lock().unwrap().clear();
    }

    fn push(&self, call: Call) {
        self.log.lock().unwrap().push(call);
    }
}

impl Interface for Fake {
    fn kind(&self) -> &'static str {
        "fake"
    }

    fn open(&mut self) -> Result<Box<dyn Session>, OpenError> {
        self.push(Call::Open);
        let n = {
            let mut opened = self.opened.lock().unwrap();
            *opened += 1;
            *opened - 1
        };
        let opens = &self.script.opens;
        match opens[n.min(opens.len() - 1)] {
            Opens::Ok => Ok(Box::new(FakeSession(self.clone()))),
            Opens::Missing => Err(OpenError::LibraryMissing),
            Opens::InitFail => Err(OpenError::InitFailed("fake init failed".into())),
        }
    }
}

struct FakeSession(Fake);

impl Session for FakeSession {
    fn device<'a>(&'a self, pci: &str) -> Result<Box<dyn Gpu + 'a>, String> {
        self.0.push(Call::Device(pci.into()));
        match self.0.script.answer(pci) {
            Answer::DeviceError => Err("fake Device error".into()),
            answer => Ok(Box::new(FakeGpu {
                fake: &self.0,
                pci: pci.into(),
                answer,
            })),
        }
    }

    fn close(self: Box<Self>) -> Result<(), String> {
        self.0.push(Call::Close);
        Ok(())
    }
}

struct FakeGpu<'a> {
    fake: &'a Fake,
    pci: String,
    answer: Answer,
}

impl Gpu for FakeGpu<'_> {
    fn name(&self) -> Result<String, String> {
        self.fake.push(Call::Name(self.pci.clone()));
        match &self.answer {
            Answer::NameError => Err("fake Name error".into()),
            Answer::Good(name, _) => Ok(name.clone()),
            _ => Ok(format!("GPU {}", self.pci)),
        }
    }

    fn temperature(&self) -> Result<u32, String> {
        self.fake.push(Call::Temperature(self.pci.clone()));
        match &self.answer {
            Answer::TemperatureError => Err("fake Temperature error".into()),
            Answer::Good(_, t) => Ok(*t),
            _ => Ok(50),
        }
    }
}
