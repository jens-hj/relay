//! `run_nix`: `nix develop <workspace> --command …` run by the server for an
//! agent, outside its harness sandbox.
//!
//! The command can execute any code in the workspace's flake with the server
//! user's access, so it is execution authority, not a sandbox. Relay requires
//! the project owner's opt-in and the agent's Verify and Implement
//! permissions, asks a human when any of them is Ask, and keeps the working
//! directory inside the agent's assigned workspaces. Neither the `nix` prefix
//! nor the directory scope limits what the command itself can do.
use crate::{Error, Workspace, agent_tools, runtime};
use relay_core::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::watch};

pub(crate) const TOOL: &str = "run_nix";
const MAX_TIMEOUT_S: u64 = 600;
const MAX_ARGUMENTS: usize = 256;
const MAX_COMMAND_BYTES: usize = 32 * 1024;
const MAX_ARTIFACTS: usize = 32;
/// Output kept from the start and the end of a command; the middle is omitted.
const HEAD: usize = 8 * 1024;
const TAIL: usize = 24 * 1024;
/// How long output may keep arriving after the command ended.
const DRAIN: Duration = Duration::from_secs(2);
/// The only server environment the command inherits. Relay tokens, run
/// capabilities, provider credentials and everything else are dropped.
pub(crate) const ENVIRONMENT: [&str; 22] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TMPDIR",
    "XDG_RUNTIME_DIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    // systemd-run reaches the user manager for the worker memory scope.
    "DBUS_SESSION_BUS_ADDRESS",
    "NIX_REMOTE",
    "NIX_PATH",
    "NIX_SSL_CERT_FILE",
    "SSL_CERT_FILE",
    "CARGO_TARGET_DIR",
    "RUSTC_WRAPPER",
    "SCCACHE_DIR",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    command: Vec<String>,
    /// A development shell of the workspace flake; the default shell when omitted.
    shell: Option<String>,
    workspace: Option<String>,
    directory: Option<String>,
    timeout_s: Option<u64>,
    #[serde(default)]
    artifacts: Vec<String>,
}

/// An agent's permission to run Nix commands in its project.
pub(crate) fn permission(snapshot: &Snapshot, session: &Session) -> Result<Permission, Error> {
    let project = snapshot
        .project(&session.project_id)
        .map_err(Error::invalid)?;
    Ok(project
        .nix_commands
        .permission(&agent_tools::profile(snapshot, session)?.permissions))
}

fn denied(snapshot: &Snapshot, session: &Session) -> Error {
    let off = match snapshot.project(&session.project_id) {
        Ok(project) => project.nix_commands.is_off(),
        Err(_) => true,
    };
    Error::invalid(if off {
        "Nix commands are off for this project; the project owner can allow them under Nix Commands on the project's Connections page"
    } else {
        "This agent's profile denies Nix commands; they need Verify and Implement permission"
    })
}

fn validate(input: &Input) -> Result<(), Error> {
    if input.command.is_empty()
        || input.command.len() > MAX_ARGUMENTS
        || input.command[0].is_empty()
        || input.command.iter().any(|a| a.contains('\0'))
        || input.command.iter().map(String::len).sum::<usize>() > MAX_COMMAND_BYTES
    {
        return Err(Error::invalid(
            "command must be 1–256 arguments, a program first, within 32 KiB",
        ));
    }
    if input
        .timeout_s
        .is_some_and(|t| !(1..=MAX_TIMEOUT_S).contains(&t))
    {
        return Err(Error::invalid("timeout_s must be 1–600"));
    }
    if input.artifacts.len() > MAX_ARTIFACTS {
        return Err(Error::invalid("At most 32 artifacts"));
    }
    if input.shell.as_ref().is_some_and(|s| {
        s.is_empty()
            || s.len() > 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) {
        return Err(Error::invalid(
            "shell must be a development shell name of letters, digits, - and _",
        ));
    }
    if input.workspace.as_ref().is_some_and(|w| w.len() > 4096) {
        return Err(Error::invalid("Workspace path is too long"));
    }
    for path in input.directory.iter().chain(&input.artifacts) {
        relative(path)?;
    }
    Ok(())
}

/// A path relative to a workspace root, without `..`, roots or prefixes.
fn relative(path: &str) -> Result<&Path, Error> {
    let candidate = Path::new(path);
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || !candidate
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(Error::invalid(
            "directory and artifacts must be relative paths inside the workspace, without ..",
        ));
    }
    Ok(candidate)
}

