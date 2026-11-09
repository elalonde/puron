//! One run of liquidctl and how it ended. This is the only place in
//! `puron-engine` that starts a process.

use std::io::Read;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How a run ended.
#[derive(Debug)]
pub enum End {
    /// The program closed its output and exited, both before the time
    /// limit, and wrote no more than the cap.
    Complete {
        /// Everything the program wrote to standard output.
        output: Vec<u8>,
        /// The program's exit status, which may be non-zero.
        status: ExitStatus,
    },
    /// The time limit passed first.
    TimeLimit,
    /// The program wrote more than the cap.
    OverCap,
    /// The stop signal was raised.
    Stopped,
    /// The program could not be started, with the system error.
    NotStarted(std::io::Error),
    /// The program started, but waiting for it failed with this system
    /// error: the child's process descriptor could not be opened, a
    /// `poll(2)` or a `read(2)` of the output pipe failed, or reaping the
    /// child after a complete run failed. The group is still killed.
    WaitError(std::io::Error),
}

/// Runs `program status --json` until it has closed its output and
/// exited, or until the time limit, the output cap or the stop signal
/// ends the run. Standard input and standard error are `/dev/null`.
///
/// The child runs in a process group of its own, and gets SIGKILL if its
/// parent dies first. However the run ends, the whole group is killed before
/// this returns, and then the child is reaped. A descendant that has
/// moved to another process group escapes this kill.
pub fn run(program: &Path, limit: Duration, cap: usize, stop: BorrowedFd<'_>) -> End {
    let start = Instant::now();
    let deadline = start + limit;
    let mut cmd = Command::new(program);
    cmd.args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    // SAFETY: prctl and getppid are async-signal-safe; the closure only
    // sets the parent-death signal of the child and checks that the
    // parent has not ended before it was set. If it had, the child would
    // never get the signal, so it exits at once instead.
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(127);
            }
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return End::NotStarted(e),
    };
    let end = wait(&mut child, deadline, cap, stop);
    // Kill the group while the child is not yet reaped, so that its
    // process ID cannot have been reused by another process; then reap
    // it.
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let status = child.wait();
    match (end, status) {
        (Waited::Complete(output), Ok(status)) => End::Complete { output, status },
        // Without its exit status a complete run is not used.
        (Waited::Complete(_), Err(e)) => End::WaitError(e),
        (Waited::TimeLimit, _) => End::TimeLimit,
        (Waited::OverCap, _) => End::OverCap,
        (Waited::Stopped, _) => End::Stopped,
        (Waited::WaitError(e), _) => End::WaitError(e),
    }
}

/// How `wait` ended. `run` turns it into an `End`.
enum Waited {
    Complete(Vec<u8>),
    TimeLimit,
    OverCap,
    Stopped,
    /// The child's process descriptor could not be opened, or a
    /// `poll(2)` or `read(2)` failed.
    WaitError(std::io::Error),
}

/// Opens a process descriptor for the child with `pidfd_open(2)`. It
/// becomes readable when the child exits, so that `wait` can poll for
/// the exit together with the output.
fn pidfd(child: &Child) -> std::io::Result<OwnedFd> {
    // SAFETY: pidfd_open returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id() as libc::pid_t, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fd is a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

/// Waits for the run to end with one `poll(2)` loop on the output pipe,
/// the child's process descriptor and the stop signal, until `deadline`.
/// It collects the output on the way. The stop signal is checked first,
/// so it wins over output that is ready at the same time. It never reaps
/// the child; `run` does that.
fn wait(child: &mut Child, deadline: Instant, cap: usize, stop: BorrowedFd<'_>) -> Waited {
    let process = match pidfd(child) {
        Ok(fd) => fd,
        Err(e) => return Waited::WaitError(e),
    };
    let mut out = child.stdout.take().expect("piped standard output");
    let mut output = Vec::new();
    let mut buf = vec![0u8; 65536];
    let (mut eof, mut exited) = (false, false);
    loop {
        if eof && exited {
            return Waited::Complete(output);
        }
        let now = Instant::now();
        if now >= deadline {
            return Waited::TimeLimit;
        }
        let left = deadline - now;
        // Round up to whole milliseconds, so that the wait does not end just
        // before the limit.
        let ms = left.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32;
        let mut fds = [
            pollfd(if eof { -1 } else { out.as_raw_fd() }),
            pollfd(if exited { -1 } else { process.as_raw_fd() }),
            pollfd(stop.as_raw_fd()),
        ];
        // SAFETY: fds is a valid array of three pollfd.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if n < 0 {
            match failed(std::io::Error::last_os_error()) {
                Some(end) => return end,
                None => continue,
            }
        }
        if fds[2].revents != 0 {
            return Waited::Stopped;
        }
        if fds[0].revents != 0 {
            match out.read(&mut buf) {
                Ok(0) => eof = true,
                Ok(k) => {
                    if output.len() + k > cap {
                        return Waited::OverCap;
                    }
                    output.extend_from_slice(&buf[..k]);
                }
                // A read error is not end of output: the run is not
                // complete, and it ends the same way as a failed poll(2).
                Err(e) => {
                    if let Some(end) = failed(e) {
                        return end;
                    }
                }
            }
        }
        if fds[1].revents != 0 {
            exited = true;
        }
    }
}

/// Decides what a failed `poll(2)` or `read(2)` means: `None` for EINTR,
/// after which the loop makes the call again; for any other error, the
/// run ends with it.
fn failed(e: std::io::Error) -> Option<Waited> {
    if e.kind() == std::io::ErrorKind::Interrupted {
        None
    } else {
        Some(Waited::WaitError(e))
    }
}

/// A `pollfd` that waits for `fd` to become readable. A negative `fd`
/// makes `poll(2)` skip the entry.
fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // EINTR gives `None`, so the call is retried; EIO gives `WaitError`
    // with the same system error.
    #[test]
    fn failed_call_is_wait_error() {
        puron_testkit::limit("failed_call_is_wait_error", 2, || {
            let eintr = std::io::Error::from_raw_os_error(libc::EINTR);
            assert!(failed(eintr).is_none());
            let eio = std::io::Error::from_raw_os_error(libc::EIO);
            match failed(eio) {
                Some(Waited::WaitError(e)) => assert_eq!(e.raw_os_error(), Some(libc::EIO)),
                _ => panic!("not wait-error"),
            }
        });
    }
}
