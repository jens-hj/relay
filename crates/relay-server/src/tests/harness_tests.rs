use super::*;
use serde_json::Value;

const CLAUDE_THREAD: &str = "11111111-2222-4333-8444-555555555555";
pub(crate) const CLAUDE_FAKE: &str = r#"
printf '%s\n' "$@" >> claude-argv
while IFS= read -r line; do
  printf '%s\n' "$line" >> claude-input.jsonl
  type=$(printf '%s' "$line" | jq -r '.type')
  case "$type" in
    control_request)
      subtype=$(printf '%s' "$line" | jq -r '.request.subtype')
      if [ "$subtype" = interrupt ]; then
        echo '{"type":"result","subtype":"success","session_id":"11111111-2222-4333-8444-555555555555"}'
        continue
      fi
      request=$(printf '%s' "$line" | jq -r '.request_id')
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"models":[{"value":"workspace-model","displayName":"Workspace model"}]}}}\n' "$request" ;;
    user)
      echo '{"type":"system","subtype":"init","model":"reported-claude-model","session_id":"11111111-2222-4333-8444-555555555555"}'
      echo '{"type":"system","subtype":"session_state_changed","state":"running","sdk_host_only":true}'
      if [ -f hold ]; then continue; fi
      if [ -f permission ]; then
        echo '{"type":"control_request","request_id":"native-tool-request","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"notes.txt","content":"sample"}}}'
        continue
      fi
      if [ -f failure ]; then echo '{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["private detail"]}'; continue; fi
      echo '{"type":"stream_event","event":{"type":"message_start","message":{"id":"response"}}}'
      echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Complete "}}}'
      echo '{"type":"assistant","message":{"id":"response","content":[{"type":"text","text":"Complete λ"}]}}'
      echo '{"type":"result","subtype":"success","usage":{"input_tokens":5,"cache_read_input_tokens":11,"cache_creation_input_tokens":3,"output_tokens":7}}'
      echo '{"type":"system","subtype":"session_state_changed","state":"idle","sdk_host_only":true}' ;;
    control_response)
      printf '%s\n' "$line" > permission-response
      echo '{"type":"assistant","message":{"id":"response","content":[{"type":"text","text":"Decision received"}]}}'
      echo '{"type":"result","subtype":"success"}'
      echo '{"type":"system","subtype":"session_state_changed","state":"idle","sdk_host_only":true}' ;;
  esac
done
"#;

fn claude_workspace(dir: &Path, source: &str) -> Workspace {
    claude_workspace_with(dir, source, config())
}
fn claude_workspace_with(dir: &Path, source: &str, mut c: RuntimeConfig) -> Workspace {
    let repo = review_repo(dir);
    let bin = dir.join("fake-claude");
    script(&bin, source);
    c.repository = Some(repo);
    c.claude = bin;
    let mut s = live();
    s.projects[0].defaults = AgentProfile::default();
    s.projects[0].defaults.harness = Harness::ClaudeCode;
    workspace(&dir.join("db"), &s, c)
}
fn worker<'a>(s: &'a Snapshot, id: &str) -> &'a WorkerRun {
    s.sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
}

const MULTI_CODEX: &str = r#"
printf '%s' "${CARGO_TARGET_DIR-}" > cargo-target-dir
while IFS= read -r line; do
  id=$(printf '%s' "$line" | jq -c '.id')
  method=$(printf '%s' "$line" | jq -r '.method')
  printf '%s\n' "$line" >> codex-rpc.jsonl
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id";;
    model/list) printf '{"id":%s,"result":{"data":[{"id":"workspace-model","displayName":"Workspace model"}]}}\n' "$id" ;;
    skills/list|mcpServerStatus/list) printf '{"id":%s,"result":{"data":[]}}\n' "$id" ;;
    thread/start|thread/resume) printf '{"id":%s,"result":{"thread":{"id":"multi-workspace-thread"}}}\n' "$id";;
    turn/start)
      printf '%s' "$line" | jq -r '.params.sandboxPolicy.writableRoots[]' > roots
      while IFS= read -r root; do
        if [ -f "$root/tracked" ]; then printf 'changed\n' > "$root/tracked"; else printf 'direct\n' > "$root/direct"; fi
      done < roots
      printf '{"id":%s,"result":{"turn":{"id":"multi"}}}\n' "$id"
      printf '%s\n' '{"method":"item/completed","params":{"threadId":"multi-workspace-thread","turnId":"multi","item":{"id":"multi-answer","type":"agentMessage","text":"Edited selected workspaces"}}}'
      printf '%s\n' '{"method":"turn/completed","params":{"threadId":"multi-workspace-thread","turn":{"id":"multi","status":"completed"}}}'
      exit;;
  esac
done
"#;

async fn multi_workspace(harness: Harness) {
    let dir = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    let defaults = AgentProfile {
        harness,
        model: "workspace-model".into(),
        ..Default::default()
    };
    crate::projects::apply(
        &mut snapshot,
        Command::CreateProject {
            name: "Multi".into(),
            root: dir.path().join("project").display().to_string(),
            connections: vec![],
        },
        "multi",
        defaults,
        &RuntimeConfig::default(),
    )
    .unwrap();
    let project_id = snapshot.projects[0].id.clone();
    for index in 0..2 {
        let parent = dir.path().join(format!("source-{index}"));
        std::fs::create_dir(&parent).unwrap();
        let repo = review_repo(&parent);
        std::fs::write(repo.join("tracked"), "base\n").unwrap();
        git(&repo, &["add", "tracked"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e",
                "commit",
                "-qm",
                "tracked",
            ],
        );
        snapshot.connections.push(ProjectConnection {
            id: format!("repo-{index}"),
            project_id: project_id.clone(),
            name: format!("Repo {index}"),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Repository {
                remote: format!("git@github.com:org/repo-{index}.git"),
                checkout: Some(repo.display().to_string()),
                owned: false,
            },
        });
    }
    let external = dir.path().join("external");
    std::fs::create_dir(&external).unwrap();
    crate::projects::apply(
        &mut snapshot,
        Command::AddConnection {
            project_id: project_id.clone(),
            connection: ConnectionInput::Directory {
                path: external.display().to_string(),
            },
        },
        "external",
        AgentProfile::default(),
        &RuntimeConfig::default(),
    )
    .unwrap();
    let board = snapshot.boards[0].id.clone();
    crate::projects::apply(
        &mut snapshot,
        Command::CreateTask {
            board_id: board,
            title: "Edit both".into(),
            body: "Use all selected workspaces".into(),
            repository_connection_id: None,
        },
        "multi-task",
        AgentProfile::default(),
        &RuntimeConfig::default(),
    )
    .unwrap();
    let bin = dir.path().join("agent");
    match harness {
        Harness::Codex => script(&bin, MULTI_CODEX),
        Harness::ClaudeCode => script(
            &bin,
            &format!(
                r#"
printf '%s' "${{CARGO_TARGET_DIR-}}" > cargo-target-dir
previous=''
for arg; do
  if [ "$previous" = --settings ]; then
    printf '%s' "$arg" | jq -r '.sandbox.filesystem.allowWrite[]' > roots
    while IFS= read -r root; do
      if [ -f "$root/tracked" ]; then printf 'changed\n' > "$root/tracked"; else printf 'direct\n' > "$root/direct"; fi
    done < roots
  fi
  previous="$arg"
done
{CLAUDE_FAKE}
"#
            ),
        ),
    }
    let config = RuntimeConfig {
        codex: bin.clone(),
        claude: bin,
        ..Default::default()
    };
    let w = workspace(&dir.path().join("db"), &snapshot, config);
    let command = Command::StartSession {
        agent_config_id: snapshot.agent_configs[0].id.clone(),
        issue_id: Some(snapshot.issues[0].id.clone()),
        role: SessionRole::Worker,
        prompt: "Implement the task".into(),
        approve_implementation: false,
        connection_ids: None,
    };
    let (id, run, rx, prompt) = reserve(&w, env(snapshot.revision, command));
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    let mut completed = finished(&w, &id).await;
    assert_eq!(
        worker(&completed, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&completed, &id).error
    );
    let session = completed
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .clone();
    assert_eq!(session.workspaces.len(), 4);
    for workspace in session.workspaces.iter().filter(|s| s.repository) {
        assert_eq!(
            std::fs::read_to_string(Path::new(&workspace.path).join("tracked")).unwrap(),
            "changed\n"
        );
        assert!(
            workspace
                .changes
                .as_ref()
                .unwrap()
                .diff
                .contains("+changed")
        );
    }
    assert_eq!(
        std::fs::read_to_string(external.join("direct")).unwrap(),
        "direct\n"
    );
    for connection in &completed.connections {
        if let ConnectionKind::Repository {
            checkout: Some(path),
            ..
        } = &connection.kind
        {
            assert_eq!(
                std::fs::read_to_string(Path::new(path).join("tracked")).unwrap(),
                "base\n"
            );
        }
    }
    let roots = std::fs::read_to_string(
        Path::new(worker(&completed, &id).worktree.as_ref().unwrap()).join("roots"),
    )
    .unwrap();
    let worktree = Path::new(worker(&completed, &id).worktree.as_ref().unwrap());
    let target_dir = std::fs::read_to_string(worktree.join("cargo-target-dir")).unwrap();
    if crate::build_output::shared_target(
        Path::new("/"),
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
        std::env::var_os(crate::build_output::SHARED_CACHE_ENV).as_deref(),
    )
    .is_some()
    {
        // An enabled shared build cache reaches the harness environment and its sandbox.
        let cache = dir
            .path()
            .join("project")
            .canonicalize()
            .unwrap()
            .join(".relay/cache/cargo-target");
        assert_eq!(target_dir, cache.display().to_string());
        assert!(cache.is_dir());
        assert_eq!(roots.lines().count(), 5);
        assert!(roots.lines().any(|root| root == target_dir));
    } else {
        assert_eq!(roots.lines().count(), 4);
    }
    // Adding a connection cannot silently widen a resumed thread's roots.
    let extra = dir.path().join("later");
    std::fs::create_dir(&extra).unwrap();
    completed = apply(
        &w,
        Command::AddConnection {
            project_id,
            connection: ConnectionInput::Directory {
                path: extra.display().to_string(),
            },
        },
    );
    let thread = worker(&completed, &id).thread_id.clone();
    let (session_id, run, rx, prompt) = reserve(
        &w,
        env(
            completed.revision,
            Command::SendWorker {
                session_id: id.clone(),
                prompt: "Continue".into(),
                approve_implementation: false,
            },
        ),
    );
    runtime::run(w.clone(), session_id, run, prompt, rx).await;
    let resumed = finished(&w, &id).await;
    assert_eq!(
        worker(&resumed, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&resumed, &id).error
    );
    assert_eq!(worker(&resumed, &id).thread_id, thread);
    assert_eq!(
        resumed
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .connection_ids,
        session.connection_ids
    );
    assert_eq!(
        resumed
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .workspaces
            .len(),
        4
    );
    assert!(!extra.join("direct").exists());
}

