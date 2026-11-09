use super::*;

fn rename(snapshot: &Snapshot, id: &str, name: &str, revision: u64) -> CommandEnvelope {
    env(
        revision,
        Command::RenameAgentConfig {
            agent_config_id: id.into(),
            name: name.into(),
            expected: AgentConfigRenameGuard::capture(
                snapshot.agent_configs.iter().find(|c| c.id == id).unwrap(),
            ),
        },
    )
}
fn isolated_config(dir: &Path) -> RuntimeConfig {
    RuntimeConfig {
        remote: None,
        repository: None,
        projects_root: Some(dir.join("projects")),
        gh: dir.join("missing-gh"),
        glab: dir.join("missing-glab"),
        codex: dir.join("missing-codex"),
        claude: dir.join("missing-claude"),
        nix: dir.join("missing-nix"),
        background_task_limit: Duration::from_secs(60),
        worker: Default::default(),
    }
}
#[test]
fn guarded_config_rename_transaction_guards_legacy_receipts_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let initial = live();
    let c = isolated_config(dir.path());
    let mut store = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
    store.save(&initial).unwrap();
    let id = initial.agent_configs[0].id.clone();
    let first = rename(&initial, &id, "First", initial.revision);
    let second = rename(&initial, &id, "Second", initial.revision);
    let mut streamed = initial.clone();
    streamed.revision += 7;
    streamed.sessions[0].title = "synthetic runtime".into();
    streamed.agent_configs[0].overrides.network_access = Some(false);
    streamed.agent_configs[0].draft_selection.model = Some("synthetic-model".into());
    store.save(&streamed).unwrap();
    let (saved, action) = store.apply(first.clone(), &c).unwrap();
    assert!(action.is_none());
    assert_eq!(saved.agent_configs[0].name, "First");
    assert_eq!(
        saved.agent_configs[0].overrides,
        streamed.agent_configs[0].overrides
    );
    assert_eq!(
        saved.agent_configs[0].draft_selection,
        streamed.agent_configs[0].draft_selection
    );
    assert_eq!(
        store.apply(second.clone(), &c).err().unwrap().code,
        "agent_name_conflict"
    );
    let mut exact_stale = second;
    exact_stale.expected_revision = saved.revision;
    assert_eq!(
        store.apply(exact_stale, &c).err().unwrap().code,
        "agent_name_conflict"
    );
    let future = rename(&saved, &id, "Future", saved.revision + 1);
    assert_eq!(store.apply(future, &c).err().unwrap().code, "conflict");
    let old = env(
        initial.revision,
        Command::UpdateAgentConfig {
            agent_config_id: id.clone(),
            name: "Overwrite".into(),
            overrides: ProfileOverrides::default(),
        },
    );
    assert_eq!(store.apply(old, &c).err().unwrap().code, "conflict");
    let legacy_exact = env(
        saved.revision,
        Command::UpdateAgentConfig {
            agent_config_id: id.clone(),
            name: "First".into(),
            overrides: saved.agent_configs[0].overrides.clone(),
        },
    );
    let legacy_json = serde_json::to_string(&legacy_exact).unwrap();
    let (saved, _) = store.apply(legacy_exact.clone(), &c).unwrap();
    let stored: String = store
        .connection
        .query_row(
            "SELECT request FROM receipts WHERE request_id=?1",
            [&legacy_exact.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, legacy_json);
    assert_eq!(store.apply(legacy_exact, &c).unwrap().0, saved);
    let later = rename(&saved, &id, "Later", saved.revision);
    let (later, _) = store.apply(later, &c).unwrap();
    let (replayed, action) = store.apply(first.clone(), &c).unwrap();
    assert_eq!(replayed, later);
    assert!(action.is_none());
    let mut changed = first.clone();
    if let Command::RenameAgentConfig { expected, .. } = &mut changed.command {
        expected.automatic_name = !expected.automatic_name;
    }
    assert_eq!(store.apply(changed, &c).err().unwrap().code, "invalid");
    // Use the existing deletion-redacted receipt representation, without weakening replay.
    let original = serde_json::to_string(&first).unwrap();
    store
        .connection
        .execute(
            "UPDATE receipts SET request=?1 WHERE request_id=?2",
            params![deletion::redacted_receipt(&original), first.request_id],
        )
        .unwrap();
    let mut removed = later.clone();
    removed.agent_configs.retain(|config| config.id != id);
    removed.revision += 1;
    store.save(&removed).unwrap();
    assert_eq!(store.apply(first.clone(), &c).unwrap().0, removed);
    let fresh = env(removed.revision, first.command);
    assert_eq!(store.apply(fresh, &c).err().unwrap().code, "invalid");
    assert_eq!(store.snapshot().unwrap(), removed);
    let count: u64 = store
        .connection
        .query_row("SELECT count(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guarded_config_rename_production_router_preserves_stream_and_policy() {
    let dir = tempfile::tempdir().unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    {
        let mut store = Store::open(&dir.path().join("db"), AgentProfile::default()).unwrap();
        let mut initial = live();
        initial.projects[0].defaults.model = "synthetic-model".into();
        let root = dir.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        initial.projects[0].root = Some(root.display().to_string());
        store.save(&initial).unwrap();
    }
    let (app, shutdown) = router_with_shutdown(
        dir.path().join("db"),
        token.clone(),
        AgentProfile::default(),
        isolated_config(dir.path()),
    )
    .unwrap();
    let w = shutdown.workspace.clone();
    let (captured, id, worker, run) = {
        let mut store = w.store.lock().unwrap();
        let initial = store.snapshot().unwrap();
        let create = env(
            initial.revision,
            Command::CreateAgentConfig {
                project_id: initial.projects[0].id.clone(),
                name: "Original".into(),
                template: "director".into(),
                overrides: ProfileOverrides::default(),
            },
        );
        let (created, _) = store.apply(create, &w.config).unwrap();
        let id = created.agent_configs.last().unwrap().id.clone();
        // Isolated synthetic run metadata only: no Start command, Action or harness process.
        let mut running = created.clone();
        let session = running.sessions.first_mut().unwrap();
        session.fixture = false;
        session.worker = Some(WorkerRun {
            model: Some("synthetic-model".into()),
            context_tokens: None,
            context_window: None,
            last_usage: None,
            harness: Harness::Codex,
            execution: None,
            launched_network_policy: None,
            status: WorkerStatus::Running,
            thread_id: None,
            worktree: None,
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
        });
        let worker = session.id.clone();
        let run = uuid::Uuid::new_v4().to_string();
        running.revision += 1;
        store
            .connection
            .execute(
                "INSERT INTO runs(session_id,run_id) VALUES(?1,?2)",
                [&worker, &run],
            )
            .unwrap();
        store.save(&running).unwrap();
        w.snapshots.send_replace(running);
        (created, id, worker, run)
    };
    let request = rename(&captured, &id, "Renamed", captured.revision);
    w.update_run(&worker, &run, |s| {
        s.messages.push(Message {
            id: "synthetic-output".into(),
            session_id: worker.clone(),
            author: "fixture".into(),
            sender: Sender::default(),
            kind: "assistant".into(),
            body: "synthetic".into(),
            parts: vec![],
            tool: None,
        });
        Ok(())
    })
    .unwrap();
    {
        let mut store = w.store.lock().unwrap();
        let current = store.snapshot().unwrap();
        let mut overrides = current
            .agent_configs
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .overrides
            .clone();
        overrides.network_access = Some(false);
        let (edited, _) = store
            .apply(
                env(
                    current.revision,
                    Command::UpdateAgentConfig {
                        agent_config_id: id.clone(),
                        name: "Original".into(),
                        overrides,
                    },
                ),
                &w.config,
            )
            .unwrap();
        w.snapshots.send_replace(edited);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/commands", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let post = |e: &CommandEnvelope| {
        client
            .post(&url)
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(e)
    };
    let response = post(&request).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let ack: CommandAck = response.json().await.unwrap();
    let current = w.store.lock().unwrap().snapshot().unwrap();
    assert_eq!(ack.committed_revision, current.revision);
    assert!(current.messages.iter().any(|m| m.id == "synthetic-output"));
    let config = current.agent_configs.iter().find(|c| c.id == id).unwrap();
    assert_eq!(config.name, "Renamed");
    assert_eq!(config.overrides.network_access, Some(false));
    assert_eq!(
        post(&request).send().await.unwrap().status(),
        StatusCode::OK
    );
    let mut reused = request.clone();
    if let Command::RenameAgentConfig { name, .. } = &mut reused.command {
        *name = "different".into();
    }
    assert_eq!(
        post(&reused).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let racing = rename(&captured, &id, "Racer", current.revision);
    assert_eq!(
        post(&racing).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let mut wrong = rename(&current, &id, "Wrong", current.revision);
    if let Command::RenameAgentConfig {
        agent_config_id, ..
    } = &mut wrong.command
    {
        *agent_config_id = current.agent_configs[0].id.clone();
    }
    assert_eq!(
        post(&wrong).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        client
            .post(&url)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&token)
            .header("x-relay-protocol", "0")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let left = rename(&current, &id, "Race left", current.revision);
    let right = rename(&current, &id, "Race right", current.revision);
    let (left_result, right_result) = tokio::join!(post(&left).send(), post(&right).send());
    let mut statuses = [
        left_result.unwrap().status().as_u16(),
        right_result.unwrap().status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let current = w.store.lock().unwrap().snapshot().unwrap();
    assert_eq!(
        post(&request).send().await.unwrap().status(),
        StatusCode::OK
    );
    // A missing current policy reference must still refuse name-only edits.
    {
        let mut broken = current.clone();
        broken
            .agent_configs
            .iter_mut()
            .find(|c| c.id == id)
            .unwrap()
            .template = "missing".into();
        w.store.lock().unwrap().save(&broken).unwrap();
        let bad = rename(&broken, &id, "Refused", broken.revision);
        assert_eq!(
            post(&bad).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        w.store.lock().unwrap().save(&current).unwrap();
    }
    assert_eq!(
        w.store
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM processes", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        0
    );
    let remove = env(
        current.revision,
        Command::RemoveAgentConfig {
            agent_config_id: id,
        },
    );
    assert_eq!(post(&remove).send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        post(&request).send().await.unwrap().status(),
        StatusCode::OK
    );
    let missing = env(w.snapshots.borrow().revision, request.command.clone());
    assert_eq!(
        post(&missing).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    server.abort();
}
