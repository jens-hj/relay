//! Server-owned discovery and dispatch. No client-supplied RPCs or skill paths.
use super::*;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};

#[derive(Clone, Debug)]
pub(super) struct Invocation {
    pub name: String,
    pub argument: String,
}

pub(super) fn invocation(
    parts: &[Part],
    catalog: &HarnessCatalog,
) -> Result<Option<Invocation>, Error> {
    let text = plain_text(parts);
    let trimmed = text.trim_start();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    let (name, argument) = trimmed[1..]
        .split_once(char::is_whitespace)
        .unwrap_or((&trimmed[1..], ""));
    let entry = catalog
        .commands
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| {
            Error::invalid(format!(
                "Unknown command /{name}; open /help to see available commands"
            ))
        })?;
    if entry.dispatch == CommandDispatch::Unavailable {
        return Err(Error::invalid(
            entry.reason.as_deref().unwrap_or("Command unavailable"),
        ));
    }
    if parts
        .iter()
        .any(|p| !matches!(p.kind, PartKind::Text { .. }))
    {
        return Err(Error::invalid(
            "Send this command separately from skills, replies, and attachments",
        ));
    }
    if entry.dispatch == CommandDispatch::Flow && !matches!(name, "compact" | "review") {
        return Err(Error::invalid(format!(
            "Open /{name} in the command picker"
        )));
    }
    if catalog.harness == Harness::Codex && name == "compact" && !argument.trim().is_empty() {
        return Err(Error::invalid(
            "Codex compaction does not accept focus instructions through this interface",
        ));
    }
    Ok(Some(Invocation {
        name: name.into(),
        argument: argument.trim().into(),
    }))
}

pub(super) fn conversation(
    snapshot: &mut Snapshot,
    id: &str,
    name: &str,
    argument: &str,
    request: &str,
) -> Result<(), Error> {
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id && !s.fixture && s.worker.is_some())
        .ok_or_else(|| Error::invalid("Live session not found"))?;
    if runtime::active(&session.worker.as_ref().unwrap().status)
        || snapshot.submissions.iter().any(|s| {
            s.session_id == id
                && matches!(
                    s.state,
                    SubmissionState::Queued
                        | SubmissionState::Paused
                        | SubmissionState::Launching
                        | SubmissionState::Running
                )
        })
    {
        return Err(Error::invalid(
            "Finish or cancel pending turns before changing conversations",
        ));
    }
    if !matches!(name, "new" | "clear" | "resume") {
        return Err(Error::invalid("Unknown conversation action"));
    }
    let target = if name == "resume" {
        Some(
            session
                .conversations
                .iter()
                .find(|c| c.id == argument)
                .cloned()
                .ok_or_else(|| Error::invalid("Retained conversation not found"))?,
        )
    } else {
        None
    };
    let session = snapshot.sessions.iter_mut().find(|s| s.id == id).unwrap();
    let worker = session.worker.as_mut().unwrap();
    if let Some(thread) = worker.thread_id.take() {
        if !session.conversations.iter().any(|c| c.thread_id == thread) {
            session.conversations.push(RetainedConversation {
                id: request.into(),
                title: format!("Conversation {}", session.conversations.len() + 1),
                thread_id: thread,
                selection: session.selection.clone(),
            });
        }
    } else if target.is_none() {
        return Err(Error::invalid(
            "This conversation already has empty context",
        ));
    }
    if let Some(target) = target {
        worker.thread_id = Some(target.thread_id);
        session.selection = target.selection;
    }
    worker.status = WorkerStatus::Completed;
    worker.error = None;
    worker.usage = None;
    worker.context_tokens = None;
    worker.context_window = None;
    snapshot.messages.push(Message {
        id: format!("conversation-{request}"),
        session_id: id.into(),
        author: "Relay".into(),
        kind: "harness_command".into(),
        body: if name == "resume" {
            "Resumed retained conversation".into()
        } else {
            "Started a conversation with empty context. Previous history is retained.".into()
        },
        parts: vec![],
        tool: None,
    });
    Ok(())
}