#[tokio::test]
async fn codex_multiple_repositories_and_directories_keep_isolation_and_resume_roots() {
    multi_workspace(Harness::Codex).await;
}
#[tokio::test]
async fn claude_multiple_repositories_and_directories_keep_isolation_and_resume_roots() {
    multi_workspace(Harness::ClaudeCode).await;
}
fn apply(w: &Workspace, command: Command) -> Snapshot {
    let mut store = w.store.lock().unwrap();
    let snapshot = store.snapshot().unwrap();
    let (snapshot, action) = store
        .apply(env(snapshot.revision, command), &w.config)
        .unwrap();
    if let Some(Action::Stop { session_id: id, .. }) = action {
        store.controls.get(&id).unwrap().send_replace(true);
    }
    w.snapshots.send_replace(snapshot.clone());
    snapshot
}
async fn launch(w: &Workspace) -> (String, Snapshot) {
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(w, env(s.revision, start(&s)));
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    let snapshot = finished(w, &id).await;
    assert_eq!(
        worker(&snapshot, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&snapshot, &id).error
    );
    (id, snapshot)
}

#[tokio::test]
async fn harness_killed_by_sigkill_reports_out_of_memory() {
    let dir = tempfile::tempdir().unwrap();
    // The kernel OOM killer ends a harness with SIGKILL mid-turn.
    let w = claude_workspace(dir.path(), "read line\nkill -9 $$");
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    let snapshot = finished(&w, &id).await;
    let work = worker(&snapshot, &id);
    assert_eq!(work.status, WorkerStatus::Failed);
    assert_eq!(work.error.as_deref(), Some(crate::process::OUT_OF_MEMORY));
}

#[tokio::test]
async fn claude_first_turn_resume_modes_and_bound_harness() {
    let dir = tempfile::tempdir().unwrap();
    let w = claude_workspace(dir.path(), CLAUDE_FAKE);
    let (id, mut s) = launch(&w).await;
    let work = worker(&s, &id);
    assert_eq!(work.thread_id.as_deref(), Some(CLAUDE_THREAD));
    assert_eq!(work.last_usage, work.usage);
    assert_eq!(work.harness, Harness::ClaudeCode);
    assert_eq!(
        work.launched_network_policy,
        Some(LaunchedNetworkPolicy {
            harness: Harness::ClaudeCode,
            approval: ApprovalMode::Automatic,
            network_access: true,
            effective: EffectiveNetworkAccess::Unsupported,
        })
    );
    assert_eq!(work.model.as_deref(), Some("reported-claude-model"));
    assert_eq!(work.context_tokens, None);
    assert_eq!(work.context_window, None);
    assert_eq!(
        work.usage,
        Some(TokenUsage {
            input_tokens: 19,
            cached_input_tokens: 11,
            output_tokens: 7
        })
    );
    let messages: Vec<_> = s
        .messages
        .iter()
        .filter(|m| m.session_id == id && m.author == "Claude Code")
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].body, "Complete λ");
    let path = PathBuf::from(work.worktree.as_ref().unwrap());
    let argv = std::fs::read_to_string(path.join("claude-argv")).unwrap();
    assert!(argv.contains("\nauto\n"));
    assert!(argv.contains("\n--settings\n"));
    assert!(!argv.contains("--resume"));
    // Relay supplies director guidance as Claude's appended system prompt.
    let system = argv.split("--append-system-prompt\n").nth(1).unwrap();
    assert!(system.contains("delegate every change to repository files"));
    assert!(system.contains("do not call wait_for_agents or otherwise block your turn"));
    assert!(system.contains("accept it as done, archive that worker with archive_agent"));
    // Director harness changes apply to new workers only. Its execution policy
    // still flows through unless the worker has an explicit override.
    s.projects[0].defaults.harness = Harness::Codex;
    s.projects[0].defaults.execution.approval = ApprovalMode::Ask;
    w.store.lock().unwrap().save(&s).unwrap();
    w.snapshots.send_replace(s.clone());
    assert_eq!(harness::mode(&s, &id).unwrap(), ApprovalMode::Ask);
    s = apply(
        &w,
        Command::SetWorkerExecution {
            session_id: id.clone(),
            execution: Some(ExecutionSettings {
                approval: ApprovalMode::Unrestricted,
            }),
        },
    );
    let (session, run, rx, prompt) = reserve(
        &w,
        env(
            s.revision,
            Command::SendWorker {
                session_id: id.clone(),
                prompt: "Continue".into(),
                approve_implementation: false,
            },
        ),
    );
    runtime::run(w.clone(), session, run, prompt, rx).await;
    let s = finished(&w, &id).await;
    assert_eq!(
        worker(&s, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&s, &id).error
    );
    assert_eq!(worker(&s, &id).harness, Harness::ClaudeCode);
    assert_eq!(
        worker(&s, &id).launched_network_policy,
        Some(LaunchedNetworkPolicy {
            harness: Harness::ClaudeCode,
            approval: ApprovalMode::Unrestricted,
            network_access: true,
            effective: EffectiveNetworkAccess::Unsupported,
        })
    );
    let argv = std::fs::read_to_string(path.join("claude-argv")).unwrap();
    assert!(argv.contains(&format!("--resume\n{CLAUDE_THREAD}\n")));
    assert!(argv.contains("\nbypassPermissions\n"));
    assert_eq!(argv.matches("--settings").count(), 1);
    assert_eq!(
        argv.matches("do not call wait_for_agents or otherwise block your turn")
            .count(),
        2
    );
    let s = apply(
        &w,
        Command::SetWorkerExecution {
            session_id: id.clone(),
            execution: None,
        },
    );
    assert_eq!(harness::mode(&s, &id).unwrap(), ApprovalMode::Ask);
}

#[tokio::test]
async fn claude_multimodal_parts_keep_position_and_literal_file_context() {
    let dir = tempfile::tempdir().unwrap();
    let w = claude_workspace(dir.path(), CLAUDE_FAKE);
    let (id, s) = launch(&w).await;
    let mut png = std::io::Cursor::new(vec![]);
    image::RgbaImage::new(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let upload = |bytes: Bytes, name: &str| {
        conversation::upload_asset(
            State(w.clone()),
            RoutePath(uuid::Uuid::new_v4().to_string()),
            HeaderMap::from_iter([
                ("x-relay-filename".parse().unwrap(), name.parse().unwrap()),
                (
                    "content-type".parse().unwrap(),
                    if name.ends_with(".png") {
                        "image/png".parse().unwrap()
                    } else {
                        "text/plain".parse().unwrap()
                    },
                ),
            ]),
            bytes,
        )
    };
    let image = upload(Bytes::from(png.into_inner()), "context.png")
        .await
        .unwrap()
        .0;
    let file = upload(
        Bytes::from_static(b"literal $(do-not-run) file text"),
        "notes.txt",
    )
    .await
    .unwrap()
    .0;
    let parts = vec![
        Part::text("Before"),
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Asset { asset: image },
        },
        Part::text("Between"),
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Asset { asset: file },
        },
        Part::text("After"),
    ];
    let envelope = saved_turn(&w, &id, parts.clone()).await;
    let (session, run, rx, prompt) = reserve(&w, envelope);
    runtime::run(w.clone(), session, run, prompt, rx).await;
    let next = finished(&w, &id).await;
    assert_eq!(
        worker(&next, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&next, &id).error
    );
    let input = std::fs::read_to_string(
        Path::new(worker(&s, &id).worktree.as_ref().unwrap()).join("claude-input.jsonl"),
    )
    .unwrap();
    let messages: Vec<Value> = input
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .filter(|v: &Value| v["type"] == "user")
        .collect();
    let content = messages.last().unwrap()["message"]["content"]
        .as_array()
        .unwrap();
    assert_eq!(content[0]["text"], "Before");
    let argv = std::fs::read_to_string(
        Path::new(worker(&s, &id).worktree.as_ref().unwrap()).join("claude-argv"),
    )
    .unwrap();
    assert!(argv.contains("--append-system-prompt"));
    assert!(
        content[1]["text"]
            .as_str()
            .unwrap()
            .contains("Inline file: context.png")
    );
    assert_eq!(content[2]["type"], "image");
    assert_eq!(content[2]["source"]["media_type"], "image/png");
    assert!(!content[2]["source"]["data"].as_str().unwrap().is_empty());
    assert_eq!(content[3]["text"], "Between");
    assert!(
        content[4]["text"]
            .as_str()
            .unwrap()
            .contains("Inline file: notes.txt")
    );
    assert!(
        content[5]["text"]
            .as_str()
            .unwrap()
            .contains("Read the user-provided file at")
    );
    assert_eq!(content.last().unwrap()["text"], "After");
}

