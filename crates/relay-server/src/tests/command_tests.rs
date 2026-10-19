use super::*;
use serde_json::Value;

pub(crate) const CODEX: &str = r#"
while IFS= read -r line; do
  printf '%s\n' "$line" >> '@CAPTURE@'
  id=$(printf '%s' "$line" | jq -c '.id')
  method=$(printf '%s' "$line" | jq -r '.method')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id";;
    model/list) printf '{"id":%s,"result":{"data":[{"id":"test-model","displayName":"Test model","defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}],"serviceTiers":[{"id":"fast"}]}]}}\n' "$id";;
    skills/list) printf '{"id":%s,"result":{"data":[{"skills":[{"name":"test-skill","description":"Test skill","path":"/test/skill/SKILL.md","enabled":true}]}]}}\n' "$id";;
    mcpServerStatus/list) printf '{"id":%s,"result":{"data":[{"name":"test-mcp","authStatus":"oAuth"}]}}\n' "$id";;
    thread/start) printf '{"id":%s,"result":{"thread":{"id":"original-thread"},"model":"test-model"}}\n' "$id";;
    thread/resume) thread=$(printf '%s' "$line" | jq -r '.params.threadId');printf '{"id":%s,"result":{"thread":{"id":"%s"},"model":"test-model"}}\n' "$id" "$thread";;
    thread/fork) printf '{"id":%s,"result":{"thread":{"id":"fork-thread"},"model":"test-model"}}\n' "$id";;
    thread/compact/start)
      thread=$(printf '%s' "$line" | jq -r '.params.threadId')
      printf '{"id":%s,"result":{}}\n' "$id"
      printf '{"method":"turn/started","params":{"threadId":"%s","turn":{"id":"compact-turn"}}}\n' "$thread"
      printf '{"method":"item/completed","params":{"threadId":"%s","item":{"id":"compaction","type":"contextCompaction"}}}\n' "$thread"
      printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"compact-turn","status":"completed"}}}\n' "$thread";;
    turn/start|review/start)
      thread=$(printf '%s' "$line" | jq -r '.params.threadId')
      printf '{"id":%s,"result":{"turn":{"id":"test-turn"}}}\n' "$id"
      printf '{"method":"item/completed","params":{"threadId":"%s","item":{"id":"answer","type":"agentMessage","text":"Completed"}}}\n' "$thread"
      printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"test-turn","status":"completed"}}}\n' "$thread";;
  esac
done
"#;

async fn setup(dir: &Path) -> (Workspace, String, PathBuf) {
    let repo = review_repo(dir);
    std::fs::write(repo.join("README"), "base").unwrap();
    git(&repo, &["add", "README"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qm",
            "tracked base",
        ],
    );
    let capture = dir.join("capture.jsonl");
    let binary = dir.join("command-codex");
    script(
        &binary,
        &CODEX.replace("@CAPTURE@", capture.to_str().unwrap()),
    );
    let mut config = config();
    config.repository = Some(repo);
    config.codex = binary;
    let mut snapshot = live();
    snapshot.projects[0].defaults = AgentProfile::default();
    let w = workspace(&dir.join("db"), &snapshot, config);
    let (id, run, stop, prompt) = reserve(&w, env(snapshot.revision, start(&snapshot)));
    runtime::run(w.clone(), id.clone(), run, prompt, stop).await;
    assert_eq!(
        finished(&w, &id)
            .await
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Completed
    );
    (w, id, capture)
}

fn apply(w: &Workspace, command: Command) -> Snapshot {
    let mut store = w.store.lock().unwrap();
    let current = store.snapshot().unwrap();
    let (snapshot, _) = store
        .apply(env(current.revision, command), &w.config)
        .unwrap();
    w.snapshots.send_replace(snapshot.clone());
    snapshot
}

async fn turn(w: &Workspace, id: &str, text: &str) {
    let snapshot = w.snapshots.borrow().clone();
    let (id, run, stop, prompt) = reserve(
        w,
        env(
            snapshot.revision,
            Command::SendWorker {
                session_id: id.into(),
                prompt: text.into(),
                approve_implementation: true,
            },
        ),
    );
    runtime::run(w.clone(), id.clone(), run, prompt, stop).await;
    let snapshot = finished(w, &id).await;
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
}

