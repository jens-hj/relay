use super::*;
use serde_json::Value;

const CLAUDE_THREAD: &str = "11111111-2222-4333-8444-555555555555";
const CLAUDE_FAKE: &str = r#"
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
      echo '{"type":"control_response","response":{"subtype":"success","request_id":"relay-initialize","response":{}}}' ;;
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
    let repo = review_repo(dir);
    let bin = dir.join("fake-claude");
    script(&bin, source);
    let mut c = config();
    c.repository = Some(repo);
    c.claude = bin;
    let mut s = live();
    s.projects[0].defaults = DirectorProfile::default();
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
while IFS= read -r line; do
  id=$(printf '%s' "$line" | jq -c '.id')
  method=$(printf '%s' "$line" | jq -r '.method')
  printf '%s\n' "$line" >> codex-rpc.jsonl
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id";;
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
    let defaults = DirectorProfile {
        harness,
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
        DirectorProfile::default(),
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
        DirectorProfile::default(),
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
        director_id: snapshot.directors[0].id.clone(),
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
    assert_eq!(roots.lines().count(), 4);
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
    if let Some(Action::Stop(id)) = action {
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
async fn claude_first_turn_resume_modes_and_bound_harness() {
    let dir = tempfile::tempdir().unwrap();
    let w = claude_workspace(dir.path(), CLAUDE_FAKE);
    let (id, mut s) = launch(&w).await;
    let work = worker(&s, &id);
    assert_eq!(work.thread_id.as_deref(), Some(CLAUDE_THREAD));
    assert_eq!(work.last_usage, work.usage);
    assert_eq!(work.harness, Harness::ClaudeCode);
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
    let argv = std::fs::read_to_string(path.join("claude-argv")).unwrap();
    assert!(argv.contains(&format!("--resume\n{CLAUDE_THREAD}\n")));
    assert!(argv.contains("\nbypassPermissions\n"));
    assert_eq!(argv.matches("--settings").count(), 1);
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
    assert_eq!(content[1]["text"], "Before");
    assert!(
        content[2]["text"]
            .as_str()
            .unwrap()
            .contains("Inline file: context.png")
    );
    assert_eq!(content[3]["type"], "image");
    assert_eq!(content[3]["source"]["media_type"], "image/png");
    assert!(!content[3]["source"]["data"].as_str().unwrap().is_empty());
    assert_eq!(content[4]["text"], "Between");
    assert!(
        content[5]["text"]
            .as_str()
            .unwrap()
            .contains("Inline file: notes.txt")
    );
    assert!(
        content[6]["text"]
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
    let mut s = demo_snapshot(DirectorProfile::default());
    s.sessions[0].worker = Some(WorkerRun {
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
    s.directors[0].overrides.permissions = Some(s.projects[0].defaults.permissions.clone());
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
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    store
        .connection
        .execute("UPDATE workspace SET snapshot=?1", [json.to_string()])
        .unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 3)
        .unwrap();
    drop(store);
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    let next = store.snapshot().unwrap();
    assert_eq!(
        next.projects[0].defaults.permissions[&Task::Implement],
        Permission::Allow
    );
    assert_eq!(next.projects[1], custom);
    assert_eq!(next.directors, s.directors);
    assert_eq!(next.sessions, s.sessions);
    assert_eq!(next.messages, s.messages);
    assert_eq!(
        next.projects[0].defaults.execution.approval,
        ApprovalMode::Automatic
    );
    assert!(next.bindings.is_empty());
    drop(store);
    assert_eq!(
        Store::open(&db, DirectorProfile::default())
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
    let mut store = Store::open(&db, DirectorProfile::default()).unwrap();
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
        Store::open(&db, DirectorProfile::default())
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
    let s = demo_snapshot(DirectorProfile::default());
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
while IFS= read -r line; do
  echo "$line" >> codex-input.jsonl
  method=$(printf '%s' "$line" | jq -r '.method // "response"')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    thread/start) printf '{"id":%s,"result":{"thread":{"id":"codex-thread"}}}\n' "$id" ;;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
      if [ -f ask ]; then
        echo '{"id":"permission-91","method":"item/commandExecution/requestApproval","params":{"threadId":"codex-thread","turnId":"turn-1","command":"printf hello"}}'
      else echo '{"method":"turn/completed","params":{"threadId":"codex-thread","turn":{"id":"turn-1","status":"completed"}}}'; fi ;;
    response) echo '{"method":"turn/completed","params":{"threadId":"codex-thread","turn":{"id":"turn-1","status":"completed"}}}' ;;
  esac
done
"#;
    for mode in ApprovalMode::ALL {
        let dir = tempfile::tempdir().unwrap();
        let repo = review_repo(dir.path());
        let bin = dir.path().join("fake-codex");
        script(
            &bin,
            &format!(
                "{}\n{FAKE}",
                if mode == ApprovalMode::Ask {
                    "touch ask"
                } else {
                    "true"
                }
            ),
        );
        let mut s = live();
        s.projects[0].defaults = DirectorProfile::default();
        s.projects[0].defaults.execution.approval = mode;
        let c = RuntimeConfig {
            repository: Some(repo),
            codex: bin,
            ..config()
        };
        let w = workspace(&dir.path().join("db"), &s, c);
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
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
                    run_id: run,
                    allow: false,
                },
            );
        }
        task.await.unwrap();
        let s = finished(&w, &id).await;
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
    let mut defaults = DirectorProfile::default();
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
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.sessions[0].worker = Some(WorkerRun {
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
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    store
        .connection
        .execute("UPDATE workspace SET snapshot=?1", [json.to_string()])
        .unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 6)
        .unwrap();
    drop(store);
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    assert_eq!(store.snapshot().unwrap(), snapshot);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        8
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
        Store::open(&db, DirectorProfile::default())
            .unwrap()
            .snapshot()
            .unwrap(),
        snapshot
    );
}
