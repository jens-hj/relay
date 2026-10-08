use super::*;
use axum::{body::Bytes, extract::Path as RoutePath};
use std::os::unix::fs::PermissionsExt;
use std::{path::PathBuf, time::Duration};

mod harness_tests;
fn live() -> Snapshot {
    let mut defaults = DirectorProfile::default();
    defaults
        .permissions
        .insert(Task::Implement, Permission::Ask);
    let mut s = demo_snapshot(defaults);
    let p = &mut s.projects[0];
    p.id = "live".into();
    p.fixture = false;
    p.repository = "jens-hj/relay".into();
    p.github = Some(GitHubProject {
        owner: "jens-hj".into(),
        number: 5,
        url: "board".into(),
        last_synced_at: Some(1),
        sync_error: None,
    });
    for i in &mut s.issues {
        i.project_id = p.id.clone();
        i.reference.as_mut().unwrap().provider = Provider::Github;
        i.reference.as_mut().unwrap().repository = p.repository.clone();
    }
    for director in &mut s.directors {
        director.project_id = p.id.clone();
    }
    for session in &mut s.sessions {
        session.project_id = p.id.clone();
    }
    s
}
fn config() -> RuntimeConfig {
    RuntimeConfig {
        remote: Some(RemoteConfig {
            repository: "jens-hj/relay".into(),
            owner: "jens-hj".into(),
            number: 5,
        }),
        repository: Some(PathBuf::from("/configured/repo")),
        ..Default::default()
    }
}
pub(super) fn env(revision: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: revision,
        command,
    }
}
fn start(s: &Snapshot) -> Command {
    Command::StartWorker {
        issue_id: s.issues[0].id.clone(),
        director_id: s.directors[0].id.clone(),
        prompt: "Implement safely".into(),
        approve_implementation: true,
    }
}
pub(super) fn workspace(path: &Path, snapshot: &Snapshot, config: RuntimeConfig) -> Workspace {
    let mut store = Store::open(path, DirectorProfile::default()).unwrap();
    store.save(snapshot).unwrap();
    let (snapshots, _) = watch::channel(snapshot.clone());
    Workspace {
        harness_status: Arc::new(Mutex::new(vec![])),
        harness_probe: Arc::new(tokio::sync::Mutex::new(())),
        drafts: watch::channel(vec![]).0,
        transport_shutdown: watch::channel(false).0,
        transports: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        project_jobs: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        store: Arc::new(Mutex::new(store)),
        snapshots,
        token: "test-token".into(),
        config: Arc::new(config),
    }
}
pub(super) fn script(path: &Path, body: &str) {
    // Existing lifecycle fixtures describe a turn using the old normalized
    // events. Their fake CLI now hosts that turn behind the app-server wire
    // protocol, exercising the production transport rather than Codex exec.
    if path.file_name().is_some_and(|name| name == "codex") {
        let turn = path.with_extension("turn");
        let body = body.replace("echo $$", "echo $RELAY_TEST_PARENT");
        std::fs::write(&turn, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&turn, std::fs::Permissions::from_mode(0o700)).unwrap();
        let thread = body
            .split("\"thread_id\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("thread");
        let wrapper = format!(
            r#"#!/bin/sh
export RELAY_TEST_PARENT=$$
thread='{thread}'
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{{"id":%s,"result":{{}}}}\n' "$id" ;;
    thread/start|thread/resume) printf '{{"id":%s,"result":{{"thread":{{"id":"%s"}}}}}}\n' "$id" "$thread" ;;
    turn/start)
      printf '{{"id":%s,"result":{{"turn":{{"id":"turn"}}}}}}\n' "$id"
      printf '%s' "$line" | jq -jr '.params.input | map(.text // "") | join("\n")' | '{turn}' "$@" | while IFS= read -r event || [ -n "$event" ]; do
        case "$event" in
          *'"type":"thread.started"'*) ;;
          *'"type":"item.completed"'*) printf '%s' "$event" | jq -c --arg thread "$thread" '{{method:"item/completed",params:{{threadId:$thread,turnId:"turn",item:{{id:(.item.id // "answer"),type:"agentMessage",text:.item.text}}}}}}' ;;
          *'"type":"turn.completed"'*)
            printf '%s' "$event" | jq -c --arg thread "$thread" '{{method:"thread/tokenUsage/updated",params:{{threadId:$thread,tokenUsage:{{last:{{inputTokens:.usage.input_tokens,cachedInputTokens:.usage.cached_input_tokens,outputTokens:.usage.output_tokens}}}}}}}}'
            printf '{{"method":"turn/completed","params":{{"threadId":"%s","turn":{{"id":"turn","status":"completed"}}}}}}\n' "$thread" ;;
          *'"type":"turn.failed"'*|*'"type":"error"'*) printf '{{"method":"turn/completed","params":{{"threadId":"%s","turn":{{"id":"turn","status":"failed"}}}}}}\n' "$thread" ;;
          *) printf '%s\n' "$event" ;;
        esac
      done
      exit ;;
    turn/interrupt) printf '{{"id":%s,"result":{{}}}}\n' "$id" ;;
  esac
done
"#,
            turn = turn.display()
        );
        std::fs::write(path, wrapper).unwrap();
    } else {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

pub(super) fn git(repo: &Path, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}
#[test]
fn each_turn_checks_current_scope_permissions_harness_and_limit() {
    let mut s = live();
    let c = config();
    let issue = s.issues[0].id.clone();
    let d = s.directors[0].id.clone();
    assert!(runtime::authorize_turn(&s, &issue, &d, false, &c).is_err());
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_ok());
    s.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Deny);
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_err());
    s.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Allow);
    assert!(runtime::authorize_turn(&s, &issue, &d, false, &c).is_ok());
    s.projects[0].defaults.max_workers = 0;
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_err());
    s.projects[0].defaults.max_workers = 1;
    s.projects[0].defaults.harness = Harness::ClaudeCode;
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_ok());
    s.projects[0].defaults.harness = Harness::Codex;
    s.projects[0].defaults.scope = DirectorScope::Issues {
        issue_ids: vec!["other".into()],
    };
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_err());
    s.projects[0].defaults.scope = DirectorScope::Project;
    s.projects[0].fixture = true;
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_err());
}
#[test]
fn duplicate_receipts_reserve_once_and_revision_conflicts_do_not_reserve() {
    let dir = tempfile::tempdir().unwrap();
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let request = env(s.revision, start(&s));
    let mut store = w.store.lock().unwrap();
    let (s, action) = store.apply(request.clone(), &w.config).unwrap();
    assert!(matches!(action, Some(Action::Run { .. })));
    assert_eq!(
        store
            .run_id(&format!("session-{}", request.request_id))
            .unwrap(),
        Some(request.request_id.clone())
    );
    let (retry, action) = store.apply(request.clone(), &w.config).unwrap();
    assert!(action.is_none());
    assert_eq!(s, retry);
    assert!(store.apply(env(0, start(&s)), &w.config).is_err());
    let mut changed = request;
    changed.command = start(&s);
    if let Command::StartWorker { prompt, .. } = &mut changed.command {
        *prompt = "Changed".into();
    }
    assert!(store.apply(changed, &w.config).is_err());
}
#[test]
fn concurrent_start_reservations_enforce_worker_slots() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = live();
    s.projects[0].defaults.max_workers = 1;
    let w = workspace(&dir.path().join("db"), &s, config());
    let mut store = w.store.lock().unwrap();
    let (s, _) = store.apply(env(s.revision, start(&s)), &w.config).unwrap();
    assert!(store.apply(env(s.revision, start(&s)), &w.config).is_err());
}
#[test]
fn completed_items_are_immutable_usage_measured_and_thread_identity_checked() {
    let mut s = live();
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (new, _) = w
        .store
        .lock()
        .unwrap()
        .apply(env(s.revision, start(&s)), &w.config)
        .unwrap();
    s = new;
    let id = s.sessions.last().unwrap().id.clone();
    runtime::event(
        &mut s,
        &id,
        &serde_json::json!({"type":"thread.started","thread_id":"exact-thread"}),
    )
    .unwrap();
    let event = serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"Evidence"}});
    runtime::event(&mut s, &id, &event).unwrap();
    let prior = s.messages.last().unwrap().clone();
    runtime::event(&mut s, &id, &event).unwrap();
    assert_eq!(s.messages[s.messages.len() - 2], prior);
    assert_ne!(s.messages.last().unwrap().id, prior.id);
    assert!(runtime::event(&mut s,&id,&serde_json::json!({"type":"turn.completed","usage":{"input_tokens":11,"cached_input_tokens":3,"output_tokens":7}})).unwrap());
    assert_eq!(
        s.sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .input_tokens,
        11
    );
    assert!(
        runtime::event(
            &mut s,
            &id,
            &serde_json::json!({"type":"thread.started","thread_id":"other"})
        )
        .is_err()
    );
}