#[tokio::test]
async fn codex_discovery_skills_settings_and_compaction_use_native_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, capture) = setup(dir.path()).await;
    let catalog = commands::discover(&w, &id, true).await.unwrap();
    assert_eq!(catalog.skills[0].name, "test-skill");
    assert!(catalog.models[0].fast);
    assert!(catalog.mcp[0].contains("test-mcp"));
    let before = std::fs::read_to_string(&capture)
        .unwrap()
        .lines()
        .filter(|l| l.contains("turn/start"))
        .count();
    commands::discover(&w, &id, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .filter(|l| l.contains("turn/start"))
            .count(),
        before
    );
    let selection = HarnessSelection {
        model: Some("test-model".into()),
        effort: Some("high".into()),
        fast: true,
        plan: true,
    };
    commands::validate_selection(&catalog, &selection).unwrap();
    apply(
        &w,
        Command::SetHarnessSelection {
            session_id: id.clone(),
            selection: selection.clone(),
        },
    );
    turn(&w, &id, "Use $test-skill to review λ").await;
    turn(&w, &id, "/compact").await;
    let rpc: Vec<Value> = std::fs::read_to_string(&capture)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let turn = rpc
        .iter()
        .rev()
        .find(|v| v["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["effort"], "high");
    assert_eq!(turn["params"]["serviceTier"], "fast");
    assert_eq!(turn["params"]["collaborationMode"]["mode"], "plan");
    assert!(
        turn["params"]["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["type"] == "skill" && p["path"] == "/test/skill/SKILL.md")
    );
    assert!(
        rpc.iter().any(|v| v["method"] == "thread/compact/start"
            && v["params"]["threadId"] == "original-thread")
    );
    let snapshot = w.snapshots.borrow().clone();
    assert!(
        snapshot
            .messages
            .iter()
            .any(|m| m.body == "Compaction completed")
    );
    let reopened = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    assert_eq!(
        reopened
            .snapshot()
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .selection,
        selection
    );
    assert!(commands::invocation(&[Part::text("/compact focus")], &catalog).is_err());
    assert!(commands::invocation(&[Part::text("/unknown")], &catalog).is_err());
}

#[tokio::test]
async fn turns_store_and_run_the_default_effort_when_none_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, capture) = setup(dir.path()).await;
    let efforts = || -> Vec<Value> {
        std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str::<Value>(s).unwrap())
            .filter(|v| v["method"] == "turn/start")
            .map(|v| v["params"]["effort"].clone())
            .collect()
    };
    // Before any catalog is known, the turn runs at the harness default.
    assert_eq!(efforts(), vec![Value::Null]);
    // Any discovered catalog of the harness names the reported model's default.
    commands::discover(&w, &id, true).await.unwrap();
    turn(&w, &id, "Continue").await;
    assert_eq!(efforts().last().unwrap(), "medium");
    let reopen = || {
        Store::open(&dir.path().join("db"), AgentProfile::default())
            .unwrap()
            .snapshot()
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id == id)
            .unwrap()
            .selection
    };
    assert_eq!(
        reopen(),
        HarnessSelection {
            model: None,
            effort: Some("medium".into()),
            ..Default::default()
        }
    );
    // A selected model without an effort runs and stores that model's default.
    apply(
        &w,
        Command::SetHarnessSelection {
            session_id: id.clone(),
            selection: HarnessSelection {
                model: Some("test-model".into()),
                ..Default::default()
            },
        },
    );
    turn(&w, &id, "Continue again").await;
    assert_eq!(efforts().last().unwrap(), "medium");
    assert_eq!(reopen().effort.as_deref(), Some("medium"));
    assert_eq!(reopen().model.as_deref(), Some("test-model"));
}
#[tokio::test]
async fn clear_resume_and_fork_keep_exact_history_and_independent_files() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, _) = setup(dir.path()).await;
    let source = w
        .snapshots
        .borrow()
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .clone();
    let path = PathBuf::from(source.worker.as_ref().unwrap().worktree.as_ref().unwrap());
    std::fs::write(path.join("new-file"), "untracked λ").unwrap();
    let file = path.join("README");
    std::fs::write(&file, "staged edit").unwrap();
    git(&path, &["add", "README"]);
    std::fs::write(&file, "tracked edit").unwrap();
    let revision = w.snapshots.borrow().revision;
    let (session, run, stop, prompt) = reserve(
        &w,
        env(
            revision,
            Command::ConversationCommand {
                session_id: id.clone(),
                name: "fork".into(),
                argument: String::new(),
            },
        ),
    );
    runtime::run(w.clone(), session, run.clone(), prompt, stop).await;
    let snapshot = finished(&w, &id).await;
    let source_worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(
        source_worker.status,
        WorkerStatus::Completed,
        "{:?}",
        source_worker.error
    );
    let fork = snapshot.sessions.iter().find(|s| s.id == run).unwrap();
    let target = PathBuf::from(fork.worker.as_ref().unwrap().worktree.as_ref().unwrap());
    assert_ne!(target, path);
    let index = std::process::Command::new("git")
        .arg("-C")
        .arg(&target)
        .args(["show", ":README"])
        .output()
        .unwrap();
    assert!(index.status.success());
    assert_eq!(String::from_utf8(index.stdout).unwrap(), "staged edit");
    assert_eq!(
        fork.worker.as_ref().unwrap().thread_id.as_deref(),
        Some("fork-thread")
    );
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .as_deref(),
        Some("original-thread")
    );
    assert_eq!(
        std::fs::read_to_string(target.join("new-file")).unwrap(),
        "untracked λ"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("README")).unwrap(),
        "tracked edit"
    );
    std::fs::write(target.join("README"), "independent").unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap(), "tracked edit");
    let cleared = apply(
        &w,
        Command::ConversationCommand {
            session_id: id.clone(),
            name: "clear".into(),
            argument: String::new(),
        },
    );
    let session = cleared.sessions.iter().find(|s| s.id == id).unwrap();
    assert!(session.worker.as_ref().unwrap().thread_id.is_none());
    assert_eq!(session.conversations.len(), 1);
    let restored = apply(
        &w,
        Command::ConversationCommand {
            session_id: id.clone(),
            name: "resume".into(),
            argument: session.conversations[0].id.clone(),
        },
    );
    assert_eq!(
        restored
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .as_deref(),
        Some("original-thread")
    );
    assert!(restored.messages.len() > source.conversations.len());
}

