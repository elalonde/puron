//! Tests of the liquidctl source, through `start`, `read` and `stop` with
//! fixture directories of fake `liquidctl` programs as the search path.
//!
//! The fakes log to the file named by `PURON_TEST_LQC_LOG` and write a
//! grandchild's number to `PURON_TEST_LQC_CHILD`. The adapter passes the
//! applet's environment on unchanged, so each test runs its body in a
//! re-executed child of the test binary (as the libsensors tests do,
//! without `umockdev-run`) whose environment names its own files; the
//! parent test process never changes its own environment or directory.

#![allow(non_snake_case)]

use std::ffi::OsString;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use puron_core::rows::{self, Provider, RawReading, ShownClasses};
use puron_engine::liquidctl::{self, Liquidctl};
use puron_engine::registry::{self, ReadOutcome, Source, StartOutcome};
use puron_testkit::{fixture, rerun_self, temp_dir, umd};

/// Test time limit: the runner's time limit plus 5 s.
const SHORT: u64 = 6;
const LONG: u64 = 15;
/// Time limit of `c_src_liquidctl_10`: 5 s.
const C10: u64 = 5;

const ONE: Duration = Duration::from_secs(1);
const TEN: Duration = Duration::from_secs(10);

/// Set in the re-executed child.
const CHILD_ENV: &str = "PURON_LQC_CHILD";
const LOG_ENV: &str = "PURON_TEST_LQC_LOG";
const GRANDCHILD_ENV: &str = "PURON_TEST_LQC_CHILD";

/// The tools the fake programs need on the test machine; checked on the
/// parent side before any child.
fn need() {
    puron_testkit::host_need(
        "/bin/sh, mkfifo, setsid, pgrep, ps, /proc",
        "command -v mkfifo setsid pgrep ps",
    );
}

fn is_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some()
}

/// Parent: the command that runs test `test` again as a child, with a
/// fresh log file and grandchild file, killed if the parent ends.
fn child_command(test: &str) -> Command {
    need();
    let dir = temp_dir("lqc");
    let mut cmd = rerun_self(test);
    cmd.env(CHILD_ENV, "1")
        .env(LOG_ENV, dir.join("log"))
        .env(GRANDCHILD_ENV, dir.join("child"))
        .stdin(Stdio::null());
    // SAFETY: prctl is async-signal-safe; it only sets a flag of the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    cmd
}

/// Parent: runs the child with `setup` applied; panics unless it exits
/// 0; returns its standard output.
fn run_child(test: &str, setup: impl FnOnce(&mut Command)) -> String {
    let mut cmd = child_command(test);
    setup(&mut cmd);
    let out = cmd.output().expect("run the child test");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "child {test}: {}\n{text}\n{err}",
        out.status
    );
    text
}

/// The `RESULT ` lines of the child's output (the harness may print
/// the test name first on the same line).
fn results(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|l| l.find("RESULT ").map(|i| &l[i..]))
        .collect()
}

fn dir(name: &str) -> PathBuf {
    fixture("lqc", name)
}

/// A search path from directories, joined with `:`.
fn search(dirs: &[&Path]) -> OsString {
    let parts: Vec<&std::ffi::OsStr> = dirs.iter().map(|d| d.as_os_str()).collect();
    parts.join(std::ffi::OsStr::new(":"))
}

/// One log line of a fake.
#[derive(Debug)]
struct Line {
    args: String,
    pid: i32,
    pgid: i32,
    stdin_eof: String,
    fd2: String,
}

fn log() -> Vec<Line> {
    let path = std::env::var_os(LOG_ENV).expect("log file named");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().map(parse_line).collect()
}

fn parse_line(l: &str) -> Line {
    let rest = l.strip_prefix("args=").expect("args=");
    let (args, rest) = rest.split_once(" pid=").expect("pid=");
    let (pid, rest) = rest.split_once(" pgid=").expect("pgid=");
    let (pgid, rest) = rest.split_once(" stdin_eof=").expect("stdin_eof=");
    let (stdin_eof, fd2) = rest.split_once(" fd2=").expect("fd2=");
    Line {
        args: args.to_string(),
        pid: pid.parse().expect("pid"),
        pgid: pgid.parse().expect("pgid"),
        stdin_eof: stdin_eof.to_string(),
        fd2: fd2.to_string(),
    }
}

