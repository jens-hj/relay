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
fn unauthorized() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        "agent_capability",
        "Agent capability unavailable or expired",
    )
}
fn active_caller(store: &Store, snapshot: &Snapshot, caller: &str, run: &str) -> Result<(), Error> {
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
    snapshot.session_profile(session).map_err(Error::invalid)
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
    candidate.sessions.retain(|s| s.id != session.id);
    let root_limit = candidate
        .effective_profile(
            candidate
                .agent_configs
                .iter()
                .find(|d| d.id == session.agent_config_id)
                .ok_or_else(|| Error::invalid("Agent configuration missing"))?,
        )
        .map_err(Error::invalid)?
        .max_workers;
    let mut effective = effective;
    effective.max_workers = root_limit;
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
    runtime::authorize_session(
        &candidate,
        session
            .issue_id
            .as_deref()
            .ok_or_else(|| Error::invalid("Linked issue missing"))?,
        &session.agent_config_id,
        approved,
        &session.role,
        config,
    )
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
    #[serde(default)]
    permissions: BTreeMap<Task, Permission>,
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
            Ok(json!({"agents":agents}))
        }
        "spawn_agent" => spawn(workspace, caller, run, parse(input)?, stop).await,
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
            store.stop_controls(&current, &args.agent);
            workspace.snapshots.send_replace(current);
            Ok(json!({"agent":args.agent,"stop_requested":true}))
        }
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
        || (args.effort.is_some() && args.model.is_none())
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
    let root_limit = snapshot
        .effective_profile(
            snapshot
                .agent_configs
                .iter()
                .find(|d| d.id == parent.agent_config_id)
                .ok_or_else(|| Error::invalid("Agent configuration missing"))?,
        )
        .map_err(Error::invalid)?
        .max_workers;
    if snapshot
        .sessions
        .iter()
        .filter(|s| {
            s.agent_config_id == parent.agent_config_id
                && s.role == SessionRole::Worker
                && s.worker
                    .as_ref()
                    .is_some_and(|w| runtime::active(&w.status))
        })
        .count()
        >= usize::from(effective.max_workers.min(root_limit))
    {
        return Err(Error::invalid("Current director worker limit reached"));
    }
    let mut permissions = effective.permissions.clone();
    let widened = args
        .permissions
        .iter()
        .filter(|(task, p)| rank(**p) > rank(effective.permissions[task]))
        .map(|(task, p)| {
            format!(
                "{} {} (parent: {})",
                task.label().to_lowercase(),
                p.label().to_lowercase(),
                effective.permissions[task].label().to_lowercase()
            )
        })
        .collect::<Vec<_>>();
    match effective.permissions[&Task::Delegate] {
        Permission::Deny => return Err(Error::invalid("Parent denies delegation")),
        Permission::Ask => {
            approval(
                workspace,
                caller,
                run,
                "mcp__relay__spawn_agent",
                &format!("Spawn {}: delegation requires approval", args.title),
                stop,
            )
            .await?
        }
        Permission::Allow => {}
    }
    if !widened.is_empty() {
        approval(
            workspace,
            caller,
            run,
            "mcp__relay__spawn_agent",
            &format!("Spawn {} with: {}", args.title, widened.join(", ")),
            stop,
        )
        .await?;
    }
    permissions.extend(args.permissions.clone());
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
    child.title = args.title;
    child.role = SessionRole::Worker;
    child.parent_session_id = Some(caller.into());
    child.compactions.clear();
    child.selection = HarnessSelection {
        model: args.model,
        effort: args.effort,
        ..Default::default()
    };
    child.spawn = Some(SpawnGrant {
        task: args.task.clone(),
        context: args.context.clone(),
        permissions: args.permissions,
        requested_by_run: Some(run.into()),
    });
    let worker = child.worker.as_mut().unwrap();
    worker.harness = args.harness.unwrap_or(worker.harness);
    worker.model = None;
    worker.context_tokens = None;
    worker.context_window = None;
    worker.status = WorkerStatus::Queued;
    let prompt = match args.context {
        Some(context) => format!("{}\n\nContext:\n{}", args.task, context),
        None => args.task,
    };
    validate_prompt(&prompt)?;
    // Use any discovered catalog for the requested harness; launch discovers it
    // again in the child workspace before executing the first turn.
    if child.selection.model.is_some() || child.selection.effort.is_some() {
        let catalogs = workspace.catalogs.lock().map_err(Error::internal)?;
        let catalog = catalogs
            .values()
            .map(|(_, c)| c)
            .find(|c| c.harness == child.worker.as_ref().unwrap().harness);
        if let Some(catalog) = catalog {
            commands::validate_selection(catalog, &child.selection)?;
        }
    }
    let child_run = uuid::Uuid::new_v4().to_string();
    let parts = vec![Part::text(&prompt)];
    // An Ask implementation grant needs approval for this first turn as normal launches do.
    let implement = permissions[&Task::Implement];
    if implement == Permission::Ask {
        approval(
            workspace,
            caller,
            run,
            "mcp__relay__spawn_agent",
            &format!(
                "Spawn {}: approve implementation for its first turn",
                child.title
            ),
            stop,
        )
        .await?;
    }
    let (sender, receiver) = watch::channel(false);
    {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(unauthorized());
        }
        let mut current = store.snapshot()?;
        active_caller(&store, &current, caller, run)?;
        let latest_parent = current.sessions.iter().find(|s| s.id == caller).unwrap();
        if profile(&current, latest_parent)?.permissions != effective.permissions {
            return Err(Error::invalid("Parent permissions changed; retry spawn"));
        }
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
        current.sessions.push(child);
        current.submissions.push(Submission {
            id: child_run.clone(),
            session_id: id.clone(),
            parts: parts.clone(),
            state: SubmissionState::Launching,
            approve_implementation: true,
            error: None,
            interrupts_run: None,
            last_edit_request: None,
        });
        current
            .messages
            .push(prompt_message(&id, &child_run, &prompt));
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
    related(&snapshot, caller, &args.agent)?;
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
        related(&current, caller, &args.agent)?;
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