#[test]
fn completed_turn_with_missing_usage_keeps_measurements_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = live();
    let workspace = workspace(&dir.path().join("db"), &snapshot, config());
    let (mut snapshot, _) = workspace
        .store
        .lock()
        .unwrap()
        .apply(env(snapshot.revision, start(&snapshot)), &workspace.config)
        .unwrap();
    let id = snapshot.sessions.last().unwrap().id.clone();
    runtime::event(
        &mut snapshot,
        &id,
        &serde_json::json!({"type":"thread.started","thread_id":"usage-thread"}),
    )
    .unwrap();
    for usage in [
        serde_json::Value::Null,
        serde_json::json!({"input_tokens":11,"output_tokens":7}),
    ] {
        assert!(
            runtime::event(
                &mut snapshot,
                &id,
                &serde_json::json!({"type":"turn.completed","usage":usage})
            )
            .unwrap()
        );
        assert!(
            snapshot
                .sessions
                .last()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .usage
                .is_none()
        );
    }
    assert!(runtime::event(
        &mut snapshot,
        &id,
        &serde_json::json!({"type":"turn.completed","usage":{"input_tokens":11,"cached_input_tokens":0,"output_tokens":7}})
    ).unwrap());
    assert_eq!(
        snapshot
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .cached_input_tokens,
        0
    );
}
#[test]
fn migration_keeps_comments_and_restart_interrupts_only_unfinished_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let s = live();
    let w = workspace(&path, &s, config());
    let request = env(s.revision, start(&s));
    let (s, _) = w
        .store
        .lock()
        .unwrap()
        .apply(request.clone(), &w.config)
        .unwrap();
    // Simulate old snapshots with optional contract fields absent.
    let mut old = serde_json::to_value(demo_snapshot(DirectorProfile::default())).unwrap();
    for p in old["projects"].as_array_mut().unwrap() {
        p.as_object_mut().unwrap().remove("github");
    }
    for p in old["sessions"].as_array_mut().unwrap() {
        p.as_object_mut().unwrap().remove("worker");
    }
    let migrated: Snapshot = serde_json::from_value(old).unwrap();
    assert!(migrated.projects[0].github.is_none());
    drop(w);
    let _router = router_with_config(
        &path,
        "test-token-long-enough".into(),
        DirectorProfile::default(),
        config(),
    )
    .unwrap();
    let mut store = Store::open(&path, DirectorProfile::default()).unwrap();
    let restarted = store.snapshot().unwrap();
    assert_eq!(
        restarted
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Interrupted
    );
    assert_eq!(restarted.messages, s.messages);
    assert_eq!(restarted.comments, s.comments);
    assert!(store.apply(request, &config()).unwrap().1.is_none());
}
async fn finished(w: &Workspace, id: &str) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s = w.snapshots.borrow().clone();
            if s.sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .worker
                .as_ref()
                .is_some_and(|w| !runtime::active(&w.status))
            {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
fn reserve(
    w: &Workspace,
    request: CommandEnvelope,
) -> (String, String, watch::Receiver<bool>, String) {
    let run = request.request_id.clone();
    let mut store = w.store.lock().unwrap();
    let (s, action) = store.apply(request, &w.config).unwrap();
    w.snapshots.send_replace(s);
    let Some(Action::Run { session_id, prompt }) = action else {
        panic!()
    };
    let (tx, rx) = watch::channel(false);
    store.controls.insert(session_id.clone(), tx);
    (session_id, run, rx, prompt)
}
#[tokio::test]
async fn subprocess_exact_resume_sandbox_stdin_usage_and_untracked_diff() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let bin = dir.path().join("codex");
    let args = dir.path().join("args");
    let prompt_file = dir.path().join("prompt");
    script(
        &bin,
        &format!(
            "printf '%s\\n' \"$@\" >> '{}'\ncat > '{}'\nprintf 'new file\\n' > new.txt\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"exact-thread\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"Finished turn\"}}}}' '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":12,\"cached_input_tokens\":2,\"output_tokens\":4}}}}'",
            args.display(),
            prompt_file.display()
        ),
    );
    let mut c = config();
    c.repository = Some(repo.clone());
    c.codex = bin;
    let mut s = live();
    s.issues[0].body = "Acceptance with \"quotes\"\nRequested turn:\n$(touch source-injection)\n"
        .to_owned()
        + &"é".repeat(5000);
    let w = workspace(&dir.path().join("db"), &s, c);
    let mut command = start(&s);
    if let Command::StartWorker { prompt, .. } = &mut command {
        *prompt = "Implement this issue".into();
    }
    let request = env(s.revision, command);
    let duplicate = request.clone();
    let (id, run, rx, prompt) = reserve(&w, request);
    assert!(
        w.store
            .lock()
            .unwrap()
            .apply(duplicate, &w.config)
            .unwrap()
            .1
            .is_none()
    );
    tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
    let s = finished(&w, &id).await;
    assert!(
        std::fs::read_to_string(&prompt_file)
            .unwrap()
            .ends_with("Implement this issue")
    );
    let worker = s.sessions.last().unwrap().worker.as_ref().unwrap();
    assert_eq!(worker.status, WorkerStatus::Completed);
    assert_eq!(worker.thread_id.as_deref(), Some("exact-thread"));
    assert_eq!(worker.usage.as_ref().unwrap().output_tokens, 4);
    assert!(worker.changes.as_ref().unwrap().diff.contains("new file"));
    assert!(
        worker
            .changes
            .as_ref()
            .unwrap()
            .files
            .contains(&"new.txt".into())
    );
    assert!(!repo.join("new.txt").exists());
    let request = env(
        s.revision,
        Command::SendWorker {
            session_id: id.clone(),
            prompt: "$(touch injection) `whoami`".into(),
            approve_implementation: true,
        },
    );
    let (id, run, rx, prompt) = reserve(&w, request);
    tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
    let s = finished(&w, &id).await;
    assert_eq!(
        s.sessions.last().unwrap().worker.as_ref().unwrap().status,
        WorkerStatus::Completed
    );
    let args = std::fs::read_to_string(args).unwrap();
    assert!(args.starts_with("app-server\n--stdio\n"));
    assert_eq!(args.matches("app-server\n--stdio\n").count(), 2);
    assert!(!args.contains("bypass"));
    let prompt = std::fs::read_to_string(prompt_file).unwrap();
    assert!(prompt.ends_with("$(touch injection) `whoami`"));
    let context = prompt
        .split("Relay context JSON:\n")
        .nth(1)
        .unwrap()
        .split("\n$(touch injection)")
        .next()
        .unwrap();
    let context: serde_json::Value = serde_json::from_str(context).unwrap();
    let issue = &s.issues[0];
    assert_eq!(
        context["source_issue"],
        serde_json::json!({
            "provider": issue.reference.as_ref().unwrap().provider, "repository": issue.reference.as_ref().unwrap().repository,
            "number": issue.reference.as_ref().unwrap().number, "url": issue.reference.as_ref().unwrap().url,
            "title": issue.title, "body": context["source_issue"]["body"], "local_task": false,
        })
    );
    assert_eq!(context["source_truncated"], true);
    let supplied_body = context["source_issue"]["body"].as_str().unwrap();
    assert!(issue.body.starts_with(supplied_body));
    assert!(supplied_body.len() <= 8192);
    assert!(supplied_body.contains("$(touch source-injection)"));
    assert!(
        s.messages
            .iter()
            .filter(|m| m.kind == "issue-context")
            .all(|m| m.body.len() < 64 * 1024)
    );
    assert_eq!(
        context["effective_profile"]["completion"],
        serde_json::to_value(s.effective_profile(&s.directors[0]).unwrap().completion).unwrap()
    );
    assert!(prompt.contains("untrusted context"));
    assert!(s.messages.iter().any(|m| m.session_id == id
        && m.kind == "prompt"
        && m.body == "$(touch injection) `whoami`"));
    assert_eq!(
        s.messages
            .iter()
            .filter(|m| m.session_id == id && m.kind == "issue-context")
            .count(),
        2
    );
    assert!(prompt.contains("Do not merge, push, deploy"));
}
#[tokio::test]
async fn stop_before_launch_and_while_running_are_terminal_and_no_duplicate_launch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let marker = dir.path().join("started");
    let bin = dir.path().join("codex");
    script(
        &bin,
        &format!(
            "echo launched >> '{}'\ncat >/dev/null\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"thread\"}}'\nsleep 60",
            marker.display()
        ),
    );
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    w.store.lock().unwrap().controls[&id].send_replace(true);
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    assert_eq!(
        finished(&w, &id)
            .await
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Stopped
    );
    assert!(!marker.exists());
    let s = w.snapshots.borrow().clone();
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    w.store.lock().unwrap().controls[&id].send_replace(true);
    let s = finished(&w, &id).await;
    assert_eq!(
        s.sessions.last().unwrap().worker.as_ref().unwrap().status,
        WorkerStatus::Stopped
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap().lines().count(), 1);
}
#[tokio::test]
async fn process_failure_is_truthful() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let bin = dir.path().join("codex");
    script(&bin, "cat >/dev/null\necho secret-token >&2\nexit 7");
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    let s = finished(&w, &id).await;
    let w = s.sessions.last().unwrap().worker.as_ref().unwrap();
    assert_eq!(w.status, WorkerStatus::Failed);
    assert!(!w.error.as_ref().unwrap().contains("secret-token"));
    assert!(w.usage.is_none());
}
#[test]
fn paginated_board_maps_status_and_filters_drafts_prs_and_other_repos() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gh");
    let calls = dir.path().join("calls");
    let page = |nodes: serde_json::Value, next: bool, cursor: &str| serde_json::json!({"nodes":nodes,"pageInfo":{"hasNextPage":next,"endCursor":cursor}});
    let item = |kind: &str, typename: &str, repo: &str, number: u64, status: Option<&str>| serde_json::json!({"type":kind,"content":{"__typename":typename,"number":number,"title":format!("Issue {number}"),"body":"Task", "url":format!("https://github.com/{repo}/issues/{number}"),"repository":{"nameWithOwner":repo},"labels":{"nodes":[{"name":"dogfood"}]}},"fieldValueByName":status.map(|s|serde_json::json!({"optionId":s}))});
    let fields1 = serde_json::json!({"data":{"node":{"fields":page(serde_json::json!([{"name":"Title"}]),true,"fields-next")}}});
    let fields2 = serde_json::json!({"data":{"node":{"fields":page(serde_json::json!([{"name":"Status","options":[{"id":"todo","name":"Todo"},{"id":"doing","name":"In Progress"}]}]),false,"")}}});
    let items1 = serde_json::json!({"data":{"node":{"items":page(serde_json::json!([item("ISSUE","Issue","jens-hj/relay",7,Some("doing")),item("DRAFT_ISSUE","DraftIssue","jens-hj/relay",8,None),item("PULL_REQUEST","PullRequest","jens-hj/relay",9,None),item("ISSUE","Issue","other/repo",10,None)]),true,"items-next")}}});
    let items2 = serde_json::json!({"data":{"node":{"items":page(serde_json::json!([item("ISSUE","Issue","jens-hj/relay",11,None)]),false,"")}}});
    // Fixed test responses; inspect actual argument arrays without a shell in production.
    script(
        &bin,
        &format!(
            "printf '%s\\n' \"$@\" >> '{}'\ncase \"$*\" in\n *users/jens-hj*) echo '{{\"type\":\"User\"}}';;\n *projectV2*) echo '{{\"data\":{{\"user\":{{\"projectV2\":{{\"id\":\"P1\",\"title\":\"Relay\",\"url\":\"https://github.com/users/jens-hj/projects/5\"}}}}}}}}';;\n *fields*after=fields-next*) echo '{}';;\n *fields*) echo '{}';;\n *items*after=items-next*) echo '{}';;\n *items*) echo '{}';;\n *) exit 1;;\nesac",
            calls.display(),
            fields2,
            fields1,
            items2,
            items1
        ),
    );
    let mut c = config();
    c.gh = bin;
    let board = github::sync(&c, "live").unwrap();
    assert_eq!(board.issues.len(), 2);
    assert_eq!(board.issues[0].id, "github:jens-hj/relay:7@live");
    assert_eq!(board.issues[0].column_id, "doing");
    assert_eq!(board.issues[1].column_id, "github-no-status");
    assert_eq!(board.columns[1].title, "In Progress");
    let calls = std::fs::read_to_string(calls).unwrap();
    assert!(calls.contains("after=fields-next"));
    assert!(calls.contains("after=items-next"));
    assert!(!calls.contains("mutation"));
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c.clone());
    w.store
        .lock()
        .unwrap()
        .connection
        .execute(
            "INSERT OR REPLACE INTO syncs(project_id,request_id) VALUES('live','sync-test')",
            [],
        )
        .unwrap();
    w.synchronize("live", "sync-test");
    let good = w.snapshots.borrow().clone();
    script(&c.gh, "echo secret-auth-detail >&2\nexit 1");
    w.store
        .lock()
        .unwrap()
        .connection
        .execute(
            "INSERT OR REPLACE INTO syncs(project_id,request_id) VALUES('live','sync-test')",
            [],
        )
        .unwrap();
    w.synchronize("live", "sync-test");
    let failed = w.snapshots.borrow().clone();
    assert_eq!(failed.issues, good.issues);
    assert_eq!(failed.projects[0].columns, good.projects[0].columns);
    assert_eq!(
        failed.projects[0].github.as_ref().unwrap().last_synced_at,
        good.projects[0].github.as_ref().unwrap().last_synced_at
    );
    assert!(
        failed.projects[0]
            .github
            .as_ref()
            .unwrap()
            .sync_error
            .is_some()
    );
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("secret-auth-detail")
    );
}
#[test]
fn late_events_cannot_overwrite_another_run_and_bounded_diff_handles_binary() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    std::fs::write(repo.join("large"), "x\n".repeat(100_000)).unwrap();
    std::fs::write(repo.join("binary"), [0, 1, 2, 3]).unwrap();
    let changes = runtime::changes(repo.to_str().unwrap(), "HEAD").unwrap();
    assert!(changes.diff.len() <= 128 * 1024);
    assert!(changes.truncated);
    assert!(changes.files.contains(&"binary".into()));
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (id, run, _rx, _prompt) = reserve(&w, env(s.revision, start(&s)));
    w.store
        .lock()
        .unwrap()
        .connection
        .execute("UPDATE runs SET run_id='newer' WHERE session_id=?1", [&id])
        .unwrap();
    let before = w.snapshots.borrow().clone();
    assert!(
        w.update_run(&id, &run, |s| {
            s.messages.clear();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(*w.snapshots.borrow(), before);
}
#[tokio::test]
async fn concurrent_http_retry_launches_once_and_reconnect_observes_active_worker() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let marker = dir.path().join("launched");
    let bin = dir.path().join("codex");
    script(
        &bin,
        &format!(
            "echo launch >> '{}'\ncat >/dev/null\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"thread\"}}'\nsleep 60",
            marker.display()
        ),
    );
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let db = dir.path().join("db");
    {
        let _w = workspace(&db, &s, c.clone());
    }
    let token = "runtime-integration-token";
    let app = router_with_config(&db, token.into(), DirectorProfile::default(), c).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let current: Snapshot = client
        .get(format!("http://{addr}/v1/snapshot"))
        .bearer_auth(token)
        .header("x-relay-protocol", "2")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let request = env(current.revision, start(&s));
    let id = format!("session-{}", request.request_id);
    let send = |e: &CommandEnvelope| {
        client
            .post(format!("http://{addr}/v1/commands"))
            .bearer_auth(token)
            .header("x-relay-protocol", "2")
            .json(e)
            .send()
    };
    let (a, b) = tokio::join!(send(&request), send(&request));
    assert!(a.unwrap().status().is_success());
    assert!(b.unwrap().status().is_success());
    let read = || {
        client
            .get(format!("http://{addr}/v1/snapshot"))
            .bearer_auth(token)
            .header("x-relay-protocol", "2")
            .send()
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s: Snapshot = read().await.unwrap().json().await.unwrap();
            if s.sessions
                .last()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .thread_id
                .is_some()
                && marker.exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut upgrade = format!("ws://{addr}/v1/events")
        .into_client_request()
        .unwrap();
    upgrade.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer runtime-integration-token"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(upgrade.clone())
        .await
        .unwrap();
    let first: Snapshot =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(
        first
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Running
    );
    drop(socket);
    let (mut socket, _) = tokio_tungstenite::connect_async(upgrade).await.unwrap();
    let latest: Snapshot =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(
        latest
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Running
    );
    drop(socket);
    let stop = env(latest.revision, Command::StopWorker { session_id: id });
    assert!(send(&stop).await.unwrap().status().is_success());
    assert!(send(&stop).await.unwrap().status().is_success());
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s: Snapshot = read().await.unwrap().json().await.unwrap();
            if s.sessions.last().unwrap().worker.as_ref().unwrap().status == WorkerStatus::Stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(marker).unwrap().lines().count(), 1);
    server.abort();
}
#[test]
fn actual_v1_database_migrates_without_losing_local_comments() {
    for legacy_version in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let defaults = DirectorProfile {
            max_workers: 7,
            ..DirectorProfile::default()
        };
        let mut s = demo_snapshot(defaults);
        s.apply(
            Command::AddComment {
                message_id: "m2".into(),
                quote: None,
                author: "Reviewer".into(),
                body: "Keep this".into(),
            },
            "legacy",
            1,
        )
        .unwrap();
        let mut value = serde_json::to_value(&s).unwrap();
        for p in value["projects"].as_array_mut().unwrap() {
            p.as_object_mut().unwrap().remove("github");
        }
        for p in value["sessions"].as_array_mut().unwrap() {
            p.as_object_mut().unwrap().remove("worker");
        }
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE workspace(id INTEGER PRIMARY KEY,snapshot TEXT NOT NULL);CREATE TABLE receipts(request_id TEXT PRIMARY KEY,request TEXT NOT NULL);PRAGMA user_version=1;").unwrap();
        connection
            .pragma_update(None, "user_version", legacy_version)
            .unwrap();
        let receipt = env(
            0,
            Command::AddComment {
                message_id: "m2".into(),
                quote: None,
                author: "Reviewer".into(),
                body: "Keep this".into(),
            },
        );
        connection
            .execute(
                "INSERT INTO receipts VALUES(?1,?2)",
                params![receipt.request_id, serde_json::to_string(&receipt).unwrap()],
            )
            .unwrap();
        connection
            .execute("INSERT INTO workspace VALUES(1,?1)", [value.to_string()])
            .unwrap();
        drop(connection);
        let mut store = Store::open(&db, DirectorProfile::default()).unwrap();
        s.migrate_projects();
        assert_eq!(store.snapshot().unwrap(), s);
        // Legacy optional fields default to None; persisted defaults win over new seed defaults.
        assert!(
            store
                .snapshot()
                .unwrap()
                .projects
                .iter()
                .all(|p| p.github.is_none())
        );
        assert!(
            store
                .snapshot()
                .unwrap()
                .sessions
                .iter()
                .all(|s| s.worker.is_none())
        );
        assert!(
            store
                .apply(receipt.clone(), &RuntimeConfig::default())
                .unwrap()
                .1
                .is_none()
        );
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            5
        );
        drop(store);
        let mut reopened = Store::open(&db, DirectorProfile::default()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), s);
        assert!(
            reopened
                .apply(receipt, &RuntimeConfig::default())
                .unwrap()
                .1
                .is_none()
        );
        // Foundation's >1 guard now rejects this database before it can erase runtime fields.
        assert!(
            reopened
                .connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap()
                > 1
        );
    }
}