#[tokio::test]
async fn claude_permission_allow_deny_exact_receipt_and_stop() {
    for choice in [Some(true), Some(false), None] {
        let dir = tempfile::tempdir().unwrap();
        let source = CLAUDE_FAKE.replace(
            "printf '%s\\n' \"$@\" >> claude-argv",
            "touch permission\nprintf '%s\\n' \"$@\" >> claude-argv",
        );
        let w = claude_workspace(dir.path(), &source);
        let mut s = w.snapshots.borrow().clone();
        s.projects[0].defaults.execution.approval = ApprovalMode::Ask;
        w.store.lock().unwrap().save(&s).unwrap();
        w.snapshots.send_replace(s.clone());
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
        let request = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(p) = w.snapshots.borrow().tool_permissions.first().cloned() {
                    break p;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(request.tool, "Write");
        assert_eq!(request.run_id, run);
        let current = w.snapshots.borrow().clone();
        let bad = env(
            current.revision,
            Command::RespondPermission {
                permission_id: request.id.clone(),
                run_id: "stale".into(),
                allow: true,
            },
        );
        assert!(w.store.lock().unwrap().apply(bad, &w.config).is_err());
        if let Some(allow) = choice {
            let envelope = env(
                current.revision,
                Command::RespondPermission {
                    permission_id: request.id.clone(),
                    run_id: run.clone(),
                    allow,
                },
            );
            let mut store = w.store.lock().unwrap();
            let (snapshot, _) = store.apply(envelope.clone(), &w.config).unwrap();
            let (retry, action) = store.apply(envelope, &w.config).unwrap();
            assert_eq!(snapshot, retry);
            assert!(action.is_none());
            w.snapshots.send_replace(snapshot);
            drop(store);
        } else {
            apply(
                &w,
                Command::StopWorker {
                    session_id: id.clone(),
                    include_descendants: false,
                },
            );
        }
        task.await.unwrap();
        let s = finished(&w, &id).await;
        assert_eq!(
            worker(&s, &id).status,
            if choice.is_some() {
                WorkerStatus::Completed
            } else {
                WorkerStatus::Stopped
            }
        );
        let p = s
            .tool_permissions
            .iter()
            .find(|p| p.id == request.id)
            .unwrap();
        if let Some(allow) = choice {
            assert_eq!(p.decision, Some(allow));
            let response: Value = serde_json::from_str(
                &std::fs::read_to_string(
                    Path::new(worker(&s, &id).worktree.as_ref().unwrap())
                        .join("permission-response"),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(response["response"]["request_id"], "native-tool-request");
            assert_eq!(
                response["response"]["response"]["behavior"],
                if allow { "allow" } else { "deny" }
            );
            if allow {
                assert_eq!(
                    response["response"]["response"]["updatedInput"]["content"],
                    "sample"
                );
            }
        } else {
            assert!(p.expired);
            assert!(p.decision.is_none());
        }
        assert!(
            w.store
                .lock()
                .unwrap()
                .apply(
                    env(
                        s.revision,
                        Command::RespondPermission {
                            permission_id: p.id.clone(),
                            run_id: run,
                            allow: true
                        }
                    ),
                    &w.config
                )
                .is_err()
        );
    }
}

#[tokio::test]
async fn claude_interrupt_and_error_are_truthful() {
    for mode in ["hold", "failure"] {
        let dir = tempfile::tempdir().unwrap();
        let source = format!("touch {mode}\n{CLAUDE_FAKE}");
        let w = claude_workspace(dir.path(), &source);
        let s = w.snapshots.borrow().clone();
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
        if mode == "hold" {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if worker(&w.snapshots.borrow(), &id).thread_id.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            apply(
                &w,
                Command::StopWorker {
                    session_id: id.clone(),
                    include_descendants: false,
                },
            );
        }
        task.await.unwrap();
        let s = finished(&w, &id).await;
        let worker = worker(&s, &id);
        assert_eq!(
            worker.status,
            if mode == "hold" {
                WorkerStatus::Stopped
            } else {
                WorkerStatus::Failed
            }
        );
        assert_eq!(worker.thread_id.as_deref(), Some(CLAUDE_THREAD));
        assert!(
            !worker
                .error
                .as_deref()
                .unwrap_or("")
                .contains("private detail")
        );
        if mode == "hold" {
            assert!(
                std::fs::read_to_string(
                    Path::new(worker.worktree.as_ref().unwrap()).join("claude-input.jsonl")
                )
                .unwrap()
                .contains("relay-interrupt")
            );
        }
    }
}

#[test]
fn schema_four_updates_only_inherited_bundled_defaults_and_keeps_old_threads() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let mut s = demo_snapshot(AgentProfile::default());
    s.sessions[0].worker = Some(WorkerRun {
        launched_network_policy: None,
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Completed,
        thread_id: Some("legacy-exact-thread".into()),
        worktree: Some("/legacy/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    s.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Ask);
    let mut custom = s.projects[0].clone();
    custom.id = "custom".into();
    custom.defaults.max_workers = 1;
    s.projects.push(custom.clone());
    s.agent_configs[0].overrides.permissions = Some(s.projects[0].defaults.permissions.clone());
    let mut json = serde_json::to_value(&s).unwrap();
    for key in ["bindings", "installations", "tool_permissions"] {
        json.as_object_mut().unwrap().remove(key);
    }
    for session in json["sessions"].as_array_mut().unwrap() {
        if let Some(worker) = session["worker"].as_object_mut() {
            worker.remove("harness");
            worker.remove("execution");
        }
    }
    for p in json["projects"].as_array_mut().unwrap() {
        p["defaults"].as_object_mut().unwrap().remove("execution");
    }
    let store = Store::open(&db, AgentProfile::default()).unwrap();
    store
        .connection
        .execute("UPDATE workspace SET snapshot=?1", [json.to_string()])
        .unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 3)
        .unwrap();
    drop(store);
    let store = Store::open(&db, AgentProfile::default()).unwrap();
    let next = store.snapshot().unwrap();
    assert_eq!(
        next.projects[0].defaults.permissions[&Task::Implement],
        Permission::Allow
    );
    assert_eq!(next.projects[1], custom);
    assert_eq!(next.agent_configs, s.agent_configs);
    assert_eq!(next.sessions, s.sessions);
    assert_eq!(next.messages, s.messages);
    assert_eq!(
        next.projects[0].defaults.execution.approval,
        ApprovalMode::Automatic
    );
    assert!(next.bindings.is_empty());
    drop(store);
    assert_eq!(
        Store::open(&db, AgentProfile::default())
            .unwrap()
            .snapshot()
            .unwrap(),
        next
    );
}

#[test]
fn registered_project_and_harness_configuration_persist_without_environment() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:jens-hj/relay.git",
        ],
    );
    let db = dir.path().join("db");
    let mut store = Store::open(&db, AgentProfile::default()).unwrap();
    let demo = store.snapshot().unwrap();
    let mut s = demo.clone();
    for number in [5, 6] {
        let (next, action) = store
            .apply(
                env(
                    s.revision,
                    Command::ConfigureProject {
                        binding: ProjectBinding {
                            repository: "jens-hj/relay".into(),
                            owner: "jens-hj".into(),
                            number,
                            checkout: repo.display().to_string(),
                        },
                    },
                ),
                &RuntimeConfig::default(),
            )
            .unwrap();
        assert!(matches!(action, Some(Action::Sync(_))));
        s = next;
    }
    let (s, _) = store
        .apply(
            env(
                s.revision,
                Command::ConfigureHarness {
                    harness: Harness::ClaudeCode,
                    executable: "/saved/claude".into(),
                },
            ),
            &RuntimeConfig::default(),
        )
        .unwrap();
    assert_eq!(s.bindings.len(), 2);
    assert_eq!(s.projects.len(), 3);
    assert_eq!(s.issues, demo.issues);
    assert_eq!(s.sessions, demo.sessions);
    for number in [5, 6] {
        let id = format!("github-project:jens-hj:{number}:jens-hj/relay");
        let c = harness::configuration(&s, &id, &RuntimeConfig::default());
        assert_eq!(c.remote.unwrap().number, number);
        assert_eq!(c.repository, Some(repo.clone()));
        assert_eq!(c.claude, PathBuf::from("/saved/claude"));
    }
    let invalid = Command::ConfigureProject {
        binding: ProjectBinding {
            repository: "other/repo".into(),
            owner: "other".into(),
            number: 1,
            checkout: repo.display().to_string(),
        },
    };
    assert!(
        store
            .apply(env(s.revision, invalid), &RuntimeConfig::default())
            .is_err()
    );
    assert_eq!(store.snapshot().unwrap(), s);
    drop(store);
    assert_eq!(
        Store::open(&db, AgentProfile::default())
            .unwrap()
            .snapshot()
            .unwrap(),
        s
    );
}

#[tokio::test]
async fn harness_probes_are_read_only_cached_independent_and_credential_safe() {
    let dir = tempfile::tempdir().unwrap();
    let codex = dir.path().join("fake-codex");
    let claude = dir.path().join("fake-claude");
    let log = dir.path().join("probes");
    script(
        &codex,
        &format!(
            "echo x >> '{}'\ncase \"$*\" in '--version') echo 'codex 0.test';; '--help') echo 'app-server';; 'login status') echo 'secret-login-detail'; exit 1;; *) exit 9;; esac",
            log.display()
        ),
    );
    script(
        &claude,
        "case \"$*\" in '--version') echo '2.test (Claude Code)';; '--help') echo '--input-format --include-partial-messages --permission-mode';; 'auth status --json') echo '{\"loggedIn\":true,\"apiKey\":\"private-key\"}';; *) exit 9;; esac",
    );
    let c = RuntimeConfig {
        codex,
        claude,
        ..Default::default()
    };
    let s = demo_snapshot(AgentProfile::default());
    let w = workspace(&dir.path().join("db"), &s, c);
    let (a, b) = tokio::join!(
        harness::statuses(State(w.clone())),
        harness::statuses(State(w.clone()))
    );
    let a = a.unwrap().0;
    assert_eq!(a, b.unwrap().0);
    assert_eq!(a[0].state, "signed_out");
    assert_eq!(a[1].state, "ready");
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 3);
    let encoded = serde_json::to_string(&a).unwrap();
    assert!(!encoded.contains("private-key"));
    assert!(!encoded.contains("secret-login-detail"));
    let _ = harness::refresh(State(w.clone())).await.unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 6);
    assert_eq!(w.store.lock().unwrap().snapshot().unwrap(), s);
}

#[tokio::test]
async fn codex_policy_modes_and_native_permission_response_use_exact_request() {
    const FAKE: &str = r#"
printf '%s\n' "$1" "$2" "$3" "$4" "$5" "$6" > codex-launch
while IFS= read -r line; do
  echo "$line" >> codex-input.jsonl
  method=$(printf '%s' "$line" | jq -r '.method // "response"')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    thread/start|thread/resume) printf '{"id":%s,"result":{"thread":{"id":"codex-thread"}}}\n' "$id" ;;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
      while [ ! -f release-first-turn ]; do sleep 0.01; done
      if [ -f ask ]; then
        rm ask
        echo '{"id":"permission-91","method":"item/commandExecution/requestApproval","params":{"threadId":"codex-thread","turnId":"turn-1","command":"printf hello"}}'
      else echo '{"method":"turn/completed","params":{"threadId":"codex-thread","turn":{"id":"turn-1","status":"completed"}}}'; fi ;;
    response) echo '{"method":"turn/completed","params":{"threadId":"codex-thread","turn":{"id":"turn-1","status":"completed"}}}' ;;
  esac