/// The process number of the last fake run.
fn fake_pid() -> i32 {
    log().last().expect("a fake ran").pid
}

/// The grandchild's process number.
fn grandchild_pid() -> i32 {
    let path = std::env::var_os(GRANDCHILD_ENV).expect("grandchild file named");
    std::fs::read_to_string(path)
        .expect("grandchild file")
        .trim()
        .parse()
        .expect("grandchild number")
}

/// "Gone": `kill -0` fails and `/proc/<n>` does not exist.
fn gone(pid: i32) -> bool {
    // SAFETY: signal 0 only checks the process.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    !alive && !Path::new(&format!("/proc/{pid}")).exists()
}

/// Start; it must be available.
fn started(search_path: impl Into<OsString>, limit: Duration) -> Liquidctl {
    let mut src = Liquidctl::new(search_path, limit);
    assert_eq!(src.start(), StartOutcome::Available);
    src
}

fn read(src: &mut Liquidctl) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

fn readings(src: &mut Liquidctl) -> Vec<RawReading> {
    match read(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// Device, Item, Unit, Value, Key of one reading.
type Exp = (
    &'static str,
    &'static str,
    &'static str,
    Option<f64>,
    &'static str,
);

const KRAKEN: &str = "NZXT Kraken X (X42, X52, X62 or X72)";
const PRO: &str = "Corsair Commander Pro";

const G: [Exp; 7] = [
    (
        KRAKEN,
        "Liquid temperature",
        "°C",
        Some(30.1),
        "NZXT Kraken X (X42, X52, X62 or X72)/Liquid temperature",
    ),
    (
        KRAKEN,
        "Fan speed",
        "rpm",
        Some(1000.0),
        "NZXT Kraken X (X42, X52, X62 or X72)/Fan speed",
    ),
    (
        KRAKEN,
        "Pump speed",
        "rpm",
        Some(2000.0),
        "NZXT Kraken X (X42, X52, X62 or X72)/Pump speed",
    ),
    (
        KRAKEN,
        "Firmware version",
        "",
        None,
        "NZXT Kraken X (X42, X52, X62 or X72)/Firmware version",
    ),
    (
        PRO,
        "Fan 1 speed",
        "rpm",
        Some(800.0),
        "Corsair Commander Pro/Fan 1 speed",
    ),
    (
        PRO,
        "Fan 1 duty",
        "%",
        Some(40.0),
        "Corsair Commander Pro/Fan 1 duty",
    ),
    (
        PRO,
        "+12V rail",
        "V",
        Some(12.07),
        "Corsair Commander Pro/+12V rail",
    ),
];

/// Every field of the readings: liquidctl, Positions 0, 1, ..., no
/// kind, chip or name, and the expected Device, Item, Unit, Value, Key.
fn check(readings: &[RawReading], exp: &[Exp]) {
    type Got = (String, String, String, Option<f64>, String);
    let got: Vec<Got> = readings
        .iter()
        .map(|r| {
            (
                r.device.clone().unwrap_or_default(),
                r.item.clone().unwrap_or_default(),
                r.unit.clone().unwrap_or_default(),
                r.value,
                r.key.clone(),
            )
        })
        .collect();
    let want: Vec<Got> = exp
        .iter()
        .map(|(d, i, u, v, k)| {
            (
                d.to_string(),
                i.to_string(),
                u.to_string(),
                *v,
                k.to_string(),
            )
        })
        .collect();
    assert_eq!(got, want);
    for (n, r) in readings.iter().enumerate() {
        assert_eq!(r.provider, Provider::Liquidctl);
        assert_eq!(r.position, n as u32, "position of {:?}", r.item);
        assert_eq!((&r.kind, &r.chip, &r.name), (&None, &None, &None));
        assert!(r.device.is_some() && r.item.is_some() && r.unit.is_some());
    }
}

fn elapsed(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64()
}

#[test]
fn c_src_liquidctl_1() {
    puron_testkit::limit("C-src.liquidctl-1", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_1", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        let r = readings(&mut src);
        check(&r, &G);
        let got: Vec<(String, Option<f64>)> = rows::rows(&r, &ShownClasses::ALL)
            .into_iter()
            .map(|row| (row.name, row.value))
            .collect();
        let want = [
            ("Kraken X Liquid", 30.1),
            ("Kraken X Fan", 1000.0),
            ("Kraken X Pump", 2000.0),
            ("Commander Pro Fan 1", 800.0),
            ("Commander Pro +12V rail", 12.07),
        ];
        let want: Vec<(String, Option<f64>)> = want
            .iter()
            .map(|(n, v)| (n.to_string(), Some(*v)))
            .collect();
        assert_eq!(got, want);
        src.stop();
    });
}

/// Runner case: standard input closed in the reading process, so
/// `pidfd_open` may return descriptor 0, which is a valid descriptor.
#[test]
fn c_src_liquidctl_1__stdin_closed() {
    puron_testkit::limit("C-src.liquidctl-1", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_1__stdin_closed", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        // The stop signal first, so that it does not take descriptor 0.
        let stop = umd::stop_fd();
        // SAFETY: closes this child process's standard input only.
        unsafe { libc::close(0) };
        match src.read(stop.as_fd()) {
            ReadOutcome::Readings(r) => check(&r, &G),
            other => panic!("standard input closed: {other:?}"),
        }
    });
}

