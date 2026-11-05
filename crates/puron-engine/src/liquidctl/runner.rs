//! One liquidctl run and how it ends. The only place in `puron-engine`
//! that starts a process.

use std::io::Read;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How a run ended.
#[derive(Debug)]
pub enum End {
    /// End of file and exit, both before the time limit, at most the cap
    /// read.
    Complete {
        output: Vec<u8>,
        status: ExitStatus,
    },
    TimeLimit,
    OverCap,
    Stopped,
    /// The program could not be started; the system error.
    NotStarted(std::io::Error),
    /// A `poll(2)` or `read(2)` of the output pipe failed; the system
    /// error.
    WaitError(std::io::Error),
}

/// Runs `program status --json` until end of output and exit, the time
/// limit, the output cap or the stop signal. However the run ends, the
/// child's process group is killed while the child is not yet reaped,
/// then the child is reaped, before this returns.
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
    // parent has not ended before it was set.
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
    // Kill the group while the child is not reaped (its number cannot be
    // reused yet), then reap it.
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let status = child.wait();
    match (end, status) {
        (Waited::Complete(output), Ok(status)) => End::Complete { output, status },
        (Waited::Complete(_), Err(e)) => End::NotStarted(e),
        (Waited::TimeLimit, _) => End::TimeLimit,
        (Waited::OverCap, _) => End::OverCap,
        (Waited::Stopped, _) => End::Stopped,
        (Waited::NoPidfd(e), _) => End::NotStarted(e),
        (Waited::WaitError(e), _) => End::WaitError(e),
    }
}

enum Waited {
    Complete(Vec<u8>),
    TimeLimit,
    OverCap,
    Stopped,
    NoPidfd(std::io::Error),
    WaitError(std::io::Error),
}

/// The child's process descriptor (`pidfd_open`).
fn pidfd(child: &Child) -> std::io::Result<OwnedFd> {
    // SAFETY: pidfd_open returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id() as libc::pid_t, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fd is a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

/// One `poll(2)` loop on the output pipe, the child's process descriptor
/// and the stop signal, with the end of the time limit as the deadline.
/// Never reaps the child.
fn wait(child: &mut Child, deadline: Instant, cap: usize, stop: BorrowedFd<'_>) -> Waited {
    let process = match pidfd(child) {
        Ok(fd) => fd,
        Err(e) => return Waited::NoPidfd(e),
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
        // Round up, so that the wait does not end just before the limit.
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
                // An error is not end of file: the run is not complete,
                // and it ends as a failed poll(2) does.
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

/// A failed `poll(2)` or `read(2)`: none for EINTR (the call is made
/// again), else the run ends with the system error.
fn failed(e: std::io::Error) -> Option<Waited> {
    if e.kind() == std::io::ErrorKind::Interrupted {
        None
    } else {
        Some(Waited::WaitError(e))
    }
}

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

    // EINTR is retried; any other error is wait-error with the system
    // error.
    #[test]
    fn failed_call_is_wait_error() {
        puron_testkit::limit("p4z1-wait-error", 2, || {
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
