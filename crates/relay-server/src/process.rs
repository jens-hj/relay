//! Bounded, deadline-controlled capture for trusted server-selected CLI tools.
//! stderr is discarded, and the Relay bearer is never inherited.
use crate::Error;
use std::{
    cell::RefCell,
    io::Read,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
thread_local! {
    static CANCELLATION: RefCell<Option<tokio::sync::watch::Receiver<bool>>> = const { RefCell::new(None) };
}
pub(crate) fn with_cancellation<T>(
    stop: tokio::sync::watch::Receiver<bool>,
    run: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<tokio::sync::watch::Receiver<bool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CANCELLATION.with(|value| *value.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CANCELLATION.with(|value| value.replace(Some(stop))));
    run()
}
fn cancelled() -> Result<(), Error> {
    if CANCELLATION.with(|value| value.borrow().as_ref().is_some_and(|stop| *stop.borrow())) {
        Err(Error::invalid("Server stopped the resource operation"))
    } else {
        Ok(())
    }
}
pub(crate) struct Capture {
    pub bytes: Vec<u8>,
    pub status: ExitStatus,
    pub truncated: bool,
}
struct Owned {
    child: std::process::Child,
    finished: bool,
}
impl Owned {
    fn terminate(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.finished = true;
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.finished {
            self.terminate();
        }
    }
}
fn timeout_error(label: &str) -> Error {
    Error::invalid(format!(
        "{label} timed out; check network/repository availability"
    ))
}
#[cfg(unix)]
fn read_output(
    mut stdout: std::process::ChildStdout,
    limit: usize,
    deadline: Instant,
    label: &str,
) -> Result<Vec<u8>, Error> {
    use std::os::fd::AsRawFd;
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(Error::invalid(
            "Cannot configure bounded subprocess capture",
        ));
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        cancelled()?;
        if Instant::now() >= deadline {
            return Err(timeout_error(label));
        }
        let size = chunk.len().min(limit + 1 - bytes.len());
        match stdout.read(&mut chunk[..size]) {
            Ok(0) => return Ok(bytes),
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.len() > limit {
                    return Ok(bytes);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(Error::invalid(format!("Cannot read {label} output"))),
        }
    }
}
#[cfg(not(unix))]
fn read_output(
    stdout: std::process::ChildStdout,
    limit: usize,
    deadline: Instant,
    label: &str,
) -> Result<Vec<u8>, Error> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    // Platforms without nonblocking pipe support may retain a reader if descendants
    // escape the direct child's ownership. Reads still cannot grow beyond limit+1.
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| timeout_error(label))?
        .map_err(|_| Error::invalid(format!("Cannot read {label} output")))
}
pub(crate) fn capture(
    command: &mut Command,
    limit: usize,
    timeout: Duration,
    label: &str,
) -> Result<Capture, Error> {
    cancelled()?;
    command
        .env_remove("RELAY_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = unsafe { libc::getpid() };
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(127);
                }
                Ok(())
            });
        }
    }
    let child = command.spawn().map_err(|_| {
        Error::invalid(format!(
            "Cannot execute {label}; check tool installation and server configuration"
        ))
    })?;
    let mut owned = Owned {
        child,
        finished: false,
    };
    let stdout = owned.child.stdout.take().unwrap();
    let deadline = Instant::now() + timeout;
    let mut bytes = read_output(stdout, limit, deadline, label)?;
    let truncated = bytes.len() > limit;
    let status = if truncated {
        owned.terminate();
        owned
            .child
            .wait()
            .map_err(|_| Error::invalid(format!("Cannot wait for {label}")))?
    } else {
        loop {
            cancelled()?;
            if let Some(status) = owned
                .child
                .try_wait()
                .map_err(|_| Error::invalid(format!("Cannot wait for {label}")))?
            {
                owned.finished = true;
                break status;
            }
            if Instant::now() >= deadline {
                return Err(timeout_error(label));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    bytes.truncate(limit);
    Ok(Capture {
        bytes,
        status,
        truncated,
    })
}