#[test]
fn shared_workspaces_block_overlapping_execution() {
    let dir = tempfile::tempdir().unwrap();
    let mut snapshot = live();
    for session in &mut snapshot.sessions {
        session.workspaces = vec![SessionWorkspace {
            connection_id: "shared".into(),
            path: dir.path().display().to_string(),
            repository: false,
            branch: None,
            base_commit: None,
            changes: None,
        }];
        session.worker = Some(WorkerRun {
            model: None,
            context_tokens: None,
            context_window: None,
            last_usage: None,
            harness: Harness::Codex,
            execution: None,
            status: WorkerStatus::Running,
            thread_id: Some("thread".into()),
            worktree: Some(dir.path().display().to_string()),
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
        });
    }
    assert!(runtime::check_shared(&snapshot, &snapshot.sessions[0].id).is_err());
    for session in snapshot.sessions.iter_mut().skip(1) {
        session.worker.as_mut().unwrap().status = WorkerStatus::Completed;
    }
    runtime::check_shared(&snapshot, &snapshot.sessions[0].id).unwrap();
}

const CLAUDE: &str = r#"
printf '%s\n' "$@" >> '@ARGV@'
session=11111111-2222-4333-8444-555555555555
for arg in "$@"; do [ "$arg" = --fork-session ] && session=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee; done
while IFS= read -r line; do
  printf '%s\n' "$line" >> '@CAPTURE@'
  type=$(printf '%s' "$line" | jq -r '.type')
  case "$type" in
    control_request)
      request=$(printf '%s' "$line" | jq -r '.request.subtype')
      id=$(printf '%s' "$line" | jq -c '.request_id')
      if [ "$request" = initialize ]; then
        printf '{"type":"control_response","response":{"request_id":%s,"subtype":"success","response":{"commands":[{"name":"first","description":"First skill"},{"name":"second","description":"Second skill"},{"name":"new-command","description":"Newly discovered command"}],"models":[{"value":"test-claude","displayName":"Test Claude","supportedEffortLevels":["high"],"supportsFastMode":true}]}}}\n' "$id"
      elif [ "$request" = mcp_status ]; then
        printf '{"type":"control_response","response":{"request_id":%s,"subtype":"success","response":{"mcpServers":[{"name":"claude-mcp","status":"connected"}]}}}\n' "$id"
      fi;;
    user)
      printf '{"type":"system","subtype":"init","session_id":"%s","model":"test-claude"}\n' "$session"
      text=$(printf '%s' "$line" | jq -r '[.message.content[]|.text // ""]|join("")')
      case "$text" in
        /compact*) echo '{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":1200}}';;
        /first*) ;;
        *'explicitly selected'*)
          echo '{"type":"assistant","message":{"id":"skills","content":[{"type":"tool_use","id":"first-tool","name":"Skill","input":{"skill":"first"}},{"type":"tool_use","id":"second-tool","name":"Skill","input":{"skill":"second"}}]}}'
          echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"first-tool","content":"First skill loaded"},{"type":"tool_result","tool_use_id":"second-tool","content":"Second skill loaded"}]}}';;
      esac
      echo '{"type":"result","subtype":"success","result":"Native command completed"}'
      echo '{"type":"system","subtype":"session_state_changed","state":"idle"}';;
  esac
done
"#;

#[tokio::test]
async fn claude_uniform_skills_commands_and_forks_preserve_native_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let capture = dir.path().join("claude.jsonl");
    let argv = dir.path().join("argv");
    let binary = dir.path().join("command-claude");
    script(
        &binary,
        &CLAUDE
            .replace("@CAPTURE@", capture.to_str().unwrap())
            .replace("@ARGV@", argv.to_str().unwrap()),
    );
    let mut config = config();
    config.repository = Some(repo);
    config.claude = binary;
    let mut snapshot = live();
    snapshot.projects[0].defaults = AgentProfile {
        harness: Harness::ClaudeCode,
        ..Default::default()
    };
    let w = workspace(&dir.path().join("db"), &snapshot, config);
    let (id, run, stop, prompt) = reserve(&w, env(snapshot.revision, start(&snapshot)));
    runtime::run(w.clone(), id.clone(), run, prompt, stop).await;
    let catalog = commands::discover(&w, &id, true).await.unwrap();
    assert!(catalog.skills.iter().any(|s| s.name == "first"));
    assert!(
        catalog
            .commands
            .iter()
            .any(|c| c.name == "new-command" && c.dispatch == CommandDispatch::Direct)
    );
    assert!(catalog.mcp[0].contains("connected"));
    apply(
        &w,
        Command::SetHarnessSelection {
            session_id: id.clone(),
            selection: HarnessSelection {
                model: Some("test-claude".into()),
                effort: Some("high".into()),
                fast: true,
                plan: true,
            },
        },
    );
    turn(&w, &id, "$first check λ").await;
    turn(&w, &id, "Use $first and $second together").await;
    turn(&w, &id, "/compact keep decisions").await;
    let inputs: Vec<Value> = std::fs::read_to_string(&capture)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|v: &Value| v["type"] == "user")
        .collect();
    assert!(
        inputs
            .iter()
            .any(|v| v["message"]["content"][0]["text"] == "/first check λ")
    );
    assert_eq!(
        inputs.last().unwrap()["message"]["content"][0]["text"],
        "/compact keep decisions"
    );
    let args = std::fs::read_to_string(&argv).unwrap();
    assert!(args.contains("--append-system-prompt"));
    assert!(args.contains("\n--effort\nhigh\n"));
    assert!(args.contains("\n--permission-mode\nplan\n"));
    assert!(args.contains("\"fastMode\":true"));
    let current = w.snapshots.borrow().clone();
    assert!(
        current
            .messages
            .iter()
            .any(|m| m.body == "Compaction completed")
    );
    let (session, run, stop, prompt) = reserve(
        &w,
        env(
            current.revision,
            Command::ConversationCommand {
                session_id: id.clone(),
                name: "fork".into(),
                argument: String::new(),
            },
        ),
    );
    runtime::run(w.clone(), session, run.clone(), prompt, stop).await;
    let snapshot = finished(&w, &id).await;
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == run)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .as_deref(),
        Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
    );
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .as_deref(),
        Some("11111111-2222-4333-8444-555555555555")
    );
}

