//! Run-scoped agent tools. Capabilities live only in memory and process environments.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tokio::io::{AsyncWriteExt, BufReader};

pub(super) struct Capability {
    hash: [u8; 32],
    run: String,
}
fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}
pub(super) fn unauthorized() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        "agent_capability",
        "Agent capability unavailable or expired",
    )
}
pub(super) fn active_caller(
    store: &Store,
    snapshot: &Snapshot,
    caller: &str,
    run: &str,
) -> Result<(), Error> {
    if store.closing
        || store.run_id(caller)?.as_deref() != Some(run)
        || store.controls.get(caller).is_none_or(|c| *c.borrow())
        || !snapshot.sessions.iter().any(|s| {
            s.id == caller
                && s.worker
                    .as_ref()
                    .is_some_and(|w| runtime::active(&w.status))
        })
    {
        return Err(unauthorized());
    }
    Ok(())
}
pub(super) fn authenticate(
    workspace: &Workspace,
    headers: &HeaderMap,
) -> Result<(String, String, watch::Receiver<bool>), Error> {
    let secret = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let hash = digest(secret);
    let store = workspace.store.lock().map_err(Error::internal)?;
    let (session, capability) = store
        .agent_capabilities
        .iter()
        .find(|(_, c)| bool::from(c.hash.ct_eq(&hash)))
        .ok_or_else(unauthorized)?;
    if store.closing || store.run_id(session)?.as_deref() != Some(&capability.run) {
        return Err(unauthorized());
    }
    let control = store.controls.get(session).ok_or_else(unauthorized)?;
    if *control.borrow()
        || !store.snapshot()?.sessions.iter().any(|s| {
            s.id == *session
                && s.worker
                    .as_ref()
                    .is_some_and(|w| runtime::active(&w.status))
        })
    {
        return Err(unauthorized());
    }
    Ok((session.clone(), capability.run.clone(), control.subscribe()))
}

pub(super) struct RunCapability {
    workspace: Workspace,
    session: String,
    run: String,
}
impl Drop for RunCapability {
    fn drop(&mut self) {
        if let Ok(mut store) = self.workspace.store.lock()
            && store
                .agent_capabilities
                .get(&self.session)
                .is_some_and(|c| c.run == self.run)
        {
            store.agent_capabilities.remove(&self.session);
        }
    }
}
/// Wire a harness without putting credentials in arguments, logs or files.
pub(super) fn configure(
    workspace: &Workspace,
    session: &str,
    run: &str,
    harness: Harness,
    command: &mut tokio::process::Command,
) -> Result<Option<RunCapability>, Error> {
    let Some(endpoint) = workspace
        .agent_endpoint
        .lock()
        .map_err(Error::internal)?
        .clone()
    else {
        return Ok(None);
    };
    use rand::RngCore;
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    use base64::Engine;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);
    let executable =
        std::env::current_exe().map_err(|_| Error::invalid("Relay executable unavailable"))?;
    command
        .env("RELAY_AGENT_URL", endpoint)
        .env("RELAY_AGENT_CAPABILITY", &secret);
    match harness {
        Harness::ClaudeCode => {
            // Claude expands environment placeholders inside MCP config. No secret in argv.
            let config = json!({"mcpServers":{"relay":{"type":"stdio","command":executable,"args":["mcp"],"env":{"RELAY_AGENT_URL":"${RELAY_AGENT_URL}","RELAY_AGENT_CAPABILITY":"${RELAY_AGENT_CAPABILITY}"}}}});
            command
                .arg("--mcp-config")
                .arg(config.to_string())
                .args(["--allowedTools", "mcp__relay__*"]);
        }
        Harness::Codex => {
            for setting in [
                format!("mcp_servers.relay.command={}", json!(executable)),
                "mcp_servers.relay.args=[\"mcp\"]".into(),
                "mcp_servers.relay.env_vars=[\"RELAY_AGENT_URL\",\"RELAY_AGENT_CAPABILITY\"]"
                    .into(),
                "mcp_servers.relay.default_tools_approval_mode=\"approve\"".into(),
                "mcp_servers.relay.tool_timeout_sec=660".into(),
            ] {
                command.arg("-c").arg(setting);
            }
        }
    }
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing
        || store.run_id(session)?.as_deref() != Some(run)
        || store.controls.get(session).is_none_or(|c| *c.borrow())
    {
        return Err(unauthorized());
    }
    store.agent_capabilities.insert(
        session.into(),
        Capability {
            hash: digest(&secret),
            run: run.into(),
        },
    );
    Ok(Some(RunCapability {
        workspace: workspace.clone(),
        session: session.into(),
        run: run.into(),
    }))
}

/// Resolve ancestry iteratively, without a depth limit. Missing/cyclic links fail closed.
pub(super) fn profile(snapshot: &Snapshot, session: &Session) -> Result<AgentProfile, Error> {
    let mut profile = snapshot.session_profile(session).map_err(Error::invalid)?;
    if let Some(worker) = &session.worker {
        profile.harness = worker.harness;
    }
    if let Some(model) = &session.selection.model {
        profile.model = model.clone();
    }
    Ok(profile)
}
/// Existing scope/board/slot checks, with the session's effective permission map.
pub(super) fn authorize_session(
    snapshot: &Snapshot,
    session: &Session,
    approved: bool,
    config: &RuntimeConfig,
) -> Result<(), Error> {
    if session.archived {
        return Err(Error::invalid("Unarchive this chat before continuing"));
    }
    let effective = profile(snapshot, session)?;
    let mut candidate = snapshot.clone();
    let overrides = ProfileOverrides::between(
        &candidate
            .config_base(
                candidate
                    .agent_configs
                    .iter()
                    .find(|d| d.id == session.agent_config_id)
                    .ok_or_else(|| Error::invalid("Agent configuration missing"))?,
            )
            .map_err(Error::invalid)?,
        &effective,
    );
    candidate
        .agent_configs
        .iter_mut()
        .find(|d| d.id == session.agent_config_id)
        .ok_or_else(|| Error::invalid("Director missing"))?
        .overrides = overrides;
    runtime::authorize_issue(
        &candidate,
        session
            .issue_id
            .as_deref()
            .ok_or_else(|| Error::invalid("Linked issue missing"))?,
        &session.agent_config_id,
        approved,
        config,
    )?;
    // The session occupies a slot of its parent (or configuration, when
    // standalone); its own descendants only limit what it may spawn.
    snapshot.worker_slot(session).map_err(Error::invalid)
}
fn root<'a>(snapshot: &'a Snapshot, id: &'a str) -> &'a str {
    snapshot.ancestors(id).last().map_or(id, |s| s.id.as_str())
}
fn related(snapshot: &Snapshot, caller: &str, target: &str) -> Result<(), Error> {
    if !snapshot.sessions.iter().any(|s| s.id == target)
        || root(snapshot, caller) != root(snapshot, target)
    {
        return Err(Error::invalid("Agent is outside this tree"));
    }
    Ok(())
}
/// Messaging alone also reaches a live session a human mentioned with the
/// caller; every other tool stays within the tree.
fn can_message(snapshot: &Snapshot, caller: &str, target: &str) -> Result<(), Error> {
    if snapshot.is_peer(caller, target) {
        return Ok(());
    }
    related(snapshot, caller, target)
        .map_err(|_| Error::invalid("Agent is outside this tree and not a mentioned peer"))
}
/// What a peer may learn about another tree: no worktree, result or files.
fn peer_state(snapshot: &Snapshot, session: &Session) -> Value {
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.id == session.project_id)
        .map(|p| p.name.as_str());
    json!({"id":session.id,"title":session.title,"project":project,"status":session.worker.as_ref().map(|w| &w.status),"relationship":"peer"})
}
fn state(snapshot: &Snapshot, id: &str) -> Value {
    let session = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
    let result = snapshot
        .messages
        .iter()
        .rev()
        .find(|m| m.session_id == id && m.kind == "agent_result" && matches!(m.tool.as_ref().and_then(|t|t.detail.as_ref()),Some(ToolDetail::Result {from_session_id:Some(from),..}) if from == id));
    let result = result.and_then(|m| serde_json::from_str::<Value>(&m.body).ok());
    json!({"id":id,"title":session.title,"parent_session_id":session.parent_session_id,"status":session.worker.as_ref().map(|w| &w.status),"worktree":session.worker.as_ref().and_then(|w| w.worktree.as_ref()),"result":result})
}
fn rank(permission: Permission) -> u8 {
    match permission {
        Permission::Deny => 0,
        Permission::Ask => 1,
        Permission::Allow => 2,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spawn {
    title: String,
    task: String,
    context: Option<String>,
    harness: Option<Harness>,
    model: Option<String>,
    effort: Option<String>,
    rationale: Option<String>,
    #[serde(default)]
    permissions: BTreeMap<Task, Permission>,
    /// The child's execution mode; omitted, the child runs in the parent's.
    execution_approval: Option<ApprovalMode>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentMessage {
    agent: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wait {
    agents: Vec<String>,
    timeout_s: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultInput {
    summary: String,
    #[serde(default)]
    attachments: Vec<String>,
}
fn parse<T: serde::de::DeserializeOwned>(input: Value) -> Result<T, Error> {
    serde_json::from_value(input).map_err(|_| Error::invalid("Invalid tool arguments"))
}

/// Fail closed on missing, cyclic, or cross-project ancestry and branches.
fn archive_scope(snapshot: &Snapshot, caller: &str, agent: &str) -> Result<(), Error> {
    let parent = snapshot
        .sessions
        .iter()
        .find(|s| s.id == caller)
        .ok_or_else(|| Error::invalid("Caller missing"))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut current = Some(agent);
    let mut found_caller = false;
    while let Some(id) = current {
        if !seen.insert(id) {
            return Err(Error::invalid("Invalid agent tree"));
        }
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::invalid("Agent missing"))?;
        if session.project_id != parent.project_id {
            return Err(Error::invalid("Agents must belong to the same project"));
        }
        found_caller |= id == caller && id != agent;
        current = session.parent_session_id.as_deref();
    }
    if !found_caller
        || snapshot
            .descendants(agent)
            .iter()
            .any(|s| s.project_id != parent.project_id)
    {
        return Err(Error::invalid("Only your own descendants can be archived"));
    }
    Ok(())
}

async fn archive_agent(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    input: Value,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Archive {
        agent: String,
    }
    let args: Archive = parse(input)?;
    let delegate = {
        let snapshot = workspace.snapshots.borrow().clone();
        archive_scope(&snapshot, caller, &args.agent)?;
        let parent = snapshot.sessions.iter().find(|s| s.id == caller).unwrap();
        profile(&snapshot, parent)?.permissions[&Task::Delegate]
    };
    match delegate {
        Permission::Deny => return Err(Error::invalid("Profile denies managing subagents")),
        Permission::Ask => {
            approval(
                workspace,
                caller,
                run,
                "mcp__relay__archive_agent",
                &format!("Archive subagent {} and its descendants", args.agent),
                stop,
            )
            .await?
        }
        Permission::Allow => {}
    }
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let current = store.snapshot()?;
    active_caller(&store, &current, caller, run)?;
    archive_scope(&current, caller, &args.agent)?;
    let parent = current.sessions.iter().find(|s| s.id == caller).unwrap();
    if profile(&current, parent)?.permissions[&Task::Delegate] != delegate {
        return Err(Error::invalid("Agent policy changed; retry"));
    }
    let (snapshot, _) = store.apply(
        CommandEnvelope {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: current.revision,
            command: Command::SetSessionArchived {
                session_id: args.agent.clone(),
                archived: true,
            },
        },
        &workspace.config,
    )?;
    workspace.snapshots.send_replace(snapshot);
    Ok(json!({"agent":args.agent,"archived":true}))
}

pub(super) async fn route(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    axum::extract::Path(tool): axum::extract::Path<String>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, Error> {
    let (session, run, mut stop) = authenticate(&workspace, &headers)?;
    call(&workspace, &session, &run, &tool, input, &mut stop)
        .await
        .map(Json)
}
async fn call(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    tool: &str,
    input: Value,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    if *stop.borrow() {
        return Err(unauthorized());
    }
    match tool {
        "list_agents" => {
            if !input.is_object() || !input.as_object().unwrap().is_empty() {
                return Err(Error::invalid("list_agents takes no arguments"));
            }
            let snapshot = workspace.snapshots.borrow().clone();
            let mut agents = vec![state(&snapshot, caller)];
            agents.extend(
                snapshot
                    .descendants(caller)
                    .iter()
                    .map(|s| state(&snapshot, &s.id)),
            );
            if let Some(parent) = snapshot
                .sessions
                .iter()
                .find(|s| s.id == caller)
                .and_then(|s| s.parent_session_id.as_ref())
            {
                agents.push(state(&snapshot, parent));
            }
            for peer in snapshot.peers_of(caller) {
                if !agents.iter().any(|a| a["id"] == peer.id.as_str()) {
                    agents.push(peer_state(&snapshot, peer));
                }
            }
            let caller = snapshot
                .sessions
                .iter()
                .find(|s| s.id == caller)
                .ok_or_else(|| Error::invalid("Agent missing"))?;
            let policy = profile(&snapshot, caller)?.worker_models;
            Ok(json!({"agents":agents,"worker_models":worker_models(&snapshot, &policy)}))
        }
        // Tool specifications for this run, read by the stdio bridge.
        "tools" => {
            if !input.is_object() || !input.as_object().unwrap().is_empty() {
                return Err(Error::invalid("tools takes no arguments"));
            }
            let snapshot = workspace.snapshots.borrow().clone();
            let caller = snapshot
                .sessions
                .iter()
                .find(|s| s.id == caller)
                .ok_or_else(|| Error::invalid("Agent missing"))?;
            let approval = snapshot
                .project(&caller.project_id)
                .map_err(Error::invalid)?
                .spawn_execution_approval;
            Ok(tools(
                &profile(&snapshot, caller)?.worker_models,
                crate::nix::permission(&snapshot, caller)?,
                Some((crate::harness::mode(&snapshot, &caller.id)?, approval)),
            ))
        }
        "spawn_agent" => spawn(workspace, caller, run, parse(input)?, stop).await,
        crate::nix::TOOL => crate::nix::run(workspace, caller, run, input, stop).await,
        "stop_agent" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Stop {
                agent: String,
            }
            let args: Stop = parse(input)?;
            let snapshot = workspace.snapshots.borrow().clone();
            let parent = snapshot
                .sessions
                .iter()
                .find(|s| s.id == caller)
                .ok_or_else(|| Error::invalid("Caller missing"))?;
            let delegate = profile(&snapshot, parent)?.permissions[&Task::Delegate];
            if !snapshot
                .descendants(caller)
                .iter()
                .any(|s| s.id == args.agent)
            {
                return Err(Error::invalid("Only your own descendants can be stopped"));
            }
            match delegate {
                Permission::Deny => {
                    return Err(Error::invalid("Profile denies managing subagents"));
                }
                Permission::Ask => {
                    approval(
                        workspace,
                        caller,
                        run,
                        "mcp__relay__stop_agent",
                        &format!("Stop subagent {} and its descendants", args.agent),
                        stop,
                    )
                    .await?
                }
                Permission::Allow => {}
            }
            let mut store = workspace.store.lock().map_err(Error::internal)?;
            let mut current = store.snapshot()?;
            active_caller(&store, &current, caller, run)?;
            let parent = current.sessions.iter().find(|s| s.id == caller).unwrap();
            if profile(&current, parent)?.permissions[&Task::Delegate] != delegate
                || !current
                    .descendants(caller)
                    .iter()
                    .any(|s| s.id == args.agent)
            {
                return Err(Error::invalid("Agent policy changed; retry"));
            }
            let mut ids = vec![args.agent.clone()];
            ids.extend(
                current
                    .descendants(&args.agent)
                    .iter()
                    .map(|s| s.id.clone()),
            );
            for id in ids {
                conversation::pause(&mut current, &id, "Queue paused by an ancestor agent");
            }
            current.messages.push(Message {
                id: format!("agent-stop-{}", uuid::Uuid::new_v4()),
                session_id: caller.into(),
                author: "Relay".into(),
                sender: Sender::User,
                kind: "agent_control".into(),
                body: format!("Requested stop for {} and its descendants", args.agent),
                parts: vec![],
                tool: None,
            });
            current.revision = current
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::invalid("Revision exhausted"))?;
            store.save(&current)?;
            store.mark_ancestor_stop(&args.agent);
            store.held.remove(&args.agent);
            store.stop_controls(&current, &args.agent);
            workspace.snapshots.send_replace(current);
            Ok(json!({"agent":args.agent,"stop_requested":true}))
        }
        "archive_agent" => archive_agent(workspace, caller, run, input, stop).await,
        "message_agent" => message_agent(workspace, caller, run, parse(input)?, stop).await,
        "wait_for_agents" => {
            let args: Wait = parse(input)?;
            if args.timeout_s > 600 || args.agents.is_empty() || args.agents.len() > 64 {
                return Err(Error::invalid(
                    "Wait requires 1–64 agents and timeout_s <= 600",
                ));
            }
            let mut events = workspace.snapshots.subscribe();
            let deadline =
                tokio::time::Instant::now() + std::time::Duration::from_secs(args.timeout_s);
            loop {
                if *stop.borrow() {
                    return Err(unauthorized());
                }
                let snapshot = events.borrow().clone();
                for id in &args.agents {
                    related(&snapshot, caller, id)?;
                }
                let done = args.agents.iter().all(|id| {
                    snapshot
                        .sessions
                        .iter()
                        .find(|s| &s.id == id)
                        .and_then(|s| s.worker.as_ref())
                        .is_some_and(|w| !runtime::active(&w.status))
                });
                if done || tokio::time::Instant::now() >= deadline {
                    let result = json!({"agents":args.agents.iter().map(|id|state(&snapshot,id)).collect::<Vec<_>>(),"timed_out":!done});
                    record(
                        workspace,
                        caller,
                        run,
                        tool,
                        ToolDetail::Wait {
                            session_ids: args.agents,
                        },
                        None,
                    )?;
                    return Ok(result);
                }
                tokio::select! { _ = stop.changed() => return Err(unauthorized()), _ = tokio::time::sleep_until(deadline) => {}, change = events.changed() => { change.map_err(|_| unauthorized())?; } }
            }
        }
        "report_result" => {
            let args: ResultInput = parse(input)?;
            validate_prompt(&args.summary)?;
            if args.attachments.len() > 64 || args.attachments.iter().any(|p| p.len() > 4096) {
                return Err(Error::invalid("Too many or oversized attachments"));
            }
            workspace.update_run(caller, run, |snapshot| {
                if *stop.borrow() { return Err(unauthorized()); }
                let session = snapshot.sessions.iter().find(|s| s.id == caller).unwrap();
                let parent = session.parent_session_id.clone().ok_or_else(|| Error::invalid("Root agent has no parent"))?;
                // Paths are references, never read or uploaded by this tool.
                let worktree = session.worker.as_ref().and_then(|w| w.worktree.as_ref());
                let body = json!({"summary":args.summary,"attachments":args.attachments,"worktree":worktree}).to_string();
                let detail = ToolDetail::Result { from_session_id: Some(caller.into()), summary: Some(args.summary.clone()) };
                for target in [caller.to_owned(), parent] {
                    snapshot.messages.push(tool_message(&target, run, "report_result", detail.clone(), "agent_result", &body));
                }
                Ok(())
            })?;
            Ok(json!({"reported":true}))
        }
        _ => Err(Error::invalid("Unknown Relay tool")),
    }
}
fn tool_message(
    session: &str,
    run: &str,
    name: &str,
    detail: ToolDetail,
    kind: &str,
    body: &str,
) -> Message {
    Message {
        id: format!("agent-tool-{}", uuid::Uuid::new_v4()),
        session_id: session.into(),
        author: "Relay".into(),
        sender: Sender::User,
        kind: kind.into(),
        body: body.into(),
        parts: vec![],
        tool: Some(ToolCall {
            run_id: run.into(),
            name: format!("mcp__relay__{name}"),
            kind: ToolKind::from_name(name),
            state: ToolState::Completed,
            input: String::new(),
            target: String::new(),
            directory: None,
            exit_code: None,
            started_ms: None,
            finished_ms: Some(tool_activity::now_ms()),
            detail: Some(detail.bounded()),
        }),
    }
}
fn record(
    workspace: &Workspace,
    session: &str,
    run: &str,
    name: &str,
    detail: ToolDetail,
    message_id: Option<&str>,
) -> Result<(), Error> {
    workspace.update_run(session, run, |snapshot| {
        let mut message = tool_message(session, run, name, detail, "tool", "");
        if let Some(id) = message_id {
            message.id = id.into();
        }
        snapshot.messages.push(message);
        Ok(())
    })
}
async fn approval(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    tool: &str,
    description: &str,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    if crate::harness::permission(workspace, caller, run, tool, description, stop).await? {
        Ok(())
    } else {
        Err(Error::invalid("Human denied agent tool permission"))
    }
}
/// What a spawn was decided, and any approval asked, under. It must still
/// hold under the store lock before the child's workspaces are forked and
/// again when the child is saved.
struct SpawnDecision<'a> {
    caller: &'a str,
    run: &'a str,
    project: &'a str,
    effective: &'a AgentProfile,
    parent_mode: ApprovalMode,
    execution_policy: SpawnExecutionApproval,
}
impl SpawnDecision<'_> {
    fn holds(&self, store: &Store, current: &Snapshot) -> Result<(), Error> {
        active_caller(store, current, self.caller, self.run)?;
        let parent = current
            .sessions
            .iter()
            .find(|s| s.id == self.caller)
            .unwrap();
        let latest = profile(current, parent)?;
        if latest.permissions != self.effective.permissions {
            return Err(Error::invalid("Parent permissions changed; retry spawn"));
        }
        if latest.worker_models != self.effective.worker_models {
            return Err(Error::invalid("Worker model policy changed; retry spawn"));
        }
        // An approval covers the modes and policy it was asked under.
        if crate::harness::mode(current, self.caller)? != self.parent_mode {
            return Err(Error::invalid("Parent execution mode changed; retry spawn"));
        }
        if current
            .project(self.project)
            .map_err(Error::invalid)?
            .spawn_execution_approval
            != self.execution_policy
        {
            return Err(Error::invalid(
                "Spawn execution approval policy changed; retry spawn",
            ));
        }
        Ok(())
    }
}
async fn spawn(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    args: Spawn,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    validate_prompt(&args.task)?;
    if args.title.trim().is_empty()
        || args.title.len() > 200
        || args.context.as_ref().is_some_and(|c| c.len() > 16000)
    {
        return Err(Error::invalid("Invalid agent title or context"));
    }
    if args
        .model
        .as_ref()
        .is_some_and(|m| m.is_empty() || m.len() > 256)
        || args
            .effort
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 64)
    {
        return Err(Error::invalid("Choose a valid model before setting effort"));
    }
    let snapshot = workspace.snapshots.borrow().clone();
    let parent = snapshot
        .sessions
        .iter()
        .find(|s| s.id == caller)
        .ok_or_else(|| Error::invalid("Parent missing"))?
        .clone();
    let effective = profile(&snapshot, &parent)?;
    if effective.permissions[&Task::Delegate] == Permission::Deny {
        return Err(Error::invalid("Parent denies delegation"));
    }
    // The child needs a free slot under the parent and each of its ancestors.
    snapshot
        .child_slot(&parent.id, None)
        .map_err(Error::invalid)?;
    // The worker model policy is checked before approvals or forks: a fixed
    // policy fills omitted values and rejects differing ones, and a delegator
    // that chooses must name harness, model and effort and explain the choice.
    let policy = effective.worker_models.clone();
    let request = policy
        .resolve_request(args.harness, args.model.clone(), args.effort.clone())
        .map_err(Error::invalid)?;
    let rationale = args
        .rationale
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    // Use any discovered catalog for the requested harness; launch discovers it
    // again in the child workspace before executing the first turn.
    let harness = request.0.unwrap_or(effective.harness);
    let mut catalog = commands::cached(workspace, caller, harness);
    // A delegator that chooses an allowed model of an undiscovered harness has
    // the harness discovered first, before approvals or forks, so a model
    // without an effort setting can be chosen without one and any effort is
    // checked before the worker exists. Discovery failure rejects the spawn.
    if catalog.is_none()
        && !policy.is_fixed()
        && request.0.is_some()
        && let Some(model) = request.1.as_deref()
        && policy.allows_model(harness, model)
    {
        let discovered = tokio::select! {
            result = commands::discover_profile(workspace, &parent.project_id, harness) => result,
            _ = stop.wait_for(|stopped| *stopped) => return Err(unauthorized()),
        };
        catalog = Some(discovered.map_err(|error| {
            Error::invalid(format!(
                "Cannot discover {} models to check this worker: {}",
                harness.label(),
                error.message
            ))
        })?);
    }
    let offers_effort = request.1.as_deref().and_then(|model| {
        let model = catalog.as_ref()?.model(Some(model))?;
        Some(!model.efforts.is_empty())
    });
    policy
        .check_selection(&request, rationale, offers_effort)
        .map_err(Error::invalid)?;
    let (_, model, effort) = request;
    let Some(selected_model) = model else {
        return Err(Error::invalid("Choose a model for this worker"));
    };
    let mut selection = HarnessSelection {
        model: Some(selected_model.clone()),
        effort,
        ..Default::default()
    };
    // Without a requested effort (a fixed policy without one) the child runs
    // and shows its model's default from the start, and the policy checks
    // that resolved effort.
    let requested_effort = selection.effort.clone();
    // A model outside the policy is refused as such, not as missing from
    // another harness's catalog.
    if !policy.allows_model(harness, &selected_model) {
        policy
            .permits(harness, &selected_model, None, None)
            .map_err(Error::invalid)?;
    }
    if let Some(catalog) = &catalog {
        commands::validate_selection(catalog, &selection)?;
        selection.effort = catalog.effective_effort(&selection, None);
    }
    policy
        .permits(
            harness,
            &selected_model,
            requested_effort.as_deref(),
            selection.effort.as_deref(),
        )
        .map_err(Error::invalid)?;
    let prompt = match &args.context {
        Some(context) => format!("{}\n\nContext:\n{}", args.task, context),
        None => args.task.clone(),
    };
    validate_prompt(&prompt)?;
    let mut permissions = effective.permissions.clone();
    permissions.extend(args.permissions.clone());
    // The child runs in the parent's current execution mode unless the spawn
    // names another. The project owner's policy, not the parent, decides
    // which child modes a human must approve: a raise always, and under
    // Strict any mode above Ask, even one lower than the parent's.
    let parent_mode = crate::harness::mode(&snapshot, caller)?;
    let requested_mode = args.execution_approval;
    let child_mode = requested_mode.unwrap_or(parent_mode);
    let execution_policy = snapshot
        .project(&parent.project_id)
        .map_err(Error::invalid)?
        .spawn_execution_approval;
    // Everything this spawn needs a human for, asked once in one request.
    let mut reasons = vec![];
    match effective.permissions[&Task::Delegate] {
        Permission::Deny => return Err(Error::invalid("Parent denies delegation")),
        Permission::Ask => reasons.push("delegation requires approval".to_string()),
        Permission::Allow => {}
    }
    if execution_policy.requires_approval(parent_mode, child_mode) {
        let mut reason = if child_mode == parent_mode {
            format!("execution mode {} (same as parent)", child_mode.label())
        } else {
            format!(
                "execution mode {} → {}",
                parent_mode.label(),
                child_mode.label()
            )
        };
        if child_mode == ApprovalMode::Unrestricted {
            reason.push_str(": runs without the harness sandbox");
        }
        reasons.push(reason);
    }
    reasons.extend(
        args.permissions
            .iter()
            .filter(|(task, p)| rank(**p) > rank(effective.permissions[task]))
            .map(|(task, p)| {
                format!(
                    "{} {} (parent: {})",
                    task.label().to_lowercase(),
                    p.label().to_lowercase(),
                    effective.permissions[task].label().to_lowercase()
                )
            }),
    );
    // An Ask implementation grant needs approval for this first turn as normal launches do.
    if permissions[&Task::Implement] == Permission::Ask {
        reasons.push("implementation for its first turn".to_string());
    }
    if !reasons.is_empty() {
        approval(
            workspace,
            caller,
            run,
            "mcp__relay__spawn_agent",
            &format!("Spawn {} with:\n{}", args.title, reasons.join("\n")),
            stop,
        )
        .await?;
    }
    let decision = SpawnDecision {
        caller,
        run,
        project: &parent.project_id,
        effective: &effective,
        parent_mode,
        execution_policy,
    };
    // A stale approval never reaches the fork: recheck before preparing the
    // child's workspaces, and again below before the child is saved.
    {
        let store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(unauthorized());
        }
        let current = store.snapshot()?;
        decision.holds(&store, &current)?;
        current
            .child_slot(&parent.id, None)
            .map_err(Error::invalid)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let source = parent.clone();
    let fork_id = id.clone();
    let cancellation = stop.clone();
    let resource_workspace = workspace.clone();
    let resources = tokio::task::spawn_blocking(move || {
        let session = crate::process::with_cancellation(cancellation, || {
            commands::fork_workspaces(&source, &fork_id)
        })?;
        Ok::<_, Error>(commands::ForkResources {
            workspace: resource_workspace,
            session,
        })
    })
    .await
    .map_err(Error::internal)??;
    let mut child = resources.session.clone();
    child.automatic_name = false;
    child.awaiting_first_message = false;
    child.profile = None;
    child.network_access = None;
    child.title = args.title;
    child.role = SessionRole::Worker;
    child.parent_session_id = Some(caller.into());
    child.compactions.clear();
    child.selection = selection;
    child.spawn = Some(SpawnGrant {
        task: args.task,
        context: args.context,
        permissions: args.permissions,
        requested_by_run: Some(run.into()),
        selection_rationale: rationale.map(str::to_owned),
        execution_approval: requested_mode,
    });
    let worker = child.worker.as_mut().unwrap();
    worker.harness = harness;
    worker.launched_network_policy = None;
    worker.model = None;
    worker.context_tokens = None;
    worker.context_window = None;
    worker.status = WorkerStatus::Queued;
    // The decided mode, named or inherited, becomes the child's own execution
    // setting, which every launch and resume of either harness reads, so
    // later profile changes cannot move the child to a mode nobody approved.
    worker.execution = Some(ExecutionSettings {
        approval: child_mode,
    });
    let child_run = uuid::Uuid::new_v4().to_string();
    let parts = vec![Part::text(&prompt)];
    let (sender, receiver) = watch::channel(false);
    {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(unauthorized());
        }
        let mut current = store.snapshot()?;
        decision.holds(&store, &current)?;
        authorize_session(&current, &child, true, &workspace.config)?;
        // Enrich the harness's calling tool when it has already been observed.
        // A separate server receipt also survives clients/harnesses that omit it.
        let grant = child.spawn.as_ref().unwrap();
        if let Some(tool) = current
            .messages
            .iter_mut()
            .filter(|m| m.session_id == caller)
            .filter_map(|m| m.tool.as_mut())
            .find(|t| {
                t.run_id == run
                    && t.kind == ToolKind::Spawn
                    && t.state == ToolState::Running
                    && t.detail.is_none()
                    && serde_json::from_str::<Value>(&t.input).is_ok_and(|input| {
                        input["title"] == child.title && input["task"] == grant.task
                    })
            })
        {
            tool.detail = Some(ToolDetail::Spawn {
                child_session_id: Some(id.clone()),
            });
        }
        let from_caller = Sender::Agent {
            session_id: caller.into(),
        };
        current.sessions.push(child);
        // The persisted child runs in exactly the mode that was decided.
        if crate::harness::mode(&current, &id)? != child_mode {
            return Err(Error::invalid("Child execution mode changed; retry spawn"));
        }
        current.submissions.push(Submission {
            id: child_run.clone(),
            session_id: id.clone(),
            parts: parts.clone(),
            state: SubmissionState::Launching,
            approve_implementation: true,
            error: None,
            interrupts_run: None,
            last_edit_request: None,
            sender: from_caller.clone(),
        });
        current.messages.push(Message {
            sender: from_caller,
            ..prompt_message(&id, &child_run, &prompt)
        });
        current.messages.push(tool_message(
            caller,
            run,
            "spawn_agent",
            ToolDetail::Spawn {
                child_session_id: Some(id.clone()),
            },
            "tool",
            "",
        ));
        current.revision += 1;
        let receipt = CommandEnvelope {
            request_id: child_run.clone(),
            expected_revision: current.revision,
            command: Command::SubmitTurn {
                session_id: id.clone(),
                draft_revision: 0,
                parts,
                approve_implementation: true,
            },
        };
        let transaction = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        transaction
            .execute(
                "INSERT INTO runs(session_id,run_id) VALUES(?1,?2)",
                params![id, child_run],
            )
            .map_err(Error::internal)?;
        transaction
            .execute(
                "INSERT INTO receipts(request_id,request) VALUES(?1,?2)",
                params![
                    child_run,
                    serde_json::to_string(&receipt).map_err(Error::internal)?
                ],
            )
            .map_err(Error::internal)?;
        transaction
            .execute(
                "UPDATE workspace SET snapshot=?1 WHERE id=1",
                [serde_json::to_string(&current).map_err(Error::internal)?],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        store.controls.insert(id.clone(), sender);
        workspace.snapshots.send_replace(current);
        tokio::spawn(runtime::run(
            workspace.clone(),
            id.clone(),
            child_run,
            prompt,
            receiver,
        ));
    }
    drop(resources); // Persisted session owns these worktrees now; cleanup guard checks storage.
    Ok(json!({"agent":id}))
}
async fn message_agent(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    args: AgentMessage,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    validate_prompt(&args.text)?;
    let snapshot = workspace.snapshots.borrow().clone();
    can_message(&snapshot, caller, &args.agent)?;
    let target = snapshot
        .sessions
        .iter()
        .find(|s| s.id == args.agent)
        .unwrap();
    let permissions = profile(&snapshot, target)?.permissions;
    match permissions[&Task::Implement] {
        Permission::Deny => return Err(Error::invalid("Target denies implementation")),
        Permission::Ask => {
            approval(
                workspace,
                caller,
                run,
                "mcp__relay__message_agent",
                &format!(
                    "Approve implementation for the message turn to {}",
                    args.agent
                ),
                stop,
            )
            .await?
        }
        Permission::Allow => {}
    }
    let id = uuid::Uuid::new_v4().to_string();
    let tool_id = format!("agent-message-{id}");
    // Submit through the same queue machinery, preserving the user's shared draft.
    let (action, next, next_run) = {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(unauthorized());
        }
        let mut current = store.snapshot()?;
        active_caller(&store, &current, caller, run)?;
        // A peer grant may have been pruned while approval was pending.
        can_message(&current, caller, &args.agent)?;
        let target = current
            .sessions
            .iter()
            .find(|s| s.id == args.agent)
            .unwrap();
        if profile(&current, target)?.permissions != permissions {
            return Err(Error::invalid("Target permissions changed; retry message"));
        }
        let transaction = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let saved = conversation::read_drafts(&transaction)?
            .into_iter()
            .find(|d| d.session_id == args.agent);
        let revision = saved.as_ref().map_or(0, |d| d.revision);
        let parts = vec![Part::text(&args.text)];
        // The normal queue validates a saved draft. Stage this tool's turn only
        // within the transaction, then restore the human's draft unchanged.
        let temporary_draft = Draft {
            session_id: args.agent.clone(),
            revision,
            parts: parts.clone(),
        };
        transaction
            .execute(
                "INSERT OR REPLACE INTO drafts(session_id,draft) VALUES(?1,?2)",
                params![
                    args.agent,
                    serde_json::to_string(&temporary_draft).map_err(Error::internal)?
                ],
            )
            .map_err(Error::internal)?;
        let command = Command::SubmitTurn {
            session_id: args.agent.clone(),
            draft_revision: revision,
            parts,
            approve_implementation: true,
        };
        let action = conversation::apply(
            &transaction,
            &mut current,
            &id,
            command.clone(),
            &workspace.config,
            &HashMap::new(),
            Sender::Agent {
                session_id: caller.into(),
            },
        )?;
        if let Some(saved) = saved {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO drafts(session_id,draft) VALUES(?1,?2)",
                    params![
                        args.agent,
                        serde_json::to_string(&saved).map_err(Error::internal)?
                    ],
                )
                .map_err(Error::internal)?;
        } else {
            transaction
                .execute("DELETE FROM drafts WHERE session_id=?1", [&args.agent])
                .map_err(Error::internal)?;
        }
        current.messages.push({
            let mut m = tool_message(
                caller,
                run,
                "message_agent",
                ToolDetail::Message {
                    target_session_id: Some(args.agent.clone()),
                    read: false,
                },
                "tool",
                "",
            );
            m.id = tool_id;
            m
        });
        current.revision += 1;
        let envelope = CommandEnvelope {
            request_id: id.clone(),
            expected_revision: current.revision,
            command,
        };
        transaction
            .execute(
                "INSERT INTO receipts(request_id,request) VALUES(?1,?2)",
                params![
                    id,
                    serde_json::to_string(&envelope).map_err(Error::internal)?
                ],
            )
            .map_err(Error::internal)?;
        if action.is_some() {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                    params![args.agent, id],
                )
                .map_err(Error::internal)?;
        }
        transaction
            .execute(
                "UPDATE workspace SET snapshot=?1 WHERE id=1",
                [serde_json::to_string(&current).map_err(Error::internal)?],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        let receiver = if action.is_some() {
            let (sender, receiver) = watch::channel(false);
            store.controls.insert(args.agent.clone(), sender);
            Some(receiver)
        } else {
            None
        };
        workspace.snapshots.send_replace(current);
        (action, receiver, id.clone())
    };
    if let (Some(Action::Run { session_id, prompt }), Some(receiver)) = (action, next) {
        tokio::spawn(runtime::run(
            workspace.clone(),
            session_id,
            next_run,
            prompt,
            receiver,
        ));
    }
    Ok(json!({"submission":id,"queued":true}))
}