pub(super) async fn catalog(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    axum::extract::Path(session): axum::extract::Path<String>,
) -> Result<Json<HarnessCatalog>, Error> {
    workspace.authorize(&headers)?;
    discover(&workspace, &session, false).await.map(Json)
}

pub(super) async fn refresh(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    axum::extract::Path(session): axum::extract::Path<String>,
) -> Result<Json<HarnessCatalog>, Error> {
    workspace.authorize(&headers)?;
    discover(&workspace, &session, true).await.map(Json)
}

fn key(snapshot: &Snapshot, session: &Session, config: &RuntimeConfig) -> String {
    format!(
        "{}:{:?}:{}:{:?}:{:?}",
        session.id,
        session.worker.as_ref().map(|w| w.harness),
        session
            .worker
            .as_ref()
            .map(|w| harness::selected_binary(snapshot, w.harness, config)
                .display()
                .to_string())
            .unwrap_or_default(),
        session.connection_ids,
        session.workspaces
    )
}

async fn write(child: &mut tokio::process::Child, value: Value) -> Result<(), Error> {
    let input = child
        .stdin
        .as_mut()
        .ok_or_else(|| Error::invalid("Harness input closed"))?;
    let mut data = serde_json::to_vec(&value).map_err(Error::internal)?;
    data.push(b'\n');
    input
        .write_all(&data)
        .await
        .map_err(|_| Error::invalid("Harness discovery disconnected"))
}

