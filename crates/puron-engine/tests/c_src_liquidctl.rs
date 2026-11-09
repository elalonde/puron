//! Tests of the liquidctl adapter (`puron_engine::liquidctl`), through
//! its `start`, `read` and `stop`. Each test gives the adapter, as its
//! search path, a directory of `fixtures/lqc/` that holds a fake
//! `liquidctl` shell script.
//!
//! Each fake appends one line per run to the file named by
//! `PURON_TEST_LQC_LOG` (its arguments, process ID, process group,
//! whether its standard input was at end of file, and where its standard
//! error went), and some write the process ID of a background child, the
//! test's grandchild, to the file named by `PURON_TEST_LQC_CHILD`. The
//! adapter passes its own environment on to the program unchanged, so
//! each test runs its body in a child process, the test binary run again
//! with an environment that names files of its own. The parent test
//! process never changes its own environment or directory.

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

/// The time limit, in seconds, of a test whose adapter limits each run
/// to `ONE`: that limit plus 5 s.
const SHORT: u64 = 6;
/// The same for a test whose adapter limits each run to `TEN`.
const LONG: u64 = 15;
/// The time limit of `c_src_liquidctl_10`, in seconds.
const C10: u64 = 5;

/// The run time limits given to the adapter in these tests.
const ONE: Duration = Duration::from_secs(1);
const TEN: Duration = Duration::from_secs(10);

/// Set in the child process that runs a test's body.
const CHILD_ENV: &str = "PURON_LQC_CHILD";
/// Names the file each fake appends its log line to.
const LOG_ENV: &str = "PURON_TEST_LQC_LOG";
/// Names the file a fake writes its background child's process ID to.
const GRANDCHILD_ENV: &str = "PURON_TEST_LQC_CHILD";

/// Checks, in the parent before any child starts, that the host has the
/// tools the fakes and these tests use.
fn need() {
    puron_testkit::host_need(
        "/bin/sh, mkfifo, setsid, pgrep, ps, /proc",
        "command -v mkfifo setsid pgrep ps",
    );
}

/// Whether this process is the child that runs a test's body.
fn is_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some()
}

/// In the parent: the command that runs test `test` again as a child,
/// with standard input from /dev/null, its own log and grandchild files
/// in a fresh temporary directory, and SIGKILL sent to it if the parent
/// dies.
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

/// In the parent: runs the child of `test` with `setup` applied to its
/// command, panics unless it exits 0, and returns its standard output.
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

/// The `RESULT ` lines of the child's output, each from `RESULT ` on: the
/// test harness may print the test name before it on the same line.
fn results(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|l| l.find("RESULT ").map(|i| &l[i..]))
        .collect()
}

/// The fixture directory `fixtures/lqc/<name>`.
fn dir(name: &str) -> PathBuf {
    fixture("lqc", name)
}

/// A search path made of `dirs`, joined with `:`.
fn search(dirs: &[&Path]) -> OsString {
    let parts: Vec<&std::ffi::OsStr> = dirs.iter().map(|d| d.as_os_str()).collect();
    parts.join(std::ffi::OsStr::new(":"))
}

/// One log line of a fake: its arguments, process ID, process group ID,
/// `yes` if its standard input was at end of file, and the target of its
/// standard error.
#[derive(Debug)]
struct Line {
    args: String,
    pid: i32,
    pgid: i32,
    stdin_eof: String,
    fd2: String,
}

/// The fakes' log lines so far, in run order; none if no fake has run.
fn log() -> Vec<Line> {
    let path = std::env::var_os(LOG_ENV).expect("log file named");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().map(parse_line).collect()
}

/// Parses one log line; panics if it is not in the fakes' form.
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

/// The process ID of the last fake that ran.
fn fake_pid() -> i32 {
    log().last().expect("a fake ran").pid
}

/// The process ID that the fake wrote for its background child.
fn grandchild_pid() -> i32 {
    let path = std::env::var_os(GRANDCHILD_ENV).expect("grandchild file named");
    std::fs::read_to_string(path)
        .expect("grandchild file")
        .trim()
        .parse()
        .expect("grandchild number")
}

/// Whether process `pid` has ended and been reaped: `kill -0` fails and
/// `/proc/<pid>` does not exist.
fn gone(pid: i32) -> bool {
    // SAFETY: signal 0 only checks the process.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    !alive && !Path::new(&format!("/proc/{pid}")).exists()
}

/// An adapter with this search path and run time limit, started; panics
/// unless the start reports it available.
fn started(search_path: impl Into<OsString>, limit: Duration) -> Liquidctl {
    let mut src = Liquidctl::new(search_path, limit);
    assert_eq!(src.start(), StartOutcome::Available);
    src
}