/// Runner case: signals to the reading thread interrupt `poll(2)`
/// (EINTR), sent as often as the signal thread runs; the run still
/// completes. One EINTR is enough to show a mishandled EINTR.
#[test]
fn c_src_liquidctl_1__eintr() {
    puron_testkit::limit("C-src.liquidctl-1", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_1__eintr", |_| {});
            return;
        }
        extern "C" fn nothing(_: libc::c_int) {}
        // SAFETY: installs an empty handler without SA_RESTART, so a
        // blocked poll(2) returns EINTR.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = nothing as extern "C" fn(libc::c_int) as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            sa.sa_flags = 0;
            assert_eq!(libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut()), 0);
        }
        let mut src = started(dir("good"), ONE);
        // SAFETY: pthread_self has no preconditions.
        let me = unsafe { libc::pthread_self() } as usize;
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (d, n) = (done.clone(), sent.clone());
        let kicker = std::thread::spawn(move || {
            while !d.load(std::sync::atomic::Ordering::SeqCst) {
                // SAFETY: the reading thread lives until `done` is set.
                unsafe { libc::pthread_kill(me as libc::pthread_t, libc::SIGUSR1) };
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::yield_now();
            }
        });
        let r = read(&mut src);
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        kicker.join().expect("signal thread");
        assert!(
            sent.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "no signal sent"
        );
        match r {
            ReadOutcome::Readings(r) => check(&r, &G),
            other => panic!("EINTR ended the run: {other:?}"),
        }
    });
}

/// Runner case: `pidfd_open` fails (ENOSYS, as on a kernel before 5.3,
/// made by a seccomp filter on the reading thread): the run is not
/// started and the `read` returns *failed* at once.
#[test]
fn c_src_liquidctl_1__nopidfd() {
    puron_testkit::limit("C-src.liquidctl-1", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_1__nopidfd", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        deny_pidfd_open();
        let t0 = Instant::now();
        assert_eq!(read(&mut src), ReadOutcome::Failed);
        let t = elapsed(t0);
        assert!(t < 0.5, "returned at {t} s");
    });
}