pub(super) async fn discover(
    workspace: &Workspace,
    id: &str,
    force: bool,
) -> Result<HarnessCatalog, Error> {
    let _probe = workspace.catalog_probe.lock().await;
    let snapshot = workspace.snapshots.borrow().clone();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id && !s.fixture)
        .ok_or_else(|| Error::invalid("Live session not found"))?;
    let worker = session
        .worker
        .as_ref()
        .ok_or_else(|| Error::invalid("Session has no harness"))?;
    let cache_key = key(&snapshot, session, &workspace.config);
    if !force
        && let Some((saved_at, catalog)) = workspace
            .catalogs
            .lock()
            .map_err(Error::internal)?
            .get(&cache_key)
        && now().saturating_sub(*saved_at) < 30
    {
        return Ok(catalog.clone());
    }
    let path = worker
        .worktree
        .clone()
        .or_else(|| {
            snapshot
                .projects
                .iter()
                .find(|p| p.id == session.project_id)
                .and_then(|p| p.root.clone())
        })
        .or_else(|| {
            workspace
                .config
                .repository
                .as_ref()
                .map(|p| p.display().to_string())
        })
        .filter(|p| !p.is_empty())
        .ok_or_else(|| Error::invalid("Session working directory unavailable"))?;
    let binary = harness::selected_binary(&snapshot, worker.harness, &workspace.config);
    let mut command = tokio::process::Command::new(binary);
    match worker.harness {
        Harness::Codex => {
            command.args(["app-server", "--stdio"]);
        }
        Harness::ClaudeCode => {
            command.args([
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
            ]);
            for space in &session.workspaces {
                if space.path != path {
                    command.args(["--add-dir", &space.path]);
                }
            }
        }
    }
    command
        .current_dir(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .env_remove("RELAY_TOKEN")
        .env_remove("RELAY_TOKEN_FILE")
        .env_remove("RELAY_SETUP_TOKEN_FILE")
        .env_remove("CREDENTIALS_DIRECTORY");
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| Error::invalid("Cannot start harness discovery"))?;
    let _guard = runtime::ProcessGuard {
        owned: child
            .id()
            .and_then(|pid| runtime::process_identity(pid).map(|identity| (pid, identity))),
    };
    let _job = ProjectJob::new(workspace);
    let output = child
        .stdout
        .take()
        .ok_or_else(|| Error::invalid("Harness output unavailable"))?;
    let mut output = BufReader::new(output);
    let probe = async {
        let mut catalog = HarnessCatalog {
            session_id: id.into(),
            harness: worker.harness,
            ..Default::default()
        };
        match worker.harness {
            Harness::Codex => {
                write(&mut child,json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"relay","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}})).await?;
                read_response(&mut output, 1).await?;
                write(&mut child, json!({"method":"initialized"})).await?;
                write(
                    &mut child,
                    json!({"id":2,"method":"model/list","params":{}}),
                )
                .await?;
                let mut models = read_response(&mut output, 2).await?;
                loop {
                    add_models(&mut catalog, &models, Harness::Codex);
                    let Some(cursor) = models["nextCursor"].as_str().filter(|s| !s.is_empty())
                    else {
                        break;
                    };
                    write(
                        &mut child,
                        json!({"id":2,"method":"model/list","params":{"cursor":cursor}}),
                    )
                    .await?;
                    models = read_response(&mut output, 2).await?;
                }
                let mut cwds: Vec<_> = session.workspaces.iter().map(|s| s.path.clone()).collect();
                if !cwds.contains(&path) {
                    cwds.push(path.clone());
                }
                write(&mut child,json!({"id":3,"method":"skills/list","params":{"cwds":cwds,"forceReload":true}})).await?;
                let skills = read_response(&mut output, 3).await?;
                for cwd in skills["data"].as_array().into_iter().flatten() {
                    for skill in cwd["skills"].as_array().into_iter().flatten() {
                        let Some(name) = skill["name"].as_str() else {
                            continue;
                        };
                        let Some(path) = skill["path"].as_str() else {
                            continue;
                        };
                        if catalog.skills.iter().any(|s| s.id == path) {
                            continue;
                        }
                        catalog.skills.push(HarnessSkill {
                            id: path.into(),
                            name: name.into(),
                            description: skill["description"].as_str().unwrap_or("").into(),
                            argument_hint: String::new(),
                            enabled: skill["enabled"].as_bool().unwrap_or(true),
                            path: Some(path.into()),
                        });
                    }
                    if let Some(errors) = cwd["errors"].as_array()
                        && !errors.is_empty()
                    {
                        catalog
                            .warnings
                            .push("Some skills could not be loaded by Codex".into());
                    }
                }
                write(
                    &mut child,
                    json!({"id":4,"method":"mcpServerStatus/list","params":{}}),
                )
                .await?;
                match read_response(&mut output, 4).await {
                    Ok(data) => {
                        for server in data["data"].as_array().into_iter().flatten() {
                            if let Some(name) = server["name"].as_str() {
                                catalog.mcp.push(format!(
                                    "{} · {}",
                                    name,
                                    server["authStatus"]
                                        .as_str()
                                        .unwrap_or("Status unavailable")
                                ));
                            }
                        }
                    }
                    Err(_) => catalog
                        .warnings
                        .push("MCP status is unavailable in this harness version".into()),
                }
            }
            Harness::ClaudeCode => {
                write(&mut child,json!({"type":"control_request","request_id":"relay-discover","request":{"subtype":"initialize","hooks":null}})).await?;
                let data = loop {
                    let bytes = runtime::line(&mut output)
                        .await?
                        .ok_or_else(|| Error::invalid("Claude discovery disconnected"))?;
                    let v: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| Error::invalid("Invalid Claude discovery event"))?;
                    if v["type"] == "control_response"
                        && v["response"]["request_id"] == "relay-discover"
                    {
                        if v["response"]["subtype"] != "success" {
                            return Err(Error::invalid("Claude rejected discovery"));
                        }
                        break v["response"]["response"].clone();
                    }
                };
                add_models(&mut catalog, &data, Harness::ClaudeCode);
                for value in data["commands"].as_array().into_iter().flatten() {
                    let Some(name) = value["name"].as_str() else {
                        continue;
                    };
                    catalog.commands.push(HarnessCommand {
                        name: name.into(),
                        description: value["description"].as_str().unwrap_or("").into(),
                        argument_hint: value["argumentHint"].as_str().unwrap_or("").into(),
                        dispatch: CommandDispatch::Direct,
                        reason: None,
                    });
                    if !is_control(name) {
                        catalog.skills.push(HarnessSkill {
                            id: name.into(),
                            name: name.into(),
                            description: value["description"].as_str().unwrap_or("").into(),
                            argument_hint: value["argumentHint"].as_str().unwrap_or("").into(),
                            enabled: true,
                            path: None,
                        });
                        for alias in value["aliases"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            catalog.skills.push(HarnessSkill {
                                id: name.into(),
                                name: alias.into(),
                                description: value["description"].as_str().unwrap_or("").into(),
                                argument_hint: value["argumentHint"].as_str().unwrap_or("").into(),
                                enabled: true,
                                path: None,
                            });
                        }
                    }
                }
                write(&mut child,json!({"type":"control_request","request_id":"relay-mcp","request":{"subtype":"mcp_status"}})).await?;
                let status = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        let bytes = runtime::line(&mut output)
                            .await?
                            .ok_or_else(|| Error::invalid("Claude MCP status disconnected"))?;
                        let value: Value =
                            serde_json::from_slice(&bytes).map_err(Error::internal)?;
                        if value["response"]["request_id"] == "relay-mcp" {
                            if value["response"]["subtype"] != "success" {
                                return Err(Error::invalid("Claude MCP status unsupported"));
                            }
                            return Ok(value["response"]["response"].clone());
                        }
                    }
                })
                .await;
                if let Ok(Ok(data)) = status {
                    for server in data["mcpServers"]
                        .as_array()
                        .or_else(|| data["mcp_servers"].as_array())
                        .into_iter()
                        .flatten()
                    {
                        if let Some(name) = server["name"].as_str() {
                            catalog.mcp.push(format!(
                                "{} · {}",
                                name,
                                server["status"].as_str().unwrap_or("Status unavailable")
                            ));
                        }
                    }
                } else {
                    catalog
                        .warnings
                        .push("MCP status is unavailable in this harness version".into());
                }
            }
        }
        registered(&mut catalog);
        catalog.commands.sort_by(|a, b| a.name.cmp(&b.name));
        catalog.skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok::<_, Error>(catalog)
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), probe)
        .await
        .map_err(|_| Error::invalid("Harness discovery timed out"));
    runtime::terminate(&mut child).await;
    let catalog = result??;
    workspace
        .catalogs
        .lock()
        .map_err(Error::internal)?
        .insert(cache_key, (now(), catalog.clone()));
    Ok(catalog)
}