/// The canonical root of the requested assigned workspace, or of the first
/// assigned workspace when none is requested.
fn assigned_root(session: &Session, requested: Option<&str>) -> Result<PathBuf, Error> {
    let outside = || Error::invalid("Workspace is not assigned to this agent");
    let Some(requested) = requested else {
        let first = session
            .workspaces
            .first()
            .ok_or_else(|| Error::invalid("This agent has no assigned workspace"))?;
        return std::fs::canonicalize(&first.path)
            .map_err(|_| Error::invalid("Assigned workspace unavailable"));
    };
    if !Path::new(requested).is_absolute() {
        return Err(outside());
    }
    let requested = std::fs::canonicalize(requested).map_err(|_| outside())?;
    session
        .workspaces
        .iter()
        .filter_map(|w| std::fs::canonicalize(&w.path).ok())
        .find(|root| *root == requested)
        .ok_or_else(outside)
}

/// `path` resolved inside `root`; symlinks that leave the workspace are rejected.
fn inside(root: &Path, path: &str) -> Result<PathBuf, Error> {
    let resolved = std::fs::canonicalize(root.join(relative(path)?))
        .map_err(|_| Error::invalid("Path does not exist in the workspace"))?;
    if !resolved.starts_with(root) {
        return Err(Error::invalid("Path leaves the workspace"));
    }
    Ok(resolved)
}

fn quote(argument: &str) -> String {
    if !argument.is_empty()
        && argument
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./:=@%+,".contains(&b))
    {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

/// The flake reference for the workspace's default or named development shell.
fn installable(root: &Path, shell: Option<&str>) -> Result<String, Error> {
    let root = root
        .to_str()
        .filter(|r| !r.contains(['#', '?']))
        .ok_or_else(|| Error::invalid("This workspace path cannot be used as a flake reference"))?;
    Ok(match shell {
        Some(shell) => format!("{root}#{shell}"),
        None => root.to_owned(),
    })
}

/// The exact command line, as shown for approval and review.
fn command_line(installable: &str, command: &[String]) -> String {
    let mut line = format!("nix develop {} --command", quote(installable));
    for argument in command {
        line.push(' ');
        line.push_str(&quote(argument));
    }
    line
}

#[derive(Default)]
struct Output {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}
impl Output {
    fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        let room = HEAD.saturating_sub(self.head.len()).min(bytes.len());
        let (head, rest) = bytes.split_at(room);
        self.head.extend_from_slice(head);
        self.tail.extend(rest);
        let excess = self.tail.len().saturating_sub(TAIL);
        self.tail.drain(..excess);
    }
    /// The kept output, and whether any of it was omitted.
    fn text(&self) -> (String, bool) {
        let omitted = self.total - self.head.len() as u64 - self.tail.len() as u64;
        let mut text = String::from_utf8_lossy(&self.head).into_owned();
        if omitted > 0 {
            text.push_str(&format!("\n[{omitted} bytes of output omitted]\n"));
        }
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        text.push_str(&String::from_utf8_lossy(&tail));
        (text, omitted > 0)
    }
}

fn collect<R>(reader: Option<R>, output: Arc<Mutex<Output>>) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let Some(mut reader) = reader else {
            return;
        };
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if let Ok(mut output) = output.lock() {
                        output.push(&chunk[..n]);
                    }
                }
            }
        }
    })
}

/// Frees the session's single Nix command slot. It is created as the store
/// lock is released and never dropped while that lock is held.
struct Reservation {
    workspace: Workspace,
    session: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut store) = self.workspace.store.lock() {
            store.nix_runs.remove(&self.session);
        }
    }
}

/// Kills the command's process group when dropped, so a cancelled call ends
/// every process in it on any Unix platform. The group is only ever
/// signalled while its leader is unreaped: an unreaped leader, even a zombie,
/// keeps its PID and so the group ID reserved, so the kill cannot reach a
/// reused group. Nothing reaps the leader before `end` has run; see
/// [`leader_exit`].
struct Group {
    pid: Option<u32>,
}
impl Group {
    /// Ends the processes still in the group, once. Call before reaping.
    fn end(&mut self) {
        kill_group(self.pid.take());
    }
}

/// How often a running command's leader is checked for exit.
#[cfg(unix)]
const EXIT_POLL: Duration = Duration::from_millis(20);

