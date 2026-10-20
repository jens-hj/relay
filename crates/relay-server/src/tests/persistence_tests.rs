use super::*;
use std::time::Instant;
use tokio::io::{AsyncWriteExt, BufReader};

/// No production database, process ownership, harness, or captured messages.
fn fixture(dir: &Path, mib: usize) -> (Workspace, String, String, usize) {
    let s = live();
    let w = workspace(&dir.join("db"), &s, config());
    let (id, run, _stop, _) = reserve(&w, env(s.revision, start(&s)));
    let mut s = w.snapshots.borrow().clone();
    for n in 0..mib * 32 {
        s.messages.push(Message {
            id: format!("filler-{n}"),
            session_id: id.clone(),
            author: "fixture".into(),
            sender: Sender::default(),
            kind: "text".into(),
            body: "x".repeat(32 * 1024),
            parts: vec![],
            tool: None,
        });
    }
    let bytes = serde_json::to_vec(&s).unwrap().len();
    w.store.lock().unwrap().save(&s).unwrap();
    w.snapshots.send_replace(s);
    (w, id, run, bytes)
}

// Quantify even the sub-100ms phases omitted by production's slow-only logging.
fn measure_phases(w: &Workspace) {
    let started = Instant::now();
    let store = w.store.lock().unwrap();
    let lock = started.elapsed();
    let started = Instant::now();
    let json: String = store
        .connection
        .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let load = started.elapsed();
    let started = Instant::now();
    let s: Snapshot = serde_json::from_str(&json).unwrap();
    let deserialize = started.elapsed();
    let started = Instant::now();
    let previous = s.clone();
    let clone = started.elapsed();
    let started = Instant::now();
    assert!(s == previous);
    let compare = started.elapsed();
    let started = Instant::now();
    let json = serde_json::to_string(&s).unwrap();
    let serialize = started.elapsed();
    let started = Instant::now();
    store
        .connection
        .execute("UPDATE workspace SET snapshot=?1 WHERE id=1", [json])
        .unwrap();
    let write = started.elapsed();
    eprintln!(
        "fixture lock {}ms load {}ms deserialize {}ms clone {}ms compare {}ms serialize {}ms write {}ms",
        lock.as_millis(),
        load.as_millis(),
        deserialize.as_millis(),
        clone.as_millis(),
        compare.as_millis(),
        serialize.as_millis(),
        write.as_millis()
    );
}

#[derive(Debug)]
struct Delays {
    heartbeat: Duration,
    initialize: Duration,
    deadline: Duration,
    spawn: Duration,
    terminate: Duration,
    cancel: Duration,
}

async fn child(
    bin: &Path,
) -> (
    tokio::process::Child,
    BufReader<tokio::process::ChildStdout>,
) {
    let mut command = tokio::process::Command::new(bin);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().unwrap();
    let output = BufReader::new(child.stdout.take().unwrap());
    (child, output)
}

