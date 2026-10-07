use crate::{Error, Workspace};
use relay_core::*;
use std::{
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::watch,
};

#[derive(Clone, Debug)]
pub struct RemoteConfig {
    pub repository: String,
    pub owner: String,
    pub number: u64,
}
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub remote: Option<RemoteConfig>,
    pub repository: Option<PathBuf>,
    pub(crate) gh: PathBuf,
    pub(crate) codex: PathBuf,
}
impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            remote: None,
            repository: None,
            gh: "gh".into(),
            codex: "codex".into(),
        }
    }
}
impl RuntimeConfig {
    pub fn from_env() -> Result<Self, Error> {
        let repo = std::env::var("RELAY_GITHUB_REPO").ok();
        let owner = std::env::var("RELAY_GITHUB_PROJECT_OWNER").ok();
        let number = std::env::var("RELAY_GITHUB_PROJECT_NUMBER").ok();
        let remote = match (repo, owner, number) {
            (None, None, None) => None,
            (Some(repository), Some(owner), Some(number)) => {
                if repository.split('/').count() != 2
                    || repository
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
                    || !repository
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/_.-".contains(&c))
                    || owner.is_empty()
                    || !owner
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err(Error::invalid(
                        "Invalid GitHub repository or owner configuration",
                    ));
                }
                let number = number
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::invalid("Invalid GitHub project number"))?;
                Some(RemoteConfig {
                    repository,
                    owner,
                    number,
                })
            }
            _ => {
                return Err(Error::invalid(
                    "Set all three RELAY_GITHUB_* configuration variables",
                ));
            }
        };
        let repository = std::env::var_os("RELAY_REPO_PATH").map(PathBuf::from);
        if let Some(path) = &repository
            && (!path.is_absolute() || !path.is_dir())
        {
            return Err(Error::invalid(
                "RELAY_REPO_PATH must be an existing absolute repository path",
            ));
        }
        Ok(Self {
            remote,
            repository,
            ..Self::default()
        })
    }
}
pub(crate) fn active(status: &WorkerStatus) -> bool {
    matches!(status, WorkerStatus::Queued | WorkerStatus::Running)
}
pub(crate) fn authorize_turn(
    snapshot: &Snapshot,
    issue_id: &str,
    director_id: &str,
    approved: bool,
    config: &RuntimeConfig,
) -> Result<(), Error> {
    let issue = snapshot
        .issues
        .iter()
        .find(|i| i.id == issue_id)
        .ok_or_else(|| Error::invalid("Live issue not found; sync the board"))?;
    let project = snapshot
        .project(&issue.project_id)
        .map_err(Error::invalid)?;
    let remote = config
        .remote
        .as_ref()
        .ok_or_else(|| Error::invalid("Configure GitHub project before starting workers"))?;
    if project.fixture
        || !project
            .columns
            .iter()
            .any(|column| column.id == issue.column_id)
        || project
            .github
            .as_ref()
            .is_none_or(|g| g.last_synced_at.is_none())
        || issue.reference.provider != Provider::Github
        || issue.reference.repository != remote.repository
    {
        return Err(Error::invalid(
            "Worker requires a synced live issue in the configured repository",
        ));
    }
    if config.repository.is_none() {
        return Err(Error::invalid("Set RELAY_REPO_PATH on the server"));
    }
    let director = snapshot
        .directors
        .iter()
        .find(|d| d.id == director_id && d.project_id == issue.project_id)
        .ok_or_else(|| Error::invalid("Director does not belong to the issue project"))?;
    let profile = snapshot
        .effective_profile(director)
        .map_err(Error::invalid)?;
    profile.validate().map_err(Error::invalid)?;
    if profile.harness != Harness::Codex {
        return Err(Error::invalid("Worker requires the Codex harness"));
    }
    if let DirectorScope::Issues { issue_ids } = &profile.scope
        && !issue_ids.iter().any(|i| i == issue_id)
    {
        return Err(Error::invalid(
            "Issue is outside the current director scope",
        ));
    }
    match profile.permissions[&Task::Implement] {
        Permission::Deny => return Err(Error::invalid("Current profile denies implementation")),
        Permission::Ask if !approved => {
            return Err(Error::invalid(
                "Implementation approval is required for this turn",
            ));
        }
        _ => {}
    }
    let count = snapshot
        .sessions
        .iter()
        .filter(|s| {
            s.director_id == director_id && s.worker.as_ref().is_some_and(|w| active(&w.status))
        })
        .count();
    if count >= usize::from(profile.max_workers) {
        return Err(Error::invalid("Current director worker limit reached"));
    }
    Ok(())
}
fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let output = crate::process::capture(
        ProcessCommand::new("git").arg("-C").arg(path).args(args),
        1024 * 1024,
        std::time::Duration::from_secs(60),
        "Git",
    )?;
    if output.truncated {
        return Err(Error::invalid("Git metadata output exceeds 1 MiB limit"));
    }
    if !output.status.success() {
        return Err(Error::invalid(
            "Git operation failed; check repository/worktree availability",
        ));
    }
    Ok(output.bytes)
}
pub(crate) fn prepare(
    config: &RuntimeConfig,
    session_id: &str,
) -> Result<(String, String, String), Error> {
    let repo = config
        .repository
        .as_ref()
        .ok_or_else(|| Error::invalid("Set RELAY_REPO_PATH"))?;
    let base = String::from_utf8(git(repo, &["rev-parse", "HEAD"])?)
        .map_err(Error::internal)?
        .trim()
        .to_owned();
    let root = repo
        .parent()
        .ok_or_else(|| Error::invalid("Repository has no parent"))?
        .join("relay-workers");
    std::fs::create_dir_all(&root).map_err(|_| Error::invalid("Cannot create worker directory"))?;
    let path = root.join(session_id);
    let branch = format!("relay/{session_id}");
    git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            path.to_str()
                .ok_or_else(|| Error::invalid("Non UTF-8 worktree path"))?,
            &base,
        ],
    )?;
    Ok((path.to_string_lossy().into_owned(), branch, base))
}
const DIFF_LIMIT: usize = 128 * 1024;
fn diff_output(command: &mut ProcessCommand, limit: usize) -> Result<(Vec<u8>, bool), Error> {
    let output = crate::process::capture(
        command,
        limit,
        std::time::Duration::from_secs(15),
        "Git review",
    )?;
    if !output.truncated && !output.status.success() && output.status.code() != Some(1) {
        return Err(Error::invalid(
            "Git review failed; inspect the worker worktree",
        ));
    }
    Ok((output.bytes, output.truncated))
}
fn append_bounded(target: &mut String, bytes: &[u8], truncated: &mut bool) {
    let s = String::from_utf8_lossy(bytes);
    let remaining = DIFF_LIMIT.saturating_sub(target.len());
    let mut end = s.len().min(remaining);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&s[..end]);
    *truncated |= end < s.len();
}
pub(crate) fn changes(path: &str, base: &str) -> Result<ChangeSet, Error> {
    let path = Path::new(path);
    let (mut names, names_cut) = diff_output(
        ProcessCommand::new("git").arg("-C").arg(path).args([
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            base,
            "--",
        ]),
        512 * 4096,
    )?;
    let (mut untracked, untracked_cut) = diff_output(
        ProcessCommand::new("git").arg("-C").arg(path).args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ]),
        512 * 4096,
    )?;
    // A truncated NUL-delimited list must not invent a partial pathname.
    if names_cut {
        names.truncate(names.iter().rposition(|b| *b == 0).map_or(0, |i| i + 1));
    }
    if untracked_cut {
        untracked.truncate(untracked.iter().rposition(|b| *b == 0).map_or(0, |i| i + 1));
    }
    let mut files = Vec::new();
    let mut diff = String::new();
    let mut truncated = names_cut || untracked_cut;
    let (bytes, cut) = diff_output(
        ProcessCommand::new("git").arg("-C").arg(path).args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            base,
            "--",
        ]),
        DIFF_LIMIT,
    )?;
    truncated |= cut;
    append_bounded(&mut diff, &bytes, &mut truncated);
    for name in names
        .split(|b| *b == 0)
        .chain(untracked.split(|b| *b == 0))
        .filter(|n| !n.is_empty())
    {
        if files.len() < 512 {
            files.push(String::from_utf8_lossy(name).into_owned());
        } else {
            truncated = true;
        }
    }
    for name in untracked.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        if diff.len() >= DIFF_LIMIT {
            truncated = true;
            break;
        }
        let name = String::from_utf8_lossy(name);
        let file = path.join(name.as_ref());
        let metadata = std::fs::symlink_metadata(&file)
            .map_err(|_| Error::invalid("Untracked file changed during review capture"))?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&file)
                .map_err(|_| Error::invalid("Cannot review untracked symlink"))?;
            append_bounded(
                &mut diff,
                format!(
                    "Untracked symlink: {name} -> {}\n",
                    target.to_string_lossy()
                )
                .as_bytes(),
                &mut truncated,
            );
            continue;
        }
        if !metadata.is_file()
            || !file
                .canonicalize()
                .map_err(|_| Error::invalid("Cannot resolve untracked file"))?
                .starts_with(
                    path.canonicalize()
                        .map_err(|_| Error::invalid("Cannot resolve worktree"))?,
                )
        {
            return Err(Error::invalid(
                "Untracked review path is outside the worktree or is not a regular file",
            ));
        }
        let (bytes, cut) = diff_output(
            ProcessCommand::new("git")
                .arg("-C")
                .arg(path)
                .args([
                    "diff",
                    "--no-index",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                ])
                .arg(name.as_ref()),
            DIFF_LIMIT.saturating_sub(diff.len()),
        )?;
        truncated |= cut;
        append_bounded(&mut diff, &bytes, &mut truncated);
    }
    files.sort();
    files.dedup();
    Ok(ChangeSet {
        files,
        diff,
        truncated,
    })
}
/// Only completed immutable items are published; deltas never mutate a message.
pub(crate) fn event(
    snapshot: &mut Snapshot,
    session_id: &str,
    value: &serde_json::Value,
) -> Result<bool, Error> {
    let session = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| Error::invalid("Session missing"))?;
    let worker = session.worker.as_mut().unwrap();
    match value["type"].as_str().unwrap_or("") {
        "thread.started" => {
            let id = value["thread_id"]
                .as_str()
                .ok_or_else(|| Error::invalid("Codex thread ID missing"))?;
            if id.is_empty() || id.len() > 128 || id.starts_with('-') {
                return Err(Error::invalid("Codex thread ID is invalid"));
            }
            if worker
                .thread_id
                .as_ref()
                .is_some_and(|previous| previous != id)
            {
                return Err(Error::invalid("Codex resumed a different thread"));
            }
            worker.thread_id = Some(id.into());
        }
        "turn.completed" => {
            if worker.thread_id.is_none() {
                return Err(Error::invalid(
                    "Codex completed without a recorded thread ID",
                ));
            }
            let u = &value["usage"];
            worker.usage = Some(TokenUsage {
                input_tokens: u["input_tokens"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("Codex usage missing"))?,
                cached_input_tokens: u["cached_input_tokens"].as_u64().unwrap_or(0),
                output_tokens: u["output_tokens"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("Codex usage missing"))?,
            });
            return Ok(true);
        }
        "turn.failed" | "error" => {
            return Err(Error::invalid(
                "Codex reported a turn failure; check Codex authentication, configuration and network",
            ));
        }
        "item.completed" => {
            let item = &value["item"];
            let kind = item["type"].as_str().unwrap_or("event");
            let body = match kind {
                "agent_message" | "reasoning" => item["text"].as_str().map(str::to_owned),
                "command_execution" => Some(format!(
                    "Command: {}\nExit: {}\n{}",
                    item["command"].as_str().unwrap_or(""),
                    item["exit_code"],
                    item["aggregated_output"].as_str().unwrap_or("")
                )),
                "file_change" => Some(item["changes"].to_string()),
                _ => None,
            };
            if let Some(mut body) = body {
                const LIMIT: usize = 64 * 1024;
                if body.len() > LIMIT {
                    let mut end = LIMIT - 32;
                    while !body.is_char_boundary(end) {
                        end -= 1;
                    }
                    body.truncate(end);
                    body.push_str("\n[Message truncated]");
                }
                snapshot.messages.push(Message {
                    id: format!("message-{}", uuid::Uuid::new_v4()),
                    session_id: session_id.into(),
                    author: "Codex".into(),
                    kind: kind.into(),
                    body,
                });
            }
        }
        _ => {}
    }
    Ok(false)
}
async fn line(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<Option<Vec<u8>>, Error> {
    let mut line = Vec::new();
    loop {
        let buf = reader
            .fill_buf()
            .await
            .map_err(|_| Error::invalid("Cannot read Codex output"))?;
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let size = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(buf.len());
        if line.len() + size > 1024 * 1024 {
            return Err(Error::invalid("Codex JSONL event exceeds 1 MiB limit"));
        }
        let done = buf[size - 1] == b'\n';
        line.extend_from_slice(&buf[..size]);
        reader.consume(size);
        if done {
            return Ok(Some(line));
        }
    }
}
pub(crate) async fn run(
    workspace: Workspace,
    session_id: String,
    run_id: String,
    prompt: String,
    mut stop: watch::Receiver<bool>,
) {
    let _guard = RunGuard {
        workspace: workspace.clone(),
        session: session_id.clone(),
        run: run_id.clone(),
    };
    let result = execute(&workspace, &session_id, &run_id, &prompt, &mut stop).await;
    // Diff collection runs outside the writer lock and async executor.
    let snapshot = workspace.snapshots.borrow().clone();
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    let review = if let (Some(path), Some(base)) = (&worker.worktree, &worker.base_commit) {
        let (path, base) = (path.clone(), base.clone());
        Some(
            tokio::task::spawn_blocking(move || changes(&path, &base))
                .await
                .map_err(Error::internal)
                .and_then(|r| r),
        )
    } else {
        None
    };
    let _ = workspace.update_run(&session_id, &run_id, |snapshot| {
        // Stop acceptance and terminal publication use the same writer lock.
        let stopped = *stop.borrow();
        let w = snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap()
            .worker
            .as_mut()
            .unwrap();
        w.status = if stopped {
            WorkerStatus::Stopped
        } else if result.is_ok() {
            WorkerStatus::Completed
        } else {
            WorkerStatus::Failed
        };
        w.error = result.err().map(|e| e.to_string());
        if let Some(review) = review {
            match review {
                Ok(c) => w.changes = Some(c),
                Err(e) => {
                    w.error = Some(e.to_string());
                    if !stopped {
                        w.status = WorkerStatus::Failed;
                    }
                }
            }
        }
        Ok(())
    });
}
struct RunGuard {
    workspace: Workspace,
    session: String,
    run: String,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        let _ = self.workspace.interrupt_run(&self.session, &self.run);
    }
}
async fn execute(
    workspace: &Workspace,
    session_id: &str,
    run_id: &str,
    prompt: &str,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    if *stop.borrow() {
        return Ok(());
    }
    let snapshot = workspace.snapshots.borrow().clone();
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .unwrap();
    let issue = snapshot
        .issues
        .iter()
        .find(|i| Some(i.id.as_str()) == session.issue_id.as_deref())
        .ok_or_else(|| {
            Error::invalid("Live worker issue is no longer on the board; sync and review scope")
        })?;
    let execution_prompt = format!(
        "You are a Relay implementation worker for the single issue below. Work in the isolated repository worktree. Do not merge, push, deploy, change remote board statuses, start other workers, or bypass sandbox permissions. Report validation only when you actually ran it. Completing this turn does not imply the issue acceptance criteria have been independently verified.\n\nIssue: {}#{}\nURL: {}\nTitle: {}\n\nIssue body:\n{}\n\nRequested turn:\n{}",
        issue.reference.repository,
        issue.reference.number,
        issue.reference.url,
        issue.title,
        issue.body,
        prompt
    );
    let (path, branch, base) = if let (Some(p), Some(b), Some(c)) =
        (&worker.worktree, &worker.branch, &worker.base_commit)
    {
        (p.clone(), b.clone(), c.clone())
    } else {
        let config = workspace.config.clone();
        let session = session_id.to_owned();
        tokio::task::spawn_blocking(move || prepare(&config, &session))
            .await
            .map_err(Error::internal)??
    };
    workspace.update_run(session_id, run_id, |s| {
        let w = s
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap()
            .worker
            .as_mut()
            .unwrap();
        w.worktree = Some(path.clone());
        w.branch = Some(branch);
        w.base_commit = Some(base);
        Ok(())
    })?;
    let mut cmd = tokio::process::Command::new(&workspace.config.codex);
    cmd.arg("exec");
    if let Some(id) = &worker.thread_id {
        cmd.args([
            "resume",
            "-c",
            "sandbox_mode=\"workspace-write\"",
            "--json",
            id,
            "-",
        ]);
    } else {
        cmd.args(["--json", "--sandbox", "workspace-write", "-"]);
    }
    cmd.current_dir(&path)
        .env_remove("RELAY_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    // Launch and stop reservation are serialized by the same store lock.
    let mut child = {
        let store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Ok(());
        }
        if store.run_id(session_id)?.as_deref() != Some(run_id) {
            return Err(Error::invalid("Run superseded"));
        }
        if !store
            .snapshot()?
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| active(&w.status))
        {
            return Err(Error::invalid("Run is no longer active"));
        }
        // Recheck the effective profile at launch, after worktree preparation.
        // The reservation itself occupies a slot, so omit it from the count.
        let request: String = store
            .connection
            .query_row(
                "SELECT request FROM receipts WHERE request_id=?1",
                [run_id],
                |r| r.get(0),
            )
            .map_err(Error::internal)?;
        let envelope: CommandEnvelope = serde_json::from_str(&request).map_err(Error::internal)?;
        let approved = match envelope.command {
            Command::StartWorker {
                approve_implementation,
                ..
            }
            | Command::SendWorker {
                approve_implementation,
                ..
            } => approve_implementation,
            _ => return Err(Error::invalid("Run receipt does not authorize a turn")),
        };
        let mut current = store.snapshot()?;
        let index = current
            .sessions
            .iter()
            .position(|s| s.id == session_id)
            .unwrap();
        let session = current.sessions.remove(index);
        authorize_turn(
            &current,
            session
                .issue_id
                .as_deref()
                .ok_or_else(|| Error::invalid("Worker issue missing"))?,
            &session.director_id,
            approved,
            &workspace.config,
        )?;
        let mut child = cmd.spawn().map_err(|_| {
            Error::invalid(
                "Cannot launch codex; install Codex and configure existing authentication",
            )
        })?;
        if let Some(pid) = child.id()
            && let Some(identity) = process_identity(pid)
            && let Err(error) = store.connection.execute("INSERT OR REPLACE INTO processes(session_id,run_id,pid,identity) VALUES(?1,?2,?3,?4)",rusqlite::params![session_id,run_id,pid,identity]) {
                reap_owned(pid, &identity);
                let _ = child.start_kill();
                return Err(Error::internal(error));
        }
        child
    };
    let _process_guard = ProcessGuard {
        owned: child
            .id()
            .and_then(|pid| process_identity(pid).map(|identity| (pid, identity))),
    };
    let publication = workspace.update_run(session_id, run_id, |s| {
        s.sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap()
            .worker
            .as_mut()
            .unwrap()
            .status = WorkerStatus::Running;
        Ok(())
    });
    if let Err(error) = publication {
        terminate(&mut child).await;
        return Err(error);
    }
    let mut stdin = child.stdin.take().unwrap();
    let write = async {
        stdin
            .write_all(execution_prompt.as_bytes())
            .await
            .map_err(|_| Error::invalid("Cannot send prompt to Codex"))?;
        drop(stdin);
        Ok::<(), Error>(())
    };
    tokio::select! { r=write=>if let Err(error) = r { terminate(&mut child).await; return Err(error); }, _=stop.changed()=>{terminate(&mut child).await;return Ok(());} }
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut completed = false;
    let result:Result<(),Error>=async {
        loop {tokio::select! {
            _=stop.changed()=>{terminate(&mut child).await;return Ok(());},
            output=line(&mut reader)=>match output? {None=>break,Some(line)=>{
                let value=serde_json::from_slice(&line).map_err(|_|Error::invalid("Invalid Codex JSONL output"))?;
                workspace.update_run(session_id,run_id,|s|{completed|=event(s,session_id,&value)?;Ok(())})?;
            }}
        }}
        let status=tokio::select! {r=child.wait()=>r.map_err(|_|Error::invalid("Cannot wait for Codex"))?,_=stop.changed()=>{terminate(&mut child).await;return Ok(());}};
        if !status.success() {return Err(Error::invalid("Codex process exited unsuccessfully; check authentication/configuration/network"));}
        if !completed {return Err(Error::invalid("Codex exited without turn.completed"));}Ok(())
    }.await;
    if result.is_err() {
        terminate(&mut child).await;
    }
    result
}
async fn terminate(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        unsafe {
            libc::kill(-(id as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}
struct ProcessGuard {
    owned: Option<(u32, String)>,
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some((pid, identity)) = &self.owned {
            reap_owned(*pid, identity);
        }
    }
}

pub(crate) fn process_identity(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        Some(format!("{}:{}", boot.trim(), fields.get(19)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}
pub(crate) fn reap_owned(pid: u32, identity: &str) {
    #[cfg(unix)]
    if process_identity(pid).as_deref() == Some(identity) {
        // The child is launched as its own process-group leader.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, identity);
    }
}