/// A seccomp filter on this thread: `pidfd_open` fails with ENOSYS.
fn deny_pidfd_open() {
    const LD_W_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    const JEQ_K: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
    const RET_K: u16 = 0x06; // BPF_RET | BPF_K
    const RET_ERRNO: u32 = 0x0005_0000;
    const RET_ALLOW: u32 = 0x7fff_0000;
    let f = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let mut prog = [
        f(LD_W_ABS, 0, 0, 0), // seccomp_data.nr
        f(JEQ_K, 0, 1, libc::SYS_pidfd_open as u32),
        f(RET_K, 0, 0, RET_ERRNO | libc::ENOSYS as u32),
        f(RET_K, 0, 0, RET_ALLOW),
    ];
    let fprog = libc::sock_fprog {
        len: prog.len() as u16,
        filter: prog.as_mut_ptr(),
    };
    // SAFETY: prctl with a valid filter program that lives across the call.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &fprog as *const _
            ),
            0
        );
    }
}

#[test]
fn c_src_liquidctl_2() {
    puron_testkit::limit("C-src.liquidctl-2", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_2", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        assert_eq!(log().len(), 0, "a run at Start");
        for _ in 0..3 {
            check(&readings(&mut src), &G);
        }
        src.stop();
        let lines = log();
        assert_eq!(lines.len(), 3, "{lines:?}");
        for l in &lines {
            assert_eq!(l.args, "status --json", "{l:?}");
            assert_eq!(l.pgid, l.pid, "{l:?}");
            assert_eq!(l.stdin_eof, "yes", "{l:?}");
            assert_eq!(l.fd2, "/dev/null", "{l:?}");
        }
    });
}

#[test]
fn c_src_liquidctl_3() {
    puron_testkit::limit("C-src.liquidctl-3", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_3", |_| {});
            return;
        }
        let names = [
            "empty",
            "invalid-1",
            "invalid-2",
            "invalid-3",
            "invalid-4",
            "invalid-5",
            "invalid-6",
            "invalid-7",
        ];
        for name in names {
            let mut src = started(dir(name), ONE);
            assert_eq!(read(&mut src), ReadOutcome::Failed, "{name}");
            src.stop();
        }
    });
}

#[test]
fn c_src_liquidctl_4() {
    puron_testkit::limit("C-src.liquidctl-4", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_4", |_| {});
            return;
        }
        let mut src = started(dir("shape"), ONE);
        check(
            &readings(&mut src),
            &[
                ("A B", "X temperature", "°C", None, "A B/X temperature"),
                ("A B", "Y speed", "rpm", None, "A B/Y speed"),
                ("A B", "Z voltage", "V", Some(1.5), "A B/Z voltage"),
            ],
        );
    });
}

#[test]
fn c_src_liquidctl_5() {
    puron_testkit::limit("C-src.liquidctl-5", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_5", |_| {});
            return;
        }
        let mut src = started(dir("exit"), ONE);
        check(&readings(&mut src), &G);
    });
}

/// Log case: with debug logging on, the exit status 1 is logged once; a
/// run with status 0 logs no exit-status event.
#[test]
fn c_src_liquidctl_5__event() {
    puron_testkit::limit("C-src.liquidctl-5", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_5__event", |_| {});
            return;
        }
        let j = puron_testkit::jrn::Journal::new("c-lqc-5");
        puron_engine::diag::set_socket(&j.path);
        puron_engine::diag::switch(true);
        // After a `read`, a marker datagram from this thread is queued after
        // every event of that `read`; waiting for the `m`-th marker leaves no
        // timing bound on the pass path.
        let sock = std::os::unix::net::UnixDatagram::unbound().expect("socket");
        let statuses = |j: &puron_testkit::jrn::Journal, m: usize| -> Vec<String> {
            sock.send_to(b"MESSAGE=marker\n", &j.path)
                .expect("send marker");
            let mut n = 1;
            let entries = loop {
                let e = j.wait(n, Duration::from_secs(SHORT));
                let markers = e
                    .iter()
                    .filter(|e| puron_testkit::jrn::field(e, "MESSAGE") == Some("marker"))
                    .count();
                if markers >= m {
                    break e;
                }
                assert!(e.len() >= n, "marker {m} not received");
                n = e.len() + 1;
            };
            entries
                .iter()
                .filter(|e| puron_testkit::jrn::field(e, "PURON_EVENT") == Some("exit-status"))
                .map(|e| {
                    puron_testkit::jrn::field(e, "PURON_STATUS")
                        .unwrap_or("<none>")
                        .to_string()
                })
                .collect()
        };
        let mut src = started(dir("good"), ONE);
        check(&readings(&mut src), &G);
        assert_eq!(statuses(&j, 1), Vec::<String>::new(), "status 0 logged");
        let mut src = started(dir("exit"), ONE);
        check(&readings(&mut src), &G);
        assert_eq!(statuses(&j, 2), vec!["1".to_string()]);
    });
}