#[test]
fn newer_database_version_is_rejected_without_mutating_history() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    let before = store.snapshot().unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 6)
        .unwrap();
    drop(store);
    let error = Store::open(&db, DirectorProfile::default()).err().unwrap();
    assert_eq!(error.code, "invalid");
    let connection = Connection::open(&db).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        6
    );
    let json: String = connection
        .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(serde_json::from_str::<Snapshot>(&json).unwrap(), before);
}
#[test]
fn sync_receipts_are_idempotent_and_obsolete_results_do_not_publish() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gh");
    script(&bin, "exit 1");
    let mut c = config();
    c.gh = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let request = env(
        s.revision,
        Command::SyncProject {
            project_id: "live".into(),
        },
    );
    let mut store = w.store.lock().unwrap();
    let (saved, action) = store.apply(request.clone(), &w.config).unwrap();
    assert!(matches!(action, Some(Action::Sync(_))));
    assert!(store.apply(request.clone(), &w.config).unwrap().1.is_none());
    w.snapshots.send_replace(saved.clone());
    drop(store);
    w.synchronize("live", "superseded");
    assert_eq!(*w.snapshots.borrow(), saved);
    w.synchronize("live", &request.request_id);
    assert!(
        w.snapshots.borrow().projects[0]
            .github
            .as_ref()
            .unwrap()
            .sync_error
            .is_some()
    );
}
#[tokio::test]
async fn restart_reaps_owned_process_and_late_output_cannot_replace_interrupted_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let bin = dir.path().join("codex");
    script(
        &bin,
        "cat >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"thread\"}'\nsleep 60",
    );
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let db = dir.path().join("db");
    let w = workspace(&db, &s, c.clone());
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    let running = tokio::spawn(runtime::run(w.clone(), id.clone(), run.clone(), prompt, rx));
    tokio::time::timeout(Duration::from_secs(10), async {
        while w
            .snapshots
            .borrow()
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (pid, identity): (u32, String) = w
        .store
        .lock()
        .unwrap()
        .connection
        .query_row(
            "SELECT pid,identity FROM processes WHERE session_id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(runtime::process_identity(pid), Some(identity));
    let _restarted = router_with_config(
        &db,
        "test-token-long-enough".into(),
        DirectorProfile::default(),
        c,
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap();
    let snapshot = w.store.lock().unwrap().snapshot().unwrap();
    assert_eq!(
        snapshot
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Interrupted
    );
    assert_eq!(
        snapshot
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread_id
            .as_deref(),
        Some("thread")
    );
    assert!(
        w.update_run(&id, &run, |s| {
            s.messages.clear();
            Ok(())
        })
        .is_err()
    );
}
#[tokio::test]
async fn resume_rechecks_current_profile_and_requires_new_approval() {
    let dir = tempfile::tempdir().unwrap();
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (id, run, _rx, _prompt) = reserve(&w, env(s.revision, start(&s)));
    w.update_run(&id, &run, |s| {
        let worker = s.sessions.last_mut().unwrap().worker.as_mut().unwrap();
        worker.status = WorkerStatus::Completed;
        worker.thread_id = Some("thread".into());
        worker.worktree = Some("/owned/worktree".into());
        Ok(())
    })
    .unwrap();
    let mut store = w.store.lock().unwrap();
    let s = store.snapshot().unwrap();
    let command = |approved| Command::SendWorker {
        session_id: id.clone(),
        prompt: "Continue".into(),
        approve_implementation: approved,
    };
    assert!(
        store
            .apply(env(s.revision, command(false)), &w.config)
            .is_err()
    );
    let mut denied = s;
    denied.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Deny);
    store.save(&denied).unwrap();
    assert!(
        store
            .apply(env(denied.revision, command(true)), &w.config)
            .is_err()
    );
    denied.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Allow);
    denied.projects[0].defaults.scope = DirectorScope::Issues {
        issue_ids: vec!["other".into()],
    };
    store.save(&denied).unwrap();
    assert!(
        store
            .apply(env(denied.revision, command(true)), &w.config)
            .is_err()
    );
}
#[tokio::test]
async fn profile_changed_after_reservation_is_enforced_before_process_launch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    let marker = dir.path().join("launched");
    let bin = dir.path().join("codex");
    script(&bin, &format!("echo launched > '{}'", marker.display()));
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    {
        let mut store = w.store.lock().unwrap();
        let mut s = store.snapshot().unwrap();
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Deny);
        store.save(&s).unwrap();
        w.snapshots.send_replace(s);
    }
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    assert!(!marker.exists());
    let s = finished(&w, &id).await;
    let worker = s.sessions.last().unwrap().worker.as_ref().unwrap();
    assert_eq!(worker.status, WorkerStatus::Failed);
    assert!(
        worker
            .error
            .as_ref()
            .unwrap()
            .contains("denies implementation")
    );
}
#[test]
fn codex_jsonl_tolerates_unknown_events_items_and_extra_fields() {
    let dir = tempfile::tempdir().unwrap();
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (mut s, _) = w
        .store
        .lock()
        .unwrap()
        .apply(env(s.revision, start(&s)), &w.config)
        .unwrap();
    let id = s.sessions.last().unwrap().id.clone();
    runtime::event(&mut s, &id, &serde_json::json!({"type":"thread.started","thread_id":"recorded-thread","future_metadata":{"version":2}})).unwrap();
    let before = s.clone();
    assert!(
        !runtime::event(
            &mut s,
            &id,
            &serde_json::json!({"type":"future.event","new_field":true})
        )
        .unwrap()
    );
    assert!(!runtime::event(&mut s, &id, &serde_json::json!({"type":"item.completed","item":{"id":"new-item","type":"future_item","payload":{"new":true}}})).unwrap());
    assert_eq!(s, before);
    runtime::event(&mut s, &id, &serde_json::json!({"type":"item.completed","future_field":1,"item":{"id":"message","type":"agent_message","text":"Compatible response","future_field":[1,2,3]}})).unwrap();
    assert_eq!(s.messages.last().unwrap().body, "Compatible response");
    assert!(runtime::event(&mut s, &id, &serde_json::json!({"type":"turn.completed","future_field":{},"usage":{"input_tokens":9,"cached_input_tokens":2,"output_tokens":4,"future_tokens":7}})).unwrap());
    assert_eq!(
        s.sessions.last().unwrap().worker.as_ref().unwrap().usage,
        Some(TokenUsage {
            input_tokens: 9,
            cached_input_tokens: 2,
            output_tokens: 4
        })
    );
}
pub(super) fn review_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    repo
}
#[tokio::test]
async fn failure_events_invalid_output_missing_thread_and_spawn_failure_are_terminal() {
    let cases = [
        ("head -c 1048577 /dev/zero | tr '\\000' x", "exceeds 1 MiB"),
        (
            "printf '%s\\n' '{\"type\":\"turn.failed\",\"error\":{\"message\":\"secret\"}}'",
            "turn failed",
        ),
        (
            "printf '%s\\n' '{\"type\":\"error\",\"message\":\"secret\"}'",
            "turn failed",
        ),
        ("echo not-json", "Invalid Codex app-server response"),
        ("true", "disconnected"),
        (
            "printf '%s\\n' '{\"type\":\"thread.started\"}'",
            "disconnected",
        ),
        ("missing-executable", "Cannot launch agent harness"),
    ];
    for (body, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let repo = review_repo(dir.path());
        let bin = dir.path().join("codex");
        if body != "missing-executable" {
            script(&bin, &format!("cat >/dev/null\n{body}"));
        }
        let mut c = config();
        c.repository = Some(repo);
        c.codex = bin;
        let s = live();
        let w = workspace(&dir.path().join("db"), &s, c);
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
        let s = finished(&w, &id).await;
        let worker = s.sessions.last().unwrap().worker.as_ref().unwrap();
        assert_eq!(worker.status, WorkerStatus::Failed, "{body}");
        assert!(
            worker.error.as_ref().unwrap().contains(expected),
            "{:?}",
            worker.error
        );
        assert!(!worker.error.as_ref().unwrap().contains("secret"));
    }
}
#[tokio::test]
async fn stop_cancel_and_shutdown_terminate_owned_descendants() {
    for mode in ["stop", "cancel", "shutdown"] {
        let dir = tempfile::tempdir().unwrap();
        let repo = review_repo(dir.path());
        let pid_file = dir.path().join("descendant");
        let bin = dir.path().join("codex");
        script(
            &bin,
            &format!(
                "cat >/dev/null\nsleep 60 &\necho $! > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}'\nwait",
                pid_file.display()
            ),
        );
        let mut c = config();
        c.repository = Some(repo);
        c.codex = bin;
        let s = live();
        let w = workspace(&dir.path().join("db"), &s, c);
        let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
        let task = tokio::spawn(runtime::run(w.clone(), id.clone(), run, prompt, rx));
        tokio::time::timeout(Duration::from_secs(10), async {
            while std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .is_none()
                || w.snapshots
                    .borrow()
                    .sessions
                    .last()
                    .unwrap()
                    .worker
                    .as_ref()
                    .unwrap()
                    .thread_id
                    .is_none()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        match mode {
            "stop" => {
                w.store.lock().unwrap().controls[&id].send_replace(true);
            }
            "cancel" => task.abort(),
            "shutdown" => {
                use futures_util::StreamExt;
                use tokio_tungstenite::tungstenite::client::IntoClientRequest;
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let app = Router::new()
                    .route("/events", get(events))
                    .with_state(w.clone());
                let (signal, received) = tokio::sync::oneshot::channel::<()>();
                let shutdown = Shutdown {
                    workspace: w.clone(),
                };
                let server = tokio::spawn(async move {
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            received.await.unwrap();
                            shutdown.shutdown().await.unwrap();
                        })
                        .await
                        .unwrap();
                });
                let mut request = format!("ws://{address}/events")
                    .into_client_request()
                    .unwrap();
                request
                    .headers_mut()
                    .insert("authorization", "Bearer test-token".parse().unwrap());
                let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
                assert!(client.next().await.unwrap().unwrap().is_text());
                signal.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        match client.next().await {
                            Some(Ok(message)) if message.is_close() => break,
                            None => break,
                            Some(Err(error)) => panic!("socket shutdown: {error}"),
                            _ => {}
                        }
                    }
                    server.await.unwrap();
                })
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap();
        let s = finished(&w, &id).await;
        assert_eq!(
            s.sessions.last().unwrap().worker.as_ref().unwrap().status,
            if mode == "cancel" {
                WorkerStatus::Interrupted
            } else {
                WorkerStatus::Stopped
            }
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .is_some_and(|stat| {
                        stat.rsplit_once(')').unwrap().1.split_whitespace().next() != Some("Z")
                    });
                if !alive {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
#[test]
fn review_covers_base_commits_staged_unstaged_deletions_renames_and_safe_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    for name in ["committed", "staged", "unstaged", "deleted", "old-name"] {
        std::fs::write(repo.join(name), "base\n").unwrap();
    }
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
            "files",
        ],
    );
    let base = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    std::fs::write(repo.join("committed"), "worker commit\n").unwrap();
    git(&repo, &["add", "committed"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qm",
            "worker",
        ],
    );
    std::fs::write(repo.join("staged"), "staged change\n").unwrap();
    git(&repo, &["add", "staged"]);
    std::fs::write(repo.join("unstaged"), "unstaged change\n").unwrap();
    std::fs::remove_file(repo.join("deleted")).unwrap();
    git(&repo, &["mv", "old-name", "new-name"]);
    std::fs::write(dir.path().join("outside"), "outside-secret-content\n").unwrap();
    symlink(dir.path().join("outside"), repo.join("link")).unwrap();
    std::fs::write(repo.join("binary"), [0, 255, 1]).unwrap();
    let changes = runtime::changes(repo.to_str().unwrap(), &base).unwrap();
    for name in [
        "committed",
        "staged",
        "unstaged",
        "deleted",
        "old-name",
        "new-name",
        "link",
        "binary",
    ] {
        assert!(changes.files.contains(&name.into()), "{name}");
    }
    for text in [
        "worker commit",
        "staged change",
        "unstaged change",
        "deleted file",
        "Untracked symlink",
    ] {
        assert!(changes.diff.contains(text), "{text}");
    }
    assert!(!changes.diff.contains("outside-secret-content"));
}
#[test]
fn messages_and_file_lists_have_explicit_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (mut s, _) = w
        .store
        .lock()
        .unwrap()
        .apply(env(s.revision, start(&s)), &w.config)
        .unwrap();
    let id = s.sessions.last().unwrap().id.clone();
    runtime::event(&mut s,&id,&serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"é".repeat(100_000)}})).unwrap();
    let body = &s.messages.last().unwrap().body;
    assert!(body.len() <= 64 * 1024);
    assert!(body.ends_with("[Message truncated]"));
    let repo = review_repo(dir.path());
    for i in 0..600 {
        std::fs::write(repo.join(format!("file-{i:04}")), "x\n").unwrap();
    }
    let changes = runtime::changes(repo.to_str().unwrap(), "HEAD").unwrap();
    assert!(changes.files.len() <= 512);
    assert!(changes.truncated);
}
#[test]
fn removed_board_items_keep_linked_identity_without_becoming_active() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gh");
    let items = dir.path().join("items");
    std::fs::write(
        &items,
        "{\"data\":{\"node\":{\"items\":{\"nodes\":[],\"pageInfo\":{\"hasNextPage\":false}}}}}",
    )
    .unwrap();
    script(
        &bin,
        &format!(
            "case \"$*\" in\n *users/*) echo '{{\"type\":\"User\"}}';;\n *projectV2*) echo '{{\"data\":{{\"user\":{{\"projectV2\":{{\"id\":\"P1\",\"title\":\"Relay\",\"url\":\"board\"}}}}}}}}';;\n *fields*) echo '{{\"data\":{{\"node\":{{\"fields\":{{\"nodes\":[],\"pageInfo\":{{\"hasNextPage\":false}}}}}}}}}}';;\n *items*) cat '{}';;\n *) exit 1;;\nesac",
            items.display()
        ),
    );
    let mut c = config();
    c.gh = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let (id, run, _rx, _prompt) = reserve(&w, env(s.revision, start(&s)));
    w.update_run(&id, &run, |s| {
        let worker = s.sessions.last_mut().unwrap().worker.as_mut().unwrap();
        worker.status = WorkerStatus::Completed;
        worker.thread_id = Some("t".into());
        worker.worktree = Some("/owned".into());
        Ok(())
    })
    .unwrap();
    let issue_id = s.issues[0].id.clone();
    let director = s.directors[0].id.clone();
    w.store
        .lock()
        .unwrap()
        .connection
        .execute("INSERT INTO syncs VALUES('live','sync')", [])
        .unwrap();
    w.synchronize("live", "sync");
    let removed = w.snapshots.borrow().clone();
    let historic = removed.issues.iter().find(|i| i.id == issue_id).unwrap();
    assert_eq!(historic.column_id, "github-removed-from-board");
    assert_eq!(removed.projects[0].columns.len(), 1);
    assert_eq!(removed.projects[0].columns[0].title, "No status");
    assert!(
        !removed.projects[0]
            .columns
            .iter()
            .any(|c| c.id == historic.column_id)
    );
    assert_eq!(
        removed.sessions.last().unwrap().issue_id.as_deref(),
        Some(issue_id.as_str())
    );
    assert!(runtime::authorize_turn(&removed, &issue_id, &director, true, &w.config).is_err());
    let original = s.issues[0].clone();
    let response = serde_json::json!({"data":{"node":{"items":{"nodes":[{"type":"ISSUE","content":{"__typename":"Issue","number":original.reference.as_ref().unwrap().number,"title":original.title,"body":original.body,"url":original.reference.as_ref().unwrap().url,"repository":{"nameWithOwner":"jens-hj/relay"},"labels":{"nodes":[]}}}],"pageInfo":{"hasNextPage":false}}}}});
    std::fs::write(items, response.to_string()).unwrap();
    // Fixture ID differs from provider-qualified remote ID; simulate provider-qualified history.
    {
        let mut store = w.store.lock().unwrap();
        let mut snapshot = store.snapshot().unwrap();
        let new_id = format!(
            "github:jens-hj/relay:{}",
            original.reference.as_ref().unwrap().number
        );
        snapshot
            .issues
            .iter_mut()
            .find(|i| i.id == issue_id)
            .unwrap()
            .id = new_id.clone();
        snapshot.sessions.last_mut().unwrap().issue_id = Some(new_id);
        store.save(&snapshot).unwrap();
    }
    w.synchronize("live", "sync");
    let restored = w.snapshots.borrow().clone();
    let restored_id = restored.sessions.last().unwrap().issue_id.as_ref().unwrap();
    let issue = restored
        .issues
        .iter()
        .find(|i| &i.id == restored_id)
        .unwrap();
    assert_eq!(issue.column_id, "github-no-status");
    assert!(runtime::authorize_turn(&restored, &issue.id, &director, true, &w.config).is_ok());
}
#[test]
fn token_exclusion_is_verified_in_an_isolated_test_process() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::token_exclusion_child", "--nocapture"])
        .env("RELAY_TOKEN", "test-bearer-must-not-be-inherited")
        .env("RELAY_TOKEN_CHILD_TEST", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
}
#[tokio::test]
async fn token_exclusion_child() {
    if std::env::var_os("RELAY_TOKEN_CHILD_TEST").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = review_repo(dir.path());
    let bin = dir.path().join("codex");
    script(
        &bin,
        "test -z \"$RELAY_TOKEN\" || exit 99\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"t\"}' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}'",
    );
    let gh = dir.path().join("gh");
    script(
        &gh,
        "test -z \"$RELAY_TOKEN\" || exit 99\ncase \"$*\" in\n *users/*) echo '{\"type\":\"User\"}';;\n *projectV2*) echo '{\"data\":{\"user\":{\"projectV2\":{\"id\":\"P1\",\"title\":\"Relay\",\"url\":\"board\"}}}}';;\n *fields*) echo '{\"data\":{\"node\":{\"fields\":{\"nodes\":[],\"pageInfo\":{\"hasNextPage\":false}}}}}';;\n *items*) echo '{\"data\":{\"node\":{\"items\":{\"nodes\":[],\"pageInfo\":{\"hasNextPage\":false}}}}}';;\n *) exit 1;;\nesac",
    );
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    c.gh = gh;
    assert!(github::sync(&c, "live").is_ok());
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    assert_eq!(
        finished(&w, &id)
            .await
            .sessions
            .last()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Completed
    );
}
#[tokio::test]
async fn concurrent_runtime_events_publish_monotonic_committed_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, config());
    let (a, ar, _rx, _p) = reserve(&w, env(s.revision, start(&s)));
    let s = w.snapshots.borrow().clone();
    let (b, br, _rx, _p) = reserve(&w, env(s.revision, start(&s)));
    let initial = w.snapshots.borrow().revision;
    let initial_messages = w.snapshots.borrow().messages.len();
    let mut receiver = w.snapshots.subscribe();
    let writer = w.clone();
    let writing = tokio::task::spawn_blocking(move || {
        std::thread::scope(|scope| {
            for (id, run) in [(a, ar), (b, br)] {
                let w = writer.clone();
                scope.spawn(move||{for i in 0..25 {w.update_run(&id,&run,|s|{runtime::event(s,&id,&serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":format!("Evidence {i}")}}))?;Ok(())}).unwrap();}});
            }
        });
    });
    tokio::pin!(writing);
    let mut revision = initial;
    loop {
        tokio::select! { result=&mut writing=>{result.unwrap();break;}, changed=receiver.changed()=>{changed.unwrap();let next=receiver.borrow_and_update().revision;assert!(next>=revision);revision=next;} }
    }
    let saved = w.store.lock().unwrap().snapshot().unwrap();
    let published = w.snapshots.borrow().clone();
    assert_eq!(saved, published);
    assert_eq!(published.revision, initial + 50);
    assert_eq!(published.messages.len(), initial_messages + 50);
    assert!(published.revision >= revision);
}
#[test]
fn capture_limits_bytes_and_times_out_both_output_and_process_waits() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("tool");
    script(&bin, "head -c 1000000 /dev/zero\nsleep 60");
    let started = Instant::now();
    let result = crate::process::capture(
        &mut std::process::Command::new(&bin),
        1024,
        Duration::from_secs(2),
        "test tool",
    )
    .unwrap();
    assert_eq!(result.bytes.len(), 1024);
    assert!(result.truncated);
    assert!(started.elapsed() < Duration::from_secs(2));
    for closed in [false, true] {
        let pid_file = dir.path().join("pid");
        let close = if closed { "exec 1>&-" } else { "" };
        script(
            &bin,
            &format!(
                "{close}\nsleep 60 &\necho $! > '{}'\nwait",
                pid_file.display()
            ),
        );
        let started = Instant::now();
        let error = crate::process::capture(
            &mut std::process::Command::new(&bin),
            1024,
            Duration::from_millis(150),
            "test tool",
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .is_some_and(|stat| {
                    stat.rsplit_once(')').unwrap().1.split_whitespace().next() != Some("Z")
                });
            if !alive {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    script(
        &bin,
        "echo useful-output\necho secret-diagnostic >&2\nexit 7",
    );
    let output = crate::process::capture(
        &mut std::process::Command::new(&bin),
        1024,
        Duration::from_secs(1),
        "test tool",
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(!output.truncated);
    assert_eq!(output.bytes, b"useful-output\n");
}
#[test]
fn oversized_github_capture_reports_error_without_destroying_last_good_board() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gh");
    script(&bin, "head -c 131073 /dev/zero");
    let mut c = config();
    c.gh = bin;
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    w.store
        .lock()
        .unwrap()
        .connection
        .execute("INSERT INTO syncs VALUES('live','sync')", [])
        .unwrap();
    w.synchronize("live", "sync");
    let failed = w.snapshots.borrow().clone();
    assert_eq!(failed.issues, s.issues);
    assert_eq!(failed.projects[0].columns, s.projects[0].columns);
    let remote = failed.projects[0].github.as_ref().unwrap();
    assert_eq!(remote.last_synced_at, Some(1));
    assert!(
        remote
            .sync_error
            .as_ref()
            .unwrap()
            .contains("exceeds 128 KiB")
    );
}
#[test]
fn configured_live_project_migration_preserves_demo_and_historical_identities() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let defaults = DirectorProfile::default();
    let mut store = Store::open(&db, defaults.clone()).unwrap();
    let mut demo = store.snapshot().unwrap();
    demo.apply(
        Command::AddComment {
            message_id: "m2".into(),
            quote: None,
            author: "Reviewer".into(),
            body: "Retain fixture history".into(),
        },
        "fixture-comment",
        1,
    )
    .unwrap();
    store.save(&demo).unwrap();
    drop(store);
    let c = config();
    let _router = router_with_config(
        &db,
        "token-at-least-sixteen".into(),
        defaults.clone(),
        c.clone(),
    )
    .unwrap();
    let a = Store::open(&db, defaults.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let current = a.projects[0].clone();
    assert_ne!(current.id, "demo");
    assert!(!current.fixture);
    assert!(current.github.as_ref().unwrap().last_synced_at.is_none());
    assert_eq!(
        a.projects.iter().find(|p| p.id == "demo"),
        Some(&demo.projects[0])
    );
    assert_eq!(a.issues, demo.issues);
    assert_eq!(a.sessions, demo.sessions);
    assert_eq!(a.messages, demo.messages);
    assert_eq!(a.comments, demo.comments);
    for director in &demo.directors {
        assert!(a.directors.contains(director));
        assert_eq!(
            a.effective_profile(director).unwrap(),
            demo.effective_profile(director).unwrap()
        );
    }
    let director = a
        .directors
        .iter()
        .find(|d| d.project_id == current.id)
        .unwrap();
    assert_eq!(director.overrides, ProfileOverrides::default());
    assert_eq!(a.effective_profile(director).unwrap(), defaults);
    let _router = router_with_config(
        &db,
        "token-at-least-sixteen".into(),
        defaults.clone(),
        c.clone(),
    )
    .unwrap();
    assert_eq!(
        Store::open(&db, defaults.clone())
            .unwrap()
            .snapshot()
            .unwrap(),
        a
    );
    let mut next = c.clone();
    next.remote.as_mut().unwrap().number = 6;
    let _router = router_with_config(
        &db,
        "token-at-least-sixteen".into(),
        defaults.clone(),
        next.clone(),
    )
    .unwrap();
    let b = Store::open(&db, defaults.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_ne!(b.projects[0].id, current.id);
    assert_eq!(b.projects.len(), 3);
    assert_eq!(
        b.projects.iter().find(|p| p.id == current.id),
        Some(&current)
    );
    assert_eq!(
        b.projects.iter().find(|p| p.id == "demo"),
        Some(&demo.projects[0])
    );
    assert_eq!(b.issues, a.issues);
    assert_eq!(b.directors[..a.directors.len()], a.directors);
    assert!(!runtime::configured_project(&current, &next));
    let mut store = Store::open(&db, defaults.clone()).unwrap();
    assert!(
        store
            .apply(
                env(
                    b.revision,
                    Command::SyncProject {
                        project_id: current.id.clone()
                    }
                ),
                &next
            )
            .is_ok()
    );
    drop(store);
    let _router =
        router_with_config(&db, "token-at-least-sixteen".into(), defaults.clone(), c).unwrap();
    let restored = Store::open(&db, defaults).unwrap().snapshot().unwrap();
    assert_eq!(restored.projects[0], current);
    assert_eq!(restored.projects.len(), 3);
    assert_eq!(restored.directors, b.directors);
    assert_eq!(restored.comments, demo.comments);
}
#[test]
fn live_sync_does_not_touch_fixture_scope_history_and_default_director_can_delegate() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let bin = dir.path().join("gh");
    script(
        &bin,
        "case \"$*\" in\n *users/*) echo '{\"type\":\"User\"}';;\n *projectV2*) echo '{\"data\":{\"user\":{\"projectV2\":{\"id\":\"P1\",\"title\":\"Live Relay\",\"url\":\"board\"}}}}';;\n *fields*) echo '{\"data\":{\"node\":{\"fields\":{\"nodes\":[],\"pageInfo\":{\"hasNextPage\":false}}}}}';;\n *items*) echo '{\"data\":{\"node\":{\"items\":{\"nodes\":[{\"type\":\"ISSUE\",\"content\":{\"__typename\":\"Issue\",\"number\":3,\"title\":\"Live task\",\"body\":\"Implement\",\"url\":\"issue\",\"repository\":{\"nameWithOwner\":\"jens-hj/relay\"},\"labels\":{\"nodes\":[]}}}],\"pageInfo\":{\"hasNextPage\":false}}}}}';;\n *) exit 1;;\nesac",
    );
    let mut c = config();
    c.gh = bin;
    let _router = router_with_config(
        &db,
        "token-at-least-sixteen".into(),
        DirectorProfile::default(),
        c.clone(),
    )
    .unwrap();
    let s = Store::open(&db, DirectorProfile::default())
        .unwrap()
        .snapshot()
        .unwrap();
    let live_id = s.projects[0].id.clone();
    let fixtures = s.issues.clone();
    let w = workspace(&db, &s, c.clone());
    w.store
        .lock()
        .unwrap()
        .connection
        .execute("INSERT INTO syncs VALUES(?1,'sync')", [&live_id])
        .unwrap();
    w.synchronize(&live_id, "sync");
    let synced = w.snapshots.borrow().clone();
    assert_eq!(
        synced
            .issues
            .iter()
            .filter(|i| i.project_id == "demo")
            .cloned()
            .collect::<Vec<_>>(),
        fixtures
    );
    assert_eq!(synced.sessions, s.sessions);
    assert_eq!(synced.messages, s.messages);
    assert_eq!(synced.comments, s.comments);
    assert_eq!(synced.directors, s.directors);
    assert!(
        synced
            .validate_profile(
                "demo",
                &synced
                    .effective_profile(
                        synced
                            .directors
                            .iter()
                            .find(|d| d.id == "director-review")
                            .unwrap()
                    )
                    .unwrap()
            )
            .is_ok()
    );
    let issue = synced
        .issues
        .iter()
        .find(|i| i.project_id == live_id)
        .unwrap();
    let director = synced
        .directors
        .iter()
        .find(|d| d.project_id == live_id)
        .unwrap();
    let profile = synced.effective_profile(director).unwrap();
    assert!(!profile.responsibilities.contains(&Task::Implement));
    assert!(runtime::authorize_turn(&synced, &issue.id, &director.id, false, &c).is_ok());
    assert!(runtime::authorize_turn(&synced, &issue.id, &director.id, true, &c).is_ok());
    // Saved project bindings, rather than the last environment configuration,
    // remain authoritative for every registered board.
    for changed in ["owner", "number", "repository"] {
        let mut other = c.clone();
        let r = other.remote.as_mut().unwrap();
        match changed {
            "owner" => r.owner = "different-owner".into(),
            "number" => r.number += 1,
            "repository" => r.repository = "other/repo".into(),
            _ => unreachable!(),
        };
        assert!(runtime::authorize_turn(&synced, &issue.id, &director.id, true, &other).is_ok());
        let mut unregistered = synced.clone();
        unregistered.bindings.clear();
        // Migrated independent connections are authoritative too.
        assert!(
            runtime::authorize_turn(&unregistered, &issue.id, &director.id, true, &other).is_ok()
        );
        unregistered.connections.clear();
        assert!(
            runtime::authorize_turn(&unregistered, &issue.id, &director.id, true, &other).is_err()
        );
    }
    // A provider issue mirrored on a second board needs a distinct local identity,
    // while the original project's issue and result remain unchanged.
    {
        let mut store = w.store.lock().unwrap();
        let mut snapshot = store.snapshot().unwrap();
        snapshot
            .issues
            .iter_mut()
            .find(|i| i.id == issue.id)
            .unwrap()
            .result = Some("Reviewed on original board".into());
        snapshot
            .directors
            .iter_mut()
            .find(|d| d.id == director.id)
            .unwrap()
            .overrides
            .scope = Some(DirectorScope::Issues {
            issue_ids: vec![issue.id.clone()],
        });
        let mut history = snapshot.sessions[0].clone();
        history.id = "retained-board-history".into();
        history.project_id = live_id.clone();
        history.director_id = director.id.clone();
        history.issue_id = Some(issue.id.clone());
        history.worker = None;
        snapshot.sessions.push(history);
        store.save(&snapshot).unwrap();
    }
    let mut second = c.clone();
    second.remote.as_mut().unwrap().number = 6;
    let _router = router_with_config(
        &db,
        "token-at-least-sixteen".into(),
        DirectorProfile::default(),
        second.clone(),
    )
    .unwrap();
    let second_snapshot = Store::open(&db, DirectorProfile::default())
        .unwrap()
        .snapshot()
        .unwrap();
    let second_id = second_snapshot.projects[0].id.clone();
    let second_workspace = workspace(&db, &second_snapshot, second);
    second_workspace
        .store
        .lock()
        .unwrap()
        .connection
        .execute("INSERT INTO syncs VALUES(?1,'second-sync')", [&second_id])
        .unwrap();
    second_workspace.synchronize(&second_id, "second-sync");
    let both = second_workspace.snapshots.borrow().clone();
    let old = both
        .issues
        .iter()
        .find(|i| i.project_id == live_id)
        .unwrap();
    let new = both
        .issues
        .iter()
        .find(|i| i.project_id == second_id)
        .unwrap();
    assert_eq!(old.id, issue.id);
    assert_eq!(old.result.as_deref(), Some("Reviewed on original board"));
    assert_ne!(new.id, old.id);
    assert!(new.id.ends_with(&format!("@{second_id}")));
    assert_eq!(new.reference, old.reference);
    assert_eq!(
        both.sessions
            .iter()
            .find(|s| s.id == "retained-board-history")
            .unwrap()
            .issue_id
            .as_deref(),
        Some(old.id.as_str())
    );
    assert_eq!(
        both.effective_profile(both.directors.iter().find(|d| d.id == director.id).unwrap())
            .unwrap()
            .scope,
        DirectorScope::Issues {
            issue_ids: vec![old.id.clone()]
        }
    );
    assert_eq!(both.issues.iter().filter(|i| i.id == old.id).count(), 1);
    let new_director = both
        .directors
        .iter()
        .find(|d| d.project_id == second_id)
        .unwrap();
    assert!(
        runtime::authorize_turn(
            &both,
            &new.id,
            &new_director.id,
            true,
            &second_workspace.config
        )
        .is_ok()
    );
    assert!(
        runtime::authorize_turn(&both, &old.id, &director.id, true, &second_workspace.config)
            .is_ok()
    );
    let new_id = new.id.clone();
    second_workspace.synchronize(&second_id, "second-sync");
    assert_eq!(
        second_workspace
            .snapshots
            .borrow()
            .issues
            .iter()
            .find(|i| i.project_id == second_id)
            .unwrap()
            .id,
        new_id
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn hard_crash_kills_codex_parent_before_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::hard_crash_child"])
        .env("RELAY_CRASH_TEST_DIRECTORY", dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let db = dir.path().join("db");
    let pid_file = dir.path().join("pid");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if pid_file.exists() && db.exists() {
                let store = Store::open(&db, DirectorProfile::default()).unwrap();
                if store
                    .snapshot()
                    .unwrap()
                    .sessions
                    .last()
                    .unwrap()
                    .worker
                    .as_ref()
                    .unwrap()
                    .thread_id
                    .is_some()
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid: u32 = std::fs::read_to_string(pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .is_some_and(|stat| {
                    stat.rsplit_once(')').unwrap().1.split_whitespace().next() != Some("Z")
                });
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (_router, shutdown) = router_with_shutdown(
        &db,
        "crash-test-valid-token".into(),
        DirectorProfile::default(),
        RuntimeConfig::default(),
    )
    .unwrap();
    let snapshot = shutdown.workspace.store.lock().unwrap().snapshot().unwrap();
    let worker = snapshot.sessions.last().unwrap().worker.as_ref().unwrap();
    assert_eq!(worker.status, WorkerStatus::Interrupted);
    assert_eq!(worker.thread_id.as_deref(), Some("crash-thread"));
    assert!(worker.worktree.is_some());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn hard_crash_child() {
    let Some(directory) = std::env::var_os("RELAY_CRASH_TEST_DIRECTORY") else {
        return;
    };
    let dir = std::path::PathBuf::from(directory);
    let repo = review_repo(&dir);
    let bin = dir.join("codex");
    script(
        &bin,
        &format!(
            "cat >/dev/null\necho $$ > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"crash-thread\"}}'\nexec sleep 60",
            dir.join("pid").display()
        ),
    );
    let mut c = config();
    c.repository = Some(repo);
    c.codex = bin;
    let s = live();
    let w = workspace(&dir.join("db"), &s, c);
    let (id, run, rx, prompt) = reserve(&w, env(s.revision, start(&s)));
    runtime::run(w, id, run, prompt, rx).await;
}

async fn saved_turn(w: &Workspace, session: &str, parts: Vec<Part>) -> CommandEnvelope {
    let revision = conversation::read_drafts(&w.store.lock().unwrap().connection)
        .unwrap()
        .into_iter()
        .find(|d| d.session_id == session)
        .map(|d| d.revision)
        .unwrap_or(0);
    let saved = conversation::save_draft(
        State(w.clone()),
        RoutePath(session.into()),
        Json(SaveDraft {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: revision,
            parts: parts.clone(),
        }),
    )
    .await
    .unwrap()
    .0;
    env(
        w.snapshots.borrow().revision,
        Command::SubmitTurn {
            session_id: session.into(),
            draft_revision: saved.revision,
            parts,
            approve_implementation: true,
        },
    )
}

fn completed_app_workspace(dir: &Path, code: &str) -> (Workspace, String, PathBuf) {
    let repo = review_repo(dir);
    let bin = dir.join("fake-app-server");
    script(&bin, code);
    let mut c = config();
    c.repository = Some(repo.clone());
    c.codex = bin;
    let w = workspace(&dir.join("db"), &live(), c);
    let (id, run, _rx, _prompt) = reserve(&w, env(0, start(&live())));
    w.update_run(&id, &run, |s| {
        let worker = s
            .sessions
            .iter_mut()
            .find(|s| s.id == id)
            .unwrap()
            .worker
            .as_mut()
            .unwrap();
        worker.status = WorkerStatus::Completed;
        worker.thread_id = Some("12345678-1234-4234-8234-123456789abc".into());
        worker.worktree = Some(repo.to_str().unwrap().into());
        worker.branch = Some("main".into());
        worker.base_commit = Some(
            String::from_utf8(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .trim()
            .into(),
        );
        Ok(())
    })
    .unwrap();
    (w, id, repo)
}

const APP_FAKE: &str = r#"
[ "$1" = app-server ] && [ "$2" = --stdio ] || exit 9
thread=12345678-1234-4234-8234-123456789abc
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  printf '%s\n' "$line" >> rpc-input.jsonl
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    thread/resume) printf '{"id":%s,"result":{"thread":{"id":"%s"}}}\n' "$id" "$thread" ;;
    turn/start)
      printf '{"method":"turn/started","params":{"threadId":"%s","turn":{"id":"turn-1"}}}\n' "$thread"
      printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
      if [ -f hold-first ] && [ ! -f first-started ]; then touch first-started; continue; fi
      printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","turnId":"turn-1","itemId":"answer","delta":"Ordered "}}\n' "$thread"
      printf '{"method":"item/completed","params":{"threadId":"%s","turnId":"turn-1","item":{"id":"answer","type":"agentMessage","text":"Ordered response λ"}}}\n' "$thread"
      printf '{"method":"thread/tokenUsage/updated","params":{"threadId":"%s","tokenUsage":{"last":{"inputTokens":31,"cachedInputTokens":7,"outputTokens":4}}}}\n' "$thread"
      printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"turn-1","status":"completed"}}}\n' "$thread" ;;
    turn/interrupt)
      printf '{"id":%s,"result":{}}\n' "$id"
      printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"turn-1","status":"interrupted"}}}\n' "$thread" ;;
  esac
done
"#;

#[tokio::test]
async fn app_server_preserves_ordered_multimodal_context_and_exact_existing_thread() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, repo) = completed_app_workspace(dir.path(), APP_FAKE);
    let mut encoded = std::io::Cursor::new(vec![]);
    image::RgbaImage::new(2, 2)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let image = conversation::upload_asset(
        State(w.clone()),
        RoutePath(uuid::Uuid::new_v4().to_string()),
        HeaderMap::from_iter([
            (
                "content-type".parse().unwrap(),
                "image/png".parse().unwrap(),
            ),
            (
                "x-relay-filename".parse().unwrap(),
                "context.png".parse().unwrap(),
            ),
        ]),
        Bytes::from(encoded.into_inner()),
    )
    .await
    .unwrap()
    .0;
    let file = conversation::upload_asset(
        State(w.clone()),
        RoutePath(uuid::Uuid::new_v4().to_string()),
        HeaderMap::from_iter([(
            "x-relay-filename".parse().unwrap(),
            "notes.txt".parse().unwrap(),
        )]),
        Bytes::from_static(b"position dependent notes"),
    )
    .await
    .unwrap()
    .0;
    let parts = vec![
        Part::text("Before λ"),
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
    let request = saved_turn(&w, &id, parts.clone()).await;
    let request_id = request.request_id.clone();
    let (session, run, rx, prompt) = reserve(&w, request.clone());
    runtime::run(w.clone(), session, run, prompt, rx).await;
    let s = finished(&w, &id).await;
    let worker = s
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
    assert_eq!(
        worker.thread_id.as_deref(),
        Some("12345678-1234-4234-8234-123456789abc")
    );
    assert_eq!(
        worker.usage,
        Some(TokenUsage {
            input_tokens: 31,
            cached_input_tokens: 7,
            output_tokens: 4
        })
    );
    let answer = s
        .messages
        .iter()
        .find(|m| m.id == format!("codex-{request_id}-answer"))
        .unwrap();
    assert_eq!(answer.body, "Ordered response λ");
    assert_eq!(
        s.messages
            .iter()
            .find(|m| m.id == format!("prompt-{request_id}"))
            .unwrap()
            .parts,
        parts
    );
    assert_eq!(s.submissions[0].state, SubmissionState::Completed);
    let lines = std::fs::read_to_string(repo.join("rpc-input.jsonl")).unwrap();
    let rpc = lines
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        rpc[2]["params"]["threadId"],
        "12345678-1234-4234-8234-123456789abc"
    );
    let input = rpc.iter().find(|v| v["method"] == "turn/start").unwrap()["params"]["input"]
        .as_array()
        .unwrap();
    assert_eq!(input[1]["text"], "Before λ");
    assert_eq!(input[3]["type"], "localImage");
    assert_eq!(input[4]["text"], "Between");
    assert!(
        input[6]["text"]
            .as_str()
            .unwrap()
            .contains("Read the user-provided file")
    );
    assert_eq!(input[7]["text"], "After");
    assert!(!Path::new(input[3]["path"].as_str().unwrap()).exists());
    assert!(
        w.store
            .lock()
            .unwrap()
            .apply(request, &w.config)
            .unwrap()
            .1
            .is_none()
    );
}

#[tokio::test]
async fn promoted_queue_interrupts_one_specific_turn_then_resumes_without_duplicate_submission() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, repo) = completed_app_workspace(dir.path(), APP_FAKE);
    std::fs::write(repo.join("hold-first"), "").unwrap();
    let first = saved_turn(&w, &id, vec![Part::text("First")]).await;
    let first_id = first.request_id.clone();
    let (session, run, rx, prompt) = reserve(&w, first);
    let active = tokio::spawn(runtime::run(w.clone(), session, run, prompt, rx));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !repo.join("first-started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let queued = saved_turn(&w, &id, vec![Part::text("Queued correction")]).await;
    let queued_id = queued.request_id.clone();
    {
        let mut store = w.store.lock().unwrap();
        let (s, a) = store.apply(queued.clone(), &w.config).unwrap();
        assert!(a.is_none());
        w.snapshots.send_replace(s);
        assert!(store.apply(queued, &w.config).unwrap().1.is_none());
    }
    let stale = env(
        w.snapshots.borrow().revision,
        Command::PromoteTurn {
            submission_id: queued_id.clone(),
            active_run_id: Some("different-run".into()),
        },
    );
    assert!(w.store.lock().unwrap().apply(stale, &w.config).is_err());
    let promote = env(
        w.snapshots.borrow().revision,
        Command::PromoteTurn {
            submission_id: queued_id.clone(),
            active_run_id: Some(first_id.clone()),
        },
    );
    {
        let mut store = w.store.lock().unwrap();
        let (s, a) = store.apply(promote, &w.config).unwrap();
        assert!(matches!(a, Some(Action::Promote(_))));
        w.snapshots.send_replace(s);
        store.controls[&id].send_replace(true);
    }
    active.await.unwrap();
    let s = finished(&w, &id).await;
    assert_eq!(
        s.submissions
            .iter()
            .find(|s| s.id == first_id)
            .unwrap()
            .state,
        SubmissionState::Interrupted
    );
    assert_eq!(
        s.submissions
            .iter()
            .find(|s| s.id == queued_id)
            .unwrap()
            .state,
        SubmissionState::Completed
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|m| m.id == format!("prompt-{queued_id}"))
            .count(),
        1
    );
    let rpc = std::fs::read_to_string(repo.join("rpc-input.jsonl")).unwrap();
    assert_eq!(
        rpc.lines()
            .filter(|l| l.contains("\"method\":\"turn/interrupt\""))
            .count(),
        1
    );
}