async fn read_response(
    output: &mut BufReader<tokio::process::ChildStdout>,
    id: u64,
) -> Result<Value, Error> {
    loop {
        let bytes = runtime::line(output)
            .await?
            .ok_or_else(|| Error::invalid("Codex discovery disconnected"))?;
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Invalid Codex discovery event"))?;
        if v["id"].as_u64() == Some(id) {
            if v.get("error").is_some() {
                return Err(Error::invalid("Codex discovery request unavailable"));
            }
            return Ok(v["result"].clone());
        }
    }
}

fn add_models(catalog: &mut HarnessCatalog, data: &Value, harness: Harness) {
    let field = if harness == Harness::Codex {
        "data"
    } else {
        "models"
    };
    for model in data[field].as_array().into_iter().flatten() {
        let Some(id) = model[if harness == Harness::Codex {
            "id"
        } else {
            "value"
        }]
        .as_str() else {
            continue;
        };
        let efforts = if harness == Harness::Codex {
            model["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["reasoningEffort"].as_str())
                .map(str::to_owned)
                .collect()
        } else {
            model["supportedEffortLevels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        };
        catalog.models.push(HarnessModel {
            id: model["model"].as_str().unwrap_or(id).into(),
            name: model["displayName"].as_str().unwrap_or(id).into(),
            efforts,
            default_effort: model["defaultReasoningEffort"].as_str().map(str::to_owned),
            fast: model["supportsFastMode"].as_bool().unwrap_or(false)
                || model["serviceTiers"]
                    .as_array()
                    .is_some_and(|tiers| tiers.iter().any(|t| t["id"] == "fast")),
        });
    }
}

fn is_control(name: &str) -> bool {
    matches!(
        name,
        "compact"
            | "clear"
            | "reset"
            | "new"
            | "resume"
            | "fork"
            | "branch"
            | "model"
            | "effort"
            | "fast"
            | "permissions"
            | "plan"
            | "status"
            | "context"
            | "usage"
            | "cost"
            | "help"
            | "skills"
            | "mcp"
            | "config"
            | "settings"
            | "theme"
            | "terminal-setup"
            | "color"
            | "exit"
            | "quit"
            | "add-dir"
            | "cd"
            | "login"
            | "logout"
            | "background"
            | "remote-control"
    )
}

fn registered(catalog: &mut HarnessCatalog) {
    for (name, description, hint) in [
        ("help", "Available commands and skills", ""),
        ("model", "Choose the session model", "[model]"),
        ("effort", "Choose reasoning effort", "[level]"),
        ("fast", "Set fast mode", "[on|off]"),
        ("permissions", "Choose execution mode", ""),
        ("plan", "Enter or leave planning mode", ""),
        (
            "compact",
            "Compact conversation history",
            if catalog.harness == Harness::ClaudeCode {
                "[instructions]"
            } else {
                ""
            },
        ),
        ("diff", "Review workspace changes", ""),
        ("status", "Session details", ""),
        ("context", "Measured context usage", ""),
        ("usage", "Measured token usage", ""),
        ("skills", "Available skills", ""),
        ("mcp", "Configured MCP servers", ""),
        ("new", "Start with empty context", ""),
        ("clear", "Start with empty context", ""),
        ("resume", "Resume a retained conversation", "[conversation]"),
        ("fork", "Branch conversation and repository state", ""),
    ] {
        catalog.commands.retain(|c| c.name != name);
        catalog.commands.push(HarnessCommand {
            name: name.into(),
            description: description.into(),
            argument_hint: hint.into(),
            dispatch: CommandDispatch::Flow,
            reason: None,
        });
    }
    if catalog.harness == Harness::Codex {
        catalog.commands.push(HarnessCommand {
            name: "review".into(),
            description: "Review changes, a branch, commit, or custom target".into(),
            argument_hint: "[target]".into(),
            dispatch: CommandDispatch::Flow,
            reason: None,
        });
    }
    for (name, reason) in [
        (
            "theme",
            "This changes the harness terminal appearance; use Relay appearance settings",
        ),
        ("terminal-setup", "Relay does not host the harness terminal"),
        (
            "add-dir",
            "Use project connections to configure session workspaces",
        ),
        ("cd", "The linked task owns its session workspaces"),
        ("login", "Authenticate the harness on the server machine"),
        (
            "logout",
            "Harness authentication is shared with other sessions",
        ),
        (
            "background",
            "Relay owns persistent execution and reconnects",
        ),
        (
            "remote-control",
            "Use Relay's authenticated client connection",
        ),
    ] {
        catalog.commands.retain(|c| c.name != name);
        catalog.commands.push(HarnessCommand {
            name: name.into(),
            description: reason.into(),
            argument_hint: String::new(),
            dispatch: CommandDispatch::Unavailable,
            reason: Some(reason.into()),
        });
    }
    // The CLI's interactive-only commands are not part of its metadata protocol.
    // Keep known entries visible with a reason instead of passing them as model prose.
    let known = if catalog.harness == Harness::Codex {
        "ide keymap vim setup-default-sandbox sandbox-add-read-dir agent subagents apps plugins hooks rename archive delete copy exit quit experimental approve memories import feedback init mention goal personality ps stop clean app side btw raw debug-config statusline title pets pet"
    } else {
        "advisor agents artifacts auto-mode-setup autocompact autofix-pr bg btw bug share chrome color config settings copy exit quit export feedback focus goal heapdump hooks ide import init insights install-github-app install-slack-app keybindings list-agents peers memory mobile ios android output-style passes plugin powerup privacy-settings radio rate-limit-options recap release-notes reload-plugins reload-skills rc rename rewind checkpoint sandbox search stats stickies subtask tasks teleport terminal-setup theme todos upgrade usage voice web worktree"
    };
    for name in known.split_whitespace() {
        if catalog.commands.iter().any(|command| command.name == name) {
            continue;
        }
        let reason = match name {
            "copy" => "Select recorded output and use Copy in Relay",
            "stop" | "clean" => "Use Stop in Relay to interrupt the active turn",
            "apps" | "plugins" | "plugin" | "hooks" | "memories" | "memory" | "import"
            | "config" | "settings" => {
                "Configure this feature on the server; the harness does not expose its interactive editor here"
            }
            "init" => {
                "Ask the agent to create the project instruction file; the harness does not expose this command here"
            }
            "exit" | "quit" => "Close the client or use Stop; Relay owns the harness process",
            "archive" | "delete" | "rename" => {
                "Relay owns the task-linked session and its retained history"
            }
            _ => {
                "This command requires harness terminal controls or capabilities not exposed by this installed harness"
            }
        };
        catalog.commands.push(HarnessCommand {
            name: name.into(),
            description: reason.into(),
            argument_hint: String::new(),
            dispatch: CommandDispatch::Unavailable,
            reason: Some(reason.into()),
        });
    }
    for name in if catalog.harness == Harness::Codex {
        vec!["approvals"]
    } else {
        vec!["allowed-tools", "reset", "cost", "branch"]
    } {
        if let Some(mut entry) = catalog
            .commands
            .iter()
            .find(|c| c.name == canonical_command(name))
            .cloned()
        {
            entry.name = name.into();
            catalog.commands.retain(|c| c.name != name);
            catalog.commands.push(entry);
        }
    }
    if catalog.models.is_empty() {
        for c in &mut catalog.commands {
            if matches!(c.name.as_str(), "model" | "effort" | "fast") {
                c.dispatch = CommandDispatch::Unavailable;
                c.reason = Some("The harness did not report a model catalog".into());
            }
        }
    }
}

pub(super) fn validate_selection(
    catalog: &HarnessCatalog,
    selection: &HarnessSelection,
) -> Result<(), Error> {
    if let Some(id) = &selection.model {
        let model = catalog
            .models
            .iter()
            .find(|m| &m.id == id)
            .ok_or_else(|| Error::invalid("Model is no longer available"))?;
        if selection
            .effort
            .as_ref()
            .is_some_and(|e| !model.efforts.contains(e))
        {
            return Err(Error::invalid("Effort is not supported by this model"));
        }
        if selection.fast && !model.fast {
            return Err(Error::invalid("Fast mode is not supported by this model"));
        }
    } else if selection.effort.is_some() || selection.fast {
        return Err(Error::invalid(
            "Choose a model before setting effort or fast mode",
        ));
    }
    Ok(())
}

pub(super) fn reserve_fork(snapshot: &mut Snapshot, id: &str) -> Result<(), Error> {
    runtime::check_shared(snapshot, id)?;
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id && !s.fixture && s.worker.is_some())
        .ok_or_else(|| Error::invalid("Live session not found"))?;
    if runtime::active(&session.worker.as_ref().unwrap().status)
        || snapshot.submissions.iter().any(|s| {
            s.session_id == id
                && matches!(
                    s.state,
                    SubmissionState::Queued
                        | SubmissionState::Paused
                        | SubmissionState::Launching
                        | SubmissionState::Running
                )
        })
    {
        return Err(Error::invalid(
            "Finish or cancel pending turns before forking",
        ));
    }
    if session.worker.as_ref().unwrap().thread_id.is_none() {
        return Err(Error::invalid("This conversation has no history to fork"));
    }
    snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_mut()
        .unwrap()
        .status = WorkerStatus::Queued;
    Ok(())
}

pub(super) fn fork_workspaces(session: &Session, id: &str) -> Result<Session, Error> {
    let mut fork = session.clone();
    fork.id = id.into();
    fork.title = format!("{} (fork)", session.title);
    fork.conversations.clear();
    let mut prepared = vec![];
    let result = (|| {
        for (index, space) in fork.workspaces.iter_mut().enumerate() {
            if !space.repository {
                continue;
            }
            let source = std::path::PathBuf::from(&space.path);
            let target = source
                .parent()
                .ok_or_else(|| Error::invalid("Workspace has no parent"))?
                .join(format!("fork-{id}-{index}"));
            let base = String::from_utf8(git_capture(&source, &["rev-parse", "HEAD"])?)
                .map_err(Error::internal)?
                .trim()
                .to_owned();
            let branch = format!("relay/fork-{id}-{index}");
            let target_text = target
                .to_str()
                .ok_or_else(|| Error::invalid("Non UTF-8 workspace path"))?;
            if target.exists() || !git_capture(&source, &["branch", "--list", &branch])?.is_empty()
            {
                return Err(Error::invalid("Fork workspace already exists"));
            }
            prepared.push((source.clone(), target.clone(), branch.clone()));
            git_capture(
                &source,
                &["worktree", "add", "-b", &branch, target_text, &base],
            )?;
            for (staged, args) in [
                (
                    true,
                    vec![
                        "diff",
                        "--cached",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--binary",
                        "--full-index",
                        "--src-prefix=a/",
                        "--dst-prefix=b/",
                        "HEAD",
                        "--",
                    ],
                ),
                (
                    false,
                    vec![
                        "diff",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--binary",
                        "--full-index",
                        "--src-prefix=a/",
                        "--dst-prefix=b/",
                        "--",
                    ],
                ),
            ] {
                let diff = git_capture(&source, &args)?;
                if !diff.is_empty() {
                    use std::io::Write;
                    let mut patch = tempfile::NamedTempFile::new().map_err(Error::internal)?;
                    patch.write_all(&diff).map_err(Error::internal)?;
                    let patch = patch
                        .path()
                        .to_str()
                        .ok_or_else(|| Error::invalid("Non UTF-8 patch path"))?;
                    if staged {
                        git_capture(&target, &["apply", "--index", "--binary", patch])?;
                    } else {
                        git_capture(&target, &["apply", "--binary", patch])?;
                    }
                }
            }
            let files = git_capture(
                &source,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )?;
            for name in files.split(|b| *b == 0).filter(|n| !n.is_empty()) {
                crate::process::cancelled()?;
                let name = std::str::from_utf8(name)
                    .map_err(|_| Error::invalid("Cannot fork a non UTF-8 filename"))?;
                let relative = std::path::Path::new(name);
                if relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(Error::invalid("Invalid untracked file path"));
                }
                let src = source.join(relative);
                let dst = target.join(relative);
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(Error::internal)?;
                }
                let metadata = std::fs::symlink_metadata(&src).map_err(Error::internal)?;
                if metadata.file_type().is_symlink() {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(
                        std::fs::read_link(&src).map_err(Error::internal)?,
                        &dst,
                    )
                    .map_err(Error::internal)?;
                    #[cfg(not(unix))]
                    return Err(Error::invalid(
                        "Forking untracked symlinks is unavailable on this platform",
                    ));
                } else if metadata.is_file() {
                    copy_untracked(&src, &dst)?;
                } else {
                    return Err(Error::invalid("Cannot fork an untracked special file"));
                }
            }
            space.path = target_text.into();
            space.branch = Some(branch);
            space.base_commit = Some(base);
            space.changes = None;
        }
        let worker = fork.worker.as_mut().unwrap();
        let primary = session
            .worker
            .as_ref()
            .and_then(|w| w.worktree.as_ref())
            .and_then(|path| session.workspaces.iter().position(|s| &s.path == path))
            .and_then(|index| fork.workspaces.get(index))
            .ok_or_else(|| Error::invalid("Primary fork workspace missing"))?;
        worker.worktree = Some(primary.path.clone());
        worker.branch = primary.branch.clone();
        worker.base_commit = primary.base_commit.clone();
        worker.thread_id = None;
        worker.status = WorkerStatus::Completed;
        worker.error = None;
        worker.usage = None;
        worker.last_usage = None;
        worker.changes = None;
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        crate::process::without_cancellation(|| {
            for (source, target, branch) in prepared {
                let _ = git_capture(
                    &source,
                    &[
                        "worktree",
                        "remove",
                        "--force",
                        target.to_str().unwrap_or_default(),
                    ],
                );
                let _ = git_capture(&source, &["branch", "-D", &branch]);
            }
        });
        return Err(error);
    }
    Ok(fork)
}

fn copy_untracked(source: &std::path::Path, target: &std::path::Path) -> Result<(), Error> {
    use std::io::{Read, Write};
    let mut input = std::fs::File::open(source).map_err(Error::internal)?;
    let permissions = input.metadata().map_err(Error::internal)?.permissions();
    let mut output = std::fs::File::create(target).map_err(Error::internal)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut chunk = [0; 128 * 1024];
    loop {
        crate::process::cancelled()?;
        if std::time::Instant::now() > deadline {
            return Err(Error::invalid("Copying an untracked fork file timed out"));
        }
        let count = input.read(&mut chunk).map_err(Error::internal)?;
        if count == 0 {
            break;
        }
        output.write_all(&chunk[..count]).map_err(Error::internal)?;
    }
    output.set_permissions(permissions).map_err(Error::internal)
}

fn git_capture(path: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let output = crate::process::capture(
        std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args),
        64 * 1024 * 1024,
        std::time::Duration::from_secs(60),
        "Fork workspace",
    )?;
    if output.truncated {
        return Err(Error::invalid("Fork metadata or patch exceeds 64 MiB"));
    }
    if !output.status.success() {
        return Err(Error::invalid(format!(
            "Cannot {} repository state into the fork",
            args.first().unwrap_or(&"copy")
        )));
    }
    Ok(output.bytes)
}

