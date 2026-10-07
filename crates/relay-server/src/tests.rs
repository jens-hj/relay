use super::*;
use std::os::unix::fs::PermissionsExt;
use std::{path::PathBuf, time::Duration};
fn live() -> Snapshot {
    let mut s = demo_snapshot(DirectorProfile::default());
    let p = &mut s.projects[0];
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
        i.reference.provider = Provider::Github;
        i.reference.repository = p.repository.clone();
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
fn env(revision: u64, command: Command) -> CommandEnvelope {
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
fn workspace(path: &Path, snapshot: &Snapshot, config: RuntimeConfig) -> Workspace {
    let mut store = Store::open(path, DirectorProfile::default()).unwrap();
    store.save(snapshot).unwrap();
    let (snapshots, _) = watch::channel(snapshot.clone());
    Workspace {
        store: Arc::new(Mutex::new(store)),
        snapshots,
        token: "test-token".into(),
        config: Arc::new(config),
    }
}
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn git(repo: &Path, args: &[&str]) {
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
    assert!(runtime::authorize_turn(&s, &issue, &d, true, &c).is_err());
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
    let s = live();
    let w = workspace(&dir.path().join("db"), &s, c);
    let request = env(s.revision, start(&s));
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
    assert!(args.starts_with("exec\n--json\n--sandbox\nworkspace-write\n-\n"));
    assert!(args.contains("resume\n-c\nsandbox_mode=\"workspace-write\"\n--json\nexact-thread\n-"));
    assert!(!args.contains("bypass"));
    let prompt = std::fs::read_to_string(prompt_file).unwrap();
    assert!(prompt.ends_with("$(touch injection) `whoami`"));
    assert!(prompt.contains("Issue: jens-hj/relay#"));
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
    let board = github::sync(&c, "demo").unwrap();
    assert_eq!(board.issues.len(), 2);
    assert_eq!(board.issues[0].id, "github:jens-hj/relay:7");
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
            "INSERT OR REPLACE INTO syncs(project_id,request_id) VALUES('demo','sync-test')",
            [],
        )
        .unwrap();
    w.synchronize("demo", "sync-test");
    let good = w.snapshots.borrow().clone();
    script(&c.gh, "echo secret-auth-detail >&2\nexit 1");
    w.store
        .lock()
        .unwrap()
        .connection
        .execute(
            "INSERT OR REPLACE INTO syncs(project_id,request_id) VALUES('demo','sync-test')",
            [],
        )
        .unwrap();
    w.synchronize("demo", "sync-test");
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
    let request = env(s.revision, start(&s));
    let id = format!("session-{}", request.request_id);
    let send = |e: &CommandEnvelope| {
        client
            .post(format!("http://{addr}/v1/commands"))
            .bearer_auth(token)
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
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    let mut s = demo_snapshot(DirectorProfile::default());
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
        .execute("INSERT INTO workspace VALUES(1,?1)", [value.to_string()])
        .unwrap();
    drop(connection);
    let store = Store::open(&db, DirectorProfile::default()).unwrap();
    assert_eq!(store.snapshot().unwrap(), s);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
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
            project_id: "demo".into(),
        },
    );
    let mut store = w.store.lock().unwrap();
    let (saved, action) = store.apply(request.clone(), &w.config).unwrap();
    assert!(matches!(action, Some(Action::Sync(_))));
    assert!(store.apply(request.clone(), &w.config).unwrap().1.is_none());
    w.snapshots.send_replace(saved.clone());
    drop(store);
    w.synchronize("demo", "superseded");
    assert_eq!(*w.snapshots.borrow(), saved);
    w.synchronize("demo", &request.request_id);
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
