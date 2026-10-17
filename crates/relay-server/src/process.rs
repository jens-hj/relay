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
pub(crate) fn without_cancellation<T>(run: impl FnOnce() -> T) -> T {
    struct Restore(Option<tokio::sync::watch::Receiver<bool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CANCELLATION.with(|value| *value.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CANCELLATION.with(|value| value.replace(None)));
    run()
}
pub(crate) fn cancelled() -> Result<(), Error> {
    if CANCELLATION.with(|value| value.borrow().as_ref().is_some_and(|stop| *stop.borrow())) {
        Err(Error::invalid("Server stopped the resource operation"))
    } else {
        Ok(())
    }
}
/// Lower worker CPU priority before exec so harness tools inherit it, including
/// descendants that create their own process groups. Never raise an inherited
/// lower priority or change the server thread's priority.
pub(crate) fn background_worker(command: &mut tokio::process::Command) {
    #[cfg(unix)]
    unsafe {
        // Only async-signal-safe syscalls in the child between fork and exec.
        command.pre_exec(|| {
            let inherited = libc::getpriority(libc::PRIO_PROCESS, 0);
            if inherited < 10 && libc::setpriority(libc::PRIO_PROCESS, 0, 10) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            #[cfg(target_os = "linux")]
            prefer_oom_kill();
            Ok(())
        });
    }
    // Other platforms retain their existing launch behavior.
    #[cfg(not(unix))]
    let _ = command;
}
/// Make the worker a preferred OOM victim over the server. Unprivileged
/// processes may raise but not lower their score, so keep a higher inherited
/// value. Best effort; runs between fork and exec using raw syscalls only.
#[cfg(target_os = "linux")]
unsafe fn prefer_oom_kill() {
    unsafe {
        let fd = libc::open(c"/proc/self/oom_score_adj".as_ptr(), libc::O_RDWR);
        if fd < 0 {
            return;
        }
        let mut buffer = [0u8; 16];
        let read = libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len());
        let mut current = 0i32;
        let mut negative = false;
        for &byte in &buffer[..read.max(0) as usize] {
            match byte {
                b'-' => negative = true,
                b'0'..=b'9' => current = current * 10 + (byte - b'0') as i32,
                _ => break,
            }
        }
        if negative {
            current = -current;
        }
        if current < WORKER_OOM_SCORE_ADJ && libc::lseek(fd, 0, libc::SEEK_SET) == 0 {
            libc::write(
                fd,
                WORKER_OOM_SCORE_ADJ_TEXT.as_ptr().cast(),
                WORKER_OOM_SCORE_ADJ_TEXT.len(),
            );
        }
        libc::close(fd);
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
        .env_remove("RELAY_TOKEN_FILE")
        .env_remove("RELAY_SETUP_TOKEN_FILE")
        .env_remove("CREDENTIALS_DIRECTORY")
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

/// Optional per-worker memory containment. When enabled, each harness runs in
/// its own transient systemd user scope so the kernel throttles and OOM-kills a
/// runaway build inside that scope instead of the server's service cgroup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkerLimits {
    pub(crate) scope: Option<WorkerScope>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkerScope {
    pub(crate) systemd_run: std::path::PathBuf,
    pub(crate) memory_high: String,
    pub(crate) memory_max: String,
    /// The server's own unit; scopes stop with it, as control-group kill would.
    pub(crate) bind: Option<String>,
}
pub(crate) const DEFAULT_MEMORY_HIGH: &str = "30%";
pub(crate) const DEFAULT_MEMORY_MAX: &str = "40%";
/// Kernel OOM preference for harnesses and their builds; the server keeps the
/// lower value it inherited.
const WORKER_OOM_SCORE_ADJ: i32 = 500;
const WORKER_OOM_SCORE_ADJ_TEXT: &[u8] = b"500";

impl WorkerLimits {
    pub(crate) fn from_env() -> Result<Self, Error> {
        let setting = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let memory = |name: &str, default: &str| {
            let value = setting(name).unwrap_or_else(|| default.into());
            if memory_value(&value) {
                Ok(value.trim().to_owned())
            } else {
                Err(Error::invalid(format!(
                    "{name} must be bytes with an optional K/M/G/T suffix, a percentage, or infinity"
                )))
            }
        };
        let memory_high = memory("RELAY_WORKER_MEMORY_HIGH", DEFAULT_MEMORY_HIGH)?;
        let memory_max = memory("RELAY_WORKER_MEMORY_MAX", DEFAULT_MEMORY_MAX)?;
        match setting("RELAY_WORKER_SCOPE").as_deref().map(str::trim) {
            None | Some("auto") => {}
            Some("off") => return Ok(Self::default()),
            Some(_) => return Err(Error::invalid("RELAY_WORKER_SCOPE must be auto or off")),
        }
        let systemd_run = crate::runtime::installed_binary("systemd-run", "RELAY_SYSTEMD_RUN_BIN");
        let scope = WorkerScope {
            systemd_run,
            memory_high,
            memory_max,
            bind: std::fs::read_to_string("/proc/self/cgroup")
                .ok()
                .and_then(|cgroup| service_unit(&cgroup)),
        };
        Ok(Self {
            scope: scope_available(&scope).then_some(scope),
        })
    }
}
fn memory_value(value: &str) -> bool {
    let value = value.trim();
    if value == "infinity" {
        return true;
    }
    let digits = value
        .strip_suffix('%')
        .or_else(|| value.strip_suffix(['K', 'M', 'G', 'T']))
        .unwrap_or(value);
    !digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|b| b.is_ascii_digit())
}
/// The service unit owning this process when it runs under a user manager.
fn service_unit(cgroup: &str) -> Option<String> {
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let unit = path.rsplit('/').next()?;
    (path.contains("/user@") && unit.ends_with(".service") && !unit.starts_with("user@"))
        .then(|| unit.to_owned())
}
/// Probe once at startup; dev shells, containers, macOS and system services
/// without a user manager keep launching harnesses directly.
fn scope_available(scope: &WorkerScope) -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        return false;
    }
    let mut probe = Command::new(&scope.systemd_run);
    probe.args(SCOPE_ARGUMENTS).arg("true");
    capture(&mut probe, 1024, Duration::from_secs(10), "systemd-run")
        .is_ok_and(|output| output.status.success())
}
/// Build the harness command, wrapped in a memory-limited scope when enabled.
/// `systemd-run --scope` execs the harness in place, so the PID, process group,
/// parent-death signal and priority set by the caller apply to the harness.
/// Prompts are passed as arguments, so systemd must not expand `$` in them.
/// Managers without `--expand-environment` fail the probe and run directly.
const SCOPE_ARGUMENTS: [&str; 5] = [
    "--user",
    "--scope",
    "--quiet",
    "--collect",
    "--expand-environment=no",
];
pub(crate) fn worker_command(
    binary: &std::path::Path,
    limits: &WorkerLimits,
) -> tokio::process::Command {
    let Some(scope) = &limits.scope else {
        return tokio::process::Command::new(binary);
    };
    let mut command = tokio::process::Command::new(&scope.systemd_run);
    command.args(SCOPE_ARGUMENTS).args([
        "--description=Relay worker",
        // A build killed inside the scope fails alone; the harness continues.
        "--property=OOMPolicy=continue",
    ]);
    command.arg(format!("--property=MemoryHigh={}", scope.memory_high));
    command.arg(format!("--property=MemoryMax={}", scope.memory_max));
    if let Some(unit) = &scope.bind {
        command.arg(format!("--property=BindsTo={unit}"));
        command.arg(format!("--property=After={unit}"));
    }
    command.arg("--").arg(binary);
    command
}
/// The cgroup a launched worker runs in, once it differs from the server's.
pub(crate) async fn worker_cgroup(pid: u32, limits: &WorkerLimits) -> Option<std::path::PathBuf> {
    limits.scope.as_ref()?;
    let read = |pid: &str| {
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .ok()
            .and_then(|c| {
                c.lines()
                    .find_map(|l| l.strip_prefix("0::"))
                    .map(str::to_owned)
            })
    };
    let server = read("self")?;
    // systemd-run joins the scope before it execs the harness.
    for _ in 0..100 {
        let current = read(&pid.to_string())?;
        if current != server {
            return Some(
                std::path::Path::new("/sys/fs/cgroup").join(current.trim_start_matches('/')),
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}
/// Processes in the cgroup killed by any OOM killer, if the cgroup still exists.
pub(crate) fn oom_kills(cgroup: &std::path::Path) -> Option<u64> {
    let events = std::fs::read_to_string(cgroup.join("memory.events")).ok()?;
    events
        .lines()
        .find_map(|line| line.strip_prefix("oom_kill "))
        .and_then(|count| count.trim().parse().ok())
}
pub(crate) const OUT_OF_MEMORY: &str = "Worker build ran out of memory and the harness was killed; reduce build parallelism or raise RELAY_WORKER_MEMORY_MAX";
/// Relay only sends SIGKILL after a turn has finished, so a harness that
/// disconnects after SIGKILL was killed externally, almost always by the
/// kernel OOM killer. A readable scope with no OOM kills rules that out.
pub(crate) fn exit_failure(status: Option<ExitStatus>, oom_kills: Option<u64>) -> Option<Error> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.and_then(|s| s.signal()) != Some(libc::SIGKILL) {
            return None;
        }
        Some(Error::invalid(if oom_kills == Some(0) {
            "Worker harness was killed (SIGKILL)"
        } else {
            OUT_OF_MEMORY
        }))
    }
    #[cfg(not(unix))]
    {
        let _ = (status, oom_kills);
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::path::Path;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn background_worker_priority_is_inherited_without_renicing_server() {
        let parent_priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        for inherited in [None, Some(19)] {
            let mut command = tokio::process::Command::new("sh");
            command
                .args(["-c", "sleep 30 & printf '%s\\n' \"$!\"; wait"])
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .process_group(0);
            if let Some(priority) = inherited {
                unsafe {
                    command.pre_exec(move || {
                        if libc::setpriority(libc::PRIO_PROCESS, 0, priority) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
            background_worker(&mut command);
            let mut child = command.spawn().unwrap();
            let pid = child.id().unwrap();
            let _guard = crate::runtime::ProcessGuard {
                owned: Some((pid, crate::runtime::process_identity(pid).unwrap())),
            };
            let mut output = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let descendant: u32 = line.trim().parse().unwrap();
            let expected = inherited.unwrap_or(parent_priority).max(10);
            assert_eq!(
                unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) },
                expected
            );
            assert_eq!(
                unsafe { libc::getpriority(libc::PRIO_PROCESS, descendant) },
                expected
            );
            assert_eq!(unsafe { libc::getpgid(pid as i32) }, pid as i32);
            assert_eq!(unsafe { libc::getpgid(descendant as i32) }, pid as i32);
            assert_eq!(
                unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) },
                parent_priority
            );
            crate::runtime::terminate(&mut child).await;
            assert!(!child.wait().await.unwrap().success());
        }
    }

    fn scope() -> WorkerScope {
        WorkerScope {
            systemd_run: "/run/current-system/sw/bin/systemd-run".into(),
            memory_high: "3G".into(),
            memory_max: "40%".into(),
            bind: Some("relay.service".into()),
        }
    }
    fn arguments(command: &tokio::process::Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn worker_command_uses_scope_only_when_available() {
        let direct = worker_command(Path::new("/bin/claude"), &WorkerLimits::default());
        assert_eq!(direct.as_std().get_program(), "/bin/claude");
        assert!(arguments(&direct).is_empty());

        let limits = WorkerLimits {
            scope: Some(scope()),
        };
        let wrapped = worker_command(Path::new("/bin/claude"), &limits);
        assert_eq!(
            wrapped.as_std().get_program(),
            "/run/current-system/sw/bin/systemd-run"
        );
        assert_eq!(
            arguments(&wrapped),
            [
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--expand-environment=no",
                "--description=Relay worker",
                "--property=OOMPolicy=continue",
                "--property=MemoryHigh=3G",
                "--property=MemoryMax=40%",
                "--property=BindsTo=relay.service",
                "--property=After=relay.service",
                "--",
                "/bin/claude",
            ]
        );
        let unbound = WorkerLimits {
            scope: Some(WorkerScope {
                bind: None,
                ..scope()
            }),
        };
        let args = arguments(&worker_command(Path::new("/bin/codex"), &unbound));
        assert!(!args.iter().any(|a| a.contains("BindsTo")));
        assert_eq!(args.last().unwrap(), "/bin/codex");
    }

    #[test]
    fn scope_binds_only_to_user_manager_services() {
        assert_eq!(
            service_unit(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/relay.service\n"
            )
            .as_deref(),
            Some("relay.service")
        );
        for cgroup in [
            "0::/user.slice/user-1000.slice/session-3.scope\n",
            "0::/system.slice/relay.service\n",
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-term.scope\n",
            "0::/user.slice/user-1000.slice/user@1000.service\n",
            "",
        ] {
            assert_eq!(service_unit(cgroup), None, "{cgroup}");
        }
    }

    #[test]
    fn memory_limits_accept_systemd_sizes_only() {
        for valid in ["6G", "512M", "1073741824", "40%", "infinity", " 8G "] {
            assert!(memory_value(valid), "{valid}");
        }
        for invalid in ["", "G", "6GB", "-1G", "1.5G", "6G --property=X", "%"] {
            assert!(!memory_value(invalid), "{invalid}");
        }
    }

    #[test]
    fn killed_harness_is_reported_as_out_of_memory() {
        use std::os::unix::process::ExitStatusExt;
        let killed = Some(ExitStatus::from_raw(libc::SIGKILL));
        let message = |status, kills| exit_failure(status, kills).map(|e| e.to_string());
        assert_eq!(message(killed, None).as_deref(), Some(OUT_OF_MEMORY));
        assert_eq!(message(killed, Some(2)).as_deref(), Some(OUT_OF_MEMORY));
        assert_eq!(
            message(killed, Some(0)).as_deref(),
            Some("Worker harness was killed (SIGKILL)")
        );
        // Ordinary exits, other signals and still-running harnesses keep their error.
        for status in [
            Some(ExitStatus::from_raw(1 << 8)),
            Some(ExitStatus::from_raw(libc::SIGTERM)),
            None,
        ] {
            assert!(exit_failure(status, Some(1)).is_none());
            assert!(exit_failure(status, None).is_none());
        }
    }

    #[test]
    fn oom_kills_reads_memory_events() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(oom_kills(directory.path()), None);
        std::fs::write(
            directory.path().join("memory.events"),
            "low 0\nhigh 4\nmax 3\noom 1\noom_kill 1\noom_group_kill 0\n",
        )
        .unwrap();
        assert_eq!(oom_kills(directory.path()), Some(1));
    }

    #[tokio::test]
    async fn background_worker_prefers_workers_for_oom_kill() {
        let server = std::fs::read_to_string("/proc/self/oom_score_adj").unwrap();
        let server: i32 = server.trim().parse().unwrap();
        let mut command = tokio::process::Command::new("cat");
        command
            .arg("/proc/self/oom_score_adj")
            .stdout(Stdio::piped());
        background_worker(&mut command);
        let output = command.output().await.unwrap();
        let worker: i32 = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(worker, server.max(WORKER_OOM_SCORE_ADJ));
        let after = std::fs::read_to_string("/proc/self/oom_score_adj").unwrap();
        assert_eq!(after.trim().parse::<i32>().unwrap(), server);
    }

    /// Exercises a real transient scope when a systemd user manager is reachable.
    #[tokio::test]
    async fn scoped_worker_keeps_pid_group_and_limits() {
        let Some(systemd_run) = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join("systemd-run"))
                .find(|p| p.is_file())
        }) else {
            return;
        };
        let scope = WorkerScope {
            systemd_run,
            memory_high: "1G".into(),
            memory_max: "2G".into(),
            bind: None,
        };
        if !scope_available(&scope) {
            return;
        }
        let limits = WorkerLimits { scope: Some(scope) };
        let mut command = worker_command(Path::new("sh"), &limits);
        command
            .args(["-c", "echo $$; echo '$HOME'; read line"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        background_worker(&mut command);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        // systemd-run execs the harness in place.
        assert_eq!(line.trim().parse::<u32>().ok(), Some(pid), "{line:?}");
        // Arguments such as prompts reach the harness without expansion.
        line.clear();
        output.read_line(&mut line).await.unwrap();
        assert_eq!(line, "$HOME\n");
        assert_eq!(unsafe { libc::getpgid(pid as i32) }, pid as i32);
        let cgroup = worker_cgroup(pid, &limits).await.unwrap();
        assert!(cgroup.to_string_lossy().ends_with(".scope"), "{cgroup:?}");
        assert_eq!(
            std::fs::read_to_string(cgroup.join("memory.max"))
                .unwrap()
                .trim(),
            "2147483648"
        );
        assert_eq!(oom_kills(&cgroup), Some(0));
        // Relay's process-group termination still reaches the scoped harness.
        crate::runtime::terminate(&mut child).await;
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.wait().await.unwrap().signal(), Some(libc::SIGKILL));
    }
}