done
"#;
    for (mode, network_access) in ApprovalMode::ALL
        .into_iter()
        .flat_map(|mode| [true, false].map(|network| (mode, network)))
    {
        let dir = tempfile::tempdir().unwrap();
        let repo = review_repo(dir.path());
        let bin = dir.path().join("fake-codex");
        script(
            &bin,
            &format!(
                "{}\n{FAKE}",
                if mode == ApprovalMode::Ask {
                    "if [ ! -f asked ]; then touch ask asked; fi"
                } else {
                    "true"
                }
            ),
        );
        let mut s = live();
        s.projects[0].defaults = AgentProfile::default();
        s.projects[0].defaults.execution.approval = mode;
        s.projects[0].defaults.network_access = network_access;
        let c = RuntimeConfig {
            repository: Some(repo),
            codex: bin,
            ..config()
        };
        let w = workspace(&dir.path().join("db"), &s, c);
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        assert_eq!(
            worker(&w.snapshots.borrow(), &id).launched_network_policy,
            None
        );
        let launched = LaunchedNetworkPolicy::new(Harness::Codex, mode, network_access);
        let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
        let worktree = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let current = w.snapshots.borrow().clone();
                let worker = worker(&current, &id);
                if let Some(path) = &worker.worktree {
                    let path = PathBuf::from(path);
                    let input =
                        std::fs::read_to_string(path.join("codex-input.jsonl")).unwrap_or_default();
                    if input.lines().any(|line| {
                        serde_json::from_str::<Value>(line)
                            .is_ok_and(|v| v["method"] == "turn/start")
                    }) {
                        assert_eq!(worker.status, WorkerStatus::Running);
                        assert_eq!(worker.launched_network_policy, Some(launched));
                        break path;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // Saved edits while the real harness process is held cannot rewrite
        // the current policy. The next process must receive the changed value.
        apply(
            &w,
            Command::SetSessionNetworkAccess {
                session_id: id.clone(),
                network_access: Some(!network_access),
            },
        );
        apply(
            &w,
            Command::SetWorkerExecution {
                session_id: id.clone(),
                execution: Some(ExecutionSettings {
                    approval: ApprovalMode::Unrestricted,
                }),
            },
        );
        let mut edited_profile = w.snapshots.borrow().projects[0].defaults.clone();
        edited_profile.harness = Harness::ClaudeCode;
        let project_id = w.snapshots.borrow().projects[0].id.clone();
        let edited = apply(
            &w,
            Command::UpdateProjectDefaults {
                project_id: project_id.clone(),
                profile: edited_profile,
            },
        );
        assert_eq!(worker(&edited, &id).launched_network_policy, Some(launched));
        assert_eq!(worker(&edited, &id).harness, Harness::Codex);
        // Check the durable snapshot as well as publication.
        assert_eq!(
            worker(&w.store.lock().unwrap().snapshot().unwrap(), &id).launched_network_policy,
            Some(launched)
        );
        apply(
            &w,
            Command::UpdateProjectDefaults {
                project_id,
                profile: s.projects[0].defaults.clone(),
            },
        );
        apply(
            &w,
            Command::SetWorkerExecution {
                session_id: id.clone(),
                execution: None,
            },
        );
        std::fs::write(worktree.join("release-first-turn"), "").unwrap();
        if mode == ApprovalMode::Ask {
            let permission = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Some(p) = w.snapshots.borrow().tool_permissions.first().cloned() {
                        break p;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            apply(
                &w,
                Command::RespondPermission {
                    permission_id: permission.id,
                    run_id: run.clone(),
                    allow: false,
                },
            );
        }
        task.await.unwrap();
        let s = finished(&w, &id).await;
        assert_eq!(worker(&s, &id).launched_network_policy, Some(launched));
        assert_eq!(
            worker(&s, &id).status,
            WorkerStatus::Completed,
            "{:?}",
            worker(&s, &id).error
        );
        let lines = std::fs::read_to_string(
            Path::new(worker(&s, &id).worktree.as_ref().unwrap()).join("codex-input.jsonl"),
        )
        .unwrap();
        let values: Vec<Value> = lines
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let turn = values.iter().find(|v| v["method"] == "turn/start").unwrap();
        let thread = values
            .iter()
            .find(|v| v["method"] == "thread/start")
            .unwrap();
        let sandbox = if mode == ApprovalMode::Unrestricted {
            "danger-full-access"
        } else {
            "workspace-write"
        };
        assert_eq!(thread["params"]["sandbox"], sandbox);
        assert_eq!(
            thread["params"]["config"]["sandbox_workspace_write.network_access"],
            network_access
        );
        assert_eq!(
            thread["params"]["approvalPolicy"],
            turn["params"]["approvalPolicy"]
        );
        let worktree = Path::new(worker(&s, &id).worktree.as_ref().unwrap());
        assert_eq!(
            std::fs::read_to_string(worktree.join("codex-launch")).unwrap(),
            format!(
                "app-server\n--stdio\n-c\nsandbox_mode=\"{sandbox}\"\n-c\nsandbox_workspace_write.network_access={network_access}\n"
            )
        );
        if mode != ApprovalMode::Unrestricted {
            assert_eq!(
                turn["params"]["sandboxPolicy"]["networkAccess"],
                network_access
            );
            assert!(
                turn["params"]["sandboxPolicy"]["writableRoots"]
                    .as_array()
                    .is_some_and(|roots| !roots.is_empty())
            );
        }
        assert_eq!(
            turn["params"]["approvalPolicy"],
            if mode == ApprovalMode::Ask {
                "on-request"
            } else {
                "never"
            }
        );
        assert_eq!(
            turn["params"]["sandboxPolicy"]["type"],
            if mode == ApprovalMode::Unrestricted {
                "dangerFullAccess"
            } else {
                "workspaceWrite"
            }
        );
        if mode == ApprovalMode::Ask {
            assert!(
                values
                    .iter()
                    .any(|v| v["id"] == "permission-91" && v["result"]["decision"] == "decline")
            );
        }
        // A session override is independent of the saved execution mode and
        // must replace both defaults and a resumed thread's previous policy.
        apply(
            &w,
            Command::SetSessionNetworkAccess {
                session_id: id.clone(),
                network_access: Some(!network_access),
            },
        );
        let override_snapshot = apply(
            &w,
            Command::SetWorkerExecution {
                session_id: id.clone(),
                execution: Some(ExecutionSettings { approval: mode }),
            },
        );
        let override_session = override_snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap();
        assert_eq!(
            override_snapshot
                .session_profile(override_session)
                .unwrap()
                .network_access,
            !network_access
        );
        let inherited = apply(
            &w,
            Command::SetWorkerExecution {
                session_id: id.clone(),
                execution: None,
            },
        );
        assert_eq!(
            inherited
                .sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .network_access,
            Some(!network_access)
        );
        let next = w.snapshots.borrow().clone();
        let (_, next_run, rx, prompt) = reserve(
            &w,
            env(
                next.revision,
                Command::SendWorker {
                    session_id: id.clone(),
                    prompt: "Continue".into(),
                    approve_implementation: true,
                },
            ),
        );
        assert_eq!(
            worker(&w.snapshots.borrow(), &id).launched_network_policy,
            Some(launched)
        );
        runtime::run(w.clone(), id.clone(), next_run, prompt, rx).await;
        let resumed = finished(&w, &id).await;
        assert_eq!(
            worker(&resumed, &id).launched_network_policy,
            Some(LaunchedNetworkPolicy::new(
                Harness::Codex,
                mode,
                !network_access,
            ))
        );
        assert_eq!(
            worker(&resumed, &id).status,
            WorkerStatus::Completed,
            "{:?}",
            worker(&resumed, &id).error
        );
        let lines = std::fs::read_to_string(worktree.join("codex-input.jsonl")).unwrap();
        let values: Vec<Value> = lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let thread = values
            .iter()
            .find(|v| v["method"] == "thread/resume")
            .unwrap();
        assert_eq!(
            thread["params"]["config"]["sandbox_workspace_write.network_access"],
            !network_access
        );
        assert_eq!(thread["params"]["sandbox"], sandbox);
        assert_eq!(
            thread["params"]["approvalPolicy"],
            turn["params"]["approvalPolicy"]
        );
        let resumed_turn = values
            .iter()
            .rfind(|v| v["method"] == "turn/start")
            .unwrap();
        assert_eq!(
            resumed_turn["params"]["approvalPolicy"],
            turn["params"]["approvalPolicy"]
        );
        if mode != ApprovalMode::Unrestricted {
            assert_eq!(
                resumed_turn["params"]["sandboxPolicy"]["networkAccess"],
                !network_access
            );
            assert_eq!(
                resumed_turn["params"]["sandboxPolicy"]["writableRoots"],
                turn["params"]["sandboxPolicy"]["writableRoots"]
            );
        }
        if mode == ApprovalMode::Automatic && network_access {
            // A subsequent queued turn and an actual exec failure retain the
            // last successful launch; neither may claim the edited preference.
            let previous = worker(&resumed, &id).launched_network_policy;
            apply(
                &w,
                Command::SetSessionNetworkAccess {
                    session_id: id.clone(),
                    network_access: Some(network_access),
                },
            );
            std::fs::remove_file(&w.config.codex).unwrap();
            let revision = w.snapshots.borrow().revision;
            let (_, failed_run, rx, prompt) = reserve(
                &w,
                env(
                    revision,
                    Command::SendWorker {
                        session_id: id.clone(),
                        prompt: "Fail to launch".into(),
                        approve_implementation: true,
                    },
                ),
            );
            assert_eq!(
                worker(&w.snapshots.borrow(), &id).launched_network_policy,
                previous
            );
            runtime::run(w.clone(), id.clone(), failed_run, prompt, rx).await;
            let failed = finished(&w, &id).await;
            assert_eq!(worker(&failed, &id).status, WorkerStatus::Failed);
            assert!(
                worker(&failed, &id)
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("Cannot launch agent harness")
            );
            assert_eq!(worker(&failed, &id).launched_network_policy, previous);
        }
    }
}

#[tokio::test]
async fn claude_keeps_control_input_open_until_native_background_work_is_idle() {
    let dir = tempfile::tempdir().unwrap();
    let source=CLAUDE_FAKE.replace(
        "echo '{\"type\":\"result\",\"subtype\":\"success\",\"usage\":",
        "echo '{\"type\":\"system\",\"subtype\":\"task_started\",\"task_id\":\"task-1\",\"task_type\":\"local_agent\"}'\n      echo '{\"type\":\"result\",\"subtype\":\"success\",\"usage\":"
    ).replace(
        "echo '{\"type\":\"system\",\"subtype\":\"session_state_changed\",\"state\":\"idle\",\"sdk_host_only\":true}' ;;",
        "echo '{\"type\":\"system\",\"subtype\":\"task_notification\",\"task_id\":\"task-1\"}'\n      echo '{\"type\":\"control_request\",\"request_id\":\"native-tool-request\",\"request\":{\"subtype\":\"can_use_tool\",\"tool_name\":\"Read\",\"input\":{\"file_path\":\"notes.txt\"}}}' ;;"
    );
    // Restore the idle marker in the permission response branch, which runs
    // only if stdin was kept open after the first result.
    let index = source.find("    control_response)").unwrap();
    let source = format!(
        "{}{}",
        &source[..index],
        &CLAUDE_FAKE[CLAUDE_FAKE.find("    control_response)").unwrap()..]
    );
    let w = claude_workspace(dir.path(), &source);
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(p) = w.snapshots.borrow().tool_permissions.first().cloned() {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    apply(
        &w,
        Command::RespondPermission {
            permission_id: request.id,
            run_id: run,
            allow: true,
        },
    );
    task.await.unwrap();
    let s = finished(&w, &id).await;
    assert_eq!(
        worker(&s, &id).status,
        WorkerStatus::Completed,
        "{:?}",
        worker(&s, &id).error
    );
}

/// A turn that ends while a background shell task runs. Unless `never`
/// exists, the task notifies after `BG_DELAY` seconds and starts a follow-up
/// turn whose control request needs the input channel still open.
const CLAUDE_BACKGROUND_FAKE: &str = r#"
session='"session_id":"11111111-2222-4333-8444-555555555555"'
while IFS= read -r line; do
  printf '%s\n' "$line" >> claude-input.jsonl
  type=$(printf '%s' "$line" | jq -r '.type')
  case "$type" in
    control_request)
      if [ "$(printf '%s' "$line" | jq -r '.request.subtype')" = interrupt ]; then
        echo "{\"type\":\"result\",\"subtype\":\"success\",$session}"
        continue
      fi
      request=$(printf '%s' "$line" | jq -r '.request_id')
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}\n' "$request" ;;
    user)
      echo "{\"type\":\"system\",\"subtype\":\"session_state_changed\",\"state\":\"running\",$session}"
      echo "{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"m\",$session}"
      echo "{\"type\":\"assistant\",\"message\":{\"id\":\"first\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_bg\",\"name\":\"Bash\",\"input\":{\"command\":\"sleep 20; echo done\",\"run_in_background\":true}}]},$session}"
      echo "{\"type\":\"system\",\"subtype\":\"background_tasks_changed\",\"tasks\":[{\"task_id\":\"bg-1\",\"task_type\":\"local_bash\",\"description\":\"Run tests\"}],$session}"
      echo "{\"type\":\"system\",\"subtype\":\"task_started\",\"task_id\":\"bg-1\",\"task_type\":\"local_bash\",\"description\":\"Run tests\",\"is_backgrounded\":true,$session}"
      echo "{\"type\":\"assistant\",\"message\":{\"id\":\"first-text\",\"content\":[{\"type\":\"text\",\"text\":\"I'll report when the tests finish.\"}]},$session}"
      echo "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"I'll report when the tests finish.\",$session}"
      echo "{\"type\":\"system\",\"subtype\":\"session_state_changed\",\"state\":\"idle\",$session}"
      (
        sleep "${BG_DELAY:-1}"
        if [ -f never ]; then sleep 600; fi
        echo "{\"type\":\"system\",\"subtype\":\"task_updated\",\"task_id\":\"bg-1\",\"patch\":{\"status\":\"completed\"},$session}"
        echo "{\"type\":\"system\",\"subtype\":\"task_notification\",\"task_id\":\"bg-1\",\"status\":\"completed\",$session}"
        echo "{\"type\":\"system\",\"subtype\":\"background_tasks_changed\",\"tasks\":[],$session}"
        echo "{\"type\":\"system\",\"subtype\":\"session_state_changed\",\"state\":\"running\",$session}"
        echo "{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"m\",$session}"
        echo '{"type":"control_request","request_id":"follow-up","request":{"subtype":"mcp_message"}}'
      ) & ;;
    control_response)
      echo "{\"type\":\"assistant\",\"message\":{\"id\":\"follow-up\",\"content\":[{\"type\":\"text\",\"text\":\"Tests passed: 12\"}]},$session}"
      echo "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"Tests passed: 12\",$session}"
      echo "{\"type\":\"system\",\"subtype\":\"session_state_changed\",\"state\":\"idle\",$session}" ;;
  esac