fn tools() -> Value {
    let string = json!({"type":"string"});
    let permissions = json!({"type":"object","propertyNames":{"enum":["plan","delegate","implement","verify","review","merge","deploy"]},"additionalProperties":{"enum":["allow","ask","deny"]}});
    let specs = [
        (
            "spawn_agent",
            "Start a child Relay session. Relay enforces delegation and human approval for wider permissions.",
            json!({"title":string,"task":string,"context":string,"harness":{"enum":["codex","claude_code"]},"model":string,"effort":string,"permissions":permissions}),
            vec!["title", "task"],
        ),
        (
            "stop_agent",
            "Stop one of your descendants and its subtree. Requires delegation permission; retains history and workspaces.",
            json!({"agent":string}),
            vec!["agent"],
        ),
        (
            "message_agent",
            "Queue a turn for an agent in this tree.",
            json!({"agent":string,"text":string}),
            vec!["agent", "text"],
        ),
        (
            "wait_for_agents",
            "Wait until agents are terminal or the timeout expires.",
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
            "List your subtree and parent with current statuses.",
            json!({}),
            vec![],
        ),
    ];
    json!({"tools":specs.into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect::<Vec<_>>()})
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
                Some("tools/list") => Ok(tools()),
                Some("tools/call") => {
                    let name = request["params"]["name"].as_str().unwrap_or("");
                    if ![
                        "spawn_agent",
                        "stop_agent",
                        "message_agent",
                        "wait_for_agents",
                        "report_result",
                        "list_agents",
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
        assert_eq!(schema, 17);
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
            s.projects[0].defaults.max_workers = 7;
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
        crate::tests::script(
            &f.workspace.config.codex,
            "sleep 0.2\nprintf '%s\\n' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}'",
        );
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
                if read {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
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
        let snapshot = f.workspace.snapshots.borrow().clone();
        let envelope = crate::tests::env(
            snapshot.revision,
            Command::StopWorker {
                session_id: f.caller.clone(),
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
}