#[tokio::test]
async fn edited_queue_uses_fresh_approval_at_actual_process_launch() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, repo) = completed_app_workspace(dir.path(), APP_FAKE);
    let first = saved_turn(&w, &id, vec![Part::text("Previous turn")]).await;
    let (_, first_id, _, _) = reserve(&w, first);
    w.update_run(&id, &first_id, |s| {
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Allow);
        Ok(())
    })
    .unwrap();
    let mut queued = saved_turn(&w, &id, vec![Part::text("Original unapproved payload")]).await;
    let Command::SubmitTurn {
        approve_implementation,
        ..
    } = &mut queued.command
    else {
        panic!()
    };
    *approve_implementation = false;
    let queued_id = queued.request_id.clone();
    {
        let mut store = w.store.lock().unwrap();
        let (s, action) = store.apply(queued, &w.config).unwrap();
        assert!(action.is_none());
        w.snapshots.send_replace(s);
    }
    w.update_run(&id, &first_id, |s| {
        s.sessions
            .iter_mut()
            .find(|session| session.id == id)
            .unwrap()
            .worker
            .as_mut()
            .unwrap()
            .status = WorkerStatus::Completed;
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Ask);
        s.submissions
            .iter_mut()
            .find(|submission| submission.id == first_id)
            .unwrap()
            .state = SubmissionState::Completed;
        conversation::pause(s, &id, "Review changed permission");
        Ok(())
    })
    .unwrap();
    let edited = saved_turn(&w, &id, vec![Part::text("Reviewed and approved payload")]).await;
    let Command::SubmitTurn {
        draft_revision,
        parts,
        ..
    } = edited.command
    else {
        panic!()
    };
    let edit = env(
        w.snapshots.borrow().revision,
        Command::EditQueuedTurn {
            submission_id: queued_id.clone(),
            draft_revision,
            parts,
            approve_implementation: true,
        },
    );
    let (run, rx, prompt) = {
        let mut store = w.store.lock().unwrap();
        let (s, _) = store.apply(edit, &w.config).unwrap();
        let (s, action) = store
            .apply(
                env(
                    s.revision,
                    Command::ResumeQueue {
                        session_id: id.clone(),
                    },
                ),
                &w.config,
            )
            .unwrap();
        w.snapshots.send_replace(s);
        let Some(Action::Run { prompt, .. }) = action else {
            panic!()
        };
        let run = store.run_id(&id).unwrap().unwrap();
        let (tx, rx) = watch::channel(false);
        store.controls.insert(id.clone(), tx);
        (run, rx, prompt)
    };
    assert_eq!(run, queued_id);
    runtime::run(w.clone(), id.clone(), run, prompt, rx).await;
    let s = finished(&w, &id).await;
    let worker = s
        .sessions
        .iter()
        .find(|session| session.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(worker.status, WorkerStatus::Completed, "{:?}", worker.error);
    assert_eq!(
        s.submissions
            .iter()
            .find(|submission| submission.id == queued_id)
            .unwrap()
            .state,
        SubmissionState::Completed
    );
    assert!(
        std::fs::read_to_string(repo.join("rpc-input.jsonl"))
            .unwrap()
            .contains("Reviewed and approved payload")
    );
}

