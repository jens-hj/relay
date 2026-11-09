//! Isolated numeric fixtures only. No production hooks or metrics collection.
use super::*;
use std::time::Instant;
use tokio::time::{Duration, timeout};

#[derive(Clone, Copy, Debug)]
enum Auth {
    Bearer,
    Cookie,
}
#[derive(Clone, Copy, Debug)]
enum Route {
    Ping,
    Draft,
    ConfigRename,
    ConfigUpdate,
}

fn request(
    s: &Server,
    auth: Auth,
    cookie: &str,
    csrf: &str,
    route: Route,
) -> reqwest::RequestBuilder {
    let r = match route {
        Route::Ping => s.client.get(format!("{}/v1/ping", s.url)),
        Route::Draft => s.client.post(format!("{}/v1/drafts/unused", s.url)),
        Route::ConfigRename | Route::ConfigUpdate => {
            s.client.post(format!("{}/v1/commands", s.url))
        }
    }
    .header("x-relay-protocol", PROTOCOL_VERSION.to_string());
    match auth {
        Auth::Bearer => r.bearer_auth(TOKEN),
        Auth::Cookie => r
            .header("cookie", cookie)
            .header("origin", ORIGIN)
            .header("x-relay-csrf", csrf),
    }
}

// Header wait includes queue/auth/server/transport; body wait includes delivery/scheduling.
// Never print requests, credentials, IDs, text, URLs, or error Display/Debug values.
async fn measure(r: reqwest::RequestBuilder, auth: Auth, route: Route, subscribers: usize) -> u16 {
    let start = Instant::now();
    let result = timeout(Duration::from_secs(5), async {
        let response = r.send().await.map_err(|_| ())?;
        let headers = start.elapsed().as_micros();
        let status = response.status().as_u16();
        let body_start = Instant::now();
        let bytes = response.bytes().await.map_err(|_| ())?;
        let body = body_start.elapsed().as_micros();
        let decode_start = Instant::now();
        let decoded = status == 204 || serde_json::from_slice::<serde_json::Value>(&bytes).is_ok();
        Ok::<_, ()>((
            status,
            headers,
            body,
            decode_start.elapsed().as_micros(),
            decoded,
        ))
    })
    .await;
    match result {
        Ok(Ok((status, headers, body, decode, decoded))) => {
            println!(
                "LATENCY auth={auth:?} route={route:?} subscribers={subscribers} status={status} headers_us={headers} body_us={body} decode_us={decode} decoded={} total_us={}",
                u8::from(decoded),
                start.elapsed().as_micros()
            );
            status
        }
        _ => {
            println!(
                "LATENCY auth={auth:?} route={route:?} subscribers={subscribers} failure=1 total_us={}",
                start.elapsed().as_micros()
            );
            0
        }
    }
}