/// Whether the leader has exited, without reaping it (`WNOWAIT`) and without
/// waiting (`WNOHANG`): `Some(false)` while it runs, `None` if its state
/// cannot be read.
#[cfg(unix)]
fn exit_observed(pid: u32) -> Option<bool> {
    loop {
        // With WNOHANG and no exit to report, waitid leaves si_signo zero.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if result == 0 {
            return Some(info.si_signo != 0);
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return None;
        }
    }
}

/// Resolves when the leader exits, without reaping it, so the group can
/// still be killed safely before the final `wait`. Every check returns at
/// once and nothing runs outside this future, so dropping it leaves no
/// waiter behind. Returns false if the exit cannot be observed.
#[cfg(unix)]
async fn leader_exit(child: &tokio::process::Child) -> bool {
    let Some(pid) = child.id() else {
        return false;
    };
    loop {
        match exit_observed(pid) {
            Some(true) => return true,
            Some(false) => tokio::time::sleep(EXIT_POLL).await,
            None => return false,
        }
    }
}
/// Without process groups there is nothing to keep reserved.
#[cfg(not(unix))]
async fn leader_exit(child: &mut tokio::process::Child) -> bool {
    child.wait().await.is_ok()
}
impl Drop for Group {
    fn drop(&mut self) {
        self.end();
    }
}
fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

const RECORD_NAME: &str = "mcp__relay__run_nix";

/// The final state of a command's audit record.
#[derive(Clone)]
struct Final {
    body: String,
    state: ToolState,
    exit_code: Option<i64>,
    duration_ms: Option<u64>,
    finished_ms: u64,
}
impl Final {
    fn apply(&self, message: &mut Message) {
        message.body = self.body.clone();
        if let Some(tool) = &mut message.tool {
            tool.state = self.state;
            tool.exit_code = self.exit_code;
            tool.finished_ms = Some(self.finished_ms);
            if self.duration_ms.is_some() {
                tool.detail = Some(ToolDetail::Run {
                    exit_code: self.exit_code,
                    duration_ms: self.duration_ms,
                });
            }
        }
    }
}

/// Attempts to save a command's outcome before the call reports failure.
const FINISH_ATTEMPTS: u64 = 3;
/// Delays between background retries of a record that could not be saved.
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];

/// The server's transcript record of one command. It is persisted as
/// Running before launch and finalized in place; finalizing changes only
/// this record, so it still works after the run ended and cannot add any
/// other message for a stale run. Until a save succeeds the record's final
/// state stays pending, and dropping the audit keeps retrying it.
struct Audit {
    workspace: Workspace,
    session: String,
    id: String,
    header: String,
    /// The command's outcome once known; Interrupted when dropped without one.
    pending: Option<Final>,
    saved: bool,
}
impl Audit {
    /// Saves the command's outcome, retrying briefly. On failure the outcome
    /// stays pending for the drop path to retry, and the error is returned.
    async fn finish(&mut self, record: Final) -> Result<(), Error> {
        self.pending = Some(record.clone());
        let mut attempt = 0;
        loop {
            attempt += 1;
            match finalize(&self.workspace, &self.session, &self.id, |m| {
                record.apply(m)
            }) {
                Ok(()) => {
                    self.saved = true;
                    return Ok(());
                }
                Err(error) if attempt == FINISH_ATTEMPTS => return Err(error),
                Err(_) => tokio::time::sleep(Duration::from_millis(100 * attempt)).await,
            }
        }
    }
}
impl Drop for Audit {
    // Dropped after the process group is killed when the call is cancelled.
    fn drop(&mut self) {
        if self.saved {
            return;
        }
        let record = self.pending.take().unwrap_or_else(|| Final {
            body: format!(
                "{}\nInterrupted: the tool call ended before the command finished, and Relay terminated it",
                self.header
            ),
            state: ToolState::Interrupted,
            exit_code: None,
            duration_ms: None,
            finished_ms: crate::tool_activity::now_ms(),
        });
        if finalize(&self.workspace, &self.session, &self.id, |m| {
            record.apply(m)
        })
        .is_ok()
        {
            return;
        }
        let (workspace, session, id) = (
            self.workspace.clone(),
            self.session.clone(),
            self.id.clone(),
        );
        let lost = move |id: &str| {
            eprintln!("Relay could not save Nix command record {id}; it may still show Running")
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    for delay in RETRY_DELAYS {
                        tokio::time::sleep(delay).await;
                        if finalize(&workspace, &session, &id, |m| record.apply(m)).is_ok() {
                            return;
                        }
                    }
                    lost(&id);
                });
            }
            Err(_) => lost(&id),
        }
    }
}
fn finalize(
    workspace: &Workspace,
    session: &str,
    id: &str,
    update: impl FnOnce(&mut Message),
) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let mut snapshot = store.snapshot()?;
    let Some(message) = snapshot.messages.iter_mut().find(|m| {
        m.id == id
            && m.session_id == session
            && m.author == "Relay"
            && m.tool.as_ref().is_some_and(|t| t.name == RECORD_NAME)
    }) else {
        // The session was deleted.
        return Ok(());
    };
    update(message);
    snapshot.revision = snapshot
        .revision
        .checked_add(1)
        .ok_or_else(|| Error::invalid("Revision exhausted"))?;
    store.save(&snapshot)?;
    workspace.snapshots.send_replace(snapshot);
    Ok(())
}