/// Reads once, with a stop descriptor that is never raised.
fn read(src: &mut Liquidctl) -> ReadOutcome {
    let stop = umd::stop_fd();
    src.read(stop.as_fd())
}

/// Reads once and returns the readings; panics if the read gives
/// anything else.
fn readings(src: &mut Liquidctl) -> Vec<RawReading> {
    match read(src) {
        ReadOutcome::Readings(r) => r,
        other => panic!("read gave {other:?}"),
    }
}

/// One expected reading: its device, item, unit, value and key. Its
/// position is its index in the list of expected readings.
type Exp = (
    &'static str,
    &'static str,
    &'static str,
    Option<f64>,
    &'static str,
);

/// The two device descriptions of `G.json`.
const KRAKEN: &str = "NZXT Kraken X (X42, X52, X62 or X72)";
const PRO: &str = "Corsair Commander Pro";

/// The readings of `fixtures/lqc/G.json`, the output that most fakes
/// print: two devices, seven status entries.
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

/// Checks every field of `readings`: device, item, unit, value and key as
/// in `exp`, in that order; provider `Liquidctl`; positions 0, 1, 2 and
/// so on; and no kind, chip or name.
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

/// Seconds since `t0`.
fn elapsed(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64()
}

/// A fake that prints `G.json`. Expects the seven readings of `G`, and
/// the rows built from them for display (`rows::rows`) to be the five
/// with values that are not a duty, under their short names.
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

/// A read with standard input closed in the process, so that the
/// descriptor the adapter opens to watch its run may be number 0.
/// Expects the readings of `G`: descriptor 0 is a valid descriptor.
#[test]
fn c_src_liquidctl_1__stdin_closed() {
    puron_testkit::limit("C-src.liquidctl-1", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_1__stdin_closed", |_| {});
            return;
        }
        let mut src = started(dir("good"), ONE);
        // Make the stop descriptor first, so that it cannot be the one
        // that takes number 0.
        let stop = umd::stop_fd();
        // SAFETY: closes this child process's standard input only.
        unsafe { libc::close(0) };
        match src.read(stop.as_fd()) {
            ReadOutcome::Readings(r) => check(&r, &G),
            other => panic!("standard input closed: {other:?}"),
        }
    });
}

/// A read while another thread sends signals to the reading thread as
/// fast as it can, so that the adapter's poll(2) returns EINTR. Expects
/// at least one signal sent and the readings of `G`: an interrupted wait
/// does not end the run.
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

/// A read on a thread where `pidfd_open` fails with ENOSYS, as on a
/// kernel older than 5.3; a seccomp filter makes it fail. Expects the
/// read to fail within 0.5 s: the program is started, but killed at once
/// because the run cannot be watched.
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

// Classic BPF opcodes and seccomp return values for the filters below.
const LD_W_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
const JEQ_K: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
const JGT_K: u16 = 0x25; // BPF_JMP | BPF_JGT | BPF_K
const RET_K: u16 = 0x06; // BPF_RET | BPF_K
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7fff_0000;
/// Offset in `seccomp_data` of the system call number, and of the low 32
/// bits of the first argument (little-endian).
const NR: u32 = 0;
const ARG0: u32 = 16;