pub(super) fn cleanup_fork(fork: &Session) {
    for space in &fork.workspaces {
        if space.repository {
            let path = std::path::Path::new(&space.path);
            if let Ok(common) = git_capture(
                path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            ) && let Ok(common) = String::from_utf8(common)
                && let Some(root) = std::path::Path::new(common.trim()).parent()
            {
                let _ = git_capture(root, &["worktree", "remove", "--force", &space.path]);
                if let Some(branch) = &space.branch {
                    let _ = git_capture(root, &["branch", "-D", branch]);
                }
            }
        }
    }
}

pub(super) struct ForkResources {
    pub workspace: Workspace,
    pub session: Session,
}
impl Drop for ForkResources {
    fn drop(&mut self) {
        if self
            .workspace
            .snapshots
            .borrow()
            .sessions
            .iter()
            .any(|s| s.id == self.session.id)
        {
            return;
        }
        let fork = self.session.clone();
        let job = ProjectJob::new(&self.workspace);
        tokio::task::spawn_blocking(move || {
            let _job = job;
            cleanup_fork(&fork);
        });
    }
}

pub(super) fn publish_fork(
    workspace: &Workspace,
    source: &str,
    run: &str,
    fork: &Session,
    thread: &str,
) -> Result<(), Error> {
    if workspace
        .snapshots
        .borrow()
        .sessions
        .iter()
        .find(|s| s.id == source)
        .and_then(|s| s.worker.as_ref())
        .and_then(|w| w.thread_id.as_deref())
        == Some(thread)
    {
        return Err(Error::invalid("Harness did not create an independent fork"));
    }
    workspace.update_run(source, run, |snapshot| {
        if snapshot.sessions.iter().any(|s| s.id == fork.id) {
            return Err(Error::invalid("Fork already exists"));
        }
        let mut fork = fork.clone();
        fork.worker.as_mut().unwrap().thread_id = Some(thread.into());
        let messages: Vec<_> = snapshot
            .messages
            .iter()
            .filter(|m| m.session_id == source)
            .cloned()
            .map(|mut m| {
                m.id = format!("fork-{}-{}", fork.id, m.id);
                m.session_id = fork.id.clone();
                remap_anchors(&mut m.parts, &fork.id);
                m
            })
            .collect();
        snapshot.messages.extend(messages);
        snapshot.messages.push(Message {
            id: format!("fork-result-{run}"),
            session_id: source.into(),
            author: "Relay".into(),
            kind: "harness_command".into(),
            body: format!("Created {} in independent repository worktrees", fork.title),
            parts: vec![],
            tool: None,
        });
        snapshot.sessions.push(fork);
        Ok(())
    })
}

fn remap_anchors(parts: &mut [Part], fork: &str) {
    for part in parts {
        if let PartKind::Reply { anchor, parts } = &mut part.kind {
            anchor.message_id = format!("fork-{fork}-{}", anchor.message_id);
            remap_anchors(parts, fork);
        }
    }
}