// Test hook between building the command and the final locked checks.
#[cfg(test)]
thread_local! {
    pub(crate) static BEFORE_LAUNCH: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn before_launch() {
    if let Some(hook) = BEFORE_LAUNCH.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
}
#[cfg(not(test))]
fn before_launch() {}

enum End {
    Exited,
    TimedOut,
    Stopped,
    Failed,
}

pub(crate) async fn run(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    input: Value,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    let recorded_input = input.to_string();
    let input: Input =
        serde_json::from_value(input).map_err(|_| Error::invalid("Invalid tool arguments"))?;
    validate(&input)?;
    let snapshot = workspace.snapshots.borrow().clone();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == caller)
        .ok_or_else(|| Error::invalid("Agent missing"))?;
    let allowed = permission(&snapshot, session)?;
    if allowed == Permission::Deny {
        return Err(denied(&snapshot, session));
    }
    let root = assigned_root(session, input.workspace.as_deref())?;
    let directory = match &input.directory {
        Some(path) => inside(&root, path)?,
        None => root.clone(),
    };
    if !directory.is_dir() {
        return Err(Error::invalid(
            "directory must be a directory in the workspace",
        ));
    }
    let installable = installable(&root, input.shell.as_deref())?;
    let line = command_line(&installable, &input.command);
    let directory_text = directory.display().to_string();
    let description =
        format!("Run on the server, outside the agent sandbox, in {directory_text}: {line}");
    // Approval requests keep only a bounded description; a command must be
    // shown in full, so anything longer is refused in every mode.
    if description.len() > crate::harness::DESCRIPTION_LIMIT {
        return Err(Error::invalid(
            "This command is too long to show in full for approval; put it in a script in the workspace and run that",
        ));
    }
    if allowed == Permission::Ask {
        let approved =
            crate::harness::permission(workspace, caller, run, RECORD_NAME, &description, stop)
                .await?;
        if *stop.borrow() {
            return Err(agent_tools::unauthorized());
        }
        if !approved {
            return Err(Error::invalid("Human denied the Nix command"));
        }
    }
    let mut command =
        crate::process::worker_command(&workspace.config.nix, &workspace.config.worker);
    command
        .arg("develop")
        .arg(&installable)
        .arg("--command")
        .args(&input.command)
        .env_clear();
    for name in ENVIRONMENT {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let project_root = snapshot
        .projects
        .iter()
        .find(|p| p.id == session.project_id)
        .and_then(|p| p.root.as_deref());
    crate::build_output::configure(
        &mut command,
        project_root,
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
        std::env::var_os(crate::build_output::SHARED_CACHE_ENV).as_deref(),
    );
    command
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    {
        let parent = unsafe { libc::getpid() };
        // Only async-signal-safe operations between fork and exec.
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
    crate::process::background_worker(&mut command);
    before_launch();
    // The final checks, the slot, the Running record and the launch share
    // one store lock, which stops, shutdown and policy changes also take, so
    // none of them can land between the checks and the spawn.
    let header = format!("Command: {line}\nDirectory: {directory_text}");
    let id = format!("nix-command-{}", uuid::Uuid::new_v4());
    let (_reservation, mut audit, mut child) = {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(agent_tools::unauthorized());
        }
        let mut current = store.snapshot()?;
        agent_tools::active_caller(&store, &current, caller, run)?;
        let latest = current
            .sessions
            .iter()
            .find(|s| s.id == caller)
            .ok_or_else(|| Error::invalid("Agent missing"))?;
        if permission(&current, latest)? != allowed
            || assigned_root(latest, Some(&root.display().to_string()))? != root
        {
            return Err(Error::invalid("Nix command policy changed; retry"));
        }
        if store.nix_runs.contains(caller) {
            return Err(Error::invalid(
                "A Nix command is already running for this agent",
            ));
        }
        current.messages.push(Message {
            id: id.clone(),
            session_id: caller.into(),
            author: "Relay".into(),
            sender: Sender::User,
            kind: "tool".into(),
            body: format!("{header}\nRunning"),
            parts: vec![],
            tool: Some(ToolCall {
                run_id: run.into(),
                name: RECORD_NAME.into(),
                kind: ToolKind::Run,
                state: ToolState::Running,
                input: recorded_input,
                target: line.clone(),
                directory: Some(directory_text.clone()),
                exit_code: None,
                started_ms: Some(crate::tool_activity::now_ms()),
                finished_ms: None,
                detail: None,
            }),
        });
        current.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&current)?;
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                if let Some(record) = current.messages.iter_mut().find(|m| m.id == id) {
                    record.body = format!("{header}\nCould not start nix");
                    if let Some(tool) = &mut record.tool {
                        tool.state = ToolState::Failed;
                        tool.finished_ms = Some(crate::tool_activity::now_ms());
                    }
                }
                current.revision += 1;
                store.save(&current)?;
                workspace.snapshots.send_replace(current);
                return Err(Error::invalid(
                    "Cannot run nix; check the server's Nix installation or RELAY_NIX_BIN",
                ));
            }
        };
        workspace.snapshots.send_replace(current);
        store.nix_runs.insert(caller.to_owned());
        (
            Reservation {
                workspace: workspace.clone(),
                session: caller.to_owned(),
            },
            Audit {
                workspace: workspace.clone(),
                session: caller.to_owned(),
                id: id.clone(),
                header,
                pending: None,
                saved: false,
            },
            child,
        )
    };
    let started = tokio::time::Instant::now();
    // Declared last, so a cancelled call kills the group before the child,
    // the audit record and the slot are dropped.
    let mut group = Group { pid: child.id() };
    let output = Arc::new(Mutex::new(Output::default()));
    let mut readers = [
        collect(child.stdout.take(), output.clone()),
        collect(child.stderr.take(), output.clone()),
    ];
    let timeout = Duration::from_secs(input.timeout_s.unwrap_or(MAX_TIMEOUT_S));
    let end = tokio::select! {
        exited = leader_exit(&mut child) => if exited { End::Exited } else { End::Failed },
        _ = stop.wait_for(|stopped| *stopped) => End::Stopped,
        _ = tokio::time::sleep(timeout) => End::TimedOut,
    };
    // The leader has exited or is still running, but is not reaped yet, so
    // the group ID still belongs to it: end every process left in the group,
    // then reap the leader.
    group.end();
    let status = match end {
        End::Exited => child.wait().await.ok(),
        _ => {
            runtime::terminate(&mut child).await;
            None
        }
    };
    for reader in &mut readers {
        let _ = tokio::time::timeout(DRAIN, &mut *reader).await;
        reader.abort();
    }
    let duration_ms = started.elapsed().as_millis() as u64;
    let (text, truncated, total) = output
        .lock()
        .map(|o| {
            let (text, truncated) = o.text();
            (text, truncated, o.total)
        })
        .map_err(|_| Error::invalid("Nix command output unavailable"))?;
    let (exit_code, signal) = match status {
        Some(status) => {
            #[cfg(unix)]
            let signal = {
                use std::os::unix::process::ExitStatusExt;
                status.signal()
            };
            #[cfg(not(unix))]
            let signal: Option<i32> = None;
            (status.code(), signal)
        }
        None => (None, None),
    };
    let outcome = match &end {
        End::Exited => match (exit_code, signal) {
            (Some(code), _) => format!("Exit: {code}"),
            (None, Some(signal)) => format!("Killed by signal {signal}"),
            (None, None) => "Exit: unknown".into(),
        },
        End::TimedOut => format!("Timed out after {}s and was terminated", timeout.as_secs()),
        End::Stopped => "Stopped with the agent's run and was terminated".into(),
        End::Failed => "Relay could not wait for the command".into(),
    };
    let state = match &end {
        End::Exited if exit_code == Some(0) => ToolState::Completed,
        End::Stopped => ToolState::Interrupted,
        _ => ToolState::Failed,
    };
    let record = Final {
        body: format!("{}\n{outcome}\n{text}", audit.header),
        state,
        exit_code: exit_code.map(i64::from),
        duration_ms: Some(duration_ms),
        finished_ms: crate::tool_activity::now_ms(),
    };
    if let Err(error) = audit.finish(record).await {
        return Err(Error::invalid(format!(
            "The Nix command ended ({outcome}), but Relay could not save its transcript record and will keep retrying: {error}"
        )));
    }
    if matches!(end, End::Stopped) {
        return Err(Error::invalid(
            "Agent run stopped; the Nix command was terminated",
        ));
    }
    let mut artifacts = vec![];
    let mut unavailable = vec![];
    for path in &input.artifacts {
        match inside(&root, path) {
            Ok(resolved) if resolved.is_file() => artifacts.push(json!({
                "path": resolved.display().to_string(),
                "bytes": std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or_default(),
            })),
            Ok(_) => unavailable.push(json!({"path": path, "reason": "not a file"})),
            Err(error) => unavailable.push(json!({"path": path, "reason": error.to_string()})),
        }
    }
    Ok(json!({
        "command": line,
        "directory": directory_text,
        "exit_code": exit_code,
        "signal": signal,
        "timed_out": matches!(end, End::TimedOut),
        "duration_ms": duration_ms,
        "output": text,
        "output_bytes": total,
        "output_truncated": truncated,
        "artifacts": artifacts,
        "unavailable_artifacts": unavailable,
    }))
}