fn op(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// Installs a seccomp filter on this thread that makes `pidfd_open` fail
/// with ENOSYS and allows every other system call.
fn deny_pidfd_open() {
    install(&mut [
        op(LD_W_ABS, 0, 0, NR),
        op(JEQ_K, 0, 1, libc::SYS_pidfd_open as u32),
        op(RET_K, 0, 0, RET_ERRNO | libc::ENOSYS as u32),
        op(RET_K, 0, 0, RET_ALLOW),
    ]);
}

/// Installs a seccomp filter on this thread that makes waiting for one
/// given child fail with ECHILD: `wait4` with a pid above 0, and `waitid`
/// with `P_PID`. Waiting for any child (`wait4` with -1, as the fake's
/// shell does for its own commands) still works. The started program
/// inherits the filter, so this keeps the fake working while the
/// adapter's reap of it fails.
fn deny_child_wait() {
    install(&mut [
        op(LD_W_ABS, 0, 0, NR),
        op(JEQ_K, 1, 0, libc::SYS_wait4 as u32),
        op(JEQ_K, 3, 5, libc::SYS_waitid as u32),
        // wait4: allow 0 and the negative pids (above 0x7fffffff as u32).
        op(LD_W_ABS, 0, 0, ARG0),
        op(JEQ_K, 3, 0, 0),
        op(JGT_K, 2, 3, 0x7fff_ffff),
        // waitid: deny P_PID.
        op(LD_W_ABS, 0, 0, ARG0),
        op(JEQ_K, 1, 0, libc::P_PID),
        op(RET_K, 0, 0, RET_ALLOW),
        op(RET_K, 0, 0, RET_ERRNO | libc::ECHILD as u32),
    ]);
}

/// Installs `prog` as a seccomp filter of this thread, after setting
/// `no_new_privs`, which seccomp requires of a user who is not root.
fn install(prog: &mut [libc::sock_filter]) {
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

/// Three reads with the fake that prints `G.json`. Expects no run at
/// start, and one run per read, each with the arguments `status --json`,
/// in a process group of its own, with standard input at end of file and
/// standard error going to /dev/null.
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

/// Outputs that cannot be used: none, cut-off JSON, text that is not
/// JSON, an invalid UTF-8 byte, a top level that is not an array, a
/// description that is not a string, a status that is not an array, and
/// a sensor entry (unit °C) without a key. Expects each read to fail.
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

/// Status entries of odd shapes: a number, entries without a key whose
/// unit is not a sensor unit, a null value, a value given as a string,
/// and an entry with an extra field. Expects the entries that are not
/// objects or have no key to be skipped, the null and string values to
/// give readings without values, and the extra field to be ignored.
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

/// A fake that prints `G.json` and exits with status 1. Expects the
/// readings of `G`: the exit status does not decide the result.
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

/// With diagnostics on and sent to a test journal socket: a run that
/// exits with status 0 and then one that exits with status 1. Expects no
/// `exit-status` entry for the first, and one, with status 1, for the
/// second.
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
        // The events of a `read` are sent before it returns, so a marker
        // datagram sent after it arrives after all of them. Waiting for
        // the `m`-th marker therefore waits for every event of the `m`
        // reads so far, with no guess at a time.
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

/// Runs that do not end within the 1 s limit: a fake that prints part of
/// its output and sleeps, and one that prints all of it, closes its
/// output and sleeps. Expects each read to fail after 1 to 1.5 s, and
/// the fake to be killed and reaped.
#[test]
fn c_src_liquidctl_6() {
    puron_testkit::limit("C-src.liquidctl-6", SHORT, || {
        if !is_child() {
            run_child("c_src_liquidctl_6", |_| {});
            return;
        }
        // A fake that never ends, then one that closes its output and
        // then sleeps: end of output before the program has exited does
        // not complete the run.
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

/// Whether the grandchild `pid` has been killed. The test cannot reap it,
/// so any of these counts: `kill -0` fails, its state in
/// `/proc/<pid>/stat` is `Z`, or SIGKILL is pending for it (bit 0x100 of
/// `SigPnd` or `ShdPnd` in `/proc/<pid>/status`). A `/proc` file that
/// cannot be read means it has been reaped meanwhile.
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

/// Background children of the fake. First a fake that leaves a `sleep
/// 100` behind that keeps its output open: expects the read to fail after
/// 1 to 1.5 s, with the fake and that child both killed. Then a fake whose
/// background child has closed its output: expects the readings of `G`
/// within 1 s, and the child killed too.
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

/// The output size limit of 1 MiB. Expects the fakes to print exactly
/// 1 048 576 and 1 048 577 bytes. With the run limit at 10 s, expects
/// the first to give the readings of `G`, and the second to fail within
/// 1 s, with the fake killed and reaped.
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

/// A fake that never ends, with the run limit at 10 s; another thread
/// raises the stop descriptor after 0.5 s. Expects the read to fail
/// within 1 s, with the fake killed and reaped.
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

/// The process that reads is killed with SIGKILL during a run of a fake
/// that never ends. Expects the fake to be gone within 1.5 s: it does not
/// outlive the process that started it.
#[test]
fn c_src_liquidctl_10() {
    puron_testkit::limit("C-src.liquidctl-10", C10, || {
        if is_child() {
            // On a worker thread, start and read with a fake that never
            // ends; the parent kills this process during the read.
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
        // The read has started the fake once the fake's line is in the log.
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

/// The search for the program. Search path `d1:d2:d3`, where `d1` holds a
/// `liquidctl` the user may not run, `d2` a directory named `liquidctl`,
/// and `d3` a link to the fake that prints `G.json`: expects `d3`'s to
/// be found. With `d4`, an empty directory, in place of `d3`: expects
/// `start` to report unavailable. Then in a temporary copy: started with
/// `d4:d3`, a program copied into `d4` afterwards is not used, and once
/// `d3`'s link is removed the read fails: the program found at start is
/// kept.
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
        // name, d3 a link to the fake that prints G.json.
        let mut src = started(search(&[&d1, &d2, &d3]), ONE);
        check(&readings(&mut src), &G);
        src.stop();
        // No entry holds a liquidctl the user may run.
        let mut src = Liquidctl::new(search(&[&d1, &d2, &d4]), ONE);
        assert_eq!(src.start(), StartOutcome::Unavailable, "case 2");
        // In a temporary directory: d3 holds a link to the fake that
        // prints G.json, d4 is empty until after the start.
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

/// With the current directory `d3`, which holds the fake, and the search
/// path `d4:` (an empty entry last). Expects the empty entry to stand for
/// the current directory: the start reports available and the read
/// gives the 7 readings of `G`.
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

/// The adapter that the registry builds for production. Expects its
/// search path to be `PATH`, or `/bin:/usr/bin` when `PATH` is not set,
/// its run limit 10 s and its output limit 1 MiB.
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

/// Expects the default search path to be `/bin:/usr/bin`, with no empty
/// entry and so no search of the current directory.
#[test]
fn c_src_liquidctl_13() {
    puron_testkit::limit("C-src.liquidctl-13", SHORT, || {
        need();
        let entries: Vec<&str> = liquidctl::DEFAULT_SEARCH_PATH.split(':').collect();
        assert_eq!(entries, ["/bin", "/usr/bin"]);
    });
}

/// With the current directory `d3`, which holds the fake, and the search
/// path `d4:d2`, without an empty entry. Expects `start` to report
/// unavailable: the current directory is not searched.
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

/// The keys of the readings. Expects `<description>/<status key>` for
/// devices with a description of their own. When devices share a
/// description, expects the device's address between the two, as in
/// `Corsair Commander Pro//dev/hidraw1/Fan 1 speed`, and an empty address
/// when the device has none, so that two such devices get the same key.
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

/// With diagnostics on and sent to a test journal, reads the fake that
/// prints `G.json` with `deny`'s seccomp filter on the reading thread.
/// Expects the read to fail with one `run-failed` entry of reason
/// `wait-error` whose text is the system error `errno`, and no
/// `not-started` entry: the program did start.
fn wait_error_logged(test: &str, deny: fn(), errno: i32) {
    if !is_child() {
        run_child(test, |_| {});
        return;
    }
    let j = puron_testkit::jrn::Journal::new("c-lqc-15");
    puron_engine::diag::set_socket(&j.path);
    puron_engine::diag::switch(true);
    let mut src = started(dir("good"), ONE);
    deny();
    assert_eq!(read(&mut src), ReadOutcome::Failed);
    // The read's events are sent before it returns, so the marker sent
    // now arrives after them.
    let sock = std::os::unix::net::UnixDatagram::unbound().expect("socket");
    sock.send_to(b"MESSAGE=marker\n", &j.path)
        .expect("send marker");
    let mut n = 1;
    let entries = loop {
        let e = j.wait(n, Duration::from_secs(SHORT));
        if e.iter()
            .any(|e| puron_testkit::jrn::field(e, "MESSAGE") == Some("marker"))
        {
            break e;
        }
        assert!(e.len() >= n, "marker not received");
        n = e.len() + 1;
    };
    let events: Vec<_> = entries
        .iter()
        .filter_map(|e| puron_testkit::jrn::field(e, "PURON_EVENT").map(|ev| (ev, e)))
        .collect();
    let ids: Vec<&str> = events.iter().map(|(ev, _)| *ev).collect();
    assert_eq!(
        ids,
        ["run-failed"],
        "{}",
        puron_testkit::jrn::dump(&entries)
    );
    let e = events[0].1;
    assert_eq!(
        puron_testkit::jrn::field(e, "PURON_REASON"),
        Some("wait-error")
    );
    let text = std::io::Error::from_raw_os_error(errno).to_string();
    assert_eq!(
        puron_testkit::jrn::field(e, "PURON_TEXT"),
        Some(text.as_str())
    );
}

/// `pidfd_open` fails with ENOSYS after the program has started: logged
/// as a failed wait with ENOSYS's text.
#[test]
fn c_src_liquidctl_15__nopidfd() {
    puron_testkit::limit("C-src.liquidctl-15", SHORT, || {
        wait_error_logged("c_src_liquidctl_15__nopidfd", deny_pidfd_open, libc::ENOSYS)
    });
}

/// The run completes, and reaping the program fails with ECHILD: logged
/// as a failed wait with ECHILD's text.
#[test]
fn c_src_liquidctl_15__reap() {
    puron_testkit::limit("C-src.liquidctl-15", SHORT, || {
        wait_error_logged("c_src_liquidctl_15__reap", deny_child_wait, libc::ECHILD)
    });
}
