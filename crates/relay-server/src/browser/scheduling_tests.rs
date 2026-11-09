//! Constrained production-router scheduling and unchanged session semantics.
use super::*;
use std::time::Instant;
use tokio::time::{Duration, timeout};

struct Fixture {
    server: Server,
    handle: tokio::runtime::Handle,
    entered: tokio::sync::mpsc::Receiver<()>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn fixture(old_synchronous_wait: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("setup"), CODE).unwrap();
    let (app, shutdown) = router_with_browser(
        dir.path().join("db"),
        TOKEN.into(),
        AgentProfile::default(),
        RuntimeConfig {
            codex: dir.path().join("missing-codex"),
            claude: dir.path().join("missing-claude"),
            nix: dir.path().join("missing-nix"),
            ..Default::default()
        },
        BrowserConfig {
            public_origin: Some(ORIGIN.into()),
            setup_token_file: Some(dir.path().join("setup")),
            web_root: None,
        },
    )
    .unwrap();
    let (entered_tx, entered) = tokio::sync::mpsc::channel(2);
    let w = shutdown.workspace.clone();
    let cookie_ready = Arc::new(tokio::sync::Barrier::new(2));
    let app = app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let w = w.clone();
            let entered_tx = entered_tx.clone();
            let cookie_ready = cookie_ready.clone();
            async move {
                if request.uri().path() == "/v1/ping" && request.headers().contains_key("cookie") {
                    // Admit both HTTP handlers before either synchronous wait can strand
                    // an unpolled peer in the executor local queue. Yield makes both stealable.
                    cookie_ready.wait().await;
                    tokio::task::yield_now().await;
                    // Each valid fixture cookie request reaches this immediately before auth.
                    entered_tx.try_send(()).unwrap();
                    if old_synchronous_wait {
                        // Controlled model of the OLD unwrapped mutex wait, not an old binary.
                        // Production auth still runs afterwards with all checks intact.
                        drop(w.store.lock().unwrap());
                    }
                }
                next.run(request).await
            }
        },
    ));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        ready_tx.send(runtime.handle().clone()).unwrap();
        runtime.block_on(async move {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
    });
    Fixture {
        server: Server {
            dir,
            url,
            client: Client::new(),
            task: tokio::spawn(std::future::pending()),
            shutdown,
        },
        handle: ready_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        entered,
        stop: Some(stop),
        thread: Some(thread),
    }
}
async fn hold(w: Workspace) -> (std::sync::mpsc::SyncSender<()>, tokio::task::JoinHandle<()>) {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let holder = tokio::task::spawn_blocking(move || {
        let _store = w.store.lock().unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    ready_rx.await.unwrap();
    (release_tx, holder)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_scheduling_saturated_cookie_waits_allow_bearer_and_timer() {
    for old in [true, false] {
        let mut f = fixture(old);
        let (cookie, _) = f.server.enroll().await;
        assert_eq!(f.server.session(&cookie).await.status(), 200); // Valid before lock saturation.
        let (release, holder) = hold(f.server.shutdown.workspace.clone()).await;
        let mut cookies = Vec::new();
        for _ in 0..2 {
            let client = f.server.client.clone();
            let url = format!("{}/v1/ping", f.server.url);
            let cookie = cookie.clone();
            cookies.push(tokio::spawn(async move {
                client
                    .get(url)
                    .header("cookie", cookie)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16()
            }));
        }
        for _ in 0..2 {
            timeout(Duration::from_secs(3), f.entered.recv())
                .await
                .unwrap()
                .unwrap();
        }
        let start = Instant::now();
        let (timer_tx, timer_rx) = tokio::sync::oneshot::channel();
        f.handle.spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let _ = timer_tx.send(());
        });
        let ping = f
            .server
            .client
            .get(format!("{}/v1/ping", f.server.url))
            .bearer_auth(TOKEN)
            .send();
        let deadline = if old {
            Duration::from_millis(120)
        } else {
            Duration::from_secs(2)
        };
        let (ping, timer) = tokio::join!(timeout(deadline, ping), timeout(deadline, timer_rx));
        let before_release_us = start.elapsed().as_micros();
        let cookie_pending = cookies.iter().all(|c| !c.is_finished());
        // Always release BEFORE asserting so failure cannot strand the production server.
        release.send(()).unwrap();
        holder.await.unwrap();
        let cookie_statuses = [
            cookies.remove(0).await.unwrap(),
            cookies.remove(0).await.unwrap(),
        ];
        assert!(cookie_pending);
        assert_eq!(cookie_statuses, [204, 204]);
        if old {
            assert!(ping.is_err());
            assert!(timer.is_err());
        } else {
            assert_eq!(ping.unwrap().unwrap().status(), 204);
            timer.unwrap().unwrap();
        }
        println!(
            "SCHED old_wait_model={} workers=2 cookie_waiters=2 bearer_before_release={} timer_before_release={} observation_us={before_release_us} cookie_completed_after_release=2",
            u8::from(old),
            u8::from(!old),
            u8::from(!old)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_scheduling_cancelled_session_touch_finishes_and_rechecks_revocation() {
    let f = fixture(false);
    let (cookie, _) = f.server.enroll().await;
    let w = f.server.shutdown.workspace.clone();
    let mut headers = HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    let key = hash(cookie.split_once('=').unwrap().1);
    for revoked in [false, true] {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let holder_w = w.clone();
        let holder_key = key.clone();
        let holder = tokio::task::spawn_blocking(move || {
            let store = holder_w.store.lock().unwrap();
            store
                .connection
                .execute(
                    "UPDATE browser_sessions SET seen=?1 WHERE session_hash=?2",
                    params![seconds() - 60, holder_key],
                )
                .unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            if revoked {
                store
                    .connection
                    .execute("DELETE FROM browser_sessions", [])
                    .unwrap();
            }
        });
        ready_rx.await.unwrap();
        let call_w = w.clone();
        let headers = headers.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let call = tokio::spawn(async move {
            started_tx.send(()).unwrap();
            let allowed = call_w.browser_session(&headers, true).is_ok();
            let _ = finished_tx.send(allowed);
        });
        started_rx.await.unwrap();
        // Prove the task passed its cancellation point into the synchronous section:
        // on this task there is no await between started and the complete section/result.
        call.abort();
        release_tx.send(()).unwrap();
        holder.await.unwrap();
        let allowed = timeout(Duration::from_secs(3), finished_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(allowed, !revoked);
        let store = w.store.lock().unwrap();
        let seen: Option<i64> = store
            .connection
            .query_row(
                "SELECT seen FROM browser_sessions WHERE session_hash=?1",
                [&key],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        if revoked {
            assert!(seen.is_none());
        } else {
            assert!(seen.unwrap() >= seconds() - 5);
        }
        println!(
            "AUTH_CANCEL revoked={} completed=1 touch_present={}",
            u8::from(revoked),
            u8::from(seen.is_some())
        );
    }
}

// Current-thread runtime exercises the synchronous fallback, including rejected/untouched rows.
#[tokio::test]
async fn browser_scheduling_session_checks_touch_and_expiry_are_unchanged() {
    let f = fixture(false);
    let (cookie, _) = f.server.enroll().await;
    let w = &f.server.shutdown.workspace;
    let key = hash(cookie.split_once('=').unwrap().1);
    let mut headers = HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    let csrf = w.browser_session(&headers, false).unwrap().csrf;
    let sync_w = w.clone();
    let sync_headers = headers.clone();
    assert!(
        std::thread::spawn(move || sync_w.browser_session(&sync_headers, false).is_ok())
            .join()
            .unwrap()
    );
    let now = seconds();
    let set = |created, seen| {
        w.store
            .lock()
            .unwrap()
            .connection
            .execute(
                "UPDATE browser_sessions SET created=?1,seen=?2 WHERE session_hash=?3",
                params![created, seen, key],
            )
            .unwrap();
    };
    let seen = || -> i64 {
        w.store
            .lock()
            .unwrap()
            .connection
            .query_row(
                "SELECT seen FROM browser_sessions WHERE session_hash=?1",
                [&key],
                |r| r.get(0),
            )
            .unwrap()
    };
    set(now, now - 60);
    assert!(w.browser_session(&headers, false).is_ok());
    assert_eq!(seen(), now - 60);
    assert!(w.browser_session(&headers, true).is_ok());
    assert!(seen() >= now);
    for (created, last) in [(now - ABSOLUTE, now), (now, now - IDLE)] {
        set(created, last);
        assert!(w.browser_session(&headers, true).is_err());
        assert_eq!(seen(), last);
    }
    set(now, now - 60);
    for origin in [None, Some("https://wrong.invalid")] {
        let mut h = headers.clone();
        if let Some(origin) = origin {
            h.insert("origin", origin.parse().unwrap());
        }
        h.insert("x-relay-csrf", csrf.parse().unwrap());
        assert!(w.browser_request(&h, &Method::POST, false).is_err());
        assert_eq!(seen(), now - 60);
    }
    let mut valid = headers.clone();
    valid.insert("origin", ORIGIN.parse().unwrap());
    assert!(w.browser_request(&valid, &Method::POST, false).is_err());
    assert_eq!(seen(), now - 60);
    valid.insert("x-relay-csrf", csrf.parse().unwrap());
    w.browser_request(&valid, &Method::POST, false).unwrap();
    assert!(seen() >= now);
    let mut duplicate = valid.clone();
    duplicate.append("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    duplicate.append("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    assert!(w.browser_request(&duplicate, &Method::POST, false).is_err());
    let mut duplicate_cookie = headers.clone();
    duplicate_cookie.append("cookie", cookie.parse().unwrap());
    assert!(w.browser_session(&duplicate_cookie, false).is_err());
    w.store
        .lock()
        .unwrap()
        .connection
        .execute("DELETE FROM browser_sessions", [])
        .unwrap();
    assert!(w.browser_session(&valid, true).is_err());
    assert!(w.browser_session(&HeaderMap::new(), false).is_err());
}