done
wait
"#;

async fn background_run(
    source: &str,
    config: RuntimeConfig,
) -> (
    tempfile::TempDir,
    Workspace,
    String,
    tokio::task::JoinHandle<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    let w = claude_workspace_with(dir.path(), source, config);
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
    (dir, w, id, task)
}
fn transcript(s: &Snapshot, id: &str) -> Vec<String> {
    s.messages
        .iter()
        .filter(|m| m.session_id == id)
        .map(|m| m.body.clone())
        .collect()
}

#[tokio::test]
async fn claude_waits_for_background_shell_task_and_captures_follow_up_turn() {
    let (_dir, w, id, task) = background_run(CLAUDE_BACKGROUND_FAKE, config()).await;
    task.await.unwrap();
    let s = finished(&w, &id).await;
    let worker = worker(&s, &id);
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
    let input = std::fs::read_to_string(
        Path::new(worker.worktree.as_ref().unwrap()).join("claude-input.jsonl"),
    )
    .unwrap();
    // The follow-up turn's control request was answered on the open channel.
    assert!(input.contains("\"request_id\":\"follow-up\""), "{input}");
    let transcript = transcript(&s, &id);
    assert!(
        transcript
            .iter()
            .any(|m| m == "I'll report when the tests finish.")
    );
    assert!(
        transcript.iter().any(|m| m == "Tests passed: 12"),
        "{transcript:?}"
    );
}

#[tokio::test]
async fn claude_background_task_past_limit_fails_with_task_name() {
    let source = format!("touch never\n{CLAUDE_BACKGROUND_FAKE}");
    let config = RuntimeConfig {
        background_task_limit: Duration::from_secs(1),
        ..config()
    };
    let (_dir, w, id, task) = background_run(&source, config).await;
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    let s = finished(&w, &id).await;
    let worker = worker(&s, &id);
    assert_eq!(worker.status, WorkerStatus::Failed);
    let error = worker.error.as_deref().unwrap_or("");
    assert!(
        error.contains("background task \"Run tests\" was still running"),
        "{error}"
    );
    assert!(
        !error.contains("did not finish after its result"),
        "{error}"
    );
}

#[tokio::test]
async fn claude_stop_interrupts_while_waiting_for_background_task() {
    let source = format!("touch never\n{CLAUDE_BACKGROUND_FAKE}");
    let (_dir, w, id, task) = background_run(&source, config()).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if transcript(&w.snapshots.borrow(), &id)
                .iter()
                .any(|m| m == "I'll report when the tests finish.")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Let Relay consume the result and begin waiting on the pending task.
    tokio::time::sleep(Duration::from_millis(200)).await;
    apply(
        &w,
        Command::StopWorker {
            session_id: id.clone(),
            include_descendants: false,
        },
    );
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    let s = finished(&w, &id).await;
    let worker = worker(&s, &id);
    assert_eq!(worker.status, WorkerStatus::Stopped, "{:?}", worker.error);
    assert!(
        std::fs::read_to_string(
            Path::new(worker.worktree.as_ref().unwrap()).join("claude-input.jsonl")
        )
        .unwrap()
        .contains("relay-interrupt")
    );
}

#[tokio::test]
#[ignore = "requires installed authenticated Claude Code"]
async fn installed_claude_background_command_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = claude_workspace(dir.path(), "");
    let mut config = (*w.config).clone();
    config.claude = RuntimeConfig::default().claude;
    w.config = std::sync::Arc::new(config);
    let mut s = w.snapshots.borrow().clone();
    s.projects[0].defaults.execution.approval = ApprovalMode::Unrestricted;
    w.store.lock().unwrap().save(&s).unwrap();
    w.snapshots.send_replace(s.clone());
    let Command::StartWorker {
        issue_id,
        agent_config_id,
        ..
    } = start(&s)
    else {
        unreachable!()
    };
    let command = Command::StartWorker {
        issue_id,
        agent_config_id,
        prompt: "Use the Bash tool with run_in_background set to true to run exactly: sleep 20; echo relay-background-done. Then end your turn immediately, saying you will wait for the notification. When notified, reply with the command's output line only.".into(),
        approve_implementation: true,
    };
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, command));
    tokio::time::timeout(
        Duration::from_secs(180),
        runtime::run(w.clone(), id.clone(), run, prompt, rx),
    )
    .await
    .unwrap();
    let s = finished(&w, &id).await;
    let worker = worker(&s, &id);
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
    let transcript = transcript(&s, &id);
    assert!(
        transcript
            .iter()
            .any(|m| m.trim_matches('`').trim() == "relay-background-done"),
        "{transcript:?}"
    );
}

#[test]
fn fresh_workspace_and_project_respect_explicit_template_without_migrating_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:jens-hj/relay.git",
        ],
    );
    // Even a template identical to the former bundled profile is an explicit
    // choice when creating new data, not legacy data requiring migration.
    let mut defaults = AgentProfile::default();
    defaults
        .permissions
        .insert(Task::Implement, Permission::Ask);
    let mut store = Store::open(&dir.path().join("db"), defaults.clone()).unwrap();
    let s = store.snapshot().unwrap();
    assert_eq!(s.projects[0].defaults, defaults);
    let (s, _) = store
        .apply(
            env(
                s.revision,
                Command::ConfigureProject {
                    binding: ProjectBinding {
                        repository: "jens-hj/relay".into(),
                        owner: "jens-hj".into(),
                        number: 5,
                        checkout: repo.display().to_string(),
                    },
                },
            ),
            &RuntimeConfig::default(),
        )
        .unwrap();
    assert_eq!(s.projects[0].defaults, defaults);
}

#[tokio::test]
async fn claude_completion_cleans_descendants_even_after_native_parent_exits() {
    let dir = tempfile::tempdir().unwrap();
    let source = format!("sleep 60 >/dev/null 2>&1 &\necho $! > descendant.pid\n{CLAUDE_FAKE}");
    let w = claude_workspace(dir.path(), &source);
    let (id, s) = launch(&w).await;
    let pid: i32 = std::fs::read_to_string(
        Path::new(worker(&s, &id).worktree.as_ref().unwrap()).join("descendant.pid"),
    )
    .unwrap()
    .trim()
    .parse()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let gone = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .map(|s| s.rsplit_once(')').unwrap().1.split_whitespace().next() == Some("Z"))
                .unwrap_or(true);
            if gone {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn claude_large_streaming_event_preserves_completion_and_bounds_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let large = r#"printf '{"type":"assistant","message":{"id":"response","content":[{"type":"text","text":"'
      head -c 2097152 /dev/zero | tr '\000' x
      printf '"}]}}\n'"#;
    let source = CLAUDE_FAKE.replace(
        r#"echo '{"type":"assistant","message":{"id":"response","content":[{"type":"text","text":"Complete λ"}]}}'"#,
        large,
    );
    let source = source.replace("content_block_delta", "ignored_delta");
    let w = claude_workspace(dir.path(), &source);
    let (id, snapshot) = launch(&w).await;
    let message = snapshot
        .messages
        .iter()
        .find(|m| m.session_id == id && m.author == "Claude Code")
        .unwrap();
    assert!(message.body.starts_with("xxxx"));
    assert!(message.body.len() < 2097152);
    assert_eq!(
        worker(&snapshot, &id).thread_id.as_deref(),
        Some(CLAUDE_THREAD)
    );
}

#[tokio::test]
async fn harness_streaming_reader_preserves_large_events_and_enforces_ceiling() {
    for (size, newline) in [
        (2097152, true),
        (2097152, false),
        (64 * 1024 * 1024, false),
        (64 * 1024 * 1024 + 1, false),
    ] {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "head -c {size} /dev/zero{}",
                if newline {
                    "; printf '\\n'; printf 'next\\n'"
                } else {
                    ""
                }
            ))
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut reader = tokio::io::BufReader::new(child.stdout.take().unwrap());
        let result = runtime::line(&mut reader).await;
        if size > 64 * 1024 * 1024 {
            assert!(result.is_err());
            child.kill().await.unwrap();
        } else {
            assert_eq!(result.unwrap().unwrap().len(), size + usize::from(newline));
            if newline {
                assert_eq!(
                    runtime::line(&mut reader).await.unwrap().unwrap(),
                    b"next\n"
                );
            }
            assert!(runtime::line(&mut reader).await.unwrap().is_none());
            assert!(child.wait().await.unwrap().success());
        }
    }
}

