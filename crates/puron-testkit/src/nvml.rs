//! A fake of NVML, the NVIDIA management library, for the tests of the
//! NVIDIA provider. [`Fake`] implements `puron_engine::nvidia::Interface`
//! and answers from a [`Script`] written in code: the outcome of each
//! `open` call in turn (the n-th `open` of the fake is the n-th read that
//! opens NVML), and, per PCI address, the GPU's name and temperature or
//! an error from one of its calls. Every call is logged in order, with
//! the address it names.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use puron_engine::nvidia::{Gpu, Interface, OpenError, Session};

/// One entry of the call log: the call and, for the calls about one
/// GPU, its PCI address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Open,
    Device(String),
    Name(String),
    Temperature(String),
    Close,
}

/// What one `open` call gives: a session (`Ok`),
/// `OpenError::LibraryMissing` (`Missing`), or `OpenError::InitFailed`
/// (`InitFail`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opens {
    Ok,
    Missing,
    InitFail,
}

/// What the GPU at one PCI address gives. `Good` holds its name and its
/// temperature in degrees Celsius. With `DeviceError` the session's
/// `device` call fails; with `NameError` or `TemperatureError` that one
/// call fails, and the other answers as the default GPU of [`good`] does
/// (name `GPU <address>`, 50 °C).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Good(String, u32),
    DeviceError,
    NameError,
    TemperatureError,
}

/// What the fake answers.
#[derive(Clone, Debug)]
pub struct Script {
    /// The outcomes of the first, second, ... `open` calls; the last one
    /// repeats for every later call. Must not be empty.
    pub opens: Vec<Opens>,
    /// The answer per PCI address. An address not listed answers as in
    /// [`good`].
    pub gpus: HashMap<String, Answer>,
}

/// The answer of [`good`] for `pci`: `NVIDIA GeForce RTX 3060` at
/// 45 °C for `0000:01:00.0`, `GPU <pci>` at 50 °C for any other address.
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

/// Every `open` succeeds, and every address gives a name and a
/// temperature (see `good_answer`).
pub fn good() -> Script {
    script(&[Opens::Ok], &[])
}

/// Every `open` fails as if the NVML library were not installed.
pub fn missing() -> Script {
    script(&[Opens::Missing], &[])
}

/// Every `open` finds the library, but its initialization fails.
pub fn initfail() -> Script {
    script(&[Opens::InitFail], &[])
}

/// The library is missing at the first `open`; from the second `open`
/// on, everything answers as in [`good`].
pub fn flaky() -> Script {
    script(&[Opens::Missing, Opens::Ok], &[])
}

/// Answers for the first seven addresses of the sysfs fixture
/// `pci-status`, in its address order. The first six are GPUs that the
/// provider must leave out: an empty name, a name of spaces only, a
/// temperature of 0, and an error from `device`, from `name` and from
/// `temperature`. The seventh must be kept: a name with spaces around
/// it, at 1 °C.
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

/// Two GPUs, for the sysfs fixture `pci-two`: `A` at 0 °C and `B` at
/// 50 °C.
pub fn names() -> Script {
    script(
        &[Opens::Ok],
        &[
            ("0000:01:00.0", good_name("A", 0)),
            ("0000:02:00.0", good_name("B", 50)),
        ],
    )
}

/// The fake NVML. Clones share the count of `open` calls and the log,
/// so a test can keep one clone to read the log while the provider owns
/// another.
#[derive(Clone)]
pub struct Fake {
    script: Arc<Script>,
    opened: Arc<Mutex<usize>>,
    log: Arc<Mutex<Vec<Call>>>,
}

impl Fake {
    /// A fake that answers from `script`, with no `open` call counted
    /// and an empty log.
    pub fn new(script: Script) -> Self {
        Fake {
            script: Arc::new(script),
            opened: Arc::new(Mutex::new(0)),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The calls so far, in order.
    pub fn log(&self) -> Vec<Call> {
        self.log.lock().unwrap().clone()
    }

    /// Empties the log. The count of `open` calls is kept, so the script
    /// goes on where it was.
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

/// A session opened by the fake; it logs each call into the fake's log.
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

/// The GPU at `pci` in a session; it answers from `answer`.
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