/// The run_nix tool specification for an agent with `permission`.
pub(crate) fn spec(permission: Permission) -> Value {
    let string = json!({"type":"string"});
    let approval = if permission == Permission::Ask {
        " A human approves each command before it runs."
    } else {
        ""
    };
    json!({
        "name": TOOL,
        "description": format!("Run `nix develop <workspace> --command <command>` on the Relay server, outside your sandbox, for builds, tests and screenshots the sandbox blocks. The command runs with the server user's access and can execute any code in the workspace; use it only for the user's task. shell names a development shell of the workspace flake (default: its default shell). workspace is an assigned workspace path (default: the first); directory and artifacts are relative to it. Returns the exit code, the start and end of the output, and the paths of requested artifact files to pass to report_result.{approval}"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {"type":"array","items":string,"minItems":1,"maxItems":MAX_ARGUMENTS},
                "shell": string,
                "workspace": string,
                "directory": string,
                "timeout_s": {"type":"integer","minimum":1,"maximum":MAX_TIMEOUT_S},
                "artifacts": {"type":"array","items":string,"maxItems":MAX_ARTIFACTS},
            },
            "required": ["command"],
            "additionalProperties": false,
        },
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn zombie(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
        })
    }

    #[tokio::test]
    async fn the_leader_stays_unreaped_until_its_group_has_been_ended() {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "exit 7"]).process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        let mut group = Group { pid: child.id() };
        assert!(leader_exit(&child).await);
        // Observing the exit does not reap the leader, so its PID and group
        // ID stay reserved while the group is killed.
        assert!(zombie(pid));
        group.end();
        assert_eq!(group.pid, None);
        assert!(zombie(pid));
        // Only then is the leader reaped, with its real status.
        assert_eq!(child.wait().await.unwrap().code(), Some(7));
        // An ended group is never signalled again, including on drop.
        group.end();
        drop(group);
    }

    #[tokio::test]
    async fn observing_the_leader_never_blocks_or_outlives_the_observation() {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "sleep 30"]).process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        // One check returns at once while the leader runs.
        let started = std::time::Instant::now();
        assert_eq!(exit_observed(pid), Some(false));
        assert!(started.elapsed() < Duration::from_secs(1));
        // A timed-out observation is simply dropped: it spawned no task or
        // thread, so nothing keeps waiting on the PID.
        let runtime = tokio::runtime::Handle::current();
        let tasks = runtime.metrics().num_alive_tasks();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), leader_exit(&child))
                .await
                .is_err()
        );
        assert_eq!(runtime.metrics().num_alive_tasks(), tasks);
        assert!(!zombie(pid));
        // The leader is still Relay's own: end its group, observe the exit
        // without reaping, and only then reap it.
        let mut group = Group { pid: child.id() };
        group.end();
        assert!(leader_exit(&child).await);
        assert!(zombie(pid));
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.wait().await.unwrap().signal(), Some(libc::SIGKILL));
    }
}