#[tokio::test]
async fn catalog_and_session_choices_cross_http_and_reconnect_without_replaying_turns() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, capture) = setup(dir.path()).await;
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        "relay-test-token-command".into(),
        AgentProfile::default(),
        (*w.config).clone(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{endpoint}/v1/version"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let build: BuildInfo = client
        .get(format!("{endpoint}/v1/version"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    // The server also reports where it keeps projects created without a root.
    assert_eq!(
        build,
        BuildInfo {
            projects_root: build.projects_root.clone(),
            ..BuildInfo::current()
        }
    );
    assert_eq!(
        Some(build.projects_root.as_str()).filter(|r| !r.is_empty()),
        w.config.projects_root.as_ref().and_then(|p| p.to_str())
    );
    // Board address lookup needs the token and refuses addresses that carry
    // credentials before any provider process starts.
    let lookup = serde_json::json!({"url": "https://user:pw@gitlab.com/groups/t/-/boards"});
    let resolve = |token: Option<&str>| {
        let mut request = client
            .post(format!("{endpoint}/v1/boards/resolve"))
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&lookup);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send()
    };
    assert_eq!(
        resolve(None).await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        resolve(Some("relay-test-token-command"))
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
    let catalog: HarnessCatalog = client
        .get(format!("{endpoint}/v1/sessions/{id}/catalog"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(catalog.harness, Harness::Codex);
    let before: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let agent_config_id = &before
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .agent_config_id;
    let draft_id = format!("agent-draft-{agent_config_id}");
    let draft_catalog: HarnessCatalog = client
        .get(format!("{endpoint}/v1/sessions/{draft_id}/catalog"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(draft_catalog.skills.iter().any(|s| s.name == "test-skill"));
    assert!(
        draft_catalog
            .commands
            .iter()
            .any(|c| c.name == "help" && c.dispatch == CommandDispatch::Flow)
    );
    assert!(draft_catalog.commands.iter().any(|c| c.name == "compact"
        && c.dispatch == CommandDispatch::Unavailable
        && c.reason.as_deref().unwrap().contains("first message")));
    let parts = vec![Part {
        id: "first-skill".into(),
        kind: PartKind::Skill {
            skill: SkillReference {
                id: "/removed-skill".into(),
                name: "test-skill".into(),
            },
        },
    }];
    let rejected = client
        .post(format!("{endpoint}/v1/commands"))
        .bearer_auth("relay-test-token-command")
        .header("x-relay-protocol", relay_core::PROTOCOL_VERSION.to_string())
        .json(&env(
            before.revision,
            Command::StartRootAgent {
                issue_id: None,
                agent_config_id: agent_config_id.clone(),
                prompt: plain_text(&parts),
                parts,
                approve_implementation: false,
            },
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        rejected
            .json::<ApiError>()
            .await
            .unwrap()
            .message
            .contains("no longer available")
    );
    let after: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "Discovery must not create a session or run a turn"
    );
    let current: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let envelope = env(
        current.revision,
        Command::SetHarnessSelection {
            session_id: id.clone(),
            selection: HarnessSelection {
                model: Some("test-model".into()),
                effort: Some("high".into()),
                ..Default::default()
            },
        },
    );
    for _ in 0..2 {
        let acknowledgement: CommandAck = client
            .post(format!("{endpoint}/v1/commands"))
            .bearer_auth("relay-test-token-command")
            .header("x-relay-protocol", relay_core::PROTOCOL_VERSION.to_string())
            .json(&envelope)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(acknowledgement.request_id, envelope.request_id);
        let snapshot: Snapshot = client
            .get(format!("{endpoint}/v1/snapshot"))
            .bearer_auth("relay-test-token-command")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .selection
                .effort
                .as_deref(),
            Some("high")
        );
    }
    let snapshot: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .selection
            .model
            .as_deref(),
        Some("test-model")
    );
    assert_eq!(
        std::fs::read_to_string(capture)
            .unwrap()
            .lines()
            .filter(|l| l.contains("turn/start"))
            .count(),
        1
    );
    let parts = vec![
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Skill {
                skill: SkillReference {
                    id: "removed-skill".into(),
                    name: "removed".into(),
                },
            },
        },
        Part::text(" preserve my draft"),
    ];
    let saved: Draft = client
        .post(format!("{endpoint}/v1/drafts/{id}"))
        .bearer_auth("relay-test-token-command")
        .header("x-relay-protocol", relay_core::PROTOCOL_VERSION.to_string())
        .json(&SaveDraft {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 0,
            parts: parts.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let rejected = client
        .post(format!("{endpoint}/v1/conversation/commands"))
        .bearer_auth("relay-test-token-command")
        .header("x-relay-protocol", relay_core::PROTOCOL_VERSION.to_string())
        .json(&env(
            snapshot.revision,
            Command::SubmitTurn {
                session_id: id.clone(),
                draft_revision: saved.revision,
                parts: parts.clone(),
                approve_implementation: true,
            },
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let drafts: Vec<Draft> = client
        .get(format!("{endpoint}/v1/drafts"))
        .bearer_auth("relay-test-token-command")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        drafts
            .iter()
            .find(|draft| draft.session_id == id)
            .unwrap()
            .parts,
        parts
    );
    shutdown.shutdown().await.unwrap();
    task.abort();
}

// Explicit opt-in: these tests use installed, authenticated harnesses and paid turns.
async fn installed_harness_commands(harness: Harness) {
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let skill_dir = if harness == Harness::Codex {
        ".agents/skills/relay-command-check"
    } else {
        ".claude/skills/relay-command-check"
    };
    std::fs::create_dir_all(repo.join(skill_dir)).unwrap();
    std::fs::write(repo.join(skill_dir).join("SKILL.md"), "---\nname: relay-command-check\ndescription: Verify explicit Relay skill dispatch in an isolated fixture\n---\nReply with RELAY_SKILL_CONFIRMED. Do not edit files or run commands.\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qm",
            "fixture skill",
        ],
    );
    let mut config = config();
    config.repository = Some(repo);
    let mut snapshot = live();
    snapshot.projects[0].defaults = AgentProfile {
        harness,
        ..Default::default()
    };
    let w = workspace(&dir.path().join("db"), &snapshot, config);
    let command = Command::StartWorker {
        issue_id: snapshot.issues[0].id.clone(),
        agent_config_id: snapshot.agent_configs[0].id.clone(),
        prompt: "Reply READY. Do not edit files, use tools, or perform implementation.".into(),
        approve_implementation: true,
    };
    let (id, run, stop, prompt) = reserve(&w, env(snapshot.revision, command));
    tokio::time::timeout(
        Duration::from_secs(180),
        runtime::run(w.clone(), id.clone(), run, prompt, stop),
    )
    .await
    .unwrap();
    let snapshot = finished(&w, &id).await;
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
    let catalog = commands::discover(&w, &id, true).await.unwrap();
    assert!(
        catalog
            .skills
            .iter()
            .any(|s| s.name == "relay-command-check")
    );
    tokio::time::timeout(
        Duration::from_secs(180),
        turn(&w, &id, "$relay-command-check"),
    )
    .await
    .unwrap();
    assert!(
        w.snapshots
            .borrow()
            .messages
            .iter()
            .any(|m| m.session_id == id && m.body.contains("RELAY_SKILL_CONFIRMED"))
    );
    tokio::time::timeout(Duration::from_secs(180), turn(&w, &id, "/compact"))
        .await
        .unwrap();
    let snapshot = w.snapshots.borrow().clone();
    let (session, run, stop, prompt) = reserve(
        &w,
        env(
            snapshot.revision,
            Command::ConversationCommand {
                session_id: id.clone(),
                name: "fork".into(),
                argument: String::new(),
            },
        ),
    );
    tokio::time::timeout(
        Duration::from_secs(180),
        runtime::run(w.clone(), session, run.clone(), prompt, stop),
    )
    .await
    .unwrap();
    let snapshot = finished(&w, &id).await;
    let source = snapshot.sessions.iter().find(|s| s.id == id).unwrap();
    assert_eq!(
        source.worker.as_ref().unwrap().status,
        WorkerStatus::Completed,
        "{:?}",
        source.worker.as_ref().unwrap().error
    );
    let fork = snapshot.sessions.iter().find(|s| s.id == run).unwrap();
    assert_ne!(
        source.worker.as_ref().unwrap().thread_id,
        fork.worker.as_ref().unwrap().thread_id
    );
    assert_ne!(
        source.worker.as_ref().unwrap().worktree,
        fork.worker.as_ref().unwrap().worktree
    );
}

#[tokio::test]
#[ignore = "requires installed authenticated Codex"]
async fn installed_codex_command_smoke() {
    installed_harness_commands(Harness::Codex).await;
}

#[tokio::test]
#[ignore = "requires installed authenticated Claude Code"]
async fn installed_claude_command_smoke() {
    installed_harness_commands(Harness::ClaudeCode).await;
}

#[tokio::test]
async fn interrupted_fork_creation_cleans_partial_worktrees_and_branches() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, _) = setup(dir.path()).await;
    let source = w
        .snapshots
        .borrow()
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .clone();
    let path = PathBuf::from(source.worker.as_ref().unwrap().worktree.as_ref().unwrap());
    let common = std::process::Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .unwrap();
    let hooks = PathBuf::from(String::from_utf8(common.stdout).unwrap().trim()).join("hooks");
    let marker = dir.path().join("copy-started");
    script(
        &hooks.join("post-checkout"),
        &format!("touch '{}'\nwhile :; do sleep 1; done\n", marker.display()),
    );
    let fork_id = uuid::Uuid::new_v4().to_string();
    let target = path.parent().unwrap().join(format!("fork-{fork_id}-0"));
    let (stop, cancel) = watch::channel(false);
    let native_id = fork_id.clone();
    let task = tokio::task::spawn_blocking(move || {
        crate::process::with_cancellation(cancel, || commands::fork_workspaces(&source, &native_id))
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(!target.exists());
    let branches = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["branch", "--list", &format!("relay/fork-{fork_id}-0")])
        .output()
        .unwrap();
    assert!(branches.status.success());
    assert!(branches.stdout.is_empty());
}

#[tokio::test]
async fn example_sessions_describe_the_demo_catalog_without_a_harness() {
    let dir = tempfile::tempdir().unwrap();
    let mut snapshot = demo_snapshot(AgentProfile::default());
    demo_orchestration(&mut snapshot);
    let w = workspace(&dir.path().join("db"), &snapshot, config());
    let claude = commands::discover(&w, "session-plan", false).await.unwrap();
    assert_eq!(claude.harness, Harness::ClaudeCode);
    let opus = claude
        .models
        .iter()
        .find(|m| m.id == "claude-opus-4-5")
        .unwrap();
    assert!(opus.efforts.iter().any(|e| e == "max"));
    let codex = commands::discover(&w, "session-backfill", false)
        .await
        .unwrap();
    assert_eq!(codex.harness, Harness::Codex);
    // An example session without a run still has nothing to discover.
    assert!(
        commands::discover(&w, "session-review", false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn branch_archive_and_confirmed_deletion_remove_owned_files_and_persist_over_http() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, _) = setup(dir.path()).await;
    let mut snapshot = w.snapshots.borrow().clone();
    let parent = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .clone();
    let parent_path = PathBuf::from(parent.worker.as_ref().unwrap().worktree.as_ref().unwrap());
    let child_id = "session-delete-child";
    let (child_path, child_branch, _) = runtime::prepare(&w.config, child_id).unwrap();
    let mut child = parent.clone();
    child.id = child_id.into();
    child.parent_session_id = Some(id.clone());
    child.worker.as_mut().unwrap().worktree = Some(child_path.clone());
    child.worker.as_mut().unwrap().branch = Some(child_branch.clone());
    child.workspaces.clear();
    child.workspaces.push(SessionWorkspace {
        connection_id: "repository".into(),
        path: child_path.clone(),
        repository: true,
        branch: Some(child_branch),
        base_commit: None,
        changes: None,
    });
    let shared = dir.path().join("shared-directory");
    std::fs::create_dir(&shared).unwrap();
    std::fs::write(shared.join("keep.txt"), "shared data").unwrap();
    child.workspaces.push(SessionWorkspace {
        connection_id: "shared".into(),
        path: shared.to_str().unwrap().into(),
        repository: false,
        branch: None,
        base_commit: None,
        changes: None,
    });
    snapshot.sessions.push(child);
    w.store.lock().unwrap().save(&snapshot).unwrap();
    let draft = Draft {
        session_id: child_id.into(),
        parts: vec![Part::text("delete this draft")],
        ..Default::default()
    };
    w.store
        .lock()
        .unwrap()
        .connection
        .execute(
            "INSERT INTO drafts(session_id,draft) VALUES(?1,?2)",
            params![child_id, serde_json::to_string(&draft).unwrap()],
        )
        .unwrap();
    std::fs::write(
        parent_path.join("delete-parent.txt"),
        "unsaved parent change",
    )
    .unwrap();
    std::fs::write(
        Path::new(&child_path).join("delete-child.txt"),
        "unsaved child change",
    )
    .unwrap();
    let unaffected: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|s| s.id != id && s.id != child_id)
        .map(|s| s.id.clone())
        .collect();
    let token = uuid::Uuid::new_v4().to_string();
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        token.clone(),
        AgentProfile::default(),
        (*w.config).clone(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    let send = |command: CommandEnvelope| {
        client
            .post(format!("{endpoint}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&command)
    };
    let snapshot: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rejected = send(env(
        snapshot.revision,
        Command::DeleteSessionBranch {
            session_id: id.clone(),
        },
    ))
    .send()
    .await
    .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        send(env(
            snapshot.revision,
            Command::SetSessionArchived {
                session_id: id.clone(),
                archived: true
            }
        ))
        .send()
        .await
        .unwrap()
        .status()
        .is_success()
    );
    let archived: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        archived
            .sessions
            .iter()
            .filter(|s| s.id == id || s.id == child_id)
            .all(|s| s.archived)
    );
    let deletion = env(
        archived.revision,
        Command::DeleteSessionBranch {
            session_id: id.clone(),
        },
    );
    git(Path::new(&child_path), &["branch", "-m", "user-kept"]);
    assert_eq!(
        send(deletion.clone()).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        parent_path.exists(),
        "Validate the whole removal plan before deleting any worktrees"
    );
    assert!(Path::new(&child_path).exists());
    git(
        Path::new(&child_path),
        &["branch", "-m", "relay/session-delete-child"],
    );
    assert!(
        send(deletion.clone())
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert!(
        send(deletion).send().await.unwrap().status().is_success(),
        "retry must be idempotent"
    );
    let remaining: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !remaining
            .sessions
            .iter()
            .any(|s| s.id == id || s.id == child_id)
    );
    assert!(
        unaffected
            .iter()
            .all(|id| remaining.sessions.iter().any(|s| &s.id == id))
    );
    assert!(
        !remaining
            .messages
            .iter()
            .any(|m| m.session_id == id || m.session_id == child_id)
    );
    assert!(!parent_path.exists());
    assert!(!Path::new(&child_path).exists());
    assert!(shared.join("keep.txt").exists());
    shutdown.shutdown().await.unwrap();
    task.abort();
    let store = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    assert!(
        !store
            .snapshot()
            .unwrap()
            .sessions
            .iter()
            .any(|s| s.id == id || s.id == child_id)
    );
    assert!(
        !conversation::read_drafts(&store.connection)
            .unwrap()
            .iter()
            .any(|d| d.session_id == child_id)
    );
    let initial_receipt: String = store
        .connection
        .query_row(
            "SELECT request FROM receipts WHERE request_id=?1",
            [id.strip_prefix("session-").unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        initial_receipt.starts_with("deleted:"),
        "Deleted initial prompts must not remain in command receipts"
    );
}

#[tokio::test]
async fn profile_models_are_discovered_before_creation_and_passed_to_new_agents() {
    let dir = tempfile::tempdir().unwrap();
    let (w, _, capture) = setup(dir.path()).await;
    let token = uuid::Uuid::new_v4().to_string();
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        token.clone(),
        AgentProfile::default(),
        (*w.config).clone(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    let catalog: HarnessCatalog = client
        .get(format!("{endpoint}/v1/projects/live/catalog/codex"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(catalog.models.iter().any(|m| m.id == "test-model"));
    let read = || {
        client
            .get(format!("{endpoint}/v1/snapshot"))
            .bearer_auth(&token)
    };
    let send = |command: CommandEnvelope| {
        client
            .post(format!("{endpoint}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&command)
    };
    let before: Snapshot = read().send().await.unwrap().json().await.unwrap();
    let invalid = env(
        before.revision,
        Command::CreateAgentConfig {
            project_id: "live".into(),
            name: "Explicit model".into(),
            template: "director".into(),
            overrides: Default::default(),
        },
    );
    assert_eq!(
        send(invalid).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let request = env(
        before.revision,
        Command::CreateAgentConfig {
            project_id: "live".into(),
            name: "Explicit model".into(),
            template: "director".into(),
            overrides: ProfileOverrides {
                model: Some("test-model".into()),
                ..Default::default()
            },
        },
    );
    let config_id = format!("agent-config-{}", request.request_id);
    assert!(send(request).send().await.unwrap().status().is_success());
    let configured: Snapshot = read().send().await.unwrap().json().await.unwrap();
    let config = configured
        .agent_configs
        .iter()
        .find(|c| c.id == config_id)
        .unwrap();
    assert!(config.draft_selection.model.is_none());
    let profile = configured.effective_profile(config).unwrap();
    assert_eq!(profile.model, "test-model");
    let start = env(
        configured.revision,
        Command::StartRootAgent {
            agent_config_id: config_id,
            issue_id: None,
            prompt: "Plan with the configured model".into(),
            parts: vec![],
            approve_implementation: false,
        },
    );
    let session_id = format!("session-{}", start.request_id);
    assert!(send(start).send().await.unwrap().status().is_success());
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot: Snapshot = read().send().await.unwrap().json().await.unwrap();
            if snapshot.sessions.iter().any(|s| {
                s.id == session_id
                    && s.worker
                        .as_ref()
                        .is_some_and(|w| w.status == WorkerStatus::Completed)
            }) {
                break snapshot;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        completed
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .selection
            .model
            .as_deref(),
        Some("test-model")
    );
    let captured = std::fs::read_to_string(capture).unwrap();
    let native = captured
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| v["method"] == "thread/start")
        .next_back()
        .unwrap();
    assert_eq!(native["params"]["model"], "test-model");
    let child = env(
        completed.revision,
        Command::StartChildAgent {
            parent_session_id: session_id,
            name: String::new(),
            profile: AgentProfile::worker(&profile),
            prompt: String::new(),
            approve_implementation: false,
        },
    );
    let child_id = format!("session-{}", child.request_id);
    assert!(send(child).send().await.unwrap().status().is_success());
    let children: Snapshot = read().send().await.unwrap().json().await.unwrap();
    assert_eq!(
        children
            .sessions
            .iter()
            .find(|s| s.id == child_id)
            .unwrap()
            .selection
            .model
            .as_deref(),
        Some("test-model")
    );
    shutdown.shutdown().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn archived_chat_is_readable_but_cannot_execute_until_unarchived() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, _) = setup(dir.path()).await;
    let token = uuid::Uuid::new_v4().to_string();
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        token.clone(),
        AgentProfile::default(),
        (*w.config).clone(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    for archived in [true, false] {
        let before: Snapshot = client
            .get(format!("{endpoint}/v1/snapshot"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let response = client
            .post(format!("{endpoint}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&env(
                before.revision,
                Command::SetSessionArchived {
                    session_id: id.clone(),
                    archived,
                },
            ))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let snapshot: Snapshot = client
            .get(format!("{endpoint}/v1/snapshot"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .archived,
            archived
        );
        let history: HistoryPage = client
            .get(format!("{endpoint}/v1/sessions/{id}/history"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!history.messages.is_empty());
        let response = client
            .post(format!("{endpoint}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&env(
                snapshot.revision,
                Command::SendWorker {
                    session_id: id.clone(),
                    prompt: "Continue".into(),
                    approve_implementation: true,
                },
            ))
            .send()
            .await
            .unwrap();
        if archived {
            assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
            assert!(
                response
                    .json::<ApiError>()
                    .await
                    .unwrap()
                    .message
                    .contains("Unarchive")
            );
        } else {
            assert!(response.status().is_success());
        }
    }
    shutdown.shutdown().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn stale_creates_and_first_turn_use_current_selection_and_run_once() {
    let dir = tempfile::tempdir().unwrap();
    let (w, _, capture) = setup(dir.path()).await;
    let before = w.store.lock().unwrap().snapshot().unwrap();
    let config = &before.agent_configs[0];
    w.store
        .lock()
        .unwrap()
        .apply(
            env(
                before.revision,
                Command::UpdateAgentConfig {
                    agent_config_id: config.id.clone(),
                    name: String::new(),
                    overrides: config.overrides.clone(),
                },
            ),
            &w.config,
        )
        .unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        token.clone(),
        AgentProfile::default(),
        (*w.config).clone(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    let before: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let config = before.agent_configs[0].id.clone();
    for command in [
        Command::SaveAgentTemplate {
            project_id: before.projects[0].id.clone(),
            name: "Concurrent profile".into(),
            profile: AgentProfile::worker(&before.projects[0].defaults),
        },
        Command::CreateAgentConfig {
            project_id: before.projects[0].id.clone(),
            name: "Concurrent agent".into(),
            template: "worker".into(),
            overrides: ProfileOverrides {
                model: Some("test-model".into()),
                ..Default::default()
            },
        },
    ] {
        let request = env(0, command);
        for _ in 0..2 {
            let response = client
                .post(format!("{endpoint}/v1/commands"))
                .bearer_auth(&token)
                .header("x-relay-protocol", PROTOCOL_VERSION)
                .json(&request)
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
        }
    }
    let before: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        before
            .agent_templates
            .iter()
            .filter(|p| p.name == "Concurrent profile")
            .count(),
        1
    );
    assert_eq!(
        before
            .agent_configs
            .iter()
            .filter(|a| a.name == "Concurrent agent")
            .count(),
        1
    );
    let selected = HarnessSelection {
        model: Some("test-model".into()),
        effort: Some("high".into()),
        ..Default::default()
    };
    let response = client
        .post(format!("{endpoint}/v1/commands"))
        .bearer_auth(&token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .json(&env(
            before.revision,
            Command::SetHarnessSelection {
                session_id: format!("agent-draft-{config}"),
                selection: selected.clone(),
            },
        ))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let saved: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved.agent_configs[0].draft_selection, selected);
    // The selection and completed worker have advanced the workspace since
    // the client opened its draft. Starting an independent chat remains safe.
    let request = env(
        0,
        Command::StartRootAgent {
            agent_config_id: config.clone(),
            issue_id: Some(saved.issues[0].id.clone()),
            prompt: "First message".into(),
            parts: vec![],
            approve_implementation: true,
        },
    );
    let id = format!("session-{}", request.request_id);
    let response = client
        .post(format!("{endpoint}/v1/commands"))
        .bearer_auth(&token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let launched: Snapshot = client
        .get(format!("{endpoint}/v1/snapshot"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let duplicate = client
        .post(format!("{endpoint}/v1/commands"))
        .bearer_auth(&token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert!(duplicate.status().is_success());
    assert_eq!(launched.sessions.iter().filter(|s| s.id == id).count(), 1);
    let agent = launched.sessions.iter().find(|s| s.id == id).unwrap();
    assert!(agent.automatic_name);
    let Command::StartRootAgent { prompt, .. } = &request.command else {
        unreachable!()
    };
    assert_eq!(agent.title, crate::task_title(prompt));
    assert_eq!(
        launched
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .selection,
        selected
    );
    let mut completed = false;
    for _ in 0..100 {
        let snapshot: Snapshot = client
            .get(format!("{endpoint}/v1/snapshot"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| w.status == WorkerStatus::Completed)
        {
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(completed, "the first selected-model turn must complete");
    let rpc: Vec<Value> = std::fs::read_to_string(&capture)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let started = rpc
        .iter()
        .rev()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(started["params"]["model"], "test-model");
    let turn = rpc
        .iter()
        .rev()
        .find(|v| v["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["effort"], "high");
    shutdown.shutdown().await.unwrap();
    task.abort();
    let reopened = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    assert_eq!(
        reopened
            .snapshot()
            .unwrap()
            .agent_configs
            .iter()
            .find(|c| c.id == config)
            .unwrap()
            .draft_selection,
        selected
    );
}