pub(super) fn mark_read(snapshot: &mut Snapshot, run: &str) {
    if let Some(message) = snapshot
        .messages
        .iter_mut()
        .find(|m| m.id == format!("agent-message-{run}"))
        && let Some(tool) = &mut message.tool
        && let Some(ToolDetail::Message { read, .. }) = &mut tool.detail
    {
        *read = true;
    }
}

/// The spawn_agent description, properties and required properties allowed by
/// a worker model policy. `execution` is the caller's execution mode and its
/// project's spawn execution approval policy, when known.
fn spawn_spec(
    policy: &WorkerModelPolicy,
    execution: Option<(ApprovalMode, SpawnExecutionApproval)>,
) -> (String, Value, Vec<&'static str>) {
    let string = json!({"type":"string"});
    let permissions = json!({"type":"object","propertyNames":{"enum":["plan","delegate","implement","verify","review","merge","deploy"]},"additionalProperties":{"enum":["allow","ask","deny"]}});
    let harnesses = policy.harnesses();
    let mut models: Vec<&str> = vec![];
    for model in harnesses.iter().flat_map(|h| policy.models_for(*h)) {
        if !models.contains(&model) {
            models.push(model);
        }
    }
    let mut properties = json!({
        "title": string,
        "task": string,
        "context": string,
        "harness": {"enum": harnesses},
        "model": if models.is_empty() { string.clone() } else { json!({"type":"string","enum":models}) },
        "permissions": permissions,
        "execution_approval": {"type":"string","enum":["ask","automatic","unrestricted"]},
    });
    match policy.efforts() {
        None => properties["effort"] = string,
        Some(efforts) if efforts.is_empty() => {}
        Some(efforts) => properties["effort"] = json!({"type":"string","enum":efforts}),
    }
    let mut required = vec!["title", "task"];
    let rule = if policy.is_fixed() {
        "Worker model is fixed: omit harness, model and effort; differing values are rejected."
            .to_string()
    } else {
        properties["rationale"] = json!({"type":"string","maxLength":MAX_RATIONALE_LEN});
        required.extend(["harness", "model", "rationale"]);
        format!(
            "You choose harness, model and effort per worker and must pass all three; omit effort only for a model that offers no effort setting. Give a concise rationale covering {}. Relay rejects omitted values and choices outside the policy.",
            policy.rationale_hint()
        )
    };
    let execution = match execution {
        Some((mode, approval)) => format!(
            "execution_approval sets the child's execution mode (ask < automatic < unrestricted); omit it to run the child in yours ({}). {}",
            mode.label(),
            approval.describe()
        ),
        None => "execution_approval sets the child's execution mode (ask < automatic < unrestricted); omit it to run the child in yours. The project decides which modes need human approval.".to_string(),
    };
    (
        format!(
            "Start a child Relay session. Relay enforces delegation and human approval for wider permissions. {rule} {} {execution}",
            policy.allowed_summary()
        ),
        properties,
        required,
    )
}

/// The worker model policy, and remaining usage when the policy weighs it.
fn worker_models(snapshot: &Snapshot, policy: &WorkerModelPolicy) -> Value {
    let mut value = json!({"policy": policy, "summary": policy.allowed_summary()});
    if policy.considers_usage() {
        let now = now();
        value["usage"] = json!(
            snapshot
                .usage_remaining(&policy.harnesses(), now)
                .iter()
                .map(|u| u.describe(now))
                .collect::<Vec<_>>()
        );
    }
    value
}