#[tokio::test]
async fn queue_cancel_edit_and_restart_preserve_payload_and_never_replay_delivered_turns() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, _) = completed_app_workspace(dir.path(), APP_FAKE);
    let first = saved_turn(&w, &id, vec![Part::text("Possibly delivered")]).await;
    let first_id = first.request_id.clone();
    let (_session, _run, _rx, _prompt) = reserve(&w, first);
    let queued = saved_turn(&w, &id, vec![Part::text("Unsent")]).await;
    let queued_id = queued.request_id.clone();
    {
        let mut store = w.store.lock().unwrap();
        let (s, _) = store.apply(queued, &w.config).unwrap();
        w.snapshots.send_replace(s);
    }
    let edited = saved_turn(&w, &id, vec![Part::text("Edited unsent")]).await;
    let Command::SubmitTurn {
        draft_revision,
        parts,
        ..
    } = edited.command
    else {
        panic!()
    };
    let edit = env(
        w.snapshots.borrow().revision,
        Command::EditQueuedTurn {
            submission_id: queued_id.clone(),
            draft_revision,
            parts,
            approve_implementation: true,
        },
    );
    {
        let mut store = w.store.lock().unwrap();
        let (s, _) = store.apply(edit, &w.config).unwrap();
        w.snapshots.send_replace(s);
    }
    let c = (*w.config).clone();
    drop(w);
    let _router = router_with_config(
        dir.path().join("db"),
        "sixteen-character-token".into(),
        DirectorProfile::default(),
        c.clone(),
    )
    .unwrap();
    let mut store = Store::open(&dir.path().join("db"), DirectorProfile::default()).unwrap();
    let s = store.snapshot().unwrap();
    assert_eq!(
        s.submissions
            .iter()
            .find(|s| s.id == first_id)
            .unwrap()
            .state,
        SubmissionState::Interrupted
    );
    let unsent = s.submissions.iter().find(|s| s.id == queued_id).unwrap();
    assert_eq!(unsent.state, SubmissionState::Paused);
    assert_eq!(plain_text(&unsent.parts), "Edited unsent");
    assert!(
        store
            .apply(
                env(
                    s.revision,
                    Command::PromoteTurn {
                        submission_id: first_id,
                        active_run_id: None
                    }
                ),
                &c
            )
            .is_err()
    );
    let (s, _) = store
        .apply(
            env(
                s.revision,
                Command::CancelTurn {
                    submission_id: queued_id.clone(),
                },
            ),
            &c,
        )
        .unwrap();
    assert_eq!(
        s.submissions
            .iter()
            .find(|s| s.id == queued_id)
            .unwrap()
            .state,
        SubmissionState::Cancelled
    );
}