async fn exercise(mib: usize, rounds: usize, hold: Duration, baseline: bool) -> Delays {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, run, bytes) = fixture(dir.path(), mib);
    measure_phases(&w);
    let initial = w.snapshots.borrow().revision;
    let bin = dir.path().join("stub");
    // Initialize responds immediately; MCP deliberately never responds. No forks.
    script(
        &bin,
        r#"while IFS= read -r request; do
case "$request" in
initialize) printf '%s\n' '{"id":1,"result":{}}';;
esac
done"#,
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(5));
    let mut writers = vec![];
    for writer in 0..4 {
        let (w, id, run, barrier) = (w.clone(), id.clone(), run.clone(), barrier.clone());
        writers.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut elapsed = vec![];
            for sequence in 0..rounds {
                let started = Instant::now();
                let update = |s: &mut Snapshot| {
                    // Deterministic mutex contention for the inexpensive regression.
                    std::thread::sleep(hold);
                    s.messages.push(Message {
                        id: format!("update-{writer}-{sequence}"),
                        session_id: id.clone(),
                        author: "fixture".into(),
                        sender: Sender::default(),
                        kind: "text".into(),
                        body: String::new(),
                        parts: vec![],
                        tool: None,
                    });
                    Ok(())
                };
                if baseline {
                    // The original synchronous path, only for the opt-in comparison.
                    w.update_run_persisted(&id, &run, update)
                } else {
                    w.update_run(&id, &run, update)
                }
                .unwrap();
                elapsed.push(started.elapsed());
                tokio::task::yield_now().await;
            }
            elapsed
        }));
    }
    let (done, mut done_rx) = watch::channel(false);
    let (ready, ready_rx) = tokio::sync::oneshot::channel();
    let heartbeat = tokio::spawn(async move {
        let mut worst = Duration::ZERO;
        let mut ready = Some(ready);
        loop {
            let deadline = tokio::time::Instant::now() + Duration::from_millis(10);
            let sleep = tokio::time::sleep_until(deadline);
            tokio::pin!(sleep);
            if let Some(ready) = ready.take() {
                ready.send(()).unwrap();
            }
            tokio::select! {
                _ = &mut sleep => {},
                _ = done_rx.changed() => {
                    // Account for a tick delayed beyond the end of the workload too.
                    worst = worst.max(tokio::time::Instant::now().saturating_duration_since(deadline));
                    break;
                }
            }
            worst = worst.max(tokio::time::Instant::now().saturating_duration_since(deadline));
        }
        worst
    });
    ready_rx.await.unwrap();

    // Cancellation crosses an actual pipe/process boundary during store contention.
    let (mut canceled, mut canceled_output) = child(&bin).await;
    let canceled_pid = canceled.id().unwrap();
    let (stop, mut stop_rx) = watch::channel(false);
    let cancel_deadline = tokio::time::Instant::now() + Duration::from_millis(100);
    let cancel = tokio::spawn(async move {
        tokio::time::sleep_until(cancel_deadline).await;
        stop.send_replace(true);
    });
    let cancellation = tokio::spawn(async move {
        tokio::select! {
            _ = stop_rx.changed() => {},
            _ = runtime::line(&mut canceled_output) => panic!("stub must remain connected"),
        }
        runtime::terminate(&mut canceled).await;
        assert!(canceled.try_wait().unwrap().is_some());
        assert!(runtime::process_identity(canceled_pid).is_none());
        tokio::time::Instant::now().saturating_duration_since(cancel_deadline)
    });

    barrier.wait().await;
    let started = Instant::now();
    let (mut probe, mut output) = child(&bin).await;
    let spawn = started.elapsed();
    let started = Instant::now();
    probe
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"initialize\n")
        .await
        .unwrap();
    let response = runtime::line(&mut output).await.unwrap().unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&response).is_ok());
    let initialize = started.elapsed();
    probe
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"mcp\n")
        .await
        .unwrap();
    let deadline_at = tokio::time::Instant::now() + Duration::from_millis(100);
    assert!(
        tokio::time::timeout_at(deadline_at, runtime::line(&mut output))
            .await
            .is_err()
    );
    let deadline = tokio::time::Instant::now().saturating_duration_since(deadline_at);
    let started = Instant::now();
    runtime::terminate(&mut probe).await;
    let terminate = started.elapsed();
    assert!(probe.try_wait().unwrap().is_some());
    let mut calls = vec![];
    for writer in writers {
        calls.extend(writer.await.unwrap());
    }
    done.send_replace(true);
    let heartbeat = heartbeat.await.unwrap();
    cancel.await.unwrap();
    let cancel = cancellation.await.unwrap();
    let saved = w.store.lock().unwrap().snapshot().unwrap();
    assert!(saved == *w.snapshots.borrow());
    assert_eq!(saved.revision, initial + 4 * rounds as u64);
    for writer in 0..4 {
        let expected: Vec<_> = (0..rounds)
            .map(|sequence| format!("update-{writer}-{sequence}"))
            .collect();
        let prefix = format!("update-{writer}-");
        let actual: Vec<_> = saved
            .messages
            .iter()
            .filter(|m| m.id.starts_with(&prefix))
            .map(|m| m.id.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    calls.sort();
    let delays = Delays {
        heartbeat,
        initialize,
        deadline,
        spawn,
        terminate,
        cancel,
    };
    let phase = if baseline { "baseline" } else { "corrected" };
    eprintln!(
        "fixture {phase} bytes {bytes} cores {} workers {} calls {} min {}ms max {}ms heartbeat {}ms initialize {}ms deadline_late {}ms spawn {}ms terminate {}ms cancel_late {}ms",
        std::thread::available_parallelism().unwrap().get(),
        tokio::runtime::Handle::current().metrics().num_workers(),
        calls.len(),
        calls[0].as_millis(),
        calls.last().unwrap().as_millis(),
        delays.heartbeat.as_millis(),
        delays.initialize.as_millis(),
        delays.deadline.as_millis(),
        delays.spawn.as_millis(),
        delays.terminate.as_millis(),
        delays.cancel.as_millis()
    );
    delays
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "48 MiB synthetic persistence/runtime reproduction"]
async fn large_snapshot_runtime_reproduction() {
    exercise(48, 2, Duration::ZERO, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "48 MiB before/after comparison on a four-worker runtime"]
async fn large_snapshot_four_worker_runtime_comparison() {
    exercise(48, 2, Duration::ZERO, true).await;
    exercise(48, 2, Duration::ZERO, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistence_keeps_deadlines_and_cancellation_responsive() {
    let delays = exercise(1, 2, Duration::from_millis(150), false).await;
    let tolerance = Duration::from_millis(250);
    assert!(delays.heartbeat < tolerance, "{delays:?}");
    assert!(delays.deadline < tolerance, "{delays:?}");
    assert!(delays.cancel < tolerance, "{delays:?}");
    assert!(delays.initialize < tolerance, "{delays:?}");
    assert!(delays.terminate < tolerance, "{delays:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_during_update_finishes_one_durable_publication() {
    let dir = tempfile::tempdir().unwrap();
    let (w, id, run, _) = fixture(dir.path(), 0);
    let initial = w.snapshots.borrow().revision;
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let writer = w.clone();
    let task = tokio::spawn(async move {
        writer
            .update_run(&id, &run, |s| {
                entered.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(3)).unwrap();
                s.projects[0].name.push('x');
                Ok(())
            })
            .unwrap();
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    task.abort();
    release.send(()).unwrap();
    assert!(task.await.unwrap_err().is_cancelled());
    let saved = w.store.lock().unwrap().snapshot().unwrap();
    assert_eq!(saved.revision, initial + 1);
    assert!(saved == *w.snapshots.borrow());
}

const TOKEN: &str = "mixed-load-fixture-token";

fn note(session: &str, id: String, body: String) -> Message {
    Message {
        id,
        session_id: session.into(),
        author: "fixture".into(),
        sender: Sender::default(),
        kind: "text".into(),
        body,
        parts: vec![],
        tool: None,
    }
}

/// The original synchronous path for the opt-in baseline, or the corrected one.
fn write(
    w: &Workspace,
    baseline: bool,
    session: &str,
    run: &str,
    update: impl FnOnce(&mut Snapshot) -> Result<(), Error>,
) -> Result<(), Error> {
    if baseline {
        w.update_run_persisted(session, run, update)
    } else {
        w.update_run(session, run, update)
    }
}

/// The production router on a loopback listener; no production database.
async fn serve(db: &Path) -> (Workspace, std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let (app, shutdown) =
        router_with_shutdown(db, TOKEN.into(), AgentProfile::default(), config()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    shutdown.set_agent_endpoint(format!("http://{addr}"));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (shutdown.workspace.clone(), addr, server)
}

fn finished(done: &watch::Receiver<bool>) -> bool {
    *done.borrow()
}

fn heartbeat(mut done: watch::Receiver<bool>) -> tokio::task::JoinHandle<Duration> {
    tokio::spawn(async move {
        let mut worst = Duration::ZERO;
        loop {
            let deadline = tokio::time::Instant::now() + Duration::from_millis(10);
            let stopped = tokio::select! {
                _ = tokio::time::sleep_until(deadline) => false,
                _ = done.changed() => true,
            };
            worst = worst.max(tokio::time::Instant::now().saturating_duration_since(deadline));
            if stopped {
                break;
            }
        }
        worst
    })
}

/// Repeats one request until the writers finish, with at least one overlapping.
async fn repeat<F, R>(done: watch::Receiver<bool>, mut request: F) -> (Duration, usize)
where
    F: FnMut() -> R,
    R: std::future::Future<Output = ()>,
{
    let mut worst = Duration::ZERO;
    let mut count = 0;
    while count == 0 || !finished(&done) {
        let started = Instant::now();
        request().await;
        worst = worst.max(started.elapsed());
        count += 1;
    }
    (worst, count)
}

/// Executor responsiveness; neither the heartbeat nor ping takes the store lock.
#[derive(Debug)]
struct Mixed {
    heartbeat: Duration,
    ping: Duration,
}

impl Mixed {
    fn assert_responsive(&self) {
        let tolerance = Duration::from_millis(250);
        assert!(self.heartbeat < tolerance, "{self:?}");
        assert!(self.ping < tolerance, "{self:?}");
    }
}

fn status(s: &Snapshot, session: &str) -> WorkerStatus {
    s.sessions
        .iter()
        .find(|s| s.id == session)
        .and_then(|s| s.worker.as_ref())
        .unwrap()
        .status
        .clone()
}

/// HTTP/MCP readers, a draft writer, duplicate command receipts and an HTTP
/// stop with process cancellation, all while run updates contend for the
/// store; then a crash-style reopen of the same disposable database.
async fn mixed(mib: usize, rounds: usize, hold: Duration, baseline: bool) -> Mixed {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    drop(workspace(&db, &live(), config()));
    let (w, addr, server) = serve(&db).await;
    let current = w.snapshots.borrow().clone();
    let started_a = env(current.revision, start(&current));
    let (a, a_run, _a_stop, _) = reserve(&w, started_a.clone());
    let current = w.snapshots.borrow().clone();
    let (b, b_run, mut b_stop, _) = reserve(&w, env(current.revision, start(&current)));
    let mut s = w.snapshots.borrow().clone();
    for n in 0..mib * 32 {
        s.messages
            .push(note(&a, format!("filler-{n}"), "x".repeat(32 * 1024)));
    }
    let bytes = serde_json::to_vec(&s).unwrap().len();
    w.store.lock().unwrap().save(&s).unwrap();
    w.snapshots.send_replace(s);
    let initial = w.snapshots.borrow().revision;
    let mut command = tokio::process::Command::new("unused");
    let capability = agent_tools::configure(&w, &a, &a_run, Harness::Codex, &mut command).unwrap();
    let secret = command
        .as_std()
        .get_envs()
        .find(|(key, _)| *key == "RELAY_AGENT_CAPABILITY")
        .unwrap()
        .1
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let bin = dir.path().join("stub");
    script(&bin, "while IFS= read -r request; do :; done");
    let (mut b_child, mut b_output) = child(&bin).await;
    let b_pid = b_child.id().unwrap();
    let client = reqwest::Client::new();
    let protocol = relay_core::PROTOCOL_VERSION.to_string();

    let (done, done_rx) = watch::channel(false);
    let heartbeat = heartbeat(done_rx.clone());
    let mut published = w.snapshots.subscribe();
    let mut observer_done = done_rx.clone();
    let observer = tokio::spawn(async move {
        let mut seen = vec![published.borrow_and_update().revision];
        loop {
            tokio::select! {
                changed = published.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    seen.push(published.borrow_and_update().revision);
                }
                _ = observer_done.changed() => break,
            }
        }
        seen
    });

    let mut writers = vec![];
    for writer in 0..4 {
        let (w, a, a_run) = (w.clone(), a.clone(), a_run.clone());
        writers.push(tokio::spawn(async move {
            let mut elapsed = vec![];
            for sequence in 0..rounds {
                let started = Instant::now();
                write(&w, baseline, &a, &a_run, |s| {
                    std::thread::sleep(hold);
                    s.messages.push(note(
                        &a,
                        format!("update-{writer}-{sequence}"),
                        String::new(),
                    ));
                    Ok(())
                })
                .unwrap();
                elapsed.push(started.elapsed());
                tokio::task::yield_now().await;
            }
            elapsed
        }));
    }
    // Harness output for the stopped run continues until its terminal update.
    let b_writer = tokio::spawn({
        let (w, b, b_run) = (w.clone(), b.clone(), b_run.clone());
        async move {
            let mut written = 0;
            loop {
                let id = format!("stopped-{written}");
                let result = write(&w, baseline, &b, &b_run, |s| {
                    std::thread::sleep(hold);
                    s.messages.push(note(&b, id, String::new()));
                    Ok(())
                });
                if result.is_err() {
                    break written;
                }
                written += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    });
    let lifecycle = tokio::spawn({
        let (w, b, b_run) = (w.clone(), b.clone(), b_run.clone());
        async move {
            tokio::select! {
                _ = b_stop.changed() => {},
                _ = runtime::line(&mut b_output) => panic!("stub must remain connected"),
            }
            runtime::terminate(&mut b_child).await;
            assert!(b_child.try_wait().unwrap().is_some());
            assert!(runtime::process_identity(b_pid).is_none());
            // Terminal publication through the same writer path as runtime::run.
            write(&w, baseline, &b, &b_run, |s| {
                let session = s.sessions.iter_mut().find(|s| s.id == b).unwrap();
                session.worker.as_mut().unwrap().status = WorkerStatus::Stopped;
                Ok(())
            })
            .unwrap();
            Instant::now()
        }
    });
    let stop = tokio::spawn({
        let (client, protocol, w, b) = (client.clone(), protocol.clone(), w.clone(), b.clone());
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let started = Instant::now();
            let mut conflicts = 0;
            loop {
                // StopWorker requires the current revision; a client refreshes and retries.
                let revision = w.snapshots.borrow().revision;
                let request = env(
                    revision,
                    Command::StopWorker {
                        session_id: b.clone(),
                        include_descendants: false,
                    },
                );
                let response = client
                    .post(format!("http://{addr}/v1/commands"))
                    .bearer_auth(TOKEN)
                    .header("x-relay-protocol", &protocol)
                    .json(&request)
                    .send()
                    .await
                    .unwrap();
                if response.status() == reqwest::StatusCode::CONFLICT && conflicts < 1000 {
                    conflicts += 1;
                    continue;
                }
                assert!(response.status().is_success(), "{}", response.status());
                break (started, started.elapsed(), conflicts);
            }
        }
    });

    let ping = tokio::spawn(repeat(done_rx.clone(), {
        let (client, protocol) = (client.clone(), protocol.clone());
        move || {
            let request = client
                .get(format!("http://{addr}/v1/ping"))
                .bearer_auth(TOKEN)
                .header("x-relay-protocol", &protocol)
                .send();
            async move { assert!(request.await.unwrap().status().is_success()) }
        }
    }));
    // MCP tool calls authenticate inline against the store.
    let mcp = tokio::spawn(repeat(done_rx.clone(), {
        let (client, secret) = (client.clone(), secret.clone());
        move || {
            let request = client
                .post(format!("http://{addr}/v1/agent-tools/list_agents"))
                .bearer_auth(&secret)
                .json(&serde_json::json!({}))
                .send();
            async move {
                let response = request.await.unwrap();
                let status = response.status();
                assert!(status.is_success(), "{status} {:?}", response.text().await);
            }
        }
    }));
    // Duplicate command receipts: inline receipt lookup, then the idempotent apply.
    let receipt = tokio::spawn(repeat(done_rx.clone(), {
        let (client, protocol, request) = (client.clone(), protocol.clone(), started_a.clone());
        move || {
            let expected = request.request_id.clone();
            let request = client
                .post(format!("http://{addr}/v1/commands"))
                .bearer_auth(TOKEN)
                .header("x-relay-protocol", &protocol)
                .json(&request)
                .send();
            async move {
                let ack: CommandAck = request
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                assert_eq!(ack.request_id, expected);
            }
        }
    }));
    let drafts = tokio::spawn({
        let (client, protocol, a, done) =
            (client.clone(), protocol.clone(), a.clone(), done_rx.clone());
        async move {
            let (mut revision, mut worst, mut last) = (0, Duration::ZERO, None);
            while last.is_none() || !finished(&done) {
                let request = SaveDraft {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: revision,
                    parts: vec![Part::text(format!("Draft {revision}"))],
                };
                let started = Instant::now();
                let saved: Draft = client
                    .post(format!("http://{addr}/v1/drafts/{a}"))
                    .bearer_auth(TOKEN)
                    .header("x-relay-protocol", &protocol)
                    .json(&request)
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                worst = worst.max(started.elapsed());
                assert_eq!(saved.revision, revision + 1);
                revision = saved.revision;
                last = Some((request, saved));
            }
            (worst, last.unwrap())
        }
    });

    let mut calls = vec![];
    for writer in writers {
        calls.extend(writer.await.unwrap());
    }
    let (stop_started, stop, stop_conflicts) = stop.await.unwrap();
    // From the accepted StopWorker to the terminal publication.
    let cancel = lifecycle
        .await
        .unwrap()
        .saturating_duration_since(stop_started + stop);
    let stopped_writes = b_writer.await.unwrap();
    done.send_replace(true);
    let heartbeat = heartbeat.await.unwrap();
    let (ping, pings) = ping.await.unwrap();
    let (mcp, mcps) = mcp.await.unwrap();
    let (receipt, receipts) = receipt.await.unwrap();
    let (draft, (draft_request, draft_saved)) = drafts.await.unwrap();
    let seen = observer.await.unwrap();

    // Durable, published and ordered exactly once.
    let saved = w.store.lock().unwrap().snapshot().unwrap();
    assert!(saved == *w.snapshots.borrow());
    assert!(seen.windows(2).all(|p| p[0] <= p[1]), "{seen:?}");
    assert!(seen.iter().all(|r| *r <= saved.revision));
    // Run updates, the stopped run's output and terminal update, and one stop.
    assert_eq!(
        saved.revision,
        initial + 4 * rounds as u64 + stopped_writes + 2
    );
    for writer in 0..4 {
        let expected: Vec<_> = (0..rounds)
            .map(|sequence| format!("update-{writer}-{sequence}"))
            .collect();
        let prefix = format!("update-{writer}-");
        let actual: Vec<_> = saved
            .messages
            .iter()
            .filter(|m| m.id.starts_with(&prefix))
            .map(|m| m.id.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    let stopped: Vec<_> = saved
        .messages
        .iter()
        .filter(|m| m.id.starts_with("stopped-"))
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(
        stopped,
        (0..stopped_writes)
            .map(|n| format!("stopped-{n}"))
            .collect::<Vec<_>>()
    );
    let unique: std::collections::HashSet<_> = saved.messages.iter().map(|m| &m.id).collect();
    assert_eq!(unique.len(), saved.messages.len());
    assert!(runtime::active(&status(&saved, &a)));
    assert_eq!(status(&saved, &b), WorkerStatus::Stopped);

    // Crash-style reopen of the same database.
    drop(capability);
    drop(client);
    server.abort();
    let _ = server.await;
    drop(w);
    let (w, addr, server) = serve(&db).await;
    let client = reqwest::Client::new();
    let reopened = w.snapshots.borrow().clone();
    assert!(reopened.revision >= saved.revision);
    let ids = |s: &Snapshot| s.messages.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
    assert!(ids(&reopened).starts_with(&ids(&saved)));
    // Restart interrupts the unfinished run; the stopped run stays stopped.
    assert_eq!(status(&reopened, &a), WorkerStatus::Interrupted);
    assert_eq!(status(&reopened, &b), WorkerStatus::Stopped);
    let ack: CommandAck = client
        .post(format!("http://{addr}/v1/commands"))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", &protocol)
        .json(&started_a)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    // The receipt replays nothing: no relaunch, new session or revision.
    assert_eq!(ack.committed_revision, reopened.revision);
    assert!(w.store.lock().unwrap().snapshot().unwrap() == reopened);
    assert_eq!(
        reopened
            .sessions
            .iter()
            .filter(|s| s.id == format!("session-{}", started_a.request_id))
            .count(),
        1
    );
    let replayed: Draft = client
        .post(format!("http://{addr}/v1/drafts/{a}"))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", &protocol)
        .json(&draft_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(replayed, draft_saved);
    let listed: Vec<Draft> = client
        .get(format!("http://{addr}/v1/drafts"))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", &protocol)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(listed.contains(&draft_saved));
    server.abort();
    let _ = server.await;

    calls.sort();
    let phase = if baseline { "baseline" } else { "corrected" };
    // Store-bound requests queue behind the writers' holds, so they are reported only.
    eprintln!(
        "mixed {phase} bytes {bytes} workers {} writes {} stopped_writes {stopped_writes} min {}ms max {}ms published {} heartbeat {}ms ping {}ms/{pings} mcp {}ms/{mcps} draft {}ms receipt {}ms/{receipts} stop {}ms conflicts {stop_conflicts} cancel_after_stop {}ms",
        tokio::runtime::Handle::current().metrics().num_workers(),
        calls.len(),
        calls[0].as_millis(),
        calls.last().unwrap().as_millis(),
        seen.len(),
        heartbeat.as_millis(),
        ping.as_millis(),
        mcp.as_millis(),
        draft.as_millis(),
        receipt.as_millis(),
        stop.as_millis(),
        cancel.as_millis(),
    );
    Mixed { heartbeat, ping }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_readers_stop_and_reopen_keep_durable_order_under_update_load() {
    // Durability, order, receipts and no replay are asserted for both paths;
    // only the corrected path must keep the executor responsive.
    mixed(1, 2, Duration::from_millis(150), true).await;
    mixed(1, 2, Duration::from_millis(150), false)
        .await
        .assert_responsive();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "48 MiB mixed reader/stop before/after comparison"]
async fn large_snapshot_mixed_reader_comparison() {
    mixed(48, 2, Duration::ZERO, true).await;
    mixed(48, 2, Duration::ZERO, false)
        .await
        .assert_responsive();
}