// Separate server runtime: synchronous production auth must not block the test's client timer.
#[derive(Default)]
struct ArrivalCounters {
    arrived: [std::sync::atomic::AtomicU64; 8],
    completed: [std::sync::atomic::AtomicU64; 8],
    total_us: [std::sync::atomic::AtomicU64; 8],
    max_us: [std::sync::atomic::AtomicU64; 8],
}
impl ArrivalCounters {
    fn report(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        for auth in 0..2 {
            for route in 0..4 {
                let i = auth * 4 + route;
                println!(
                    "SERVER auth_class={auth} route_class={route} arrived={} headers_ready={} app_to_headers_total_us={} app_to_headers_max_us={}",
                    self.arrived[i].load(Relaxed),
                    self.completed[i].load(Relaxed),
                    self.total_us[i].load(Relaxed),
                    self.max_us[i].load(Relaxed)
                );
            }
        }
    }
}
struct IsolatedHttp {
    counters: Arc<ArrivalCounters>,
    server: Server,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for IsolatedHttp {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn isolated_http() -> IsolatedHttp {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("setup"), CODE).unwrap();
    let config = RuntimeConfig {
        codex: dir.path().join("missing-codex"),
        claude: dir.path().join("missing-claude"),
        nix: dir.path().join("missing-nix"),
        ..Default::default()
    };
    let (app, shutdown) = router_with_browser(
        dir.path().join("db"),
        TOKEN.into(),
        AgentProfile::default(),
        config,
        BrowserConfig {
            public_origin: Some(ORIGIN.into()),
            setup_token_file: Some(dir.path().join("setup")),
            web_root: None,
        },
    )
    .unwrap();
    let counters = Arc::new(ArrivalCounters::default());
    let measured = counters.clone();
    let app = app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let measured = measured.clone();
            async move {
                use std::sync::atomic::Ordering::Relaxed;
                let route = match request.uri().path() {
                    "/v1/ping" => 0,
                    "/v1/commands" => 2,
                    p if p.starts_with("/v1/drafts/") => 1,
                    _ => 3,
                };
                let auth = usize::from(!request.headers().contains_key("authorization"));
                let i = auth * 4 + route;
                let start = Instant::now();
                measured.arrived[i].fetch_add(1, Relaxed);
                let response = next.run(request).await;
                let us = start.elapsed().as_micros().min(u64::MAX as u128) as u64;
                measured.completed[i].fetch_add(1, Relaxed);
                measured.total_us[i].fetch_add(us, Relaxed);
                measured.max_us[i].fetch_max(us, Relaxed);
                response
            }
        },
    ));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                    .with_graceful_shutdown(async {
                        let _ = rx.await;
                    })
                    .await
                    .unwrap();
            });
    });
    IsolatedHttp {
        counters,
        server: Server {
            dir,
            url,
            client: Client::new(),
            task: tokio::spawn(std::future::pending()),
            shutdown,
        },
        stop: Some(tx),
        thread: Some(thread),
    }
}