#[test]
fn c_src_liquidctl_6() {
    puron_testkit::limit("C-src.liquidctl-6", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_6", |_| {});
            return;
        }
        // The slow fake, then the one that closes its output early: end of
        // file before the child has exited does not complete the run.
        for name in ["slow", "closed"] {
            let mut src = started(dir(name), ONE);
            let t0 = Instant::now();
            let outcome = read(&mut src);
            let t = elapsed(t0);
            assert_eq!(outcome, ReadOutcome::Failed, "{name}");
            assert!((1.0..=1.5).contains(&t), "{name}: returned at {t} s");
            let pid = fake_pid();
            assert!(gone(pid), "{name}: fake {pid} not gone");
        }
    });
}

/// "Gone" for a grandchild: killed, that is `kill -0` fails, or its
/// `/proc/<n>/stat` state is `Z`, or SIGKILL is pending for it (bit 0x100
/// of `SigPnd` or `ShdPnd` in `/proc/<n>/status`). A failed read means it
/// was reaped meanwhile.
fn grandchild_gone(pid: i32) -> bool {
    // SAFETY: signal 0 only checks the process.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return true;
    };
    // The state is the first field after the last ')' (the name may hold one).
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    if state == Some("Z") {
        return true;
    }
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return true;
    };
    status.lines().any(|l| {
        let Some((k, v)) = l.split_once(':') else {
            return false;
        };
        (k == "SigPnd" || k == "ShdPnd")
            && u64::from_str_radix(v.trim(), 16).is_ok_and(|m| m & 0x100 != 0)
    })
}

/// Sends SIGKILL to the grandchild at the end of a case, so that a
/// failing run does not leave a `sleep 100` behind.
fn kill_grandchild(pid: i32) {
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

#[test]
fn c_src_liquidctl_7() {
    puron_testkit::limit("C-src.liquidctl-7", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_7", |_| {});
            return;
        }
        let mut src = started(dir("grandchild"), ONE);
        let t0 = Instant::now();
        let outcome = read(&mut src);
        let t = elapsed(t0);
        let (pid, grand) = (fake_pid(), grandchild_pid());
        let grand_gone = grandchild_gone(grand);
        let fake_gone = gone(pid);
        kill_grandchild(grand);
        assert_eq!(outcome, ReadOutcome::Failed);
        assert!((1.0..=1.5).contains(&t), "grandchild: returned at {t} s");
        assert!(fake_gone, "grandchild case: fake {pid} not gone");
        assert!(grand_gone, "grandchild case: grandchild {grand} not gone");

        let mut src = started(dir("orphan"), ONE);
        let t0 = Instant::now();
        let r = readings(&mut src);
        let t = elapsed(t0);
        let grand = grandchild_pid();
        let grand_gone = grandchild_gone(grand);
        kill_grandchild(grand);
        check(&r, &G);
        assert!(t < 1.0, "orphan: returned at {t} s");
        assert!(grand_gone, "orphan case: grandchild {grand} not gone");
    });
}