/// This run's tools. run_nix is listed only when the agent may run Nix commands.
fn tools(
    policy: &WorkerModelPolicy,
    nix: Permission,
    execution: Option<(ApprovalMode, SpawnExecutionApproval)>,
) -> Value {
    let string = json!({"type":"string"});
    let (spawn_description, spawn_properties, spawn_required) = spawn_spec(policy, execution);
    let specs = [
        (
            "spawn_agent",
            spawn_description.as_str(),
            spawn_properties,
            spawn_required,
        ),
        (
            "stop_agent",
            "Stop one of your descendants and its subtree. Requires delegation permission; retains history and workspaces.",
            json!({"agent":string}),
            vec!["agent"],
        ),
        (
            "archive_agent",
            "Archive an inactive descendant and its subtree. Requires delegation permission; retains history and workspaces and pauses queued turns. Directors archive workers whose results they have reviewed and accepted.",
            json!({"agent":string}),
            vec!["agent"],
        ),
        (
            "message_agent",
            "Queue a turn for an agent in this tree, or for a session a human mentioned in a message with you (a peer from list_agents, possibly in another project). Messaging a peer gives no other access to it.",
            json!({"agent":string,"text":string}),
            vec!["agent", "text"],
        ),
        (
            "wait_for_agents",
            "Block until agents are terminal or the timeout expires. Directors should not use this; report status and end the turn instead.",
            json!({"agents":{"type":"array","items":string,"minItems":1,"maxItems":64},"timeout_s":{"type":"integer","minimum":0,"maximum":600}}),
            vec!["agents", "timeout_s"],
        ),
        (
            "report_result",
            "Report a result to your parent, with worktree and optional attachment paths. Does not merge.",
            json!({"summary":string,"attachments":{"type":"array","items":string}}),
            vec!["summary"],
        ),
        (
            "list_agents",
            "List your subtree and parent with current statuses, sessions a human mentioned with you (relationship \"peer\": ID, title, project and status only; message them with message_agent), and your worker model policy with remaining usage when it applies.",
            json!({}),
            vec![],
        ),
    ];
    let mut tools = specs.into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect::<Vec<_>>();
    if nix != Permission::Deny {
        tools.push(crate::nix::spec(nix));
    }
    json!({ "tools": tools })
}
/// Newline-delimited JSON-RPC over stdio. stdout contains protocol messages only.
pub async fn stdio() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("RELAY_AGENT_URL").map_err(|_| "Missing Relay agent endpoint")?;
    let secret =
        std::env::var("RELAY_AGENT_CAPABILITY").map_err(|_| "Missing Relay run capability")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut writer = tokio::io::stdout();
    while let Some(bytes) = runtime::line(&mut reader).await? {
        let request: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                writer.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32700,\"message\":\"Invalid JSON\"}}\n").await?;
                continue;
            }
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let result = if request["jsonrpc"] != "2.0"
            || !request["method"].is_string()
            || !(id.is_string() || id.is_number() || id.is_null())
        {
            Err((-32600, "Invalid JSON-RPC request"))
        } else {
            match request["method"].as_str() {
                Some("initialize") => Ok(
                    json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"relay","version":env!("CARGO_PKG_VERSION")}}),
                ),
                Some("ping") => Ok(json!({})),
                // The server describes spawn_agent with this run's worker model
                // policy; the generic list remains if it cannot be reached.
                Some("tools/list") => Ok(
                    match client
                        .post(format!("{endpoint}/v1/agent-tools/tools"))
                        .bearer_auth(&secret)
                        .json(&json!({}))
                        .send()
                        .await
                    {
                        Ok(response) if response.status().is_success() => response
                            .json::<Value>()
                            .await
                            .ok()
                            .filter(|v| v["tools"].is_array())
                            .unwrap_or_else(|| {
                                tools(&WorkerModelPolicy::default(), Permission::Deny, None)
                            }),
                        _ => tools(&WorkerModelPolicy::default(), Permission::Deny, None),
                    },
                ),
                Some("tools/call") => {
                    let name = request["params"]["name"].as_str().unwrap_or("");
                    if ![
                        "spawn_agent",
                        "stop_agent",
                        "archive_agent",
                        "message_agent",
                        "wait_for_agents",
                        "report_result",
                        "list_agents",
                        crate::nix::TOOL,
                    ]
                    .contains(&name)
                    {
                        Err((-32602, "Unknown Relay tool"))
                    } else {
                        let response = client
                            .post(format!("{endpoint}/v1/agent-tools/{name}"))
                            .bearer_auth(&secret)
                            .json(
                                &request["params"]
                                    .get("arguments")
                                    .cloned()
                                    .unwrap_or(json!({})),
                            )
                            .send()
                            .await;
                        let (value, error) = match response {
                            Ok(response) => {
                                let error = !response.status().is_success();
                                (
                                    response
                                        .json::<Value>()
                                        .await
                                        .unwrap_or(json!({"message":"Relay response unavailable"})),
                                    error,
                                )
                            }
                            Err(_) => (json!({"message":"Relay server unavailable"}), true),
                        };
                        Ok(
                            json!({"content":[{"type":"text","text":value.to_string()}],"isError":error}),
                        )
                    }
                }
                _ => Err((-32601, "Method not found")),
            }
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err((code, message)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
            }
        };
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        writer.write_all(&bytes).await?;
        writer.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    struct Fixture {
        _directory: tempfile::TempDir,
        workspace: Workspace,
        caller: String,
        run: String,
        stop: watch::Receiver<bool>,
    }
    impl Fixture {
        fn new(delegate: Permission) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let repo = crate::tests::review_repo(directory.path());
            let binary = directory.path().join("codex");
            crate::tests::script(&binary, "sleep 30");
            let mut snapshot = crate::tests::live();
            snapshot.sessions.clear();
            snapshot.messages.clear();
            snapshot.projects[0].defaults.max_workers = 64;
            snapshot.projects[0].defaults.model = "fixture-model".into();
            // Tests unrelated to model choice spawn without a selection;
            // worker model policy tests set the policy they exercise.
            snapshot.projects[0].defaults.worker_models = WorkerModelPolicy::Fixed {
                harness: Harness::Codex,
                model: "fixture-model".into(),
                effort: None,
            };
            snapshot.projects[0]
                .defaults
                .permissions
                .insert(Task::Implement, Permission::Allow);
            snapshot.projects[0]
                .defaults
                .permissions
                .insert(Task::Delegate, delegate);
            snapshot.agent_configs[0].overrides = Default::default();
            let config = RuntimeConfig {
                repository: Some(repo.clone()),
                codex: binary,
                nix: directory.path().join("nix"),
                ..crate::tests::config()
            };
            let workspace =
                crate::tests::workspace(&directory.path().join("db"), &snapshot, config);
            let envelope = crate::tests::env(snapshot.revision, crate::tests::start(&snapshot));
            let run = envelope.request_id.clone();
            let (mut snapshot, _) = workspace
                .store
                .lock()
                .unwrap()
                .apply(envelope, &workspace.config)
                .unwrap();
            let parent = snapshot.sessions.last_mut().unwrap();
            let caller = parent.id.clone();
            parent.workspaces = vec![SessionWorkspace {
                connection_id: String::new(),
                path: repo.display().to_string(),
                repository: true,
                branch: None,
                base_commit: Some("HEAD".into()),
                changes: None,
            }];
            let worker = parent.worker.as_mut().unwrap();
            worker.status = WorkerStatus::Running;
            worker.worktree = Some(repo.display().to_string());
            worker.thread_id = Some("parent-thread".into());
            let (sender, stop) = watch::channel(false);
            {
                let mut store = workspace.store.lock().unwrap();
                store.save(&snapshot).unwrap();
                store.controls.insert(caller.clone(), sender);
            }
            workspace.snapshots.send_replace(snapshot);
            Self {
                _directory: directory,
                workspace,
                caller,
                run,
                stop,
            }
        }
        async fn call(&self, name: &str, args: Value) -> Result<Value, Error> {
            call(
                &self.workspace,
                &self.caller,
                &self.run,
                name,
                args,
                &mut self.stop.clone(),
            )
            .await
        }
        fn change(&self, update: impl FnOnce(&mut Snapshot)) {
            self.workspace
                .update_run(&self.caller, &self.run, |s| {
                    update(s);
                    Ok(())
                })
                .unwrap();
        }
        async fn finish(&self) {
            self.change(|s| {
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == self.caller)
                    .unwrap()
                    .worker
                    .as_mut()
                    .unwrap()
                    .status = WorkerStatus::Completed
            });
            let shutdown = Shutdown {
                workspace: self.workspace.clone(),
            };
            shutdown.shutdown().await.unwrap();
        }
    }
    async fn decide(workspace: &Workspace, caller: &str, run: &str, allow: bool, contains: &str) {
        let mut events = workspace.snapshots.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let pending = events
                    .borrow()
                    .tool_permissions
                    .iter()
                    .find(|p| p.decision.is_none() && !p.expired)
                    .cloned();
                if let Some(pending) = pending {
                    assert!(pending.description.contains(contains));
                    let mut store = workspace.store.lock().unwrap();
                    let snapshot = store.snapshot().unwrap();
                    let envelope = crate::tests::env(
                        snapshot.revision,
                        Command::RespondPermission {
                            permission_id: pending.id,
                            run_id: pending.run_id,
                            allow,
                        },
                    );
                    let (snapshot, _) = store.apply(envelope, &workspace.config).unwrap();
                    workspace.snapshots.send_replace(snapshot);
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(
            workspace
                .store
                .lock()
                .unwrap()
                .run_id(caller)
                .unwrap()
                .as_deref()
                == Some(run)
        );
    }
    #[tokio::test]
    async fn scoped_root_profiles_start_on_the_selected_task_without_creating_another() {
        let f = Fixture::new(Permission::Allow);
        let snapshot = f.workspace.snapshots.borrow().clone();
        let parent = snapshot.sessions[0].clone();
        let task = parent.issue_id.clone().unwrap();
        let before = snapshot.issues.len();
        f.change(|s| {
            s.agent_configs
                .iter_mut()
                .find(|c| c.id == parent.agent_config_id)
                .unwrap()
                .overrides
                .scope = Some(AgentScope::Issues {
                issue_ids: vec![task.clone()],
            });
        });
        let snapshot = f.workspace.snapshots.borrow().clone();
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StartRootAgent {
                agent_config_id: parent.agent_config_id,
                issue_id: Some(task.clone()),
                prompt: "Review the scoped task".into(),
                parts: vec![],
                approve_implementation: false,
            },
        );
        let (snapshot, action) = f
            .workspace
            .store
            .lock()
            .unwrap()
            .apply(envelope, &f.workspace.config)
            .unwrap();
        assert!(matches!(action, Some(Action::Run { .. })));
        assert_eq!(
            snapshot.sessions.last().unwrap().issue_id.as_deref(),
            Some(task.as_str())
        );
        assert_eq!(
            snapshot.sessions.last().unwrap().title,
            snapshot.agent_configs[0].name
        );
        assert_eq!(snapshot.issues.len(), before);
        f.workspace.snapshots.send_replace(snapshot);
        f.finish().await;
    }

    #[tokio::test]
    async fn human_child_creation_forks_a_completed_parent_and_replays_once() {
        let f = Fixture::new(Permission::Deny);
        let path = f.workspace.snapshots.borrow().sessions[0].workspaces[0]
            .path
            .clone();
        std::fs::write(Path::new(&path).join("manual-untracked"), "parent changes").unwrap();
        f.change(|s| {
            s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
        });
        let snapshot = f.workspace.snapshots.borrow().clone();
        let profile =
            AgentProfile::worker(&snapshot.session_profile(&snapshot.sessions[0]).unwrap());
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StartChildAgent {
                parent_session_id: f.caller.clone(),
                name: "Manual worker".into(),
                profile: profile.clone(),
                prompt: "Verify parent changes".into(),
                approve_implementation: false,
            },
        );
        let id = format!("session-{}", envelope.request_id);
        let headers = || {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", "Bearer test-token".parse().unwrap());
            headers
        };
        let _ = command(
            State(f.workspace.clone()),
            headers(),
            Ok(Json(envelope.clone())),
        )
        .await
        .unwrap();
        let current = f.workspace.snapshots.borrow().clone();
        let child = current.sessions.iter().find(|s| s.id == id).unwrap();
        assert_eq!(child.parent_session_id.as_deref(), Some(f.caller.as_str()));
        assert_eq!(child.issue_id, snapshot.sessions[0].issue_id);
        assert_eq!(current.session_profile(child).unwrap(), profile);
        assert_eq!(child.spawn.as_ref().unwrap().requested_by_run, None);
        assert_ne!(child.workspaces[0].path, path);
        assert_eq!(
            std::fs::read_to_string(Path::new(&child.workspaces[0].path).join("manual-untracked"))
                .unwrap(),
            "parent changes"
        );
        let _ = command(State(f.workspace.clone()), headers(), Ok(Json(envelope)))
            .await
            .unwrap();
        assert_eq!(
            f.workspace
                .snapshots
                .borrow()
                .sessions
                .iter()
                .filter(|s| s.id == id)
                .count(),
            1
        );
        Shutdown {
            workspace: f.workspace.clone(),
        }
        .shutdown()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn empty_child_creation_waits_for_chat_submission_and_preserves_custom_names() {
        for name in ["", "Static name"] {
            let f = Fixture::new(Permission::Allow);
            f.change(|s| s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed);
            let snapshot = f.workspace.snapshots.borrow().clone();
            let mut profile =
                AgentProfile::worker(&snapshot.session_profile(&snapshot.sessions[0]).unwrap());
            profile.permissions.insert(Task::Implement, Permission::Ask);
            let envelope = crate::tests::env(
                snapshot.revision,
                Command::StartChildAgent {
                    parent_session_id: f.caller.clone(),
                    name: name.into(),
                    profile,
                    prompt: String::new(),
                    approve_implementation: false,
                },
            );
            let id = format!("session-{}", envelope.request_id);
            let mut headers = HeaderMap::new();
            headers.insert("authorization", "Bearer test-token".parse().unwrap());
            let _ = command(
                State(f.workspace.clone()),
                headers,
                Ok(Json(envelope.clone())),
            )
            .await
            .unwrap();
            let before = f.workspace.snapshots.borrow().clone();
            let child = before.sessions.iter().find(|s| s.id == id).unwrap();
            assert!(child.awaiting_first_message);
            assert_eq!(child.automatic_name, name.is_empty());
            assert_eq!(
                child.title,
                if name.is_empty() { "New agent" } else { name }
            );
            assert_eq!(child.parent_session_id.as_deref(), Some(f.caller.as_str()));
            assert_eq!(
                child.worker.as_ref().unwrap().status,
                WorkerStatus::Completed
            );
            assert!(!before.messages.iter().any(|m| m.session_id == id));
            assert!(!before.submissions.iter().any(|s| s.session_id == id));
            assert!(
                f.workspace
                    .store
                    .lock()
                    .unwrap()
                    .run_id(&id)
                    .unwrap()
                    .is_none()
            );
            let parts = vec![relay_core::Part::text("Review the sidebar borders")];
            let Json(draft) = crate::conversation::save_draft(
                State(f.workspace.clone()),
                axum::extract::Path(id.clone()),
                Json(relay_core::SaveDraft {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 0,
                    parts: parts.clone(),
                }),
            )
            .await
            .unwrap();
            let rejected = f.workspace.store.lock().unwrap().apply(
                crate::tests::env(
                    before.revision,
                    Command::SubmitTurn {
                        session_id: id.clone(),
                        draft_revision: draft.revision,
                        parts: parts.clone(),
                        approve_implementation: false,
                    },
                ),
                &f.workspace.config,
            );
            assert!(rejected.is_err());
            assert!(
                f.workspace
                    .store
                    .lock()
                    .unwrap()
                    .snapshot()
                    .unwrap()
                    .sessions
                    .iter()
                    .find(|s| s.id == id)
                    .unwrap()
                    .awaiting_first_message
            );
            let (after, action) = f
                .workspace
                .store
                .lock()
                .unwrap()
                .apply(
                    crate::tests::env(
                        before.revision,
                        Command::SubmitTurn {
                            session_id: id.clone(),
                            draft_revision: draft.revision,
                            parts,
                            approve_implementation: true,
                        },
                    ),
                    &f.workspace.config,
                )
                .unwrap();
            assert!(matches!(action, Some(Action::Run { session_id, .. }) if session_id == id));
            let child = after.sessions.iter().find(|s| s.id == id).unwrap();
            assert!(!child.awaiting_first_message);
            assert_eq!(
                child.title,
                if name.is_empty() {
                    "Review the sidebar borders"
                } else {
                    name
                }
            );
            assert_eq!(
                child.spawn.as_ref().unwrap().task,
                "Review the sidebar borders"
            );
            assert_eq!(child.worker.as_ref().unwrap().status, WorkerStatus::Queued);
            let persisted = f.workspace.store.lock().unwrap().snapshot().unwrap();
            assert_eq!(persisted.sessions.iter().find(|s| s.id == id), Some(child));
            Shutdown {
                workspace: f.workspace.clone(),
            }
            .shutdown()
            .await
            .unwrap();
        }
    }

    fn archive_fixture(f: &Fixture) {
        f.change(|s| {
            let mut child = s.sessions[0].clone();
            child.id = "archive-child".into();
            child.parent_session_id = Some(f.caller.clone());
            child.worker = None;
            s.sessions.push(child.clone());
            child.id = "archive-grandchild".into();
            child.parent_session_id = Some("archive-child".into());
            s.sessions.push(child);
        });
    }

    #[test]
    fn archive_tool_schema_is_discoverable() {
        let tools = tools(&WorkerModelPolicy::default(), Permission::Deny, None);
        let tool = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "archive_agent")
            .unwrap();
        assert_eq!(tool["inputSchema"]["required"], json!(["agent"]));
        assert_eq!(
            tool["inputSchema"]["properties"],
            json!({"agent":{"type":"string"}})
        );
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }

    #[tokio::test]
    async fn archive_preserves_history_workspaces_and_is_idempotent_for_finished_states() {
        let f = Fixture::new(Permission::Allow);
        archive_fixture(&f);
        f.change(|s| {
            s.submissions.push(Submission {
                id: "archive-queued-turn".into(),
                session_id: "archive-grandchild".into(),
                parts: vec![Part::text("Continue")],
                state: SubmissionState::Queued,
                approve_implementation: false,
                error: None,
                interrupts_run: None,
                last_edit_request: None,
                sender: Sender::User,
            });
        });
        let before = f.workspace.snapshots.borrow().clone();
        for status in [
            None,
            Some(WorkerStatus::Completed),
            Some(WorkerStatus::Stopped),
            Some(WorkerStatus::Failed),
            Some(WorkerStatus::Interrupted),
        ] {
            f.change(|s| {
                let mut worker = s.sessions[0].worker.clone().unwrap();
                if let Some(status) = &status {
                    worker.status = status.clone();
                }
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == "archive-child")
                    .unwrap()
                    .worker = status.map(|_| worker);
            });
            for _ in 0..2 {
                assert_eq!(
                    f.call("archive_agent", json!({"agent":"archive-child"}))
                        .await
                        .unwrap(),
                    json!({"agent":"archive-child","archived":true})
                );
            }
        }
        let after = f.workspace.store.lock().unwrap().snapshot().unwrap();
        assert!(!after.sessions[0].archived);
        assert!(
            after
                .sessions
                .iter()
                .filter(|s| s.id.starts_with("archive-"))
                .all(|s| s.archived)
        );
        assert_eq!(before.messages, after.messages);
        assert_eq!(before.sessions[1].workspaces, after.sessions[1].workspaces);
        assert_eq!(before.sessions.len(), after.sessions.len());
        let turn = after
            .submissions
            .iter()
            .find(|s| s.id == "archive-queued-turn")
            .unwrap();
        assert_eq!(turn.state, SubmissionState::Paused);
        assert_eq!(
            turn.error.as_deref(),
            Some("Chat archived; unarchive and resume to continue")
        );
        f.finish().await;
    }

    #[tokio::test]
    async fn archive_rejects_active_subtrees_and_invalid_arguments() {
        let f = Fixture::new(Permission::Allow);
        archive_fixture(&f);
        for target in ["archive-child", "archive-grandchild"] {
            for status in [WorkerStatus::Queued, WorkerStatus::Running] {
                f.change(|s| {
                    let mut worker = s.sessions[0].worker.clone().unwrap();
                    worker.status = status;
                    s.sessions
                        .iter_mut()
                        .find(|s| s.id == target)
                        .unwrap()
                        .worker = Some(worker);
                });
                assert!(
                    f.call("archive_agent", json!({"agent":"archive-child"}))
                        .await
                        .is_err()
                );
                assert!(!f.workspace.snapshots.borrow().sessions[1].archived);
            }
            f.change(|s| {
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == target)
                    .unwrap()
                    .worker = None
            });
        }
        for args in [
            json!({}),
            json!({"agent":42}),
            json!({"agent":"archive-child","extra":true}),
        ] {
            assert!(f.call("archive_agent", args).await.is_err());
        }
        f.finish().await;
    }

    #[tokio::test]
    async fn archive_enforces_scope_and_delegation() {
        let f = Fixture::new(Permission::Deny);
        archive_fixture(&f);
        assert!(
            f.call("archive_agent", json!({"agent":"archive-child"}))
                .await
                .is_err()
        );
        f.change(|s| {
            s.projects[0]
                .defaults
                .permissions
                .insert(Task::Delegate, Permission::Allow);
        });
        for target in [f.caller.as_str(), "missing"] {
            assert!(
                f.call("archive_agent", json!({"agent":target}))
                    .await
                    .is_err()
            );
        }
        let original = f.workspace.snapshots.borrow().clone();
        for (id, parent, project) in [
            (
                "archive-child",
                None,
                original.sessions[0].project_id.as_str(),
            ),
            ("archive-child", Some(f.caller.as_str()), "other-project"),
            ("archive-grandchild", Some("archive-child"), "other-project"),
            (
                "archive-child",
                Some("archive-grandchild"),
                original.sessions[0].project_id.as_str(),
            ),
        ] {
            f.change(|s| {
                s.sessions = original.sessions.clone();
                let session = s.sessions.iter_mut().find(|s| s.id == id).unwrap();
                session.parent_session_id = parent.map(str::to_owned);
                session.project_id = project.into();
            });
            assert!(
                f.call("archive_agent", json!({"agent":"archive-child"}))
                    .await
                    .is_err()
            );
        }
        f.change(|s| {
            s.sessions = original.sessions.clone();
            s.sessions[0].parent_session_id = Some("archive-child".into());
            s.sessions[1].parent_session_id = None;
        });
        assert!(
            f.call("archive_agent", json!({"agent":"archive-child"}))
                .await
                .is_err()
        );
        f.finish().await;
    }

    #[tokio::test]
    async fn archive_ask_requires_approval_and_rechecks_policy_and_subtree() {
        let f = Fixture::new(Permission::Ask);
        archive_fixture(&f);
        let (result, ()) = tokio::join!(
            f.call("archive_agent", json!({"agent":"archive-child"})),
            decide(&f.workspace, &f.caller, &f.run, false, "Archive subagent")
        );
        assert!(result.is_err());
        for change in ["policy", "scope", "active"] {
            let before = f.workspace.snapshots.borrow().clone();
            let (result, ()) = tokio::join!(
                f.call("archive_agent", json!({"agent":"archive-child"})),
                async {
                    decide(&f.workspace, &f.caller, &f.run, true, "Archive subagent").await;
                    f.change(|s| match change {
                        "policy" => {
                            s.projects[0]
                                .defaults
                                .permissions
                                .insert(Task::Delegate, Permission::Deny);
                        }
                        "scope" => s.sessions[1].parent_session_id = None,
                        "active" => s.sessions[2].worker = s.sessions[0].worker.clone(),
                        _ => unreachable!(),
                    });
                }
            );
            assert!(result.is_err(), "{change}");
            assert!(!f.workspace.snapshots.borrow().sessions[1].archived);
            f.change(|s| {
                s.sessions = before.sessions;
                s.projects = before.projects;
            });
        }
        let (result, ()) = tokio::join!(
            f.call("archive_agent", json!({"agent":"archive-child"})),
            decide(&f.workspace, &f.caller, &f.run, true, "Archive subagent")
        );
        assert!(result.is_ok());
        assert!(f.workspace.snapshots.borrow().sessions[1].archived);
        f.finish().await;
    }

    #[tokio::test]
    async fn stopping_descendants_requires_delegation_and_cannot_stop_the_parent() {
        let f = Fixture::new(Permission::Allow);
        let child = f
            .call("spawn_agent", json!({"title":"Child", "task":"Verify"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            f.call("stop_agent", json!({"agent":f.caller}))
                .await
                .is_err()
        );
        f.change(|s| {
            s.projects[0]
                .defaults
                .permissions
                .insert(Task::Delegate, Permission::Deny);
        });
        assert!(f.call("stop_agent", json!({"agent":child})).await.is_err());
        f.change(|s| {
            s.projects[0]
                .defaults
                .permissions
                .insert(Task::Delegate, Permission::Allow);
        });
        assert_eq!(
            f.call("stop_agent", json!({"agent":child})).await.unwrap()["stop_requested"],
            true
        );
        assert!(
            f.workspace
                .store
                .lock()
                .unwrap()
                .controls
                .get(&child)
                .is_none_or(|c| *c.borrow())
        );
        f.finish().await;
    }

    #[tokio::test]
    async fn spawn_forks_dirty_state_and_records_provenance() {
        let f = Fixture::new(Permission::Allow);
        let shared = f._directory.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        f.change(|s| {
            s.sessions[0].workspaces.push(SessionWorkspace {
                connection_id: "shared".into(),
                path: shared.display().to_string(),
                repository: false,
                branch: None,
                base_commit: None,
                changes: None,
            });
            tool_activity::start(
                s,
                &f.caller,
                "native-spawn".into(),
                "Harness",
                tool_activity::call(
                    &f.run,
                    "mcp__relay__spawn_agent",
                    ToolKind::Spawn,
                    json!({"title":"Child","task":"Test","context":"Evidence"}).to_string(),
                    String::new(),
                ),
            );
        });
        let parent_path = f.workspace.snapshots.borrow().sessions[0].workspaces[0]
            .path
            .clone();
        std::fs::write(Path::new(&parent_path).join("staged"), "one").unwrap();
        crate::tests::git(Path::new(&parent_path), &["add", "staged"]);
        std::fs::write(Path::new(&parent_path).join("staged"), "two").unwrap();
        std::fs::write(Path::new(&parent_path).join("untracked"), "three").unwrap();
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Child","task":"Test","context":"Evidence"}),
            )
            .await
            .unwrap();
        let id = value["agent"].as_str().unwrap();
        let snapshot = f.workspace.snapshots.borrow().clone();
        let child = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
        assert_eq!(child.parent_session_id.as_deref(), Some(f.caller.as_str()));
        assert_eq!(child.agent_config_id, snapshot.sessions[0].agent_config_id);
        assert_eq!(child.issue_id, snapshot.sessions[0].issue_id);
        assert_eq!(
            child.spawn.as_ref().unwrap().requested_by_run.as_deref(),
            Some(f.run.as_str())
        );
        let path = Path::new(&child.workspaces[0].path);
        assert_eq!(child.workspaces[1].path, shared.display().to_string());
        assert!(runtime::check_shared(&snapshot, &child.id).is_ok());
        assert!(
            matches!(snapshot.messages.iter().find(|m|m.id == "native-spawn").unwrap().tool.as_ref().unwrap().detail,Some(ToolDetail::Spawn {child_session_id:Some(ref actual)}) if actual == id)
        );
        assert_eq!(std::fs::read_to_string(path.join("staged")).unwrap(), "two");
        assert_eq!(
            std::fs::read_to_string(path.join("untracked")).unwrap(),
            "three"
        );
        let staged = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["show", ":staged"])
            .output()
            .unwrap();
        assert_eq!(staged.stdout, b"one");
        assert!(snapshot.messages.iter().any(|m| matches!(m.tool.as_ref().and_then(|t|t.detail.as_ref()),Some(ToolDetail::Spawn {child_session_id:Some(child)}) if child == id)));
        assert_eq!(snapshot.protocol_version, PROTOCOL_VERSION);
        let schema: u32 = f
            .workspace
            .store
            .lock()
            .unwrap()
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(schema, 27);
        f.finish().await;
    }
    #[tokio::test]
    async fn fixed_workers_without_an_effort_take_their_model_default() {
        // The fixture fixes fixture-model without an effort.
        let f = Fixture::new(Permission::Allow);
        let harness = f.workspace.snapshots.borrow().sessions[0]
            .worker
            .as_ref()
            .unwrap()
            .harness;
        f.workspace.catalogs.lock().unwrap().insert(
            "cached".into(),
            (
                0,
                HarnessCatalog {
                    harness,
                    models: vec![relay_core::HarnessModel {
                        id: "fixture-model".into(),
                        efforts: vec!["low".into(), "medium".into(), "high".into()],
                        default_effort: Some("medium".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
        );
        let defaulted = spawn_child(&f, "Defaulted").await;
        f.change(|s| s.projects[0].defaults.worker_models = WorkerModelPolicy::default());
        let chosen = f
            .call(
                "spawn_agent",
                json!({"title":"Chosen","task":"Test","harness":"codex","model":"fixture-model","effort":"high","rationale":"Needs care"}),
            )
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let snapshot = f.workspace.snapshots.borrow().clone();
        let selection = |id: &str| {
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .selection
                .clone()
        };
        assert_eq!(selection(&defaulted).effort.as_deref(), Some("medium"));
        assert_eq!(
            selection(&defaulted).model.as_deref(),
            Some("fixture-model")
        );
        assert_eq!(selection(&chosen).effort.as_deref(), Some("high"));
        f.finish().await;
    }
    #[tokio::test]
    async fn delegate_deny_and_ask() {
        let f = Fixture::new(Permission::Deny);
        assert!(
            f.call("spawn_agent", json!({"title":"Denied","task":"Test"}))
                .await
                .is_err()
        );
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1);
        f.finish().await;
        for allow in [false, true] {
            let f = Fixture::new(Permission::Ask);
            let (result, ()) = tokio::join!(
                f.call("spawn_agent", json!({"title":"Asked","task":"Test"})),
                decide(
                    &f.workspace,
                    &f.caller,
                    &f.run,
                    allow,
                    "delegation requires approval"
                )
            );
            assert_eq!(result.is_ok(), allow);
            f.finish().await;
        }
    }
    fn spawned_selection(f: &Fixture, value: &Value) -> (Harness, HarnessSelection) {
        let snapshot = f.workspace.snapshots.borrow().clone();
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id == value["agent"])
            .unwrap();
        (
            child.worker.as_ref().unwrap().harness,
            child.selection.clone(),
        )
    }
    async fn rejected(f: &Fixture, args: Value) -> String {
        let before = f.workspace.snapshots.borrow().sessions.len();
        let error = f.call("spawn_agent", args).await.unwrap_err();
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), before);
        error.message
    }
    fn spawn_schema(tools: &Value) -> Value {
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "spawn_agent")
            .unwrap()
            .clone()
    }
    fn spawn_grant(f: &Fixture, value: &Value) -> SpawnGrant {
        let snapshot = f.workspace.snapshots.borrow().clone();
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id == value["agent"])
            .unwrap();
        child.spawn.clone().unwrap()
    }
    /// Make the fixture's Codex answer discovery with `models`, its model/list data.
    fn discoverable_codex(f: &Fixture, models: Value) {
        let listed = r#"[{"id":"test-model","displayName":"Test model","defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}],"serviceTiers":[{"id":"fast"}]}]"#;
        let codex = crate::tests::command_tests::CODEX.replace(
            "@CAPTURE@",
            f._directory.path().join("capture").to_str().unwrap(),
        );
        assert!(codex.contains(listed));
        let codex = codex.replace(listed, &models.to_string());
        std::fs::write(&f.workspace.config.codex, format!("#!/bin/sh\n{codex}")).unwrap();
    }
    fn profile_discovered(f: &Fixture) -> bool {
        f.workspace
            .catalogs
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.starts_with("profile:"))
    }
    /// A capped Delegator Chooses fixture whose Codex catalog is not cached.
    fn uncached_chooser() -> Fixture {
        let f = Fixture::new(Permission::Allow);
        discoverable_codex(
            &f,
            json!([
                {"id":"gpt-plain"},
                {"id":"gpt-light","defaultReasoningEffort":"medium","supportedReasoningEfforts":[
                    {"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"xhigh"}
                ]},
            ]),
        );
        f.change(|s| {
            s.projects[0].defaults.worker_models = WorkerModelPolicy::DelegatorChooses {
                allowed_models: Vec::new(),
                max_effort: Some("high".into()),
                model_capability: true,
                available_usage: false,
            }
        });
        let cached = commands::cached(&f.workspace, &f.caller, Harness::Codex);
        assert!(cached.is_none());
        f
    }
    #[tokio::test]
    async fn uncached_models_without_effort_are_discovered_and_chosen_without_one() {
        let f = uncached_chooser();
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","rationale":"Lookup only"}),
            )
            .await
            .unwrap();
        assert!(profile_discovered(&f));
        let (harness, selection) = spawned_selection(&f, &value);
        assert_eq!(harness, Harness::Codex);
        assert_eq!(selection.model.as_deref(), Some("gpt-plain"));
        assert_eq!(selection.effort, None);
        assert_eq!(
            spawn_grant(&f, &value).selection_rationale.as_deref(),
            Some("Lookup only")
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn uncached_models_with_effort_are_checked_after_discovery() {
        let f = uncached_chooser();
        // Discovery runs before any worker exists and still requires an effort.
        let error = rejected(
            &f,
            json!({"title":"Light","task":"Test","harness":"codex","model":"gpt-light","rationale":"Small"}),
        )
        .await;
        assert!(error.contains("pass effort explicitly"), "{error}");
        assert!(profile_discovered(&f));
        for (args, expected) in [
            (
                json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","effort":"low","rationale":"Small"}),
                "Effort is not supported",
            ),
            (
                json!({"title":"Light","task":"Test","harness":"codex","model":"gpt-light","effort":"high","rationale":"Small"}),
                "Effort is not supported",
            ),
            (
                json!({"title":"Light","task":"Test","harness":"codex","model":"gpt-light","effort":"xhigh","rationale":"Hard"}),
                "requested effort xhigh exceeds the worker cap of high",
            ),
            (
                json!({"title":"Gone","task":"Test","harness":"codex","model":"gpt-gone","effort":"low","rationale":"Small"}),
                "no longer available",
            ),
        ] {
            let error = rejected(&f, args).await;
            assert!(error.contains(expected), "{error}");
        }
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Light","task":"Test","harness":"codex","model":"gpt-light","effort":"medium","rationale":"Small change"}),
            )
            .await
            .unwrap();
        let (_, selection) = spawned_selection(&f, &value);
        assert_eq!(selection.model.as_deref(), Some("gpt-light"));
        assert_eq!(selection.effort.as_deref(), Some("medium"));
        f.finish().await;
    }
    #[tokio::test]
    async fn failed_or_stopped_discovery_rejects_the_spawn() {
        let f = uncached_chooser();
        std::fs::write(&f.workspace.config.codex, "#!/bin/sh\nexit 1\n").unwrap();
        let error = rejected(
            &f,
            json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","rationale":"Lookup only"}),
        )
        .await;
        assert!(
            error.contains("Cannot discover Codex models to check this worker"),
            "{error}"
        );
        f.finish().await;

        // A Codex that never lists models holds discovery until the run stops.
        let f = uncached_chooser();
        std::fs::write(&f.workspace.config.codex, "#!/bin/sh\nsleep 30\n").unwrap();
        let stop = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let store = f.workspace.store.lock().unwrap();
            store.controls[&f.caller].send_replace(true);
        };
        let (result, ()) = tokio::join!(
            f.call(
                "spawn_agent",
                json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","rationale":"Lookup only"}),
            ),
            stop
        );
        assert!(result.is_err());
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1);
        assert!(!profile_discovered(&f));
        f.finish().await;
    }
    #[tokio::test]
    async fn delegator_chooses_requires_an_explicit_explained_selection() {
        let f = Fixture::new(Permission::Allow);
        let model = |id: &str, efforts: &[&str]| relay_core::HarnessModel {
            id: id.into(),
            efforts: efforts.iter().map(|e| e.to_string()).collect(),
            default_effort: efforts.first().map(|e| e.to_string()),
            ..Default::default()
        };
        f.workspace.catalogs.lock().unwrap().insert(
            "cached".into(),
            (
                0,
                HarnessCatalog {
                    harness: Harness::Codex,
                    models: vec![
                        model("gpt-light", &["low", "medium", "xhigh"]),
                        model("gpt-plain", &[]),
                    ],
                    ..Default::default()
                },
            ),
        );
        f.change(|s| {
            s.projects[0].defaults.worker_models = WorkerModelPolicy::DelegatorChooses {
                allowed_models: Vec::new(),
                max_effort: Some("high".into()),
                model_capability: true,
                available_usage: true,
            }
        });
        // Nothing falls back to the delegator's own harness or model.
        let error = rejected(&f, json!({"title":"Bare","task":"Test"})).await;
        assert!(
            error.contains("pass harness, model, effort, rationale explicitly"),
            "{error}"
        );
        assert!(
            error.contains("the capability the task needs, remaining usage"),
            "{error}"
        );
        let error = rejected(
            &f,
            json!({"title":"No model","task":"Test","harness":"codex","effort":"low","rationale":"Small"}),
        )
        .await;
        assert!(error.contains("pass model explicitly"), "{error}");
        // A model with an effort setting needs an explicit effort.
        let error = rejected(
            &f,
            json!({"title":"No effort","task":"Test","harness":"codex","model":"gpt-light","rationale":"Small"}),
        )
        .await;
        assert!(error.contains("pass effort explicitly"), "{error}");
        for blank in [json!(null), json!(" \n ")] {
            let error = rejected(
                &f,
                json!({"title":"No reason","task":"Test","harness":"codex","model":"gpt-light","effort":"low","rationale":blank}),
            )
            .await;
            assert!(error.contains("pass rationale explicitly"), "{error}");
        }
        let error = rejected(
            &f,
            json!({"title":"Long","task":"Test","harness":"codex","model":"gpt-light","effort":"low","rationale":"x".repeat(MAX_RATIONALE_LEN + 1)}),
        )
        .await;
        assert!(error.contains("within 1000 characters"), "{error}");
        // Explicit choices still meet the catalog and the effort cap.
        let error = rejected(
            &f,
            json!({"title":"Heavy","task":"Test","harness":"codex","model":"gpt-light","effort":"xhigh","rationale":"Hard"}),
        )
        .await;
        assert!(
            error.contains("requested effort xhigh exceeds the worker cap of high"),
            "{error}"
        );
        let error = rejected(
            &f,
            json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","effort":"low","rationale":"Simple"}),
        )
        .await;
        assert!(error.contains("Effort is not supported"), "{error}");
        let error = rejected(
            &f,
            json!({"title":"Unknown","task":"Test","harness":"codex","model":"gpt-gone","effort":"low","rationale":"Simple"}),
        )
        .await;
        assert!(error.contains("no longer available"), "{error}");

        // A complete choice spawns and keeps its trimmed rationale.
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Chosen","task":"Test","harness":"codex","model":"gpt-light","effort":"medium","rationale":"  Mechanical rename; light model, ample quota \n"}),
            )
            .await
            .unwrap();
        let (harness, selection) = spawned_selection(&f, &value);
        assert_eq!(harness, Harness::Codex);
        assert_eq!(selection.model.as_deref(), Some("gpt-light"));
        assert_eq!(selection.effort.as_deref(), Some("medium"));
        assert_eq!(
            spawn_grant(&f, &value).selection_rationale.as_deref(),
            Some("Mechanical rename; light model, ample quota")
        );
        // A model without an effort setting is chosen without one, under the cap.
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Plain","task":"Test","harness":"codex","model":"gpt-plain","rationale":"Simple lookup"}),
            )
            .await
            .unwrap();
        let (_, selection) = spawned_selection(&f, &value);
        assert_eq!(selection.model.as_deref(), Some("gpt-plain"));
        assert_eq!(selection.effort, None);
        f.finish().await;
    }
    #[tokio::test]
    async fn worker_model_policy_is_enforced_in_spawn_and_described_in_tools() {
        let f = Fixture::new(Permission::Allow);
        discoverable_codex(
            &f,
            json!([
                {"id":"other-model","supportedReasoningEfforts":[{"reasoningEffort":"xhigh"}]},
                {"id":"fixed-model","supportedReasoningEfforts":[{"reasoningEffort":"low"}]},
                {"id":"gpt-light","supportedReasoningEfforts":[
                    {"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"max"}
                ]},
            ]),
        );
        // The open default lets the delegator choose any model and effort.
        f.change(|s| s.projects[0].defaults.worker_models = WorkerModelPolicy::default());
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Open","task":"Test","harness":"codex","model":"other-model","effort":"xhigh","rationale":"Hard task"}),
            )
            .await
            .unwrap();
        let (harness, selection) = spawned_selection(&f, &value);
        assert_eq!(harness, Harness::Codex);
        assert_eq!(selection.model.as_deref(), Some("other-model"));
        assert_eq!(selection.effort.as_deref(), Some("xhigh"));
        let open = spawn_schema(&f.call("tools", json!({})).await.unwrap());
        assert_eq!(
            open["inputSchema"]["properties"]["harness"]["enum"],
            json!(["codex", "claude_code"])
        );
        assert_eq!(
            open["inputSchema"]["properties"]["effort"],
            json!({"type":"string"})
        );
        assert_eq!(
            open["inputSchema"]["properties"]["rationale"],
            json!({"type":"string","maxLength":MAX_RATIONALE_LEN})
        );
        assert_eq!(
            open["inputSchema"]["required"],
            json!(["title", "task", "harness", "model", "rationale"])
        );
        let description = open["description"].as_str().unwrap();
        assert!(
            description.contains("any model of a connected harness; any effort"),
            "{description}"
        );
        assert!(description.contains("must pass all three"), "{description}");
        assert!(
            description.contains("rationale covering why the harness, model and effort fit the task, the capability the task needs."),
            "{description}"
        );

        // Fixed: differing values are rejected; omitted values are filled.
        f.change(|s| {
            s.projects[0].defaults.worker_models = WorkerModelPolicy::Fixed {
                harness: Harness::Codex,
                model: "fixed-model".into(),
                effort: Some("low".into()),
            }
        });
        for args in [
            json!({"title":"Fixed","task":"Test","model":"other-model"}),
            json!({"title":"Fixed","task":"Test","harness":"claude_code"}),
            json!({"title":"Fixed","task":"Test","effort":"high"}),
        ] {
            let error = rejected(&f, args).await;
            assert!(error.contains("fixes the worker model"), "{error}");
            assert!(error.contains("Codex fixed-model at low effort"), "{error}");
        }
        // No rationale is needed when there is no choice.
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Fixed","task":"Test","model":"fixed-model"}),
            )
            .await
            .unwrap();
        let (harness, selection) = spawned_selection(&f, &value);
        assert_eq!(harness, Harness::Codex);
        assert_eq!(selection.model.as_deref(), Some("fixed-model"));
        assert_eq!(selection.effort.as_deref(), Some("low"));
        assert_eq!(spawn_grant(&f, &value).selection_rationale, None);
        let fixed = spawn_schema(&f.call("tools", json!({})).await.unwrap());
        assert_eq!(
            fixed["inputSchema"]["properties"]["harness"]["enum"],
            json!(["codex"])
        );
        assert_eq!(
            fixed["inputSchema"]["properties"]["model"]["enum"],
            json!(["fixed-model"])
        );
        assert_eq!(
            fixed["inputSchema"]["properties"]["effort"]["enum"],
            json!(["low"])
        );
        assert_eq!(fixed["inputSchema"]["properties"].get("rationale"), None);
        assert_eq!(fixed["inputSchema"]["required"], json!(["title", "task"]));
        assert!(
            fixed["description"]
                .as_str()
                .unwrap()
                .contains("Worker model is fixed")
        );

        // Delegator chooses within an allowlist and an effort cap.
        f.change(|s| {
            s.projects[0].defaults.worker_models = WorkerModelPolicy::DelegatorChooses {
                allowed_models: vec![
                    ModelChoice {
                        harness: Harness::Codex,
                        model: "gpt-light".into(),
                    },
                    ModelChoice {
                        harness: Harness::ClaudeCode,
                        model: "claude-heavy".into(),
                    },
                ],
                max_effort: Some("high".into()),
                model_capability: true,
                available_usage: true,
            }
        });
        // The profile model is no longer used when the model is omitted.
        let error = rejected(
            &f,
            json!({"title":"Listed","task":"Test","harness":"codex","effort":"low","rationale":"Small"}),
        )
        .await;
        assert!(error.contains("pass model explicitly"), "{error}");
        assert!(
            error.contains("Allowed models: Codex gpt-light, Claude Code claude-heavy"),
            "{error}"
        );
        let error = rejected(
            &f,
            json!({"title":"Listed","task":"Test","harness":"codex","model":"claude-heavy","effort":"low","rationale":"Small"}),
        )
        .await;
        assert!(
            error.contains("Codex claude-heavy is not allowed"),
            "{error}"
        );
        let error = rejected(
            &f,
            json!({"title":"Listed","task":"Test","harness":"codex","model":"gpt-light","effort":"max","rationale":"Hard"}),
        )
        .await;
        assert!(
            error.contains("requested effort max exceeds the worker cap of high"),
            "{error}"
        );
        assert!(
            error.contains("none, minimal, low, medium, high"),
            "{error}"
        );
        // gpt-light offers efforts, so one must be chosen.
        let error = rejected(
            &f,
            json!({"title":"Listed","task":"Test","harness":"codex","model":"gpt-light","rationale":"Small"}),
        )
        .await;
        assert!(error.contains("pass effort explicitly"), "{error}");
        let value = f
            .call(
                "spawn_agent",
                json!({"title":"Listed","task":"Test","harness":"codex","model":"gpt-light","effort":"medium","rationale":"Clear small change"}),
            )
            .await
            .unwrap();
        let (harness, selection) = spawned_selection(&f, &value);
        assert_eq!(harness, Harness::Codex);
        assert_eq!(selection.model.as_deref(), Some("gpt-light"));
        assert_eq!(selection.effort.as_deref(), Some("medium"));
        let listed = spawn_schema(&f.call("tools", json!({})).await.unwrap());
        assert_eq!(
            listed["inputSchema"]["properties"]["model"]["enum"],
            json!(["gpt-light", "claude-heavy"])
        );
        assert_eq!(
            listed["inputSchema"]["properties"]["effort"]["enum"],
            json!(["none", "minimal", "low", "medium", "high"])
        );

        // list_agents reports the policy and, with available usage on, usage.
        let now = crate::now();
        f.change(|s| {
            s.record_usage_limit(UsageLimit::new(
                Harness::Codex,
                UsageWindow::FiveHour,
                38.0,
                300,
                Some(now + 3600),
                now,
            ));
        });
        let listed = f.call("list_agents", json!({})).await.unwrap();
        let models = &listed["worker_models"];
        assert_eq!(models["policy"]["mode"], "delegator_chooses");
        assert_eq!(models["policy"]["max_effort"], "high");
        assert!(
            models["summary"]
                .as_str()
                .unwrap()
                .contains("effort at most high")
        );
        assert!(
            models["usage"][0]
                .as_str()
                .unwrap()
                .starts_with("Codex: 62% of the 5-hour window left"),
            "{models}"
        );
        assert!(
            models["usage"][1]
                .as_str()
                .unwrap()
                .starts_with("Claude Code: 5-hour window not reported")
        );
        f.change(|s| s.projects[0].defaults.worker_models = WorkerModelPolicy::default());
        let listed = f.call("list_agents", json!({})).await.unwrap();
        assert!(listed["worker_models"].get("usage").is_none());
        assert!(f.call("tools", json!({"x":1})).await.is_err());
        f.finish().await;
    }
    #[tokio::test]
    async fn widening_requires_human_and_child_map_is_enforced() {
        for allow in [false, true] {
            let f = Fixture::new(Permission::Allow);
            f.change(|s| {
                s.projects[0]
                    .defaults
                    .permissions
                    .insert(Task::Implement, Permission::Deny);
            });
            let (result, ()) = tokio::join!(
                f.call(
                    "spawn_agent",
                    json!({"title":"Writer","task":"Test","permissions":{"implement":"allow"}})
                ),
                decide(
                    &f.workspace,
                    &f.caller,
                    &f.run,
                    allow,
                    "implement allow (parent: deny)"
                )
            );
            assert_eq!(result.is_ok(), allow);
            if let Ok(result) = result {
                let snapshot = f.workspace.snapshots.borrow().clone();
                let child = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == result["agent"])
                    .unwrap();
                assert_eq!(
                    profile(&snapshot, child).unwrap().permissions[&Task::Implement],
                    Permission::Allow
                );
                assert!(authorize_session(&snapshot, child, false, &f.workspace.config).is_ok());
            }
            f.finish().await;
        }
    }
    /// Gives the calling session its own execution setting.
    fn parent_mode(f: &Fixture, mode: ApprovalMode) {
        f.change(|s| {
            s.sessions
                .iter_mut()
                .find(|s| s.id == f.caller)
                .unwrap()
                .worker
                .as_mut()
                .unwrap()
                .execution = Some(ExecutionSettings { approval: mode });
        });
    }
    fn execution_policy(f: &Fixture, policy: SpawnExecutionApproval) {
        f.change(|s| {
            s.projects
                .iter_mut()
                .for_each(|p| p.spawn_execution_approval = policy)
        });
    }
    /// The execution mode a spawned child's launches and resumes use. Every
    /// spawned child holds it as its own setting, named or inherited.
    fn child_mode(f: &Fixture, value: &Value) -> ApprovalMode {
        let snapshot = f.workspace.snapshots.borrow().clone();
        let id = value["agent"].as_str().unwrap();
        let mode = crate::harness::mode(&snapshot, id).unwrap();
        let child = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
        assert_eq!(
            child.worker.as_ref().unwrap().execution,
            Some(ExecutionSettings { approval: mode })
        );
        mode
    }
    /// Spawns a child that needs no approval: a request would hold the call.
    async fn spawn_unasked(f: &Fixture, args: Value) -> Value {
        let before = permission_requests(f);
        let value = tokio::time::timeout(Duration::from_secs(10), f.call("spawn_agent", args))
            .await
            .expect("spawn asked for approval")
            .unwrap();
        assert_eq!(permission_requests(f), before);
        value
    }
    async fn await_pending(f: &Fixture) -> ToolPermission {
        let mut events = f.workspace.snapshots.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let pending = events
                    .borrow()
                    .tool_permissions
                    .iter()
                    .find(|p| p.decision.is_none() && !p.expired)
                    .cloned();
                if let Some(pending) = pending {
                    return pending;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn unrestricted_children_need_approval_even_under_an_unrestricted_parent() {
        for explicit in [false, true] {
            for allow in [false, true] {
                let f = Fixture::new(Permission::Allow);
                parent_mode(&f, ApprovalMode::Unrestricted);
                let args = if explicit {
                    json!({"title":"Free","task":"Test","execution_approval":"unrestricted"})
                } else {
                    json!({"title":"Free","task":"Test"})
                };
                let (result, ()) = tokio::join!(
                    f.call("spawn_agent", args),
                    decide(
                        &f.workspace,
                        &f.caller,
                        &f.run,
                        allow,
                        "execution mode Unrestricted Access (same as parent): runs without the harness sandbox"
                    )
                );
                assert_eq!(result.is_ok(), allow);
                match result {
                    Ok(value) => {
                        assert_eq!(child_mode(&f, &value), ApprovalMode::Unrestricted);
                        assert_eq!(
                            spawn_grant(&f, &value).execution_approval,
                            explicit.then_some(ApprovalMode::Unrestricted)
                        );
                    }
                    // A denial creates no child and no fork.
                    Err(_) => assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1),
                }
                f.finish().await;
            }
        }
    }
    #[tokio::test]
    async fn raised_child_modes_need_approval_and_inherited_or_lower_modes_do_not() {
        for allow in [false, true] {
            let f = Fixture::new(Permission::Allow);
            parent_mode(&f, ApprovalMode::Ask);
            let (result, ()) = tokio::join!(
                f.call(
                    "spawn_agent",
                    json!({"title":"Raised","task":"Test","execution_approval":"automatic"})
                ),
                decide(
                    &f.workspace,
                    &f.caller,
                    &f.run,
                    allow,
                    "execution mode Ask → Automatic"
                )
            );
            assert_eq!(result.is_ok(), allow);
            match result {
                Ok(value) => {
                    assert_eq!(child_mode(&f, &value), ApprovalMode::Automatic);
                    assert_eq!(
                        spawn_grant(&f, &value).execution_approval,
                        Some(ApprovalMode::Automatic)
                    );
                    let snapshot = f.workspace.snapshots.borrow().clone();
                    let child = snapshot
                        .sessions
                        .iter()
                        .find(|s| s.id == value["agent"])
                        .unwrap();
                    assert_eq!(
                        child.worker.as_ref().unwrap().execution,
                        Some(ExecutionSettings {
                            approval: ApprovalMode::Automatic
                        })
                    );
                }
                Err(error) => {
                    assert!(error.message.contains("denied"), "{}", error.message);
                    assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1);
                }
            }
            f.finish().await;
        }

        // Omitted, the child runs in its parent's mode without asking, and
        // naming that same mode is not a raise.
        let f = Fixture::new(Permission::Allow);
        parent_mode(&f, ApprovalMode::Ask);
        let inherited = spawn_unasked(&f, json!({"title":"Inherited","task":"Test"})).await;
        assert_eq!(child_mode(&f, &inherited), ApprovalMode::Ask);
        assert_eq!(spawn_grant(&f, &inherited).execution_approval, None);
        let same = spawn_unasked(
            &f,
            json!({"title":"Same","task":"Test","execution_approval":"ask"}),
        )
        .await;
        assert_eq!(child_mode(&f, &same), ApprovalMode::Ask);
        f.finish().await;

        // The profile's own mode is inherited the same way.
        let f = Fixture::new(Permission::Allow);
        let parent_profile_mode = {
            let snapshot = f.workspace.snapshots.borrow().clone();
            crate::harness::mode(&snapshot, &f.caller).unwrap()
        };
        assert_eq!(parent_profile_mode, ApprovalMode::Automatic);
        let inherited = spawn_unasked(&f, json!({"title":"Profile","task":"Test"})).await;
        assert_eq!(child_mode(&f, &inherited), ApprovalMode::Automatic);
        f.finish().await;

        // A lower mode needs no approval, even below Unrestricted Access.
        let f = Fixture::new(Permission::Allow);
        parent_mode(&f, ApprovalMode::Unrestricted);
        for (mode, name) in [
            (ApprovalMode::Automatic, "automatic"),
            (ApprovalMode::Ask, "ask"),
        ] {
            let value = spawn_unasked(
                &f,
                json!({"title":name,"task":"Test","execution_approval":name}),
            )
            .await;
            assert_eq!(child_mode(&f, &value), mode);
            assert_eq!(spawn_grant(&f, &value).execution_approval, Some(mode));
        }
        f.finish().await;
    }
    #[tokio::test]
    async fn the_project_owner_chooses_which_child_modes_need_approval() {
        // Strict: every child above Ask, even in its parent's mode.
        let f = Fixture::new(Permission::Allow);
        execution_policy(&f, SpawnExecutionApproval::Strict);
        let (result, ()) = tokio::join!(
            f.call("spawn_agent", json!({"title":"Strict","task":"Test"})),
            decide(
                &f.workspace,
                &f.caller,
                &f.run,
                true,
                "execution mode Automatic (same as parent)"
            )
        );
        assert_eq!(child_mode(&f, &result.unwrap()), ApprovalMode::Automatic);
        let asking = spawn_unasked(
            &f,
            json!({"title":"Asking","task":"Test","execution_approval":"ask"}),
        )
        .await;
        assert_eq!(child_mode(&f, &asking), ApprovalMode::Ask);
        f.finish().await;

        // Relaxed: an Unrestricted Access parent starts Unrestricted Access
        // children without asking, but a raise still needs approval.
        let f = Fixture::new(Permission::Allow);
        execution_policy(&f, SpawnExecutionApproval::Relaxed);
        parent_mode(&f, ApprovalMode::Unrestricted);
        let free = spawn_unasked(&f, json!({"title":"Free","task":"Test"})).await;
        assert_eq!(child_mode(&f, &free), ApprovalMode::Unrestricted);
        parent_mode(&f, ApprovalMode::Ask);
        let (result, ()) = tokio::join!(
            f.call(
                "spawn_agent",
                json!({"title":"Raised","task":"Test","execution_approval":"unrestricted"})
            ),
            decide(
                &f.workspace,
                &f.caller,
                &f.run,
                false,
                "execution mode Ask → Unrestricted Access: runs without the harness sandbox"
            )
        );
        assert!(result.is_err());
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 2);

        // Agents cannot change the policy: there is no tool for it, and spawn
        // rejects unknown fields and modes.
        let error = f
            .call("set_spawn_execution_approval", json!({"policy":"relaxed"}))
            .await
            .unwrap_err();
        assert!(
            error.message.contains("Unknown Relay tool"),
            "{}",
            error.message
        );
        rejected(
            &f,
            json!({"title":"Bad","task":"Test","execution_approval":"never"}),
        )
        .await;
        rejected(
            &f,
            json!({"title":"Bad","task":"Test","spawn_execution_approval":"relaxed"}),
        )
        .await;
        rejected(
            &f,
            json!({"title":"Bad","task":"Test","execution":{"approval":"unrestricted"}}),
        )
        .await;
        let tools = f.call("tools", json!({})).await.unwrap();
        assert!(
            tools["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| !t["name"].as_str().unwrap().contains("approval"))
        );
        // The owner sets it with a workspace command; unknown projects fail.
        {
            let mut store = f.workspace.store.lock().unwrap();
            let revision = store.snapshot().unwrap().revision;
            assert!(
                store
                    .apply(
                        crate::tests::env(
                            revision,
                            Command::SetSpawnExecutionApproval {
                                project_id: "missing".into(),
                                policy: SpawnExecutionApproval::Strict,
                            },
                        ),
                        &f.workspace.config,
                    )
                    .is_err()
            );
            let (snapshot, _) = store
                .apply(
                    crate::tests::env(
                        revision,
                        Command::SetSpawnExecutionApproval {
                            project_id: "live".into(),
                            policy: SpawnExecutionApproval::Strict,
                        },
                    ),
                    &f.workspace.config,
                )
                .unwrap();
            assert_eq!(
                snapshot.projects[0].spawn_execution_approval,
                SpawnExecutionApproval::Strict
            );
            f.workspace.snapshots.send_replace(snapshot);
        }
        f.finish().await;
    }
    #[tokio::test]
    async fn spawn_tool_describes_the_execution_mode_and_rule() {
        let f = Fixture::new(Permission::Allow);
        parent_mode(&f, ApprovalMode::Ask);
        let spec = spawn_schema(&f.call("tools", json!({})).await.unwrap());
        assert_eq!(
            spec["inputSchema"]["properties"]["execution_approval"],
            json!({"type":"string","enum":["ask","automatic","unrestricted"]})
        );
        assert_eq!(spec["inputSchema"]["additionalProperties"], false);
        let description = spec["description"].as_str().unwrap();
        assert!(
            description.contains("omit it to run the child in yours (Ask)"),
            "{description}"
        );
        assert!(
            description.contains(SpawnExecutionApproval::Standard.describe()),
            "{description}"
        );
        execution_policy(&f, SpawnExecutionApproval::Relaxed);
        let spec = spawn_schema(&f.call("tools", json!({})).await.unwrap());
        assert!(
            spec["description"]
                .as_str()
                .unwrap()
                .contains(SpawnExecutionApproval::Relaxed.describe())
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn one_request_covers_delegation_permissions_execution_and_the_first_turn() {
        let f = Fixture::new(Permission::Ask);
        f.change(|s| {
            let permissions = &mut s.projects[0].defaults.permissions;
            permissions.insert(Task::Merge, Permission::Deny);
            permissions.insert(Task::Implement, Permission::Ask);
        });
        let before = permission_requests(&f);
        let (result, ()) = tokio::join!(
            f.call(
                "spawn_agent",
                json!({"title":"Combined","task":"Test","permissions":{"merge":"allow"},"execution_approval":"unrestricted"})
            ),
            decide(
                &f.workspace,
                &f.caller,
                &f.run,
                true,
                "delegation requires approval"
            )
        );
        let value = result.unwrap();
        assert_eq!(permission_requests(&f), before + 1);
        let snapshot = f.workspace.snapshots.borrow().clone();
        let request = snapshot.tool_permissions.last().unwrap();
        assert_eq!(request.tool, "mcp__relay__spawn_agent");
        for line in [
            "Spawn Combined with:",
            "delegation requires approval",
            "execution mode Automatic → Unrestricted Access: runs without the harness sandbox",
            "merge allow (parent: deny)",
            "implementation for its first turn",
        ] {
            assert!(
                request.description.contains(line),
                "{}",
                request.description
            );
        }
        assert_eq!(child_mode(&f, &value), ApprovalMode::Unrestricted);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id == value["agent"])
            .unwrap();
        // Merge stays a task permission; the execution mode does not grant it.
        assert_eq!(
            profile(&snapshot, child).unwrap().permissions[&Task::Merge],
            Permission::Allow
        );
        f.finish().await;

        // An execution mode never lifts a Deny: a child that may not
        // implement is refused in Unrestricted Access too.
        let f = Fixture::new(Permission::Allow);
        f.change(|s| {
            s.projects[0]
                .defaults
                .permissions
                .insert(Task::Implement, Permission::Deny);
        });
        let (result, ()) = tokio::join!(
            f.call(
                "spawn_agent",
                json!({"title":"Denied","task":"Test","execution_approval":"unrestricted"})
            ),
            decide(
                &f.workspace,
                &f.caller,
                &f.run,
                true,
                "execution mode Automatic → Unrestricted Access"
            )
        );
        assert!(result.is_err());
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1);
        f.finish().await;
    }
    #[tokio::test]
    async fn approvals_are_rechecked_against_the_current_mode_policy_and_run() {
        for change in ["none", "mode", "policy", "stop"] {
            let f = Fixture::new(Permission::Allow);
            parent_mode(&f, ApprovalMode::Ask);
            // A parent repository that does not exist fails any fork with
            // git's error, so a retry error proves the recheck came first.
            let missing = f._directory.path().join("missing-repository");
            let original = f.workspace.snapshots.borrow().sessions[0].workspaces[0]
                .path
                .clone();
            let point = |path: String| {
                f.change(|s| {
                    let parent = s.sessions.iter_mut().find(|s| s.id == f.caller).unwrap();
                    parent.workspaces[0].path = path.clone();
                    parent.worker.as_mut().unwrap().worktree = Some(path);
                })
            };
            point(missing.display().to_string());
            let respond = async {
                await_pending(&f).await;
                match change {
                    "none" => {}
                    "mode" => parent_mode(&f, ApprovalMode::Automatic),
                    "policy" => execution_policy(&f, SpawnExecutionApproval::Strict),
                    _ => {
                        let store = f.workspace.store.lock().unwrap();
                        store.controls[&f.caller].send_replace(true);
                    }
                }
                if change != "stop" {
                    decide(
                        &f.workspace,
                        &f.caller,
                        &f.run,
                        true,
                        "execution mode Ask → Automatic",
                    )
                    .await;
                }
            };
            let (result, ()) = tokio::join!(
                f.call(
                    "spawn_agent",
                    json!({"title":"Stale","task":"Test","execution_approval":"automatic"})
                ),
                respond
            );
            let error = result.unwrap_err();
            match change {
                // Control: a current approval does reach the fork, which fails.
                "none" => assert!(
                    error.message.contains("repository state into the fork"),
                    "{}",
                    error.message
                ),
                "mode" => assert!(
                    error.message.contains("Parent execution mode changed"),
                    "{}",
                    error.message
                ),
                "policy" => assert!(
                    error.message.contains("approval policy changed"),
                    "{}",
                    error.message
                ),
                _ => assert!(error.message.contains("denied"), "{}", error.message),
            }
            assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 1, "{change}");
            assert!(!missing.exists(), "{change}");
            point(original);
            f.finish().await;
        }
    }
    #[tokio::test]
    async fn omitted_modes_stay_frozen_through_profile_changes_launch_and_resume() {
        let f = Fixture::new(Permission::Allow);
        let capture = f._directory.path().join("codex-capture");
        let gate = f._directory.path().join("discovery-gate");
        // Launch discovery waits for the gate, holding the first turn after
        // the child is saved and before its harness starts.
        let codex = crate::tests::command_tests::CODEX
            .replace("@CAPTURE@", capture.to_str().unwrap())
            .replace("test-model", "fixture-model")
            .replace(
                "model/list) printf",
                &format!(
                    "model/list) while [ ! -f '{}' ]; do sleep 0.05; done; printf",
                    gate.display()
                ),
            );
        assert!(codex.contains("discovery-gate"));
        std::fs::write(&f.workspace.config.codex, format!("#!/bin/sh\n{codex}")).unwrap();
        let captured = |method: &str| {
            std::fs::read_to_string(&capture)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(|v| v["method"] == method)
                .collect::<Vec<_>>()
        };
        // The parent has no execution setting of its own and runs its
        // profile's Automatic mode.
        {
            let snapshot = f.workspace.snapshots.borrow().clone();
            let parent = snapshot.sessions.iter().find(|s| s.id == f.caller).unwrap();
            assert_eq!(parent.worker.as_ref().unwrap().execution, None);
            assert_eq!(
                crate::harness::mode(&snapshot, &f.caller).unwrap(),
                ApprovalMode::Automatic
            );
        }
        let value = spawn_unasked(&f, json!({"title":"Frozen","task":"Test"})).await;
        let child = value["agent"].as_str().unwrap().to_owned();
        assert_eq!(spawn_grant(&f, &value).execution_approval, None);
        tokio::time::timeout(Duration::from_secs(10), async {
            while captured("model/list").is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        // Raise the project and director profiles to Unrestricted Access
        // while the first launch is held.
        f.change(|s| {
            s.projects[0].defaults.execution.approval = ApprovalMode::Unrestricted;
            for config in &mut s.agent_configs {
                config.overrides.execution = Some(ExecutionSettings {
                    approval: ApprovalMode::Unrestricted,
                });
            }
        });
        {
            let snapshot = f.workspace.snapshots.borrow().clone();
            assert_eq!(
                crate::harness::mode(&snapshot, &f.caller).unwrap(),
                ApprovalMode::Unrestricted
            );
        }
        assert_eq!(child_mode(&f, &value), ApprovalMode::Automatic);
        std::fs::write(&gate, "").unwrap();
        await_ended(&f, &[&child]).await;
        let worker = {
            let snapshot = f.workspace.snapshots.borrow().clone();
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == child)
                .unwrap()
                .worker
                .clone()
                .unwrap()
        };
        assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
        // A parent message resumes the child in the same frozen mode.
        f.call("message_agent", json!({"agent":child,"text":"Continue"}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(20), async {
            while captured("turn/start").len() < 2 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(captured("thread/resume").len(), 1);
        let turns = captured("turn/start");
        assert_eq!(turns.len(), 2);
        for turn in &turns {
            assert_eq!(turn["params"]["approvalPolicy"], "never", "{turn}");
            assert_eq!(
                turn["params"]["sandboxPolicy"]["type"], "workspaceWrite",
                "{turn}"
            );
        }
        await_ended(&f, &[&child]).await;
        assert_eq!(child_mode(&f, &value), ApprovalMode::Automatic);
        f.finish().await;
    }
    #[tokio::test]
    async fn spawned_execution_modes_reach_both_harnesses() {
        // Codex: the approved Unrestricted Access mode selects full access.
        let f = Fixture::new(Permission::Allow);
        let capture = f._directory.path().join("codex-capture");
        let codex = crate::tests::command_tests::CODEX
            .replace("@CAPTURE@", capture.to_str().unwrap())
            .replace("test-model", "fixture-model");
        std::fs::write(&f.workspace.config.codex, format!("#!/bin/sh\n{codex}")).unwrap();
        let (result, ()) = tokio::join!(
            f.call(
                "spawn_agent",
                json!({"title":"Codex","task":"Test","execution_approval":"unrestricted"})
            ),
            decide(
                &f.workspace,
                &f.caller,
                &f.run,
                true,
                "execution mode Automatic → Unrestricted Access"
            )
        );
        let child = result.unwrap()["agent"].as_str().unwrap().to_owned();
        await_ended(&f, &[&child]).await;
        let turn = std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|v| v["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn["params"]["approvalPolicy"], "never");
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "dangerFullAccess");
        f.finish().await;

        // Claude Code: a lower Ask mode needs no approval and selects the
        // default permission mode with the sandbox.
        let f = Fixture::new(Permission::Allow);
        let claude = f._directory.path().join("fake-claude");
        // Launch discovery must list the child's model.
        crate::tests::script(
            &claude,
            &crate::tests::harness_tests::CLAUDE_FAKE.replace("workspace-model", "fixture-model"),
        );
        f.change(|s| {
            s.installations.push(HarnessInstallation {
                harness: Harness::ClaudeCode,
                executable: claude.display().to_string(),
            });
            s.projects[0].defaults.worker_models = WorkerModelPolicy::Fixed {
                harness: Harness::ClaudeCode,
                model: "fixture-model".into(),
                effort: None,
            };
        });
        let value = spawn_unasked(
            &f,
            json!({"title":"Claude","task":"Test","execution_approval":"ask"}),
        )
        .await;
        let child = value["agent"].as_str().unwrap().to_owned();
        await_ended(&f, &[&child]).await;
        let snapshot = f.workspace.snapshots.borrow().clone();
        let worker = snapshot
            .sessions
            .iter()
            .find(|s| s.id == child)
            .unwrap()
            .worker
            .clone()
            .unwrap();
        assert_eq!(worker.harness, Harness::ClaudeCode);
        let argv = std::fs::read_to_string(
            Path::new(worker.worktree.as_ref().unwrap()).join("claude-argv"),
        )
        .unwrap();
        // Discovery passes no permission mode; this is the turn's.
        assert!(argv.contains("\n--permission-mode\ndefault\n"), "{argv}");
        assert!(!argv.contains("bypassPermissions"), "{argv}");
        assert!(argv.contains("\n--settings\n"), "{argv}");
        f.finish().await;
    }
    #[tokio::test]
    async fn deep_tree_and_tree_wide_limit() {
        let f = Fixture::new(Permission::Allow);
        let mut caller = f.caller.clone();
        let mut run = f.run.clone();
        for depth in 0..6 {
            let value = call(
                &f.workspace,
                &caller,
                &run,
                "spawn_agent",
                json!({"title":format!("Depth {depth}"),"task":"Test"}),
                &mut f.stop.clone(),
            )
            .await
            .unwrap();
            caller = value["agent"].as_str().unwrap().into();
            run = f
                .workspace
                .store
                .lock()
                .unwrap()
                .run_id(&caller)
                .unwrap()
                .unwrap();
        }
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert_eq!(snapshot.ancestors(&caller).len(), 6);
        assert_eq!(
            profile(&snapshot, snapshot.sessions.last().unwrap())
                .unwrap()
                .permissions[&Task::Delegate],
            Permission::Allow
        );
        f.change(|s| {
            // The root occupies no slot, so six descendants fill a limit of six.
            s.projects[0].defaults.max_workers = 6;
        });
        assert!(
            call(
                &f.workspace,
                &caller,
                &run,
                "spawn_agent",
                json!({"title":"Overflow","task":"Test"}),
                &mut f.stop.clone()
            )
            .await
            .is_err()
        );
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 7);
        f.finish().await;
    }
    /// Adds an active child cloned from `parent` without launching it.
    fn active_child(s: &mut Snapshot, parent: &str, id: &str) {
        let mut child = s.sessions.iter().find(|s| s.id == parent).unwrap().clone();
        child.id = id.into();
        child.role = SessionRole::Worker;
        child.parent_session_id = Some(parent.into());
        child.worker.as_mut().unwrap().status = WorkerStatus::Running;
        s.sessions.push(child);
    }
    /// Synthetic children have no process, so settle them before shutdown.
    fn settle(f: &Fixture, ids: &[&str]) {
        f.change(|s| {
            for session in s
                .sessions
                .iter_mut()
                .filter(|s| ids.contains(&s.id.as_str()))
            {
                session.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            }
        });
    }
    fn turn_allowed(f: &Fixture, id: &str) -> bool {
        let snapshot = f.workspace.snapshots.borrow().clone();
        let session = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
        authorize_session(&snapshot, session, true, &f.workspace.config).is_ok()
    }
    #[tokio::test]
    async fn director_turns_are_not_blocked_by_their_own_full_slots() {
        let f = Fixture::new(Permission::Allow);
        f.change(|s| {
            s.projects[0].defaults.max_workers = 2;
            active_child(s, &f.caller, "child-1");
            active_child(s, &f.caller, "child-2");
        });
        // A standalone director occupies its configuration's slot, never its own.
        assert!(turn_allowed(&f, &f.caller));
        f.change(|s| {
            let root = s.sessions.iter_mut().find(|s| s.id == f.caller).unwrap();
            root.role = SessionRole::Root;
        });
        assert!(turn_allowed(&f, &f.caller));
        // Spawning past the limit is still rejected.
        assert!(
            f.call("spawn_agent", json!({"title":"Overflow","task":"Test"}))
                .await
                .is_err()
        );
        assert_eq!(f.workspace.snapshots.borrow().sessions.len(), 3);
        // A freed slot admits the next worker.
        f.change(|s| {
            let child = s.sessions.iter_mut().find(|s| s.id == "child-2").unwrap();
            child.worker.as_mut().unwrap().status = WorkerStatus::Completed;
        });
        assert!(
            f.call("spawn_agent", json!({"title":"Next","task":"Test"}))
                .await
                .is_ok()
        );
        settle(&f, &["child-1"]);
        f.finish().await;
    }
    #[tokio::test]
    async fn nested_director_counts_once_against_its_parent() {
        let f = Fixture::new(Permission::Allow);
        f.change(|s| {
            s.projects[0].defaults.max_workers = 3;
            active_child(s, &f.caller, "nested");
            let mut profile = s.effective_profile(&s.agent_configs[0]).unwrap();
            profile.max_workers = 1;
            let nested = s.sessions.iter_mut().find(|s| s.id == "nested").unwrap();
            nested.profile = Some(profile);
            active_child(s, "nested", "grandchild");
        });
        // The nested director's own slot is full, yet its turn may run.
        assert!(turn_allowed(&f, "nested"));
        assert!(turn_allowed(&f, "grandchild"));
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(snapshot.child_slot("nested", None).is_err());
        assert!(snapshot.child_slot(&f.caller, None).is_ok());
        // With one sibling the parent's three slots are full: the nested
        // director, its grandchild and the sibling. Each may still run.
        f.change(|s| active_child(s, &f.caller, "sibling"));
        assert!(turn_allowed(&f, "nested"));
        assert!(turn_allowed(&f, "sibling"));
        assert!(turn_allowed(&f, &f.caller));
        assert!(
            f.workspace
                .snapshots
                .borrow()
                .child_slot(&f.caller, None)
                .is_err()
        );
        // A fourth active descendant would exceed the parent's limit, so the
        // nested director cannot resume while it is inactive.
        f.change(|s| {
            active_child(s, &f.caller, "extra");
            let nested = s.sessions.iter_mut().find(|s| s.id == "nested").unwrap();
            nested.worker.as_mut().unwrap().status = WorkerStatus::Completed;
        });
        assert!(!turn_allowed(&f, "nested"));
        settle(&f, &["grandchild", "sibling", "extra"]);
        f.finish().await;
    }
    #[tokio::test]
    async fn message_wait_result_round_trip_and_outside_tree_rejection() {
        let f = Fixture::new(Permission::Allow);
        let value = f
            .call("spawn_agent", json!({"title":"Child","task":"Test"}))
            .await
            .unwrap();
        let child = value["agent"].as_str().unwrap().to_owned();
        let child_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        let queued = f
            .call("message_agent", json!({"agent":child,"text":"Next turn"}))
            .await
            .unwrap();
        let submission = queued["submission"].as_str().unwrap();
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(
            snapshot
                .submissions
                .iter()
                .any(|s| s.id == submission && s.state == SubmissionState::Queued)
        );
        assert!(
            snapshot
                .messages
                .iter()
                .any(|m| m.id == format!("agent-message-{submission}")
                    && matches!(
                        m.tool.as_ref().unwrap().detail,
                        Some(ToolDetail::Message { read: false, .. })
                    ))
        );
        f.change(|s| mark_read(s, submission));
        assert!(f.workspace.snapshots.borrow().messages.iter().any(|m| m.id
            == format!("agent-message-{submission}")
            && matches!(
                m.tool.as_ref().unwrap().detail,
                Some(ToolDetail::Message { read: true, .. })
            )));
        let wait = f
            .call("wait_for_agents", json!({"agents":[child],"timeout_s":0}))
            .await
            .unwrap();
        assert_eq!(wait["timed_out"], true);
        call(
            &f.workspace,
            &child,
            &child_run,
            "report_result",
            json!({"summary":"Verified","attachments":["notes.txt"]}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap();
        assert!(
            f.workspace
                .snapshots
                .borrow()
                .messages
                .iter()
                .any(|m| m.session_id == f.caller
                    && m.kind == "agent_result"
                    && m.body.contains("Verified")
                    && m.body.contains("worktree"))
        );
        f.change(|s| {
            s.sessions
                .iter_mut()
                .find(|s| s.id == child)
                .unwrap()
                .worker
                .as_mut()
                .unwrap()
                .status = WorkerStatus::Completed
        });
        let wait = f
            .call("wait_for_agents", json!({"agents":[child],"timeout_s":1}))
            .await
            .unwrap();
        assert_eq!(wait["timed_out"], false);
        assert_eq!(wait["agents"][0]["result"]["summary"], "Verified");
        assert_eq!(wait["agents"][0]["result"]["attachments"][0], "notes.txt");
        f.change(|s| {
            let mut outside = s.sessions.iter().find(|s| s.id == child).unwrap().clone();
            outside.id = "outside".into();
            outside.parent_session_id = None;
            s.sessions.push(outside);
        });
        assert!(
            f.call("message_agent", json!({"agent":"outside","text":"No"}))
                .await
                .is_err()
        );
        assert!(
            f.call("wait_for_agents", json!({"agents":[child],"timeout_s":601}))
                .await
                .is_err()
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn capability_is_hashed_scoped_and_revoked() {
        let f = Fixture::new(Permission::Allow);
        *f.workspace.agent_endpoint.lock().unwrap() = Some("http://127.0.0.1:1".into());
        let mut command = tokio::process::Command::new("unused");
        let guard = configure(
            &f.workspace,
            &f.caller,
            &f.run,
            Harness::Codex,
            &mut command,
        )
        .unwrap();
        let secret = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "RELAY_AGENT_CAPABILITY")
            .unwrap()
            .1
            .unwrap()
            .to_str()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {secret}").parse().unwrap());
        assert!(authenticate(&f.workspace, &headers).is_ok());
        assert!(!format!("{:?}", command.as_std().get_args()).contains(secret));
        assert!(
            !serde_json::to_string(&*f.workspace.snapshots.borrow())
                .unwrap()
                .contains(secret)
        );
        let mut owner = HeaderMap::new();
        owner.insert("authorization", "Bearer test-token".parse().unwrap());
        assert!(authenticate(&f.workspace, &owner).is_err());
        drop(guard);
        assert!(authenticate(&f.workspace, &headers).is_err());
        let guard = configure(
            &f.workspace,
            &f.caller,
            &f.run,
            Harness::ClaudeCode,
            &mut tokio::process::Command::new("unused"),
        )
        .unwrap();
        f.change(|s| s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed);
        assert!(authenticate(&f.workspace, &headers).is_err());
        drop(guard);
        Shutdown {
            workspace: f.workspace.clone(),
        }
        .shutdown()
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn queued_message_is_read_by_the_next_real_harness_turn() {
        let f = Fixture::new(Permission::Allow);
        let native = crate::tests::command_tests::CODEX
            .replace(
                "@CAPTURE@",
                f._directory.path().join("capture").to_str().unwrap(),
            )
            .replace("test-model", "fixture-model");
        // This fixture speaks native RPC rather than the legacy turn wrapper.
        std::fs::write(&f.workspace.config.codex, format!("#!/bin/sh\n{native}")).unwrap();
        let child = f
            .call("spawn_agent", json!({"title":"Child","task":"First"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let draft = Draft {
            session_id: child.clone(),
            revision: 9,
            parts: vec![Part::text("Human draft")],
        };
        {
            let store = f.workspace.store.lock().unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO drafts(session_id,draft) VALUES(?1,?2)",
                    params![child, serde_json::to_string(&draft).unwrap()],
                )
                .unwrap();
        }
        let submission = f
            .call("message_agent", json!({"agent":child,"text":"Second"}))
            .await
            .unwrap()["submission"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            conversation::read_drafts(&f.workspace.store.lock().unwrap().connection).unwrap(),
            vec![draft]
        );
        let mut events = f.workspace.snapshots.subscribe();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = events.borrow().clone();
                let read = snapshot.messages.iter().any(|m| {
                    m.id == format!("agent-message-{submission}")
                        && matches!(
                            m.tool.as_ref().and_then(|t| t.detail.as_ref()),
                            Some(ToolDetail::Message { read: true, .. })
                        )
                });
                let prompted = snapshot
                    .messages
                    .iter()
                    .any(|m| m.id == format!("prompt-{submission}"));
                if read && prompted {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        // Both the spawned task and the queued message keep their sending agent.
        let snapshot = f.workspace.snapshots.borrow().clone();
        let sent_by_caller = Sender::Agent {
            session_id: f.caller.clone(),
        };
        let prompts = snapshot
            .messages
            .iter()
            .filter(|m| m.session_id == child && m.kind == "prompt")
            .collect::<Vec<_>>();
        assert_eq!(prompts.len(), 2);
        assert!(prompts.iter().all(|m| m.sender == sent_by_caller));
        assert!(
            snapshot
                .submissions
                .iter()
                .filter(|s| s.session_id == child)
                .all(|s| s.sender == sent_by_caller)
        );
        assert!(
            snapshot
                .submissions
                .iter()
                .any(|s| s.id == submission && s.sender == sent_by_caller)
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn wait_observes_completion_and_permission_inheritance_has_no_depth_limit() {
        let f = Fixture::new(Permission::Allow);
        let child = f
            .call(
                "spawn_agent",
                json!({"title":"Child","task":"Test","permissions":{"delegate":"deny"}}),
            )
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let child_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        assert!(
            call(
                &f.workspace,
                &child,
                &child_run,
                "spawn_agent",
                json!({"title":"Denied","task":"Test"}),
                &mut f.stop.clone()
            )
            .await
            .is_err()
        );
        let (wait, ()) = tokio::join!(
            f.call("wait_for_agents", json!({"agents":[child],"timeout_s":2})),
            async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                f.change(|s| {
                    s.sessions
                        .iter_mut()
                        .find(|s| s.id == child)
                        .unwrap()
                        .worker
                        .as_mut()
                        .unwrap()
                        .status = WorkerStatus::Completed
                });
            }
        );
        assert_eq!(wait.unwrap()["timed_out"], false);
        let listing = f.call("list_agents", json!({})).await.unwrap();
        assert_eq!(listing["agents"].as_array().unwrap().len(), 2);
        f.finish().await;
    }
    #[tokio::test]
    async fn restart_repairs_all_descendants_and_does_not_restore_capabilities() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut snapshot = crate::tests::live();
        let mut parent = snapshot.sessions[0].clone();
        snapshot.sessions.clear();
        parent.fixture = false;
        parent.worker = Some(WorkerRun {
            launched_network_policy: None,
            status: WorkerStatus::Running,
            harness: Harness::Codex,
            thread_id: None,
            worktree: None,
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
            model: None,
            context_tokens: None,
            context_window: None,
            last_usage: None,
            execution: None,
        });
        for depth in 0..7 {
            let mut session = parent.clone();
            session.id = format!("depth-{depth}");
            session.parent_session_id = (depth > 0).then(|| format!("depth-{}", depth - 1));
            snapshot.sessions.push(session);
        }
        {
            let mut store = Store::open(&path, AgentProfile::default()).unwrap();
            store.save(&snapshot).unwrap();
        }
        let (_, shutdown) = router_with_shutdown(
            &path,
            uuid::Uuid::new_v4().to_string(),
            AgentProfile::default(),
            RuntimeConfig::default(),
        )
        .unwrap();
        assert!(
            shutdown
                .workspace
                .snapshots
                .borrow()
                .sessions
                .iter()
                .all(|s| s.worker.as_ref().unwrap().status == WorkerStatus::Interrupted)
        );
        assert!(
            shutdown
                .workspace
                .store
                .lock()
                .unwrap()
                .agent_capabilities
                .is_empty()
        );
        shutdown.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn stop_cascades_and_metadata_retains_descendant_approvals() {
        let f = Fixture::new(Permission::Allow);
        let child = f
            .call("spawn_agent", json!({"title":"Child","task":"Test"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let child_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        let grandchild = call(
            &f.workspace,
            &child,
            &child_run,
            "spawn_agent",
            json!({"title":"Grandchild","task":"Test"}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        f.change(|s| {
            s.tool_permissions.push(ToolPermission {
                id: "pending-child".into(),
                session_id: grandchild.clone(),
                run_id: "run".into(),
                tool: "Test".into(),
                description: "Test".into(),
                decision: None,
                expired: false,
            })
        });
        assert!(
            transport::metadata(&f.workspace.snapshots.borrow())
                .tool_permissions
                .iter()
                .any(|p| p.session_id == grandchild)
        );
        let queued = f
            .call("message_agent", json!({"agent":child,"text":"Next turn"}))
            .await
            .unwrap()["submission"]
            .as_str()
            .unwrap()
            .to_owned();
        let snapshot = f.workspace.snapshots.borrow().clone();
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StopWorker {
                session_id: f.caller.clone(),
                include_descendants: true,
            },
        );
        let _ = command(
            State(f.workspace.clone()),
            {
                let mut h = HeaderMap::new();
                h.insert("authorization", "Bearer test-token".parse().unwrap());
                h
            },
            Ok(Json(envelope)),
        )
        .await
        .unwrap();
        {
            let store = f.workspace.store.lock().unwrap();
            assert!(*store.controls[&f.caller].borrow());
            assert!(store.controls.get(&child).is_none_or(|c| *c.borrow()));
            assert!(store.controls.get(&grandchild).is_none_or(|c| *c.borrow()));
        }
        assert!(
            f.workspace
                .snapshots
                .borrow()
                .submissions
                .iter()
                .any(|s| s.id == queued && s.state == SubmissionState::Paused)
        );
        f.finish().await;
        assert!(
            f.workspace
                .snapshots
                .borrow()
                .descendants(&f.caller)
                .iter()
                .all(|s| s.worker.as_ref().unwrap().status == WorkerStatus::Stopped)
        );
    }
    #[tokio::test]
    async fn stopping_a_parent_turn_leaves_children_running_and_queued() {
        let f = Fixture::new(Permission::Allow);
        let child = f
            .call("spawn_agent", json!({"title":"Child","task":"Test"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let queued = f
            .call("message_agent", json!({"agent":child,"text":"Next turn"}))
            .await
            .unwrap()["submission"]
            .as_str()
            .unwrap()
            .to_owned();
        let snapshot = f.workspace.snapshots.borrow().clone();
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StopWorker {
                session_id: f.caller.clone(),
                include_descendants: false,
            },
        );
        let _ = command(
            State(f.workspace.clone()),
            {
                let mut h = HeaderMap::new();
                h.insert("authorization", "Bearer test-token".parse().unwrap());
                h
            },
            Ok(Json(envelope)),
        )
        .await
        .unwrap();
        {
            let store = f.workspace.store.lock().unwrap();
            assert!(*store.controls[&f.caller].borrow());
            assert!(!*store.controls[&child].borrow());
        }
        // The child's run task keeps going after the parent's turn stops.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            f.workspace
                .store
                .lock()
                .unwrap()
                .controls
                .get(&child)
                .is_some_and(|c| !*c.borrow())
        );
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(crate::runtime::active(
            &snapshot
                .sessions
                .iter()
                .find(|s| s.id == child)
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .status
        ));
        assert!(
            snapshot
                .submissions
                .iter()
                .any(|s| s.id == queued && s.state == SubmissionState::Queued)
        );
        // An idle parent has no turn to stop; only an explicit stop-all
        // reaches its running children.
        f.change(|s| {
            s.sessions
                .iter_mut()
                .find(|s| s.id == f.caller)
                .unwrap()
                .worker
                .as_mut()
                .unwrap()
                .status = WorkerStatus::Completed
        });
        let stop = |include_descendants| {
            let snapshot = f.workspace.snapshots.borrow().clone();
            command(
                State(f.workspace.clone()),
                {
                    let mut h = HeaderMap::new();
                    h.insert("authorization", "Bearer test-token".parse().unwrap());
                    h
                },
                Ok(Json(crate::tests::env(
                    snapshot.revision,
                    Command::StopWorker {
                        session_id: f.caller.clone(),
                        include_descendants,
                    },
                ))),
            )
        };
        assert!(stop(false).await.is_err());
        assert!(stop(true).await.is_ok());
        assert!(
            f.workspace
                .store
                .lock()
                .unwrap()
                .controls
                .get(&child)
                .is_none_or(|c| *c.borrow())
        );
        assert!(
            f.workspace
                .snapshots
                .borrow()
                .submissions
                .iter()
                .any(|s| s.id == queued && s.state == SubmissionState::Paused)
        );
        Shutdown {
            workspace: f.workspace.clone(),
        }
        .shutdown()
        .await
        .unwrap();
    }
    fn notices(f: &Fixture) -> Vec<Submission> {
        f.workspace
            .snapshots
            .borrow()
            .submissions
            .iter()
            .filter(|s| s.session_id == f.caller && s.id.starts_with(crate::notices::PREFIX))
            .cloned()
            .collect()
    }
    /// Waits for every listed child to end, then past the batching window.
    async fn await_ended(f: &Fixture, children: &[&str]) {
        let mut events = f.workspace.snapshots.subscribe();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let ended = children.iter().all(|id| {
                    events
                        .borrow()
                        .sessions
                        .iter()
                        .find(|s| s.id == *id)
                        .and_then(|s| s.worker.as_ref())
                        .is_some_and(|w| !runtime::active(&w.status))
                });
                if ended {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(2500)).await;
    }
    async fn spawn_child(f: &Fixture, title: &str) -> String {
        f.call("spawn_agent", json!({"title":title,"task":"Test"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    async fn stop_worker(f: &Fixture, session: &str, include_descendants: bool) {
        // Children publish their own launch progress, so retry a stale revision.
        for _ in 0..20 {
            let snapshot = f.workspace.snapshots.borrow().clone();
            let envelope = crate::tests::env(
                snapshot.revision,
                Command::StopWorker {
                    session_id: session.into(),
                    include_descendants,
                },
            );
            let mut headers = HeaderMap::new();
            headers.insert("authorization", "Bearer test-token".parse().unwrap());
            match command(State(f.workspace.clone()), headers, Ok(Json(envelope))).await {
                Err(error) if error.status == StatusCode::CONFLICT => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                result => {
                    let _ = result.unwrap();
                    return;
                }
            }
        }
        panic!("Workspace kept changing");
    }
    #[tokio::test]
    async fn failed_and_completed_children_without_results_notify_the_parent() {
        for status in ["failed", "completed"] {
            let f = Fixture::new(Permission::Allow);
            let native = crate::tests::command_tests::CODEX
                .replace(
                    "@CAPTURE@",
                    f._directory.path().join("capture").to_str().unwrap(),
                )
                .replace("test-model", "fixture-model")
                .replace(
                    r#""id":"test-turn","status":"completed""#,
                    &format!(r#""id":"test-turn","status":"{status}""#),
                );
            std::fs::write(&f.workspace.config.codex, format!("#!/bin/sh\n{native}")).unwrap();
            let child = spawn_child(&f, "Quiet child").await;
            await_ended(&f, &[&child]).await;
            let notices = notices(&f);
            assert_eq!(notices.len(), 1, "{status}");
            // The parent is running, so the notice waits for its next turn.
            assert_eq!(notices[0].state, SubmissionState::Queued);
            let text = plain_text(&notices[0].parts);
            assert!(text.contains("Quiet child"), "{text}");
            assert!(text.contains(&child), "{text}");
            assert!(
                text.contains(&format!("{status}, no result reported")),
                "{text}"
            );
            assert!(text.contains("Worktree: "), "{text}");
            f.finish().await;
        }
    }
    #[tokio::test]
    async fn stopped_children_notify_once_per_batch_unless_they_reported() {
        let f = Fixture::new(Permission::Allow);
        // Only directors are notified of reported results.
        f.change(|s| {
            s.projects[0].defaults.responsibilities = vec![Task::Implement, Task::Verify];
        });
        let first = spawn_child(&f, "First").await;
        let second = spawn_child(&f, "Second").await;
        let reporter = spawn_child(&f, "Reporter").await;
        let reporter_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&reporter)
            .unwrap()
            .unwrap();
        call(
            &f.workspace,
            &reporter,
            &reporter_run,
            "report_result",
            json!({"summary":"Done"}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap();
        stop_worker(&f, &reporter, false).await;
        stop_worker(&f, &first, false).await;
        stop_worker(&f, &second, false).await;
        await_ended(&f, &[&first, &second, &reporter]).await;
        let notices = notices(&f);
        assert_eq!(notices.len(), 1);
        let text = plain_text(&notices[0].parts);
        assert!(text.starts_with("Relay: 2 workers ended"), "{text}");
        assert!(text.contains("First") && text.contains("Second"), "{text}");
        assert!(text.contains("stopped, no result reported"), "{text}");
        assert!(!text.contains("Reporter"), "{text}");
        f.finish().await;
    }
    #[tokio::test]
    async fn ancestor_stops_do_not_notify_per_child() {
        let f = Fixture::new(Permission::Allow);
        let directed = spawn_child(&f, "Directed").await;
        f.call("stop_agent", json!({"agent":directed}))
            .await
            .unwrap();
        await_ended(&f, &[&directed]).await;
        assert!(notices(&f).is_empty());
        let child = spawn_child(&f, "Child").await;
        let child_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        let grandchild = call(
            &f.workspace,
            &child,
            &child_run,
            "spawn_agent",
            json!({"title":"Grandchild","task":"Test"}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        stop_worker(&f, &f.caller, true).await;
        await_ended(&f, &[&child, &grandchild]).await;
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(
            !snapshot
                .submissions
                .iter()
                .any(|s| s.id.starts_with(crate::notices::PREFIX))
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn idle_parent_starts_a_relay_authored_notice_turn() {
        let f = Fixture::new(Permission::Allow);
        let child = spawn_child(&f, "Child").await;
        f.change(|s| {
            let worker = s
                .sessions
                .iter_mut()
                .find(|s| s.id == f.caller)
                .unwrap()
                .worker
                .as_mut()
                .unwrap();
            worker.status = WorkerStatus::Completed;
            // The fixture harness resumes this thread.
            worker.thread_id = Some("thread".into());
        });
        stop_worker(&f, &child, false).await;
        await_ended(&f, &[&child]).await;
        let notices = notices(&f);
        assert_eq!(notices.len(), 1);
        assert_ne!(notices[0].state, SubmissionState::Queued);
        let snapshot = f.workspace.snapshots.borrow().clone();
        let prompt = snapshot
            .messages
            .iter()
            .find(|m| m.id == format!("prompt-{}", notices[0].id))
            .unwrap();
        assert_eq!(
            (prompt.author.as_str(), prompt.kind.as_str()),
            ("Relay", "prompt")
        );
        // The notice has no client receipt; its turn must still launch the
        // harness rather than fail on a missing receipt.
        let parent = settled(&f, &f.caller).await;
        assert_eq!((parent.status, parent.error), (WorkerStatus::Running, None));
        Shutdown {
            workspace: f.workspace.clone(),
        }
        .shutdown()
        .await
        .unwrap();
    }
    /// Waits until a session's turn has launched its harness or ended.
    async fn settled(f: &Fixture, session: &str) -> WorkerRun {
        let mut events = f.workspace.snapshots.subscribe();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let worker = events
                    .borrow()
                    .sessions
                    .iter()
                    .find(|s| s.id == session)
                    .and_then(|s| s.worker.clone())
                    .unwrap();
                if worker.status != WorkerStatus::Queued {
                    return worker;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap()
    }
    fn director(f: &Fixture) {
        f.change(|s| {
            s.projects[0].defaults.responsibilities = vec![Task::Delegate, Task::Verify];
        });
    }
    async fn spawn_grandchild(f: &Fixture, child: &str) -> String {
        let run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(child)
            .unwrap()
            .unwrap();
        call(
            &f.workspace,
            child,
            &run,
            "spawn_agent",
            json!({"title":"Grandchild","task":"Test"}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    #[tokio::test]
    async fn worker_with_active_workers_holds_its_notice_until_they_end() {
        let f = Fixture::new(Permission::Allow);
        let middle = spawn_child(&f, "Middle").await;
        let grandchild = spawn_grandchild(&f, &middle).await;
        stop_worker(&f, &middle, false).await;
        await_ended(&f, &[&middle]).await;
        // Its worker is still running, so it is not reported as ended.
        assert!(notices(&f).is_empty());
        // The director's own stop sends no notice for the grandchild, which
        // releases the held notice for its idle parent.
        f.call("stop_agent", json!({"agent":grandchild}))
            .await
            .unwrap();
        await_ended(&f, &[&grandchild]).await;
        let notices = notices(&f);
        assert_eq!(notices.len(), 1);
        let text = plain_text(&notices[0].parts);
        assert!(
            text.starts_with("Relay: a worker ended without reporting a result."),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "Middle ({middle}): stopped, no result reported; its workers have since ended"
            )),
            "{text}"
        );
        assert!(!text.contains("Grandchild"), "{text}");
        // The top-level director is not asked to report to a parent.
        assert!(!text.contains("call report_result"), "{text}");
        f.finish().await;
    }
    #[tokio::test]
    async fn stop_all_drops_held_notices() {
        let f = Fixture::new(Permission::Allow);
        let middle = spawn_child(&f, "Middle").await;
        let grandchild = spawn_grandchild(&f, &middle).await;
        stop_worker(&f, &middle, false).await;
        await_ended(&f, &[&middle]).await;
        stop_worker(&f, &f.caller, true).await;
        await_ended(&f, &[&grandchild]).await;
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(
            !snapshot
                .submissions
                .iter()
                .any(|s| s.id.starts_with(crate::notices::PREFIX))
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn worker_woken_by_its_own_workers_keeps_its_held_notice() {
        let f = Fixture::new(Permission::Allow);
        let middle = spawn_child(&f, "Middle").await;
        let grandchild = spawn_grandchild(&f, &middle).await;
        stop_worker(&f, &middle, false).await;
        await_ended(&f, &[&middle]).await;
        // A plain stop notifies the grandchild's parent, which is woken
        // instead of being reported to the director as ended.
        stop_worker(&f, &grandchild, false).await;
        await_ended(&f, &[&grandchild]).await;
        assert!(notices(&f).is_empty());
        let snapshot = f.workspace.snapshots.borrow().clone();
        let woken = snapshot
            .submissions
            .iter()
            .find(|s| s.session_id == middle && s.id.starts_with(crate::notices::PREFIX))
            .unwrap();
        let text = plain_text(&woken.parts);
        assert!(text.contains("Grandchild"), "{text}");
        // An intermediate worker is reminded to report to its own parent.
        assert!(
            text.ends_with(
                "When your delegated work is accepted, call report_result to report it to your parent."
            ),
            "{text}"
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn directors_are_notified_when_a_worker_reports() {
        let f = Fixture::new(Permission::Allow);
        director(&f);
        let child = spawn_child(&f, "Reporter").await;
        let run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        call(
            &f.workspace,
            &child,
            &run,
            "report_result",
            json!({"summary":"Implemented and tested"}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap();
        let quiet = spawn_child(&f, "Quiet").await;
        stop_worker(&f, &child, false).await;
        stop_worker(&f, &quiet, false).await;
        await_ended(&f, &[&child, &quiet]).await;
        let notices = notices(&f);
        assert_eq!(notices.len(), 1);
        let text = plain_text(&notices[0].parts);
        assert!(
            text.starts_with("Relay: 2 workers ended their turns."),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "Reporter ({child}): stopped, reported a result\n  Summary: Implemented and tested"
            )),
            "{text}"
        );
        assert!(text.contains("Quiet"), "{text}");
        assert!(text.contains("stopped, no result reported"), "{text}");
        assert!(text.contains("Review each reported result"), "{text}");
        f.finish().await;
    }

    // run_nix uses a synthetic `nix` script; nothing here runs real Nix.
    fn nix(f: &Fixture, body: &str) {
        crate::tests::script(&f._directory.path().join("nix"), body);
    }
    fn nix_commands(f: &Fixture, mode: NixCommands) {
        f.change(|s| s.projects.iter_mut().for_each(|p| p.nix_commands = mode));
    }
    fn set_permission(f: &Fixture, task: Task, permission: Permission) {
        f.change(|s| {
            for project in &mut s.projects {
                project.defaults.permissions.insert(task, permission);
            }
            for session in &mut s.sessions {
                if let Some(profile) = &mut session.profile {
                    profile.permissions.insert(task, permission);
                }
            }
        });
    }
    async fn lists_nix(f: &Fixture) -> bool {
        f.call("tools", json!({})).await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "run_nix")
    }
    fn assigned(f: &Fixture) -> std::path::PathBuf {
        std::fs::canonicalize(f._directory.path().join("repo")).unwrap()
    }
    fn permission_requests(f: &Fixture) -> usize {
        f.workspace.snapshots.borrow().tool_permissions.len()
    }
    fn nix_records(f: &Fixture) -> Vec<Message> {
        f.workspace
            .snapshots
            .borrow()
            .messages
            .iter()
            .filter(|m| m.id.starts_with("nix-command-"))
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn nix_commands_are_off_by_default_and_not_listed() {
        let f = Fixture::new(Permission::Allow);
        let marker = f._directory.path().join("ran");
        nix(&f, &format!("touch '{}'", marker.display()));
        let requests = permission_requests(&f);
        assert!(!lists_nix(&f).await);
        let error = f
            .call("run_nix", json!({"command":["true"]}))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("off for this project"),
            "{error}"
        );
        assert!(!marker.exists());
        assert_eq!(permission_requests(&f), requests);
        assert!(nix_records(&f).is_empty());
        // The setting is a project command; unknown projects are rejected.
        {
            let mut store = f.workspace.store.lock().unwrap();
            let revision = store.snapshot().unwrap().revision;
            assert!(
                store
                    .apply(
                        crate::tests::env(
                            revision,
                            Command::SetNixCommands {
                                project_id: "missing".into(),
                                mode: NixCommands::Allow,
                            },
                        ),
                        &f.workspace.config,
                    )
                    .is_err()
            );
            let (snapshot, _) = store
                .apply(
                    crate::tests::env(
                        revision,
                        Command::SetNixCommands {
                            project_id: "live".into(),
                            mode: NixCommands::Allow,
                        },
                    ),
                    &f.workspace.config,
                )
                .unwrap();
            assert_eq!(snapshot.projects[0].nix_commands, NixCommands::Allow);
            f.workspace.snapshots.send_replace(snapshot);
        }
        assert!(lists_nix(&f).await);
        f.finish().await;
    }

    #[tokio::test]
    async fn allowed_nix_commands_run_in_the_assigned_workspace_with_a_clean_environment() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let root = assigned(&f);
        std::fs::create_dir(root.join("crates")).unwrap();
        nix(
            &f,
            "printf 'args:%s\\n' \"$*\"\nprintf 'pwd:%s\\n' \"$(pwd -P)\"\nprintf 'env:%s\\n' \"$(env | cut -d= -f1 | sort | tr '\\n' ' ')\"\necho on-stderr >&2\nmkdir -p shots && printf png > shots/agents.png\nexit 3",
        );
        let requests = permission_requests(&f);
        let tools = f.call("tools", json!({})).await.unwrap();
        let spec = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "run_nix")
            .unwrap()
            .clone();
        assert_eq!(spec["inputSchema"]["required"], json!(["command"]));
        assert!(
            !spec["description"]
                .as_str()
                .unwrap()
                .contains("human approves")
        );
        let result = f
            .call(
                "run_nix",
                json!({
                    "command": ["cargo", "test", "-p", "relay-ui", "it's"],
                    "directory": "crates",
                    "artifacts": ["crates/shots/agents.png", "missing.png"],
                }),
            )
            .await
            .unwrap();
        // Allow needs no approval; failures are results, not tool errors.
        assert_eq!(permission_requests(&f), requests);
        assert_eq!(result["exit_code"], 3);
        assert_eq!(result["timed_out"], false);
        let output = result["output"].as_str().unwrap();
        assert!(
            output.contains(&format!(
                "args:develop {} --command cargo test -p relay-ui it's",
                root.display()
            )),
            "{output}"
        );
        assert!(
            output.contains(&format!("pwd:{}/crates\n", root.display())),
            "{output}"
        );
        assert!(output.contains("on-stderr"), "{output}");
        assert!(
            result["command"]
                .as_str()
                .unwrap()
                .ends_with("--command cargo test -p relay-ui 'it'\\''s'")
        );
        // Only the allowlisted server environment reaches the command.
        let names = output
            .lines()
            .find_map(|l| l.strip_prefix("env:"))
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        assert!(names.contains(&"PATH"), "{names:?}");
        for name in names {
            assert!(
                crate::nix::ENVIRONMENT.contains(&name)
                    || ["PWD", "OLDPWD", "SHLVL", "_"].contains(&name),
                "{name} leaked into the Nix command"
            );
        }
        assert_eq!(
            result["artifacts"],
            json!([{"path": root.join("crates/shots/agents.png").display().to_string(), "bytes": 3}])
        );
        assert_eq!(result["unavailable_artifacts"][0]["path"], "missing.png");
        // The server records the command and its outcome for review.
        let records = nix_records(&f);
        assert_eq!(records.len(), 1);
        let tool = records[0].tool.as_ref().unwrap();
        assert_eq!(tool.name, "mcp__relay__run_nix");
        assert_eq!(tool.kind, ToolKind::Run);
        assert_eq!(tool.state, ToolState::Failed);
        assert_eq!(tool.exit_code, Some(3));
        assert_eq!(
            tool.directory.as_deref(),
            Some(root.join("crates").display().to_string().as_str())
        );
        assert!(records[0].body.contains("Exit: 3"), "{}", records[0].body);
        assert!(records[0].body.contains("on-stderr"));
        // A named development shell of the same flake.
        let result = f
            .call("run_nix", json!({"command":["true"],"shell":"ui-test"}))
            .await
            .unwrap();
        assert!(
            result["output"].as_str().unwrap().contains(&format!(
                "args:develop {}#ui-test --command true",
                root.display()
            )),
            "{result}"
        );
        f.finish().await;
    }

    #[tokio::test]
    async fn ask_requires_human_approval_for_each_command() {
        for (setting, verify, implement) in [
            (NixCommands::Ask, Permission::Allow, Permission::Allow),
            (NixCommands::Allow, Permission::Ask, Permission::Allow),
            (NixCommands::Allow, Permission::Allow, Permission::Ask),
        ] {
            let f = Fixture::new(Permission::Allow);
            nix_commands(&f, setting);
            set_permission(&f, Task::Verify, verify);
            set_permission(&f, Task::Implement, implement);
            let marker = f._directory.path().join("ran");
            nix(&f, &format!("touch '{}'", marker.display()));
            assert!(lists_nix(&f).await);
            let (denied, ()) = tokio::join!(
                f.call("run_nix", json!({"command":["just","test"]})),
                decide(
                    &f.workspace,
                    &f.caller,
                    &f.run,
                    false,
                    "outside the agent sandbox"
                )
            );
            assert!(
                denied
                    .unwrap_err()
                    .to_string()
                    .contains("Human denied the Nix command")
            );
            assert!(!marker.exists());
            let (allowed, ()) = tokio::join!(
                f.call("run_nix", json!({"command":["just","test"]})),
                decide(&f.workspace, &f.caller, &f.run, true, "--command just test")
            );
            assert_eq!(allowed.unwrap()["exit_code"], 0);
            assert!(marker.exists());
            f.finish().await;
        }
    }

    #[tokio::test]
    async fn profiles_that_deny_verify_or_implement_cannot_run_nix_commands() {
        for task in [Task::Verify, Task::Implement] {
            let f = Fixture::new(Permission::Allow);
            nix_commands(&f, NixCommands::Allow);
            set_permission(&f, task, Permission::Deny);
            let marker = f._directory.path().join("ran");
            nix(&f, &format!("touch '{}'", marker.display()));
            let requests = permission_requests(&f);
            assert!(!lists_nix(&f).await);
            let error = f
                .call("run_nix", json!({"command":["true"]}))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("profile denies"), "{error}");
            assert!(!marker.exists());
            assert_eq!(permission_requests(&f), requests);
            f.finish().await;
        }
    }

    #[tokio::test]
    async fn nix_commands_stay_inside_assigned_workspaces() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let root = assigned(&f);
        let marker = f._directory.path().join("ran");
        nix(&f, &format!("touch '{}'", marker.display()));
        // Another agent's workspace and an unrelated directory.
        let other = f._directory.path().join("other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("secret"), "synthetic").unwrap();
        f.change(|s| {
            let mut sibling = s.sessions[0].clone();
            sibling.id = "other-agent".into();
            sibling.workspaces[0].path = other.display().to_string();
            sibling.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            s.sessions.push(sibling);
        });
        std::os::unix::fs::symlink(&other, root.join("link")).unwrap();
        for (args, error) in [
            (
                json!({"command":["true"],"workspace":other.display().to_string()}),
                "not assigned",
            ),
            (
                json!({"command":["true"],"workspace":"repo"}),
                "not assigned",
            ),
            (json!({"command":["true"],"workspace":"/"}), "not assigned"),
            (
                json!({"command":["true"],"directory":"../other"}),
                "without ..",
            ),
            (
                json!({"command":["true"],"directory":"crates/../../other"}),
                "without ..",
            ),
            (
                json!({"command":["true"],"directory":other.display().to_string()}),
                "without ..",
            ),
            (
                json!({"command":["true"],"directory":"link"}),
                "leaves the workspace",
            ),
            (
                json!({"command":["true"],"artifacts":["../other/secret"]}),
                "without ..",
            ),
            (json!({"command":[]}), "1–256 arguments"),
            (json!({"command":[""]}), "1–256 arguments"),
            (
                json!({"command":["true"],"shell":"ui-test#evil"}),
                "development shell name",
            ),
            (
                json!({"command":["true"],"shell":"../other"}),
                "development shell name",
            ),
            (json!({"command":["true"],"timeout_s":0}), "timeout_s"),
            (json!({"command":["true"],"timeout_s":601}), "timeout_s"),
            (json!({"command":"true"}), "Invalid tool arguments"),
            (
                json!({"command":["true"],"env":{"A":"B"}}),
                "Invalid tool arguments",
            ),
        ] {
            let message = f
                .call("run_nix", args.clone())
                .await
                .unwrap_err()
                .to_string();
            assert!(message.contains(error), "{args}: {message}");
        }
        assert!(!marker.exists());
        // An assigned workspace may be named by any path that resolves to it,
        // and artifacts that resolve outside it are not returned.
        let result = f
            .call(
                "run_nix",
                json!({
                    "command": ["true"],
                    "workspace": root.join(".").display().to_string(),
                    "artifacts": ["link/secret"],
                }),
            )
            .await
            .unwrap();
        assert!(marker.exists());
        assert_eq!(result["artifacts"], json!([]));
        assert_eq!(
            result["unavailable_artifacts"],
            json!([{"path":"link/secret","reason":"Path leaves the workspace"}])
        );
        assert!(!result.to_string().contains("synthetic"));
        f.finish().await;
    }

    /// A synthetic command that leaves a background child in its group,
    /// writes the child's PID to `pid` and then runs `then` (`wait` keeps the
    /// leader alive; `exit 0` ends it first).
    #[cfg(target_os = "linux")]
    fn background_nix(f: &Fixture, then: &str) -> std::path::PathBuf {
        let pidfile = f._directory.path().join("pid");
        nix(
            f,
            &format!("sleep 30 &\necho $! > '{}'\n{then}", pidfile.display()),
        );
        pidfile
    }
    #[cfg(target_os = "linux")]
    async fn background_pid(pidfile: &std::path::Path) -> u32 {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !std::fs::read_to_string(pidfile).is_ok_and(|s| s.ends_with('\n')) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        std::fs::read_to_string(pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
    /// Waits until `pid` has exited (gone, or a zombie awaiting its reaper).
    #[cfg(target_os = "linux")]
    async fn assert_ended(pid: u32) {
        let ended = || match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
            Err(_) => true,
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ended() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn stopping_the_run_terminates_the_nix_command_and_its_descendants() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let pidfile = background_nix(&f, "wait");
        let stopper = async {
            let pid = background_pid(&pidfile).await;
            f.workspace.store.lock().unwrap().stop_control(&f.caller);
            pid
        };
        let started = std::time::Instant::now();
        let (result, pid) = tokio::join!(
            f.call("run_nix", json!({"command":["sleep","30"]})),
            stopper
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        // The background child in the command's process group is gone.
        assert_ended(pid).await;
        assert!(f.workspace.store.lock().unwrap().nix_runs.is_empty());
        f.finish().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_leader_that_exits_first_still_has_its_group_ended() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        // The leader exits at once, leaving a background child that holds
        // its output open.
        let pidfile = background_nix(&f, "exit 0");
        let started = std::time::Instant::now();
        let result = f
            .call("run_nix", json!({"command":["true"]}))
            .await
            .unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_ended(background_pid(&pidfile).await).await;
        let records = nix_records(&f);
        assert_eq!(
            records[0].tool.as_ref().unwrap().state,
            ToolState::Completed
        );
        assert!(f.workspace.store.lock().unwrap().nix_runs.is_empty());
        f.finish().await;
    }

    #[tokio::test]
    async fn an_unsaved_audit_record_is_reported_and_saved_when_storage_recovers() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let go = f._directory.path().join("go");
        nix(
            &f,
            &format!("while [ ! -e '{}' ]; do sleep 0.05; done", go.display()),
        );
        // A second connection to the workspace database injects the failure.
        let database = rusqlite::Connection::open(f._directory.path().join("db")).unwrap();
        let breaker = async {
            tokio::time::timeout(Duration::from_secs(10), async {
                while nix_records(&f).is_empty() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            let stored: String = database
                .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            database
                .execute("UPDATE workspace SET snapshot='unreadable' WHERE id=1", [])
                .unwrap();
            std::fs::write(&go, "").unwrap();
            stored
        };
        let (result, stored) =
            tokio::join!(f.call("run_nix", json!({"command":["true"]})), breaker);
        // The command ran, but the call does not report plain success.
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("could not save its transcript record"),
            "{error}"
        );
        assert!(error.contains("Exit: 0"), "{error}");
        assert_eq!(
            nix_records(&f)[0].tool.as_ref().unwrap().state,
            ToolState::Running
        );
        // Once storage recovers, the retry saves the real outcome.
        database
            .execute("UPDATE workspace SET snapshot=?1 WHERE id=1", [&stored])
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while nix_records(&f)[0].tool.as_ref().unwrap().state != ToolState::Completed {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        assert!(nix_records(&f)[0].body.contains("Exit: 0"));
        f.finish().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_dropped_call_kills_the_command_group_and_finalizes_its_record() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let pidfile = background_nix(&f, "wait");
        // Dropping the call, as when its request is abandoned, must not rely
        // on Linux-only process identity to end the group.
        let pid = tokio::select! {
            result = f.call("run_nix", json!({"command":["sleep","30"]})) => panic!("{result:?}"),
            pid = background_pid(&pidfile) => pid,
        };
        assert_ended(pid).await;
        let records = nix_records(&f);
        assert_eq!(records.len(), 1);
        let tool = records[0].tool.as_ref().unwrap();
        assert_eq!(tool.state, ToolState::Interrupted);
        assert!(tool.finished_ms.is_some());
        assert!(
            records[0]
                .body
                .contains("tool call ended before the command finished"),
            "{}",
            records[0].body
        );
        assert!(f.workspace.store.lock().unwrap().nix_runs.is_empty());
        f.finish().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_audit_record_is_written_before_launch_and_survives_the_run_ending() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        let pidfile = background_nix(&f, "wait");
        let ender = async {
            let pid = background_pid(&pidfile).await;
            // Recorded as Running before the command started.
            let records = nix_records(&f);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool.as_ref().unwrap().state, ToolState::Running);
            assert!(records[0].body.contains("--command sleep 30"));
            // The run ends while the command is still running.
            f.change(|s| {
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == f.caller)
                    .unwrap()
                    .worker
                    .as_mut()
                    .unwrap()
                    .status = WorkerStatus::Completed
            });
            pid
        };
        let (result, pid) =
            tokio::join!(f.call("run_nix", json!({"command":["sleep","30"]})), ender);
        assert!(result.is_err());
        assert_ended(pid).await;
        // The record still gets its outcome, in place.
        let records = nix_records(&f);
        assert_eq!(records.len(), 1);
        let tool = records[0].tool.as_ref().unwrap();
        assert_eq!(tool.state, ToolState::Interrupted);
        assert!(tool.finished_ms.is_some());
        assert!(
            records[0].body.contains("Stopped with the agent's run"),
            "{}",
            records[0].body
        );
        // The ended run still accepts no ordinary transcript updates.
        let messages = f.workspace.snapshots.borrow().messages.len();
        assert!(
            f.workspace
                .update_run(&f.caller, &f.run, |s| {
                    s.messages.clear();
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(f.workspace.snapshots.borrow().messages.len(), messages);
        Shutdown {
            workspace: f.workspace.clone(),
        }
        .shutdown()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn stops_and_policy_changes_before_launch_prevent_the_command() {
        for change in ["stop", "shutdown", "off", "deny"] {
            let f = Fixture::new(Permission::Allow);
            nix_commands(&f, NixCommands::Allow);
            let marker = f._directory.path().join("ran");
            nix(&f, &format!("touch '{}'", marker.display()));
            let (workspace, caller, run) = (f.workspace.clone(), f.caller.clone(), f.run.clone());
            // Runs after the command is built and before the final checks.
            crate::nix::BEFORE_LAUNCH.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || match change {
                    "stop" => workspace.store.lock().unwrap().stop_control(&caller),
                    "shutdown" => workspace.store.lock().unwrap().closing = true,
                    _ => workspace
                        .update_run(&caller, &run, |s| {
                            for project in &mut s.projects {
                                if change == "off" {
                                    project.nix_commands = NixCommands::Off;
                                } else {
                                    project
                                        .defaults
                                        .permissions
                                        .insert(Task::Implement, Permission::Deny);
                                }
                            }
                            for session in &mut s.sessions {
                                if let Some(profile) = &mut session.profile {
                                    profile
                                        .permissions
                                        .insert(Task::Implement, Permission::Deny);
                                }
                            }
                            Ok(())
                        })
                        .unwrap(),
                }));
            });
            let error = f
                .call("run_nix", json!({"command":["true"]}))
                .await
                .unwrap_err()
                .to_string();
            let expected = if matches!(change, "off" | "deny") {
                "policy changed"
            } else {
                "capability"
            };
            assert!(error.contains(expected), "{change}: {error}");
            assert!(!marker.exists(), "{change}");
            assert!(nix_records(&f).is_empty(), "{change}");
            assert!(f.workspace.store.lock().unwrap().nix_runs.is_empty());
            assert!(crate::nix::BEFORE_LAUNCH.with(|hook| hook.borrow().is_none()));
            f.finish().await;
        }
    }

    #[tokio::test]
    async fn commands_too_long_to_show_for_approval_are_refused() {
        for setting in [NixCommands::Ask, NixCommands::Allow] {
            let f = Fixture::new(Permission::Allow);
            nix_commands(&f, setting);
            let marker = f._directory.path().join("ran");
            nix(&f, &format!("touch '{}'", marker.display()));
            let requests = permission_requests(&f);
            // A benign prefix fills the approval, and the operation follows it.
            let hidden = format!(
                "echo {}; touch after-limit",
                "a".repeat(crate::harness::DESCRIPTION_LIMIT)
            );
            let error = f
                .call("run_nix", json!({"command":["sh","-c",hidden]}))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("too long to show in full"),
                "{error}"
            );
            assert!(!marker.exists());
            assert_eq!(permission_requests(&f), requests);
            assert!(nix_records(&f).is_empty());
            if setting == NixCommands::Ask {
                // A command that fits is shown to the approver in full.
                let shown = format!("echo {}; touch after-limit", "a".repeat(15 * 1024));
                let (result, ()) = tokio::join!(
                    f.call("run_nix", json!({"command":["sh","-c",shown]})),
                    decide(
                        &f.workspace,
                        &f.caller,
                        &f.run,
                        true,
                        "; touch after-limit'"
                    )
                );
                assert_eq!(result.unwrap()["exit_code"], 0);
                assert!(marker.exists());
            }
            f.finish().await;
        }
    }

    #[tokio::test]
    async fn nix_commands_time_out_and_bound_their_output() {
        let f = Fixture::new(Permission::Allow);
        nix_commands(&f, NixCommands::Allow);
        nix(&f, "sleep 30");
        let started = std::time::Instant::now();
        let result = f
            .call("run_nix", json!({"command":["sleep","30"],"timeout_s":1}))
            .await
            .unwrap();
        assert_eq!(result["timed_out"], true);
        assert_eq!(result["exit_code"], Value::Null);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(f.workspace.store.lock().unwrap().nix_runs.is_empty());

        nix(&f, "head -c 200000 /dev/zero | tr '\\0' a\necho\necho END");
        let result = f
            .call("run_nix", json!({"command":["cargo","build"]}))
            .await
            .unwrap();
        assert_eq!(result["output_bytes"], 200_005);
        assert_eq!(result["output_truncated"], true);
        let output = result["output"].as_str().unwrap();
        assert!(output.len() < 33 * 1024, "{}", output.len());
        assert!(output.starts_with("aaaa"));
        assert!(output.contains("bytes of output omitted"));
        assert!(output.ends_with("END\n"));
        let records = nix_records(&f);
        assert_eq!(records.len(), 2);
        assert!(records[1].body.len() < 34 * 1024);
        assert_eq!(
            records[1].tool.as_ref().unwrap().state,
            ToolState::Completed
        );
        f.finish().await;
    }
    fn mention(session_id: &str) -> Part {
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::SessionMention {
                mention: SessionMention {
                    session_id: session_id.into(),
                    label: mention_label("Other", session_id),
                },
            },
        }
    }
    /// A running root session of a second project, able to call agent tools.
    fn other_project_session(f: &Fixture, id: &str) -> String {
        let run = format!("{id}-run");
        f.change(|s| {
            if !s.projects.iter().any(|p| p.id == "other") {
                let mut project = s.projects[0].clone();
                project.id = "other".into();
                project.name = "Other".into();
                s.projects.push(project);
                let mut issue = s.issues[0].clone();
                issue.id = "other-issue".into();
                issue.project_id = "other".into();
                s.issues.push(issue);
                let mut config = s.agent_configs[0].clone();
                config.id = "other-config".into();
                config.project_id = "other".into();
                s.agent_configs.push(config);
            }
            let mut session = s
                .sessions
                .iter()
                .find(|s| s.id == f.caller)
                .unwrap()
                .clone();
            session.id = id.into();
            session.title = format!("Session {id}");
            session.project_id = "other".into();
            session.issue_id = Some("other-issue".into());
            session.agent_config_id = "other-config".into();
            session.parent_session_id = None;
            // Its own directory, so the two running sessions share no workspace.
            let directory = f._directory.path().join(id);
            std::fs::create_dir_all(&directory).unwrap();
            session.workspaces = vec![SessionWorkspace {
                connection_id: String::new(),
                path: directory.display().to_string(),
                repository: false,
                branch: None,
                base_commit: None,
                changes: None,
            }];
            session.worker.as_mut().unwrap().worktree = Some(directory.display().to_string());
            s.sessions.push(session);
        });
        let mut store = f.workspace.store.lock().unwrap();
        store
            .connection
            .execute(
                "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                params![id, run],
            )
            .unwrap();
        store.controls.insert(id.into(), watch::channel(false).0);
        run
    }
    fn set_draft(f: &Fixture, session: &str, parts: &[Part]) -> u64 {
        let store = f.workspace.store.lock().unwrap();
        let revision = conversation::read_drafts(&store.connection)
            .unwrap()
            .into_iter()
            .find(|d| d.session_id == session)
            .map_or(0, |d| d.revision);
        let draft = Draft {
            session_id: session.into(),
            revision,
            parts: parts.to_vec(),
        };
        store
            .connection
            .execute(
                "INSERT OR REPLACE INTO drafts(session_id,draft) VALUES(?1,?2)",
                params![session, serde_json::to_string(&draft).unwrap()],
            )
            .unwrap();
        revision
    }
    /// Apply a workspace client's command through the normal command path.
    fn human(f: &Fixture, envelope: CommandEnvelope) -> Result<Snapshot, Error> {
        let mut store = f.workspace.store.lock().unwrap();
        let (snapshot, _) = store.apply(envelope, &f.workspace.config)?;
        f.workspace.snapshots.send_replace(snapshot.clone());
        Ok(snapshot)
    }
    fn human_send(f: &Fixture, session: &str, parts: Vec<Part>) -> Result<Snapshot, Error> {
        let revision = set_draft(f, session, &parts);
        let current = f.workspace.snapshots.borrow().revision;
        human(
            f,
            crate::tests::env(
                current,
                Command::SubmitTurn {
                    session_id: session.into(),
                    draft_revision: revision,
                    parts,
                    approve_implementation: true,
                },
            ),
        )
    }
    fn peers(f: &Fixture) -> Vec<SessionPeer> {
        f.workspace.snapshots.borrow().session_peers.clone()
    }
    async fn message(f: &Fixture, from: &str, run: &str, to: &str) -> Result<Value, Error> {
        call(
            &f.workspace,
            from,
            run,
            "message_agent",
            json!({"agent":to,"text":"Hello"}),
            &mut f.stop.clone(),
        )
        .await
    }

    #[tokio::test]
    async fn accepted_human_mention_lets_two_projects_message_each_other_only() {
        let f = Fixture::new(Permission::Allow);
        let other_run = other_project_session(&f, "other-agent");
        let target = "other-agent";
        assert!(message(&f, &f.caller, &f.run, target).await.is_err());
        assert!(message(&f, target, &other_run, &f.caller).await.is_err());
        // Drafts never grant.
        let parts = vec![Part::text("Ask "), mention(target)];
        let _ = conversation::save_draft(
            State(f.workspace.clone()),
            axum::extract::Path(f.caller.clone()),
            Json(SaveDraft {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                parts: parts.clone(),
            }),
        )
        .await
        .unwrap();
        assert!(peers(&f).is_empty());
        // A send that does not match the shared draft grants nothing.
        set_draft(&f, &f.caller, &[Part::text("Other text")]);
        let current = f.workspace.snapshots.borrow().revision;
        let conflict = human(
            &f,
            crate::tests::env(
                current,
                Command::SubmitTurn {
                    session_id: f.caller.clone(),
                    draft_revision: 1,
                    parts: parts.clone(),
                    approve_implementation: true,
                },
            ),
        );
        assert!(conflict.is_err());
        assert!(peers(&f).is_empty());
        // Self, missing, archived and fixture targets reject the whole send.
        f.change(|s| {
            let mut archived = s.sessions.iter().find(|s| s.id == target).unwrap().clone();
            archived.id = "archived-agent".into();
            archived.archived = true;
            archived.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            s.sessions.push(archived);
            let mut fixture = s.sessions.iter().find(|s| s.id == target).unwrap().clone();
            fixture.id = "fixture-agent".into();
            fixture.fixture = true;
            fixture.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            s.sessions.push(fixture);
        });
        for stale in [
            f.caller.as_str(),
            "missing",
            "archived-agent",
            "fixture-agent",
        ] {
            let error = human_send(&f, &f.caller, vec![mention(target), mention(stale)])
                .err()
                .unwrap_or_else(|| panic!("{stale} was accepted"));
            assert!(error.message.contains("Remove the mention"), "{stale}");
        }
        assert!(peers(&f).is_empty());
        // Typed or quoted reference text is plain text.
        let forged = mention(target).kind;
        let PartKind::SessionMention { mention: forged } = forged else {
            unreachable!()
        };
        let body = forged.reference();
        f.change(|s| {
            s.messages.push(Message {
                id: "quoted".into(),
                session_id: f.caller.clone(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "agent".into(),
                body: body.clone(),
                parts: vec![],
                tool: None,
            })
        });
        let reply = Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Reply {
                anchor: Anchor {
                    message_id: "quoted".into(),
                    start: 0,
                    end: body.len(),
                    quote: body.clone(),
                },
                parts: vec![Part::text("Agreed")],
            },
        };
        human_send(&f, &f.caller, vec![Part::text(&body), reply]).unwrap();
        assert!(peers(&f).is_empty());
        assert!(message(&f, &f.caller, &f.run, target).await.is_err());
        // An agent-sent turn can never carry a mention.
        {
            let mut store = f.workspace.store.lock().unwrap();
            let mut snapshot = store.snapshot().unwrap();
            let transaction = store.connection.transaction().unwrap();
            let parts = vec![mention(target)];
            let draft = Draft {
                session_id: f.caller.clone(),
                revision: 0,
                parts: parts.clone(),
            };
            transaction
                .execute(
                    "INSERT OR REPLACE INTO drafts(session_id,draft) VALUES(?1,?2)",
                    params![f.caller, serde_json::to_string(&draft).unwrap()],
                )
                .unwrap();
            let error = conversation::apply(
                &transaction,
                &mut snapshot,
                &uuid::Uuid::new_v4().to_string(),
                Command::SubmitTurn {
                    session_id: f.caller.clone(),
                    draft_revision: 0,
                    parts,
                    approve_implementation: true,
                },
                &f.workspace.config,
                &HashMap::new(),
                Sender::Agent {
                    session_id: target.into(),
                },
            )
            .err()
            .unwrap();
            assert!(error.message.contains("Only people"));
            assert!(snapshot.session_peers.is_empty());
        }
        // An accepted human message grants both directions, once.
        let revision = set_draft(&f, &f.caller, &parts);
        let current = f.workspace.snapshots.borrow().revision;
        let envelope = crate::tests::env(
            current,
            Command::SubmitTurn {
                session_id: f.caller.clone(),
                draft_revision: revision,
                parts: parts.clone(),
                approve_implementation: true,
            },
        );
        human(&f, envelope.clone()).unwrap();
        human(&f, envelope.clone()).unwrap();
        human_send(&f, &f.caller, vec![mention(target)]).unwrap();
        let granted = peers(&f);
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].request_id, envelope.request_id);
        assert_eq!(granted[0].source_session_id, f.caller);
        let mut pair = [f.caller.clone(), target.to_owned()];
        pair.sort();
        assert_eq!(granted[0].sessions, pair);
        assert!(message(&f, &f.caller, &f.run, target).await.is_ok());
        assert!(message(&f, target, &other_run, &f.caller).await.is_ok());
        let snapshot = f.workspace.snapshots.borrow().clone();
        assert!(snapshot.submissions.iter().any(|s| s.session_id == target
            && s.sender
                == Sender::Agent {
                    session_id: f.caller.clone()
                }));
        // The peer is listed with its stable ID and nothing about its work.
        let listed = f.call("list_agents", json!({})).await.unwrap();
        let peer = listed["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == target)
            .unwrap()
            .clone();
        assert_eq!(
            peer,
            json!({"id":target,"title":"Session other-agent","project":"Other","status":"running","relationship":"peer"})
        );
        // Messaging is the only right a peer gets.
        for (tool, args) in [
            ("wait_for_agents", json!({"agents":[target],"timeout_s":0})),
            ("stop_agent", json!({"agent":target})),
            ("archive_agent", json!({"agent":target})),
        ] {
            assert!(f.call(tool, args).await.is_err(), "{tool}");
        }
        // No transitivity, and a peer's grant is not inherited by children.
        let third_run = other_project_session(&f, "third-agent");
        human_send(&f, target, vec![mention("third-agent")]).unwrap();
        assert!(message(&f, target, &other_run, "third-agent").await.is_ok());
        assert!(message(&f, &f.caller, &f.run, "third-agent").await.is_err());
        assert!(
            message(&f, "third-agent", &third_run, &f.caller)
                .await
                .is_err()
        );
        let child = f
            .call("spawn_agent", json!({"title":"Child","task":"Test"}))
            .await
            .unwrap()["agent"]
            .as_str()
            .unwrap()
            .to_owned();
        let child_run = f
            .workspace
            .store
            .lock()
            .unwrap()
            .run_id(&child)
            .unwrap()
            .unwrap();
        assert!(message(&f, &child, &child_run, target).await.is_err());
        let listed = call(
            &f.workspace,
            &child,
            &child_run,
            "list_agents",
            json!({}),
            &mut f.stop.clone(),
        )
        .await
        .unwrap();
        assert!(
            !listed["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["relationship"] == "peer")
        );
        settle(&f, &[target, "third-agent"]);
        f.finish().await;
    }

    #[tokio::test]
    async fn queued_mentions_grant_at_acceptance_and_edits_add_without_revoking() {
        let f = Fixture::new(Permission::Allow);
        other_project_session(&f, "first-agent");
        other_project_session(&f, "second-agent");
        // The caller is running, so the human message queues.
        let snapshot = human_send(&f, &f.caller, vec![mention("first-agent")]).unwrap();
        let queued = snapshot.submissions.last().unwrap().clone();
        assert_eq!(queued.state, SubmissionState::Queued);
        assert!(snapshot.is_peer(&f.caller, "first-agent"));
        let edit = |parts: Vec<Part>| {
            let revision = set_draft(&f, &f.caller, &parts);
            let current = f.workspace.snapshots.borrow().revision;
            human(
                &f,
                crate::tests::env(
                    current,
                    Command::EditQueuedTurn {
                        submission_id: queued.id.clone(),
                        draft_revision: revision,
                        parts,
                        approve_implementation: true,
                    },
                ),
            )
        };
        f.change(|s| {
            let session = s
                .sessions
                .iter_mut()
                .find(|s| s.id == "second-agent")
                .unwrap();
            session.archived = true;
        });
        assert!(edit(vec![mention("second-agent")]).is_err());
        f.change(|s| {
            let session = s
                .sessions
                .iter_mut()
                .find(|s| s.id == "second-agent")
                .unwrap();
            session.archived = false;
        });
        let snapshot = edit(vec![mention("second-agent")]).unwrap();
        assert!(snapshot.is_peer(&f.caller, "first-agent"));
        assert!(snapshot.is_peer(&f.caller, "second-agent"));
        let current = snapshot.revision;
        let snapshot = human(
            &f,
            crate::tests::env(
                current,
                Command::CancelTurn {
                    submission_id: queued.id.clone(),
                },
            ),
        )
        .unwrap();
        assert_eq!(snapshot.session_peers.len(), 2);
        settle(&f, &["first-agent", "second-agent"]);
        f.finish().await;
    }

    #[tokio::test]
    async fn peer_messages_keep_target_permissions_and_recheck_the_grant_after_approval() {
        let f = Fixture::new(Permission::Allow);
        let target = "other-agent";
        other_project_session(&f, target);
        human_send(&f, &f.caller, vec![mention(target)]).unwrap();
        let set = |permission| {
            f.change(|s| {
                s.projects
                    .iter_mut()
                    .find(|p| p.id == "other")
                    .unwrap()
                    .defaults
                    .permissions
                    .insert(Task::Implement, permission);
            })
        };
        set(Permission::Deny);
        let error = message(&f, &f.caller, &f.run, target).await.unwrap_err();
        assert!(error.message.contains("denies implementation"));
        set(Permission::Ask);
        let (result, ()) = tokio::join!(
            message(&f, &f.caller, &f.run, target),
            decide(&f.workspace, &f.caller, &f.run, true, "message turn")
        );
        assert!(result.is_ok());
        // The target is archived while approval is pending: its grant is gone.
        let (result, ()) = tokio::join!(message(&f, &f.caller, &f.run, target), async {
            decide(&f.workspace, &f.caller, &f.run, true, "message turn").await;
            f.change(|s| {
                let session = s.sessions.iter_mut().find(|s| s.id == target).unwrap();
                session.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            });
            let current = f.workspace.snapshots.borrow().revision;
            human(
                &f,
                crate::tests::env(
                    current,
                    Command::SetSessionArchived {
                        session_id: target.into(),
                        archived: true,
                    },
                ),
            )
            .unwrap();
        });
        let error = result.unwrap_err();
        assert!(
            error.message.contains("not a mentioned peer"),
            "{}",
            error.message
        );
        assert!(peers(&f).is_empty());
        // Unarchiving does not restore the grant.
        let current = f.workspace.snapshots.borrow().revision;
        human(
            &f,
            crate::tests::env(
                current,
                Command::SetSessionArchived {
                    session_id: target.into(),
                    archived: false,
                },
            ),
        )
        .unwrap();
        assert!(peers(&f).is_empty());
        assert!(message(&f, &f.caller, &f.run, target).await.is_err());
        f.finish().await;
    }

    #[tokio::test]
    async fn first_root_message_grants_the_new_session() {
        let f = Fixture::new(Permission::Allow);
        let target = "other-agent";
        other_project_session(&f, target);
        let root = f._directory.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        f.change(|s| s.projects[0].root = Some(root.display().to_string()));
        let snapshot = f.workspace.snapshots.borrow().clone();
        let parts = vec![Part::text("Coordinate with "), mention(target)];
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StartRootAgent {
                agent_config_id: snapshot.agent_configs[0].id.clone(),
                issue_id: None,
                prompt: plain_text(&parts),
                parts: parts.clone(),
                approve_implementation: false,
            },
        );
        let session = format!("session-{}", envelope.request_id);
        let snapshot = human(&f, envelope.clone()).unwrap();
        assert!(snapshot.is_peer(&session, target));
        assert_eq!(snapshot.session_peers[0].source_session_id, session);
        assert_eq!(snapshot.session_peers[0].request_id, envelope.request_id);
        // A stale mention rejects the first message and creates nothing.
        let parts = vec![mention("missing")];
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StartRootAgent {
                agent_config_id: snapshot.agent_configs[0].id.clone(),
                issue_id: None,
                prompt: plain_text(&parts),
                parts,
                approve_implementation: false,
            },
        );
        assert!(human(&f, envelope.clone()).is_err());
        assert!(
            !f.workspace
                .snapshots
                .borrow()
                .sessions
                .iter()
                .any(|s| s.id == format!("session-{}", envelope.request_id))
        );
        settle(&f, &[target, &session]);
        f.finish().await;
    }

    #[tokio::test]
    async fn only_the_human_edit_entrypoint_grants_even_on_agent_sent_queued_turns() {
        let f = Fixture::new(Permission::Allow);
        let peer_run = other_project_session(&f, "peer-agent");
        other_project_session(&f, "third-agent");
        human_send(&f, &f.caller, vec![mention("peer-agent")]).unwrap();
        // The peer's message queues on the running caller as an agent-sent turn.
        let queued = message(&f, "peer-agent", &peer_run, &f.caller)
            .await
            .unwrap()["submission"]
            .as_str()
            .unwrap()
            .to_owned();
        let snapshot = f.workspace.snapshots.borrow().clone();
        let turn = snapshot
            .submissions
            .iter()
            .find(|s| s.id == queued)
            .unwrap();
        assert_eq!(turn.state, SubmissionState::Queued);
        assert_eq!(
            turn.sender,
            Sender::Agent {
                session_id: "peer-agent".into()
            }
        );
        let parts = vec![Part::text("Also ask "), mention("third-agent")];
        let edit = |revision| Command::EditQueuedTurn {
            submission_id: queued.clone(),
            draft_revision: revision,
            parts: parts.clone(),
            approve_implementation: true,
        };
        // The same edit applied as an agent is rejected and grants nothing.
        {
            let revision = set_draft(&f, &f.caller, &parts);
            let mut store = f.workspace.store.lock().unwrap();
            let mut snapshot = store.snapshot().unwrap();
            let transaction = store.connection.transaction().unwrap();
            let error = conversation::apply(
                &transaction,
                &mut snapshot,
                &uuid::Uuid::new_v4().to_string(),
                edit(revision),
                &f.workspace.config,
                &HashMap::new(),
                Sender::Agent {
                    session_id: "peer-agent".into(),
                },
            )
            .err()
            .unwrap();
            assert!(error.message.contains("Only people"));
            assert!(!snapshot.is_peer(&f.caller, "third-agent"));
        }
        assert!(
            !peers(&f)
                .iter()
                .any(|p| p.sessions.contains(&"third-agent".to_owned()))
        );
        // The human command endpoint's edit grants, whoever sent the turn.
        let revision = set_draft(&f, &f.caller, &parts);
        let current = f.workspace.snapshots.borrow().revision;
        let envelope = crate::tests::env(current, edit(revision));
        let snapshot = human(&f, envelope.clone()).unwrap();
        assert!(snapshot.is_peer(&f.caller, "third-agent"));
        let grant = snapshot
            .session_peers
            .iter()
            .find(|p| p.sessions.contains(&"third-agent".to_owned()))
            .unwrap();
        assert_eq!(grant.request_id, envelope.request_id);
        assert_eq!(grant.source_session_id, f.caller);
        let turn = snapshot
            .submissions
            .iter()
            .find(|s| s.id == queued)
            .unwrap();
        assert_eq!(turn.parts, parts);
        settle(&f, &["peer-agent", "third-agent"]);
        f.finish().await;
    }
}