#[tokio::test]
async fn app_server_rejects_foreign_turn_events_and_failure_pauses_unsent_queue() {
    let dir = tempfile::tempdir().unwrap();
    let altered = APP_FAKE.replace(
        "\"turnId\":\"turn-1\",\"itemId\"",
        "\"turnId\":\"foreign-turn\",\"itemId\"",
    );
    let (w, id, _) = completed_app_workspace(dir.path(), &altered);
    let first = saved_turn(&w, &id, vec![Part::text("Fail on foreign turn")]).await;
    let (session, run, rx, prompt) = reserve(&w, first);
    let next = saved_turn(&w, &id, vec![Part::text("Must stay unsent")]).await;
    let next_id = next.request_id.clone();
    {
        let mut store = w.store.lock().unwrap();
        let (s, _) = store.apply(next, &w.config).unwrap();
        w.snapshots.send_replace(s);
    }
    runtime::run(w.clone(), session, run, prompt, rx).await;
    let s = finished(&w, &id).await;
    let worker = s
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(worker.status, WorkerStatus::Failed);
    assert!(worker.error.as_ref().unwrap().contains("different turn"));
    assert_eq!(
        s.submissions
            .iter()
            .find(|s| s.id == next_id)
            .unwrap()
            .state,
        SubmissionState::Paused
    );
    assert!(
        !s.messages
            .iter()
            .any(|m| m.id == format!("prompt-{next_id}"))
    );
}