#[test]
fn c_src_liquidctl_8() {
    puron_testkit::limit("C-src.liquidctl-8", LONG, || {
        if !is_child() {
            run_child("c_src_liquidctl_8", |_| {});
            return;
        }
        for (name, size) in [("cap-exact", 1_048_576), ("cap-over", 1_048_577)] {
            let out = Command::new(dir(name).join("liquidctl"))
                .stdin(Stdio::null())
                .output()
                .expect("run the fake");
            assert_eq!(out.stdout.len(), size, "{name} output size");
        }
        let mut src = started(dir("cap-exact"), TEN);
        check(&readings(&mut src), &G);

        let mut src = started(dir("cap-over"), TEN);
        let t0 = Instant::now();
        let outcome = read(&mut src);
        let t = elapsed(t0);
        assert_eq!(outcome, ReadOutcome::Failed);
        assert!(t < 1.0, "cap-over: returned at {t} s");
        let pid = fake_pid();
        assert!(gone(pid), "fake {pid} not gone");
    });
}

#[test]
fn c_src_liquidctl_9() {
    puron_testkit::limit("C-src.liquidctl-9", LONG, || {
        if !is_child() {
            run_child("c_src_liquidctl_9", |_| {});
            return;
        }
        let mut src = started(dir("slow"), TEN);
        let stop = umd::stop_fd();
        let raw = stop.as_raw_fd();
        let t0 = Instant::now();
        let waker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            // SAFETY: `raw` stays open until the thread is joined.
            let rc = unsafe { libc::eventfd_write(raw, 1) };
            assert_eq!(rc, 0, "eventfd_write");
        });
        let outcome = src.read(stop.as_fd());
        let t = elapsed(t0);
        waker.join().expect("waker");
        assert_eq!(outcome, ReadOutcome::Failed);
        assert!(t <= 1.0, "returned at {t} s");
        let pid = fake_pid();
        assert!(gone(pid), "fake {pid} not gone");
    });
}