// Acquires the real writer before returning. Release is explicit and bounded on failure.
async fn hold_writer(
    w: Workspace,
) -> (std::sync::mpsc::SyncSender<()>, tokio::task::JoinHandle<()>) {
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let holder = tokio::task::spawn_blocking(move || {
        let _guard = w.store.lock().unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    held_rx.await.unwrap();
    (release_tx, holder)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn latency_fixture_cookie_auth_and_postcommit_writer_gates() {
    let fixture = isolated_http();
    let s = &fixture.server;
    let (cookie, _) = s.enroll().await;
    let session: serde_json::Value = s.session(&cookie).await.json().await.unwrap();
    let csrf = session["csrf_token"].as_str().unwrap();
    let w = s.shutdown.workspace.clone();
    // Gate is acquired before either HTTP attempt; no scheduling sleep establishes order.
    let (release_tx, holder) = hold_writer(w).await;
    let cookie_r =
        request(s, Auth::Cookie, &cookie, csrf, Route::Ping).timeout(Duration::from_millis(80));
    let bearer_r = request(s, Auth::Bearer, &cookie, csrf, Route::Ping);
    // Cookie cancellation is explicitly counted; a timed-out server auth may finish after release.
    let (bearer_status, cookie_status) = tokio::join!(
        measure(bearer_r, Auth::Bearer, Route::Ping, 0),
        measure(cookie_r, Auth::Cookie, Route::Ping, 0)
    );
    release_tx.send(()).unwrap();
    holder.await.unwrap();
    assert_eq!(bearer_status, 204);
    assert_eq!(cookie_status, 0);
    assert_eq!(
        measure(
            request(s, Auth::Cookie, &cookie, csrf, Route::Ping),
            Auth::Cookie,
            Route::Ping,
            0
        )
        .await,
        204
    );

    // Direct production auth boundary, with actual writer contention and numeric wait+work.
    let mut headers = HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    let w = s.shutdown.workspace.clone();
    let (release_tx, holder) = hold_writer(w.clone()).await;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let auth_w = w.clone();
    let auth = tokio::task::spawn_blocking(move || {
        let start = Instant::now();
        started_tx.send(()).unwrap();
        auth_w.authorize(&headers).unwrap();
        start.elapsed().as_micros()
    });
    started_rx.await.unwrap();
    let mut auth = auth;
    assert!(timeout(Duration::from_millis(40), &mut auth).await.is_err());
    release_tx.send(()).unwrap();
    holder.await.unwrap();
    let auth_us = auth.await.unwrap();
    println!("PHASE auth=Cookie phase=authorize_wait_plus_work us={auth_us} deadline_observed=1");

    // Actual Store commit, then the exact postcommit function used by command().
    // This is NOT a per-phase instrumented HTTP ACK; HTTP samples below are total/header/body.
    let (committed_revision, commit_us) = {
        let mut store = w.store.lock().unwrap();
        let current = store.snapshot().unwrap();
        let config = &current.agent_configs[0];
        let envelope = crate::tests::env(
            current.revision,
            Command::RenameAgentConfig {
                agent_config_id: config.id.clone(),
                name: "Measured".into(),
                expected: AgentConfigRenameGuard::capture(config),
            },
        );
        let start = Instant::now();
        let (saved, action) = store.apply(envelope, &w.config).unwrap();
        assert!(action.is_none());
        (saved.revision, start.elapsed().as_micros())
    };
    assert_eq!(
        w.store.lock().unwrap().snapshot().unwrap().revision,
        committed_revision
    );
    let db = s.db();
    let receipts: u64 = db
        .query_row("SELECT count(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(receipts, 1); // Committed and readable by a separate connection before drain.
    let (release_tx, holder) = hold_writer(w.clone()).await;
    let drain_w = w.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut drain = tokio::task::spawn_blocking(move || {
        let start = Instant::now();
        started_tx.send(()).unwrap();
        conversation::drain_workspace_waits(&drain_w).unwrap();
        start.elapsed().as_micros()
    });
    started_rx.await.unwrap();
    assert!(
        timeout(Duration::from_millis(40), &mut drain)
            .await
            .is_err()
    );
    release_tx.send(()).unwrap();
    holder.await.unwrap();
    let drain_us = drain.await.unwrap();
    println!("PHASE phase=store_apply_commit us={commit_us} receipts={receipts}");
    println!(
        "PHASE phase=postcommit_drain_wait_plus_work us={drain_us} committed_before_drain=1 deadline_observed=1"
    );
    fixture.counters.report();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn latency_fixture_http_under_update_run_load() {
    let fixture = isolated_http();
    let s = &fixture.server;
    let (cookie, _) = s.enroll().await;
    let session: serde_json::Value = s.session(&cookie).await.json().await.unwrap();
    let csrf = session["csrf_token"].as_str().unwrap();
    let w = s.shutdown.workspace.clone();
    let (worker, run, target) = {
        let mut store = w.store.lock().unwrap();
        let mut initial = store.snapshot().unwrap();
        // Synthetic run metadata only; no Start/Action/harness dispatch.
        let source = 0;
        let session = &mut initial.sessions[source];
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
        let target = initial.agent_configs[0].id.clone();
        store
            .connection
            .execute(
                "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                [&worker, &run],
            )
            .unwrap();
        store.save(&initial).unwrap();
        w.snapshots.send_replace(initial);
        (worker, run, target)
    };
    let draft_target = worker.clone();
    let mut draft_revision = 0;
    for subscribers in [1, 3] {
        let mut readers = Vec::new();
        let mut stops = Vec::new();
        for _ in 0..subscribers {
            let mut socket = s.websocket("/v1/events", &cookie, ORIGIN).await.unwrap();
            timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let (tx, mut rx) = tokio::sync::oneshot::channel();
            stops.push(tx);
            readers.push(tokio::spawn(async move {
                let mut frames = 0u64;
                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        frame = socket.next() => {
                            if frame.is_none() { break; }
                            assert!(frame.unwrap().is_ok());
                            frames += 1;
                        }
                    }
                }
                frames
            }));
        }
        let load_w = w.clone();
        let worker = worker.clone();
        let run = run.clone();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let load_barrier = barrier.clone();
        let stop_load = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let load_stop = stop_load.clone();
        let load = tokio::spawn(async move {
            load_barrier.wait().await;
            let mut total = 0u128;
            let mut max = 0u128;
            let mut events = 0;
            for n in 0..8192 {
                if load_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                events += 1;
                let start = Instant::now();
                load_w
                    .update_run(&worker, &run, |snapshot| {
                        let s = snapshot
                            .sessions
                            .iter_mut()
                            .find(|s| s.id == worker)
                            .unwrap();
                        s.title = format!("Synthetic output {n}");
                        Ok(())
                    })
                    .unwrap();
                let us = start.elapsed().as_micros();
                total += us;
                max = max.max(us);
                tokio::task::yield_now().await;
            }
            (total, max, events)
        });
        barrier.wait().await;
        for auth in [Auth::Bearer, Auth::Cookie] {
            for n in 0..6 {
                assert_eq!(
                    measure(
                        request(s, auth, &cookie, csrf, Route::Ping),
                        auth,
                        Route::Ping,
                        subscribers
                    )
                    .await,
                    204
                );
                let draft = SaveDraft {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: draft_revision,
                    parts: vec![Part::text("synthetic")],
                };
                let draft_request = request(s, auth, &cookie, csrf, Route::Draft)
                    .build()
                    .unwrap();
                let mut url = draft_request.url().clone();
                url.set_path(&format!("/v1/drafts/{draft_target}"));
                let mut r = s
                    .client
                    .post(url)
                    .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
                    .json(&draft);
                r = match auth {
                    Auth::Bearer => r.bearer_auth(TOKEN),
                    Auth::Cookie => r
                        .header("cookie", &cookie)
                        .header("origin", ORIGIN)
                        .header("x-relay-csrf", csrf),
                };
                assert_eq!(measure(r, auth, Route::Draft, subscribers).await, 200);
                draft_revision += 1;
                let current = w.snapshots.borrow().clone();
                let c = current
                    .agent_configs
                    .iter()
                    .find(|c| c.id == target)
                    .unwrap();
                let command = crate::tests::env(
                    current.revision,
                    Command::RenameAgentConfig {
                        agent_config_id: target.clone(),
                        name: format!("Fixture {n}"),
                        expected: AgentConfigRenameGuard::capture(c),
                    },
                );
                assert_eq!(
                    measure(
                        request(s, auth, &cookie, csrf, Route::ConfigRename).json(&command),
                        auth,
                        Route::ConfigRename,
                        subscribers
                    )
                    .await,
                    200
                );
                let current = w.snapshots.borrow().clone();
                let c = current
                    .agent_configs
                    .iter()
                    .find(|c| c.id == target)
                    .unwrap();
                let command = crate::tests::env(
                    current.revision,
                    Command::UpdateAgentConfig {
                        agent_config_id: target.clone(),
                        name: c.name.clone(),
                        overrides: c.overrides.clone(),
                    },
                );
                let status = measure(
                    request(s, auth, &cookie, csrf, Route::ConfigUpdate).json(&command),
                    auth,
                    Route::ConfigUpdate,
                    subscribers,
                )
                .await;
                assert!(matches!(status, 200 | 409)); // Exact-global conflicts are measured, never retried/rebased.
            }
        }
        stop_load.store(true, std::sync::atomic::Ordering::Relaxed);
        let (total, max, events) = load.await.unwrap();
        assert!(
            events < 8192,
            "load exhausted before HTTP requests completed"
        );
        for stop in stops {
            stop.send(()).unwrap();
        }
        let mut frames = 0;
        for reader in readers {
            frames += reader.await.unwrap();
        }
        println!(
            "LOAD subscribers={subscribers} events={events} update_run_total_us={total} update_run_max_us={max} frames={frames}"
        );
    }
    let store = w.store.lock().unwrap();
    let processes: u64 = store
        .connection
        .query_row("SELECT count(*) FROM processes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(processes, 0);
    fixture.counters.report();
}