#[tokio::test]
async fn codex_large_streaming_event_preserves_completion_and_bounds_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let bin = dir.path().join("fake-codex");
    let source = MULTI_CODEX.replace(
        "\"text\":\"Edited selected workspaces\"",
        "\"text\":\"'\n      head -c 2097152 /dev/zero | tr '\\000' x\n      printf '%s\\n' '\"",
    ).replace("printf '%s\\n' '{\"method\":\"item/completed\"", "printf '%s' '{\"method\":\"item/completed\"");
    script(&bin, &source);
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let w = workspace(&dir.path().join("db"), &live(), c);
    let (id, snapshot) = launch(&w).await;
    let message = snapshot
        .messages
        .iter()
        .find(|m| m.session_id == id && m.author == "Codex")
        .unwrap();
    assert!(message.body.starts_with("xxxx"));
    assert!(message.body.len() < 2097152);
    assert_eq!(
        worker(&snapshot, &id).thread_id.as_deref(),
        Some("multi-workspace-thread")
    );
}

#[test]
fn schema_six_defaults_missing_run_metadata_and_preserves_it_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let mut snapshot = demo_snapshot(AgentProfile::default());
    snapshot.sessions[0].worker = Some(WorkerRun {
        launched_network_policy: None,
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Completed,
        thread_id: Some("exact-thread".into()),
        worktree: Some("/existing/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let mut json = serde_json::to_value(&snapshot).unwrap();
    let worker = json["sessions"][0]["worker"].as_object_mut().unwrap();
    for key in ["model", "context_tokens", "context_window"] {
        worker.remove(key);
    }
    let store = Store::open(&db, AgentProfile::default()).unwrap();
    store
        .connection
        .execute("UPDATE workspace SET snapshot=?1", [json.to_string()])
        .unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 6)
        .unwrap();
    drop(store);
    let store = Store::open(&db, AgentProfile::default()).unwrap();
    assert_eq!(store.snapshot().unwrap(), snapshot);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        27
    );
    let worker = snapshot.sessions[0].worker.as_mut().unwrap();
    worker.model = Some("reported-model".into());
    worker.context_tokens = Some(0);
    worker.context_window = Some(200000);
    store
        .connection
        .execute(
            "UPDATE workspace SET snapshot=?1",
            [serde_json::to_string(&snapshot).unwrap()],
        )
        .unwrap();
    drop(store);
    assert_eq!(
        Store::open(&db, AgentProfile::default())
            .unwrap()
            .snapshot()
            .unwrap(),
        snapshot
    );
}

#[tokio::test]
async fn codex_tool_lifecycle_streams_and_survives_storage() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let bin = dir.path().join("tool-codex");
    script(
        &bin,
        r#"
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id";;
    thread/start) printf '{"id":%s,"result":{"thread":{"id":"tool-thread"}}}\n' "$id";;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"tools"}}}\n' "$id"
      echo '{"method":"item/started","params":{"threadId":"tool-thread","item":{"id":"terminal","type":"commandExecution","command":"printf live; exit 2","cwd":"/workspace","status":"inProgress"}}}'
      echo '{"method":"item/commandExecution/outputDelta","params":{"threadId":"tool-thread","itemId":"terminal","delta":"live\n"}}'
      sleep 1
      echo '{"method":"item/completed","params":{"threadId":"tool-thread","item":{"id":"terminal","type":"commandExecution","command":"printf live; exit 2","cwd":"/workspace","status":"completed","exitCode":2,"aggregatedOutput":"live\n"}}}'
      echo '{"method":"item/started","params":{"threadId":"tool-thread","item":{"id":"files","type":"fileChange","status":"inProgress","changes":[{"path":"main.rs"}]}}}'
      echo '{"method":"item/completed","params":{"threadId":"tool-thread","item":{"id":"files","type":"fileChange","status":"completed","changes":[{"path":"main.rs","diff":"-old\n+new"}]}}}'
      echo '{"method":"item/completed","params":{"threadId":"tool-thread","item":{"id":"web","type":"webSearch","action":{"query":"Rust docs"}}}}'
      echo '{"method":"item/completed","params":{"threadId":"tool-thread","item":{"id":"remote","type":"mcpToolCall","tool":"read_file","arguments":{"path":"README.md"},"result":{"content":[{"type":"text","text":"Contents"}]}}}}'
      echo '{"method":"item/completed","params":{"threadId":"tool-thread","item":{"id":"remote-error","type":"mcpToolCall","tool":"read_file","arguments":{"path":"missing.rs"},"result":null,"error":{"message":"File not found"}}}}'
      echo '{"method":"item/started","params":{"threadId":"tool-thread","item":{"id":"unfinished","type":"commandExecution","command":"sleep 100"}}}'
      echo '{"method":"turn/completed","params":{"threadId":"tool-thread","turn":{"id":"tools","status":"completed"}}}'
      ;;
  esac
done
"#,
    );
    let mut config = config();
    config.repository = Some(repo);
    config.codex = bin;
    let w = workspace(&dir.path().join("db"), &live(), config);
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    let mut events = w.snapshots.subscribe();
    let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s = events.borrow_and_update().clone();
            if let Some(m) = s.messages.iter().find(|m| {
                m.tool
                    .as_ref()
                    .is_some_and(|t| t.run_id == run && t.kind == ToolKind::Run)
                    && m.body == "live\n"
            }) {
                assert_eq!(m.tool.as_ref().unwrap().state, ToolState::Running);
                break;
            }
            events.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    task.await.unwrap();
    let snapshot = finished(&w, &id).await;
    let calls = snapshot
        .messages
        .iter()
        .filter_map(|m| m.tool.as_ref())
        .filter(|t| t.run_id == run)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[0].state, ToolState::Failed);
    assert_eq!(calls[0].exit_code, Some(2));
    assert_eq!(calls[1].kind, ToolKind::Edit);
    assert_eq!(calls[1].target, "main.rs");
    assert_eq!(calls[2].kind, ToolKind::Fetch);
    assert_eq!(calls[3].kind, ToolKind::Read);
    assert_eq!(calls[4].state, ToolState::Failed);
    assert_eq!(calls[5].state, ToolState::Interrupted);
    assert!(
        snapshot
            .messages
            .iter()
            .find(|m| m.id.ends_with("-files"))
            .unwrap()
            .body
            .contains("+new")
    );
    assert!(
        snapshot
            .messages
            .iter()
            .find(|m| m.id.ends_with("-remote-error"))
            .unwrap()
            .body
            .contains("File not found")
    );
    let reopened = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), snapshot);
}

#[tokio::test]
async fn claude_tool_requests_and_results_share_one_transcript_entry() {
    let dir = tempfile::tempdir().unwrap();
    let events = r#"
      echo '{"type":"assistant","message":{"id":"tools","content":[{"type":"tool_use","id":"bash-1","name":"Bash","input":{"command":"printf hello"}},{"type":"tool_use","id":"read-1","name":"Read","input":{"file_path":"missing.rs"}}]}}'
      echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"bash-1","content":"hello"},{"type":"tool_result","tool_use_id":"read-1","is_error":true,"content":[{"type":"text","text":"File not found"}]}]}}'
      echo '{"type":"assistant","message":{"id":"tools","content":[{"type":"tool_use","id":"bash-1","name":"Bash","input":{"command":"printf hello"}}]}}'
"#;
    let source = CLAUDE_FAKE.replace(
        "      echo '{\"type\":\"stream_event\"",
        &format!("{events}\n      echo '{{\"type\":\"stream_event\""),
    );
    let w = claude_workspace(dir.path(), &source);
    let (id, snapshot) = launch(&w).await;
    let messages = snapshot
        .messages
        .iter()
        .filter(|m| m.session_id == id && m.tool.is_some())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 2);
    let terminal = messages[0];
    assert_eq!(terminal.body, "hello");
    assert_eq!(terminal.tool.as_ref().unwrap().target, "printf hello");
    assert_eq!(terminal.tool.as_ref().unwrap().state, ToolState::Completed);
    assert_eq!(messages[1].tool.as_ref().unwrap().kind, ToolKind::Read);
    assert_eq!(messages[1].tool.as_ref().unwrap().state, ToolState::Failed);
}

#[tokio::test]
async fn claude_streamed_text_after_thinking_is_recorded_once() {
    let dir = tempfile::tempdir().unwrap();
    // Claude Code sends one `assistant` event per content block, so the text
    // block streamed at index 1 arrives at position 0 of its own event.
    let events = r#"
      echo '{"type":"stream_event","event":{"type":"message_start","message":{"id":"thought"}}}'
      echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Plan"}}}'
      echo '{"type":"assistant","message":{"id":"thought","content":[{"type":"thinking","thinking":"Plan"}]}}'
      echo '{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"First "}}}'
      echo '{"type":"assistant","message":{"id":"thought","content":[{"type":"text","text":"First answer"}]}}'
      echo '{"type":"stream_event","event":{"type":"content_block_delta","index":3,"delta":{"type":"text_delta","text":"Second "}}}'
      echo '{"type":"assistant","message":{"id":"thought","content":[{"type":"text","text":"Second answer"}]}}'
"#;
    let source = CLAUDE_FAKE.replacen(
        "      echo '{\"type\":\"stream_event\"",
        &format!("{events}\n      echo '{{\"type\":\"stream_event\""),
        1,
    );
    let w = claude_workspace(dir.path(), &source);
    let (id, snapshot) = launch(&w).await;
    let bodies = snapshot
        .messages
        .iter()
        .filter(|m| m.session_id == id && m.kind == "agent_message")
        .map(|m| m.body.as_str())
        .collect::<Vec<_>>();
    assert_eq!(bodies, ["First answer", "Second answer", "Complete λ"]);
}