#[test]
fn c_src_liquidctl_10() {
    puron_testkit::limit("C-src.liquidctl-10", C10, || {
        if is_child() {
            // On a worker thread, `start` and `read`; the parent kills this
            // process during the `read`.
            let worker = std::thread::spawn(|| {
                let mut src = started(dir("slow"), TEN);
                let _ = read(&mut src);
            });
            let _ = worker.join();
            panic!("the Read returned before the kill");
        }
        let mut cmd = child_command("c_src_liquidctl_10");
        let log_path = cmd
            .get_envs()
            .find(|(k, _)| *k == LOG_ENV)
            .and_then(|(_, v)| v.map(PathBuf::from))
            .expect("log path");
        let mut child = cmd.stdout(Stdio::null()).spawn().expect("spawn the child");
        // The `read` has started when the fake's line is in the log.
        let pid = loop {
            let text = std::fs::read_to_string(&log_path).unwrap_or_default();
            if let Some(l) = text.lines().next() {
                break parse_line(l).pid;
            }
            if let Some(status) = child.try_wait().expect("child status") {
                panic!("the child ended before its Read: {status}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_millis(500));
        // SAFETY: the child is not reaped yet, so its number is its own.
        unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
        let _ = child.wait();
        while !gone(pid) && elapsed(t0) < 1.5 {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(gone(pid), "fake {pid} not gone by 1.5 s");
    });
}

#[test]
fn c_src_liquidctl_11() {
    puron_testkit::limit("C-src.liquidctl-11", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_11", |_| {});
            return;
        }
        let p = dir("path");
        let (d1, d2, d3, d4) = (p.join("d1"), p.join("d2"), p.join("d3"), p.join("d4"));
        // d1 holds a liquidctl the user may not run, d2 a directory of that
        // name, d3 a link to the good fake: the search finds d3's.
        let mut src = started(search(&[&d1, &d2, &d3]), ONE);
        check(&readings(&mut src), &G);
        src.stop();
        // No entry holds a liquidctl the user may run: start is unavailable.
        let mut src = Liquidctl::new(search(&[&d1, &d2, &d4]), ONE);
        assert_eq!(src.start(), StartOutcome::Unavailable, "case 2");
        // On a copy: d3 holds a link to the good fake, d4 is empty.
        let t = temp_dir("lqc-path");
        let (t3, t4) = (t.join("d3"), t.join("d4"));
        std::fs::create_dir(&t3).expect("d3");
        std::fs::create_dir(&t4).expect("d4");
        std::os::unix::fs::symlink(dir("good").join("liquidctl"), t3.join("liquidctl"))
            .expect("link");
        let mut src = started(search(&[&t4, &t3]), ONE);
        std::fs::copy(dir("empty").join("liquidctl"), t4.join("liquidctl")).expect("copy");
        check(&readings(&mut src), &G);
        std::fs::remove_file(t3.join("liquidctl")).expect("remove");
        assert_eq!(read(&mut src), ReadOutcome::Failed, "case 4, removed");
        src.stop();
    });
}

/// In the child, the current directory `d3` and the search path `d4` then
/// an empty entry.
#[test]
fn c_src_liquidctl_11__cwd() {
    puron_testkit::limit("C-src.liquidctl-11", SHORT, || {
        if !is_child() {
            let out = run_child("c_src_liquidctl_11__cwd", |_| {});
            assert_eq!(results(&out), ["RESULT start=Available readings=7"]);
            return;
        }
        let p = dir("path");
        std::env::set_current_dir(p.join("d3")).expect("chdir d3");
        let mut path = p.join("d4").into_os_string();
        path.push(":");
        let mut src = Liquidctl::new(path, ONE);
        let start = src.start();
        let r = readings(&mut src);
        check(&r, &G);
        println!("RESULT start={start:?} readings={}", r.len());
    });
}

#[test]
fn c_src_liquidctl_12() {
    puron_testkit::limit("C-src.liquidctl-12", SHORT, || {
        if is_child() {
            let src = registry::liquidctl();
            println!(
                "RESULT search={} limit={:?} cap={}",
                src.search_path().to_string_lossy(),
                src.limit(),
                src.cap()
            );
            return;
        }
        let set = run_child("c_src_liquidctl_12", |c| {
            c.env("PATH", "/x:/y");
        });
        assert_eq!(results(&set), ["RESULT search=/x:/y limit=10s cap=1048576"]);
        let unset = run_child("c_src_liquidctl_12", |c| {
            c.env_remove("PATH");
        });
        assert_eq!(
            results(&unset),
            ["RESULT search=/bin:/usr/bin limit=10s cap=1048576"]
        );
    });
}

#[test]
fn c_src_liquidctl_13() {
    puron_testkit::limit("C-src.liquidctl-13", SHORT, || {
        need();
        let entries: Vec<&str> = liquidctl::DEFAULT_SEARCH_PATH.split(':').collect();
        assert_eq!(entries, ["/bin", "/usr/bin"]);
    });
}

/// In the child, the current directory `d3` and the search path `d4:d2`,
/// no empty entry.
#[test]
fn c_src_liquidctl_13__cwd() {
    puron_testkit::limit("C-src.liquidctl-13", SHORT, || {
        if !is_child() {
            let out = run_child("c_src_liquidctl_13__cwd", |_| {});
            assert_eq!(results(&out), ["RESULT start=Unavailable"]);
            return;
        }
        let p = dir("path");
        std::env::set_current_dir(p.join("d3")).expect("chdir d3");
        let mut src = Liquidctl::new(search(&[&p.join("d4"), &p.join("d2")]), ONE);
        println!("RESULT start={:?}", src.start());
    });
}

#[test]
fn c_src_liquidctl_14() {
    puron_testkit::limit("C-src.liquidctl-14", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_14", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        let keys: Vec<String> = readings(&mut src).into_iter().map(|r| r.key).collect();
        let want: Vec<&str> = G.iter().map(|e| e.4).collect();
        assert_eq!(keys, want);

        let mut src = started(dir("twins"), ONE);
        let keys: Vec<String> = readings(&mut src).into_iter().map(|r| r.key).collect();
        assert_eq!(
            keys,
            [
                "Corsair Commander Pro//dev/hidraw1/Fan 1 speed",
                "Corsair Commander Pro//dev/hidraw2/Fan 1 speed",
                "Kraken/Pump speed",
                "Dev//Fan speed",
                "Dev//Fan speed",
            ]
        );
    });
}