#[tokio::test]
async fn restart_interrupts_unfinished_tool_records_without_erasing_output() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let mut snapshot = demo_snapshot(AgentProfile::default());
    // Fixture sessions own no process, so restart repair leaves them alone.
    snapshot.sessions[0].fixture = false;
    snapshot.messages.push(Message {
        id: "unfinished-tool".into(),
        session_id: "session-plan".into(),
        author: "Codex".into(),
        sender: Sender::User,
        kind: "tool_call".into(),
        body: "retained output".into(),
        parts: vec![],
        tool: Some(ToolCall {
            run_id: "previous-run".into(),
            name: "commandExecution".into(),
            kind: ToolKind::Run,
            state: ToolState::Running,
            input: "sleep 100".into(),
            target: "sleep 100".into(),
            directory: None,
            exit_code: None,
            started_ms: None,
            finished_ms: None,
            detail: None,
        }),
    });
    // The demo orchestration's example states survive a restart.
    demo_orchestration(&mut snapshot);
    let mut store = Store::open(&db, AgentProfile::default()).unwrap();
    store.save(&snapshot).unwrap();
    drop(store);
    let (_, shutdown) = router_with_shutdown(
        &db,
        "tool-restart-test-token".into(),
        AgentProfile::default(),
        RuntimeConfig::default(),
    )
    .unwrap();
    let restarted = shutdown.workspace.snapshots.borrow().clone();
    let message = restarted
        .messages
        .iter()
        .find(|m| m.id == "unfinished-tool")
        .unwrap();
    assert_eq!(message.body, "retained output");
    assert_eq!(message.tool.as_ref().unwrap().state, ToolState::Interrupted);
    let example = restarted
        .messages
        .iter()
        .find(|m| m.id == "t-migrate")
        .unwrap();
    assert_eq!(example.tool.as_ref().unwrap().state, ToolState::Running);
    assert!(restarted.tool_permissions.iter().all(|p| !p.expired));
    assert!(restarted.sessions.iter().any(|s| {
        s.worker
            .as_ref()
            .is_some_and(|w| w.status == WorkerStatus::Running)
    }));
    assert_eq!(
        shutdown.workspace.store.lock().unwrap().snapshot().unwrap(),
        restarted
    );
}

#[tokio::test]
async fn codex_reports_compactions_rate_limits_plans_and_tool_details() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let bin = dir.path().join("detail-codex");
    script(
        &bin,
        r#"
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id";;
    thread/start) printf '{"id":%s,"result":{"model":"gpt-test","thread":{"id":"t"}}}\n' "$id";;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"turn"}}}\n' "$id"
      echo '{"method":"thread/tokenUsage/updated","params":{"threadId":"t","tokenUsage":{"modelContextWindow":1000,"last":{"totalTokens":900,"inputTokens":800,"cachedInputTokens":100,"outputTokens":50}}}}'
      echo '{"method":"item/started","params":{"threadId":"t","item":{"id":"c1","type":"contextCompaction"}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"c1","type":"contextCompaction"}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"c1","type":"contextCompaction"}}}'
      echo '{"method":"thread/tokenUsage/updated","params":{"threadId":"t","tokenUsage":{"modelContextWindow":1000,"last":{"totalTokens":300,"inputTokens":250,"cachedInputTokens":100,"outputTokens":50}}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"c2","type":"contextCompaction"}}}'
      echo '{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":42.5,"windowDurationMins":300,"resetsAt":1900000000},"secondary":{"usedPercent":10,"windowDurationMins":10080,"resetsAt":null}}}}'
      echo '{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"other","primary":{"usedPercent":99,"windowDurationMins":300,"resetsAt":1}}}}'
      echo '{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":null,"primary":null,"secondary":{"usedPercent":11,"windowDurationMins":10080,"resetsAt":1900500000}}}}'
      echo '{"method":"turn/plan/updated","params":{"threadId":"t","turnId":"turn","explanation":null,"plan":[{"step":"Read code","status":"completed"},{"step":"Fix bug","status":"inProgress"}]}}'
      echo '{"method":"turn/plan/updated","params":{"threadId":"t","turnId":"turn","explanation":null,"plan":[{"step":"Read code","status":"completed"},{"step":"Fix bug","status":"completed"}]}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"add","type":"fileChange","status":"completed","changes":[{"path":"new.rs","kind":{"type":"add"},"diff":"a\nb\nc\n"}]}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"update","type":"fileChange","status":"completed","changes":[{"path":"main.rs","kind":{"type":"update","move_path":null},"diff":"@@ -1,2 +1,3 @@ fn main\n-old\n+new\n+more\n keep\n"}]}}}'
      echo '{"method":"item/started","params":{"threadId":"t","item":{"id":"test","type":"commandExecution","command":"/bin/bash -lc \"cargo test -p x\"","status":"inProgress"}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"test","type":"commandExecution","command":"/bin/bash -lc \"cargo test -p x\"","status":"failed","exitCode":101,"durationMs":900,"aggregatedOutput":"test a::b ... FAILED\ntest result: FAILED. 3 passed; 1 failed; 0 ignored\n"}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"ls","type":"commandExecution","command":"ls","status":"completed","exitCode":0,"durationMs":12,"aggregatedOutput":"x\n"}}}'
      echo '{"method":"item/completed","params":{"threadId":"t","item":{"id":"spawn","type":"mcpToolCall","server":"relay","tool":"spawn_agent","arguments":{"task":"x"},"result":{"content":[]}}}}'
      echo '{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn","status":"completed"}}}'
      ;;
  esac
done
"#,
    );
    let mut config = config();
    config.repository = Some(repo);
    config.codex = bin;
    let w = workspace(&dir.path().join("db"), &live(), config);
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx).await;
    let snapshot = finished(&w, &id).await;
    let session = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
    assert_eq!(
        session.compactions,
        [
            Compaction {
                at_tokens: Some(900)
            },
            Compaction {
                at_tokens: Some(300)
            }
        ]
    );
    assert_eq!(snapshot.usage_limits.len(), 2);
    let five = &snapshot.usage_limits[0];
    assert_eq!(five.id, "codex-five_hour");
    assert_eq!(
        (five.used_percent, five.window_minutes, five.resets_at),
        (42.5, 300, Some(1_900_000_000))
    );
    let weekly = &snapshot.usage_limits[1];
    assert_eq!(weekly.window, UsageWindow::Weekly);
    assert_eq!(
        (weekly.used_percent, weekly.resets_at),
        (11.0, Some(1_900_500_000))
    );
    let tool = |suffix: &str| {
        snapshot
            .messages
            .iter()
            .find(|m| m.id == format!("codex-{run}-{suffix}"))
            .and_then(|m| m.tool.clone())
            .unwrap()
    };
    let plan = tool("plan-turn");
    assert_eq!(plan.kind, ToolKind::Plan);
    assert!(
        matches!(plan.detail, Some(ToolDetail::Plan { ref steps }) if steps.len() == 2 && steps.iter().all(|s| s.done))
    );
    let add = tool("add");
    assert_eq!(add.kind, ToolKind::Create);
    assert_eq!(add.detail, Some(ToolDetail::Create { lines: 3 }));
    assert!(add.started_ms.is_some() && add.finished_ms >= add.started_ms);
    assert_eq!(
        tool("update").detail,
        Some(ToolDetail::Edit {
            hunks: vec![Hunk {
                line: 1,
                label: Some("fn main".into()),
                added: 2,
                removed: 1
            }],
            added: 2,
            removed: 1
        })
    );
    let test = tool("test");
    assert_eq!(test.kind, ToolKind::Test);
    assert_eq!(test.state, ToolState::Failed);
    assert_eq!(test.exit_code, Some(101));
    assert_eq!(
        test.detail,
        Some(ToolDetail::Test {
            passed: Some(3),
            failed: Some(1),
            failures: vec!["a::b".into()]
        })
    );
    assert_eq!(
        tool("ls").detail,
        Some(ToolDetail::Run {
            exit_code: Some(0),
            duration_ms: Some(12)
        })
    );
    assert_eq!(tool("spawn").kind, ToolKind::Spawn);
    assert_eq!(snapshot.usage_ledger.len(), 1);
    let entry = &snapshot.usage_ledger[0];
    assert_eq!(entry.id, run);
    assert_eq!(entry.session_id, id);
    assert_eq!(entry.harness, Harness::Codex);
    assert_eq!(entry.model.as_deref(), Some("gpt-test"));
    assert_eq!(
        (
            entry.input_tokens,
            entry.cached_input_tokens,
            entry.output_tokens
        ),
        (250, 100, 50)
    );
    let reopened = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), snapshot);
}

#[tokio::test]
async fn claude_reports_compactions_context_rate_limits_and_tool_details() {
    let dir = tempfile::tempdir().unwrap();
    let events = dir.path().join("events.jsonl");
    std::fs::write(&events, CLAUDE_DETAIL_EVENTS).unwrap();
    let source = CLAUDE_FAKE.replacen(
        "      echo '{\"type\":\"stream_event\"",
        &format!(
            "      cat '{}'\n      echo '{{\"type\":\"stream_event\"",
            events.display()
        ),
        1,
    );
    let source = source.replace(
        r#"echo '{"type":"result","subtype":"success","usage":"#,
        r#"echo '{"type":"result","subtype":"success","modelUsage":{"reported-claude-model":{"contextWindow":200000}},"usage":"#,
    );
    let w = claude_workspace(dir.path(), &source);
    let (id, snapshot) = launch(&w).await;
    let session = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
    let work = session.worker.as_ref().unwrap();
    assert_eq!(work.context_tokens, Some(1000 + 200 + 30));
    assert_eq!(work.context_window, Some(200_000));
    assert_eq!(
        session.compactions,
        [
            Compaction {
                at_tokens: Some(150_000)
            },
            Compaction {
                at_tokens: Some(160_000)
            }
        ]
    );
    assert_eq!(
        snapshot
            .messages
            .iter()
            .filter(|m| m.session_id == id && m.body == "Compaction completed")
            .count(),
        2
    );
    let limits = &snapshot.usage_limits;
    assert_eq!(limits.len(), 2);
    assert_eq!(limits[0].id, "claude_code-five_hour");
    assert_eq!(
        (
            limits[0].used_percent,
            limits[0].window_minutes,
            limits[0].resets_at
        ),
        (25.0, 300, Some(1_900_000_000))
    );
    assert_eq!(limits[1].window, UsageWindow::Weekly);
    assert_eq!(limits[1].window_minutes, 10080);
    let tool = |key: &str| {
        snapshot
            .messages
            .iter()
            .find(|m| m.session_id == id && m.id.ends_with(&format!("-tool-{key}")))
            .and_then(|m| m.tool.clone())
            .unwrap()
    };
    let created = tool("write-new");
    assert_eq!(created.kind, ToolKind::Create);
    assert_eq!(created.detail, Some(ToolDetail::Create { lines: 2 }));
    assert!(created.finished_ms.is_some());
    let overwritten = tool("write-old");
    assert_eq!(overwritten.kind, ToolKind::Edit);
    assert_eq!(overwritten.detail, None);
    assert_eq!(
        tool("edit").detail,
        Some(ToolDetail::Edit {
            hunks: vec![],
            added: 2,
            removed: 1
        })
    );
    assert_eq!(
        tool("read").detail,
        Some(ToolDetail::Read {
            start_line: Some(10),
            end_line: Some(12),
            total_lines: None
        })
    );
    assert_eq!(
        tool("grep").detail,
        Some(ToolDetail::Search {
            query: "fn".into(),
            files: vec![
                SearchFile {
                    path: "a.rs".into(),
                    hits: 3
                },
                SearchFile {
                    path: "b.rs".into(),
                    hits: 1
                }
            ]
        })
    );
    let test = tool("test");
    assert_eq!(test.kind, ToolKind::Test);
    assert_eq!(
        test.detail,
        Some(ToolDetail::Test {
            passed: Some(2),
            failed: Some(0),
            failures: vec![]
        })
    );
    assert_eq!(tool("bash").kind, ToolKind::Run);
    assert_eq!(tool("bash").detail, None);
    assert!(
        matches!(tool("todo").detail, Some(ToolDetail::Plan { ref steps }) if steps.len() == 2 && steps[0].done && !steps[1].done)
    );
    assert_eq!(tool("task").kind, ToolKind::Spawn);
    assert_eq!(
        tool("fetch").detail,
        Some(ToolDetail::Fetch {
            status: Some(200),
            title: None
        })
    );
    let entry = snapshot.usage_ledger.last().unwrap();
    assert_eq!(entry.session_id, id);
    assert_eq!(entry.harness, Harness::ClaudeCode);
    assert_eq!(
        (
            entry.input_tokens,
            entry.cached_input_tokens,
            entry.output_tokens
        ),
        (19, 11, 7)
    );
}

const CLAUDE_DETAIL_EVENTS: &str = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Working"}],"usage":{"input_tokens":900,"cache_read_input_tokens":100,"cache_creation_input_tokens":10,"output_tokens":5}}}
{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":150000}}
{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":160000}}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"five_hour":{"utilization":0.25,"resetsAt":1900000000},"seven_day":{"utilization":0.5,"resetsAt":1900500000}}},"uuid":"u","session_id":"11111111-2222-4333-8444-555555555555"}
{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"write-new","name":"Write","input":{"file_path":"new.txt","content":"a\nb\n"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"write-new","content":"File created successfully at: new.txt"}]},"tool_use_result":{"type":"create","filePath":"new.txt","content":"a\nb\n","structuredPatch":[]}}
{"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"write-old","name":"Write","input":{"file_path":"old.txt","content":"x"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"write-old","content":"The file old.txt has been updated."}]},"tool_use_result":{"type":"update","filePath":"old.txt","content":"x","structuredPatch":[]}}
{"type":"assistant","message":{"id":"m4","content":[{"type":"tool_use","id":"edit","name":"Edit","input":{"file_path":"a.rs","old_string":"keep\nold\nkeep","new_string":"keep\nnew\nnewer\nkeep"}},{"type":"tool_use","id":"read","name":"Read","input":{"file_path":"a.rs","offset":10,"limit":3}},{"type":"tool_use","id":"grep","name":"Grep","input":{"pattern":"fn","output_mode":"count"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"edit","content":"ok"},{"type":"tool_result","tool_use_id":"read","content":"    10\tone\n    11\ttwo\n    12\tthree\n"},{"type":"tool_result","tool_use_id":"grep","content":"a.rs:3\nb.rs:1\n\nFound 4 total occurrences across 2 files."}]}}
{"type":"assistant","message":{"id":"m5","content":[{"type":"tool_use","id":"test","name":"Bash","input":{"command":"cargo test"}},{"type":"tool_use","id":"bash","name":"Bash","input":{"command":"ls"}},{"type":"tool_use","id":"todo","name":"TodoWrite","input":{"todos":[{"content":"One","status":"completed","activeForm":"Doing one"},{"content":"Two","status":"in_progress","activeForm":"Doing two"}]}},{"type":"tool_use","id":"task","name":"Task","input":{"description":"Explore","prompt":"look"}},{"type":"tool_use","id":"fetch","name":"WebFetch","input":{"url":"https://example.com","prompt":"x"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"test","content":"test result: ok. 2 passed; 0 failed; 0 ignored"},{"type":"tool_result","tool_use_id":"bash","content":"x"},{"type":"tool_result","tool_use_id":"todo","content":"Todos have been modified successfully."},{"type":"tool_result","tool_use_id":"task","content":"done"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"fetch","content":"Example"}]},"tool_use_result":{"bytes":10,"code":200,"codeText":"OK","result":"Example","durationMs":5,"url":"https://example.com"}}
{"type":"assistant","message":{"id":"m6","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":1000,"cache_read_input_tokens":200,"cache_creation_input_tokens":30,"output_tokens":5}}}
{"type":"assistant","parent_tool_use_id":"task","message":{"id":"m7","content":[{"type":"text","text":"sub"}],"usage":{"input_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":5}}}
"#;

#[test]
fn harness_environment_shares_build_cache_only_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let root_text = root.display().to_string();
    let target_env = |cmd: &tokio::process::Command| {
        cmd.as_std()
            .get_envs()
            .find(|(key, _)| *key == "CARGO_TARGET_DIR")
            .and_then(|(_, value)| value.map(|v| v.to_owned()))
    };
    // Off by default, and when explicitly disabled.
    for setting in [None, Some("0"), Some("off"), Some("")] {
        let mut cmd = tokio::process::Command::new("true");
        assert!(
            crate::build_output::configure(
                &mut cmd,
                Some(&root_text),
                None,
                setting.map(std::ffi::OsStr::new)
            )
            .is_none()
        );
        assert_eq!(target_env(&cmd), None);
    }
    assert!(!root.join(".relay").exists());
    let mut cmd = tokio::process::Command::new("true");
    let cache = crate::build_output::configure(
        &mut cmd,
        Some(&root_text),
        None,
        Some(std::ffi::OsStr::new("1")),
    )
    .unwrap();
    let expected = root
        .canonicalize()
        .unwrap()
        .join(".relay/cache/cargo-target");
    assert_eq!(cache, expected.display().to_string());
    assert_eq!(target_env(&cmd), Some(expected.clone().into_os_string()));
    assert!(expected.is_dir());
    // An inherited override takes precedence over the opt-in.
    for setting in ["1", "on"] {
        let mut cmd = tokio::process::Command::new("true");
        assert!(
            crate::build_output::configure(
                &mut cmd,
                Some(&root_text),
                Some(std::ffi::OsStr::new("/custom/target")),
                Some(std::ffi::OsStr::new(setting))
            )
            .is_none()
        );
        assert_eq!(target_env(&cmd), None);
    }
    // An empty inherited value does not count as a choice.
    let mut cmd = tokio::process::Command::new("true");
    assert!(
        crate::build_output::configure(
            &mut cmd,
            Some(&root_text),
            Some(std::ffi::OsStr::new("")),
            Some(std::ffi::OsStr::new("1"))
        )
        .is_some()
    );
    let mut cmd = tokio::process::Command::new("true");
    assert!(
        crate::build_output::configure(&mut cmd, None, None, Some(std::ffi::OsStr::new("1")))
            .is_none()
    );
    assert_eq!(target_env(&cmd), None);
}

#[tokio::test]
async fn archive_reclaims_ignored_build_output_and_keeps_changes_and_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let repo = review_repo(&source);
    std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    std::fs::write(repo.join("tracked"), "base\n").unwrap();
    git(&repo, &["add", ".gitignore", "tracked"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qm",
            "tracked",
        ],
    );
    let root = dir.path().join("project");
    let worktree = root.join(".relay/sessions/session-reclaim/repo-0");
    std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "relay/session-reclaim",
            worktree.to_str().unwrap(),
        ],
    );
    for output in ["target/debug", "crates/app/target/debug"] {
        std::fs::create_dir_all(worktree.join(output)).unwrap();
        std::fs::write(worktree.join(output).join("artifact"), "build").unwrap();
    }
    std::fs::write(worktree.join("tracked"), "changed\n").unwrap();
    std::fs::write(worktree.join("crates/app/new.rs"), "untracked").unwrap();
    git(
        &worktree,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qam",
            "unmerged",
        ],
    );
    std::fs::write(worktree.join("tracked"), "uncommitted\n").unwrap();
    // Connected directories are edited in place and are never cleaned.
    let connected = dir.path().join("connected");
    std::fs::create_dir_all(connected.join("target")).unwrap();

    let mut snapshot = live();
    snapshot.projects[0].root = Some(root.display().to_string());
    let mut session = snapshot.sessions[0].clone();
    session.id = "session-reclaim".into();
    session.fixture = false;
    session.archived = false;
    session.parent_session_id = None;
    session.worker = None;
    session.workspaces = vec![
        SessionWorkspace {
            connection_id: "repo-0".into(),
            path: worktree.display().to_string(),
            repository: true,
            branch: Some("relay/session-reclaim".into()),
            base_commit: None,
            changes: None,
        },
        SessionWorkspace {
            connection_id: "connected".into(),
            path: connected.display().to_string(),
            repository: false,
            branch: None,
            base_commit: None,
            changes: None,
        },
    ];
    snapshot.sessions.push(session);
    let w = workspace(&dir.path().join("db"), &snapshot, config());
    let archived = apply(
        &w,
        Command::SetSessionArchived {
            session_id: "session-reclaim".into(),
            archived: true,
        },
    );
    assert!(
        archived
            .sessions
            .iter()
            .any(|s| s.id == "session-reclaim" && s.archived)
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while worktree.join("target").exists() || worktree.join("crates/app/target").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(worktree.join("tracked")).unwrap(),
        "uncommitted\n"
    );
    assert!(worktree.join("crates/app/new.rs").is_file());
    assert!(worktree.join(".git").is_file());
    assert!(connected.join("target").is_dir());
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["log", "-1", "--format=%s", "relay/session-reclaim"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(log.stdout).unwrap().trim(), "unmerged");
    // Repeated and missing paths are no-ops.
    assert!(
        crate::build_output::reclaim(&root, &worktree)
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::build_output::reclaim(&root, &root.join(".relay/sessions/missing"))
            .unwrap()
            .is_empty()
    );
    // A connected checkout outside the session directories is never reclaimed.
    std::fs::create_dir_all(repo.join("target")).unwrap();
    assert!(
        crate::build_output::reclaim(&root, &repo)
            .unwrap()
            .is_empty()
    );
    assert!(repo.join("target").is_dir());
}
