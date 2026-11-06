//! Process boundary coverage: real binary, HTTP server, fake harness and stdio MCP.
use relay_core::*;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

async fn responsive_bridge_with_pending_call(tool: &str) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = axum::Router::new()
        .route(
            &format!("/v1/agent-tools/{tool}"),
            axum::routing::post({
                let started = started.clone();
                let release = release.clone();
                let calls = calls.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        started.notify_one();
                        release.notified().await;
                        axum::Json(json!({"finished":true}))
                    }
                }
            }),
        )
        .route(
            "/v1/agent-tools/tools",
            axum::routing::post(|| async {
                axum::Json(json!({"tools":[{"name":"report_result"},{"name":"list_agents"}]}))
            }),
        )
        .route(
            "/v1/agent-tools/list_agents",
            axum::routing::post(|| async { axum::Json(json!({"agents":[]})) }),
        );
    let peer = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .arg("mcp")
        .env_clear()
        .env("RELAY_AGENT_URL", format!("http://{address}"))
        .env("RELAY_AGENT_CAPABILITY", "disposable-fixture-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = bridge.stdin.take().unwrap();
    let mut output = BufReader::new(bridge.stdout.take().unwrap()).lines();
    let pending =
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":{}}});
    input
        .write_all(format!("{pending}\n").as_bytes())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    for request in [
        json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"initialize"}),
    ] {
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }
    let responsive = tokio::time::timeout(Duration::from_secs(2), async {
        let mut ids = Vec::new();
        for _ in 0..4 {
            let value: Value =
                serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
            let id = value["id"].as_u64().unwrap();
            assert_ne!(
                id, 1,
                "Pending long call was cancelled or completed without release"
            );
            if id == 3 {
                assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 2);
            }
            if id == 4 {
                assert_eq!(value["result"]["isError"], false);
            }
            ids.push(id);
        }
        ids.sort();
        assert_eq!(ids, vec![2, 3, 4, 5]);
    })
    .await;
    release.notify_one();
    drop(input);
    if responsive.is_ok() {
        let line = tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["id"], 1);
        assert_eq!(value["result"]["isError"], false);
        let content: Value =
            serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["finished"], true);
    }
    bridge.kill().await.ok();
    bridge.wait().await.ok();
    peer.abort();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        responsive.is_ok(),
        "Pending tools/call blocked ping, tools/list, initialize and independent call"
    );
}

#[tokio::test]
async fn pending_call_does_not_block_ping_catalog_or_another_call() {
    responsive_bridge_with_pending_call("wait_for_agents").await;
}

#[tokio::test]
async fn pending_unarchive_is_forwarded_without_blocking_discovery_or_other_calls() {
    responsive_bridge_with_pending_call("unarchive_agent").await;
}

#[tokio::test]
async fn bridge_preserves_fragmented_input_and_does_not_replay_active_ids() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = axum::Router::new().route(
        "/v1/agent-tools/message_agent",
        axum::routing::post({
            let started = started.clone();
            let release = release.clone();
            let calls = calls.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    started.notify_one();
                    release.notified().await;
                    axum::Json(json!({"queued":true}))
                }
            }
        }),
    );
    let peer = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .arg("mcp")
        .env_clear()
        .env("RELAY_AGENT_URL", format!("http://{address}"))
        .env("RELAY_AGENT_CAPABILITY", "disposable-fixture-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = bridge.stdin.take().unwrap();
    let mut output = BufReader::new(bridge.stdout.take().unwrap()).lines();
    let request = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"message_agent","arguments":{"agent":"fixture","text":"once"}}});
    input
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    input
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    // Exceed the pipe buffer so write completion proves the bridge consumed a
    // partial frame before the pending HTTP operation completes.
    let partial = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\",\"params\":{{\"padding\":\"{}",
        "x".repeat(128 * 1024)
    );
    tokio::time::timeout(Duration::from_secs(5), input.write_all(partial.as_bytes()))
        .await
        .unwrap()
        .unwrap();
    let duplicate: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(duplicate["id"], Value::Null);
    assert_eq!(duplicate["error"]["code"], -32600);
    release.notify_one();
    let reply: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["result"]["isError"], false);
    input.write_all(b"\"}}\n").await.unwrap();
    drop(input);
    let ping: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ping["id"], 8);
    assert_eq!(ping["result"], json!({}));
    assert!(
        tokio::time::timeout(Duration::from_secs(5), bridge.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    peer.abort();
}

#[tokio::test]
async fn bridge_bounds_calls_without_blocking_control_requests() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release, _) = tokio::sync::watch::channel(false);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = axum::Router::new()
        .route(
            "/v1/agent-tools/wait_for_agents",
            axum::routing::post({
                let release = release.clone();
                let calls = calls.clone();
                move || {
                    let mut release = release.subscribe();
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        while !*release.borrow_and_update() {
                            release.changed().await.unwrap();
                        }
                        axum::Json(json!({"finished":true}))
                    }
                }
            }),
        )
        .route(
            "/v1/agent-tools/tools",
            axum::routing::post(|| async {
                axum::Json(json!({"tools":[{"name":"report_result"}]}))
            }),
        );
    let peer = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .arg("mcp")
        .env_clear()
        .env("RELAY_AGENT_URL", format!("http://{address}"))
        .env("RELAY_AGENT_CAPABILITY", "disposable-fixture-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = bridge.stdin.take().unwrap();
    let mut output = BufReader::new(bridge.stdout.take().unwrap()).lines();
    for id in 1..=33 {
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"wait_for_agents","arguments":{}}});
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }
    for (id, method) in [(100, "ping"), (101, "tools/list")] {
        input
            .write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":id,"method":method})).as_bytes())
            .await
            .unwrap();
    }
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..3 {
        let value: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(5), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let id = value["id"].as_u64().unwrap();
        ids.insert(id);
        if id == 33 {
            assert_eq!(value["error"]["code"], -32000);
        }
        if id == 101 {
            assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 1);
        }
    }
    assert_eq!(ids, [33, 100, 101].into());
    release.send_replace(true);
    drop(input);
    for _ in 0..32 {
        let value: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(5), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["result"]["isError"], false);
        assert!(ids.insert(value["id"].as_u64().unwrap()));
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 32);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), bridge.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    peer.abort();
}

#[derive(Debug, PartialEq)]
enum StartupFailure {
    Exited {
        reason: &'static str,
        code: Option<i32>,
        signal: Option<i32>,
    },
    Deadline(&'static str),
    ReadinessStatus(u16),
}

fn startup_exit(reason: &'static str, status: Option<std::process::ExitStatus>) -> StartupFailure {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal = None;
    StartupFailure::Exited {
        reason,
        code: status.and_then(|s| s.code()),
        signal,
    }
}

/// Consume startup stderr without retaining or printing arbitrary log contents.
/// The announced address belongs to the child; no bind/drop/rebind handoff.
async fn ready_child(
    child: &mut tokio::process::Child,
    client: &reqwest::Client,
    token: &str,
) -> Result<String, StartupFailure> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let stderr = child.stderr.take().expect("Fixture stderr must be piped");
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        let mut reason = "early_exit";
        while let Ok(Some(line)) = reader.next_line().await {
            if line.contains("Address already in use") {
                reason = "bind_conflict";
            } else if line.contains("Resource temporarily unavailable") {
                reason = "resource_unavailable";
            } else if line.contains("Database schema is newer") {
                reason = "schema_incompatible";
            }
            if let Some(value) = line
                .strip_prefix("Relay server listening on ")
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<std::net::SocketAddr>().ok())
                .filter(|a| a.ip().is_loopback() && a.port() != 0)
            {
                let _ = sender.send(Ok(value));
            }
        }
        let _ = sender.send(Err(reason));
    });
    let mut phase = "listener";
    tokio::time::timeout(Duration::from_secs(15), async {
        let address = match events.recv().await {
            Some(Ok(address)) => address,
            other => {
                return Err(startup_exit(
                    match other {
                        Some(Err(reason)) => reason,
                        _ => "stderr_closed",
                    },
                    child.wait().await.ok(),
                ));
            }
        };
        phase = "authenticated_ping";
        let endpoint = format!("http://{address}");
        loop {
            if let Some(status) = child
                .try_wait()
                .expect("Fixture process status unavailable")
            {
                return Err(startup_exit("after_listener", Some(status)));
            }
            if let Ok(response) = client
                .get(format!("{endpoint}/v1/ping"))
                .bearer_auth(token)
                .send()
                .await
            {
                if !response.status().is_success() {
                    return Err(StartupFailure::ReadinessStatus(response.status().as_u16()));
                }
                return Ok(endpoint);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or(Err(StartupFailure::Deadline(phase)))
}

async fn checked_startup(
    child: &mut tokio::process::Child,
    client: &reqwest::Client,
    token: &str,
    case: &'static str,
) -> String {
    match ready_child(child, client, token).await {
        Ok(endpoint) => endpoint,
        Err(error) => {
            child.kill().await.ok();
            child.wait().await.ok();
            panic!("Boundary startup {case}: {error:?}");
        }
    }
}

fn private_environment(command: &mut tokio::process::Command, directory: &std::path::Path) {
    // PATH is the pinned tool lookup path, not service configuration. Every
    // server setting and HOME/XDG location otherwise belongs to this fixture.
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", directory)
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("XDG_CACHE_HOME", directory.join("cache"))
        .env("XDG_DATA_HOME", directory.join("data"));
}

#[tokio::test]
async fn startup_bind_conflict_explains_legacy_elapsed_without_hiding_child_exit() {
    // Model another listener claiming the address before the old fixture's
    // child binds it. This is the released-port handoff's failure mechanism.
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = blocker.local_addr().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"));
    private_environment(&mut command, directory.path());
    let mut child = command
        .env("RELAY_BIND", address.to_string())
        .env("RELAY_DATABASE", directory.path().join("db"))
        .env("RELAY_TOKEN", &token)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let result = ready_child(&mut child, &client, &token).await;
    assert!(
        matches!(
            result,
            Err(StartupFailure::Exited {
                reason: "bind_conflict",
                ..
            })
        ),
        "Expected classified bind conflict, got {result:?}"
    );
    assert!(!child.wait().await.unwrap().success());
    // The previous helper ignored that exit and merely polled this port. Its
    // only observable result is elapsed, regardless of a longer timeout.
    let legacy = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if client
                .get(format!("http://{address}/v1/ping"))
                .bearer_auth(&token)
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(legacy.is_err());
}

#[tokio::test]
async fn startup_requires_successful_authenticated_ping() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/v1/ping",
                axum::routing::get(|| async { axum::http::StatusCode::UNAUTHORIZED }),
            ),
        )
        .await
        .unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new("sh");
    private_environment(&mut command, directory.path());
    let mut child = command.arg("-c").arg(format!("printf '%s\\n' 'Relay server listening on {address} · database synthetic' >&2; read line"))
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    let result = ready_child(
        &mut child,
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        "fixture-only",
    )
    .await;
    assert_eq!(result, Err(StartupFailure::ReadinessStatus(401)));
    child.kill().await.ok();
    child.wait().await.ok();
    peer.abort();
}

/// Exercise the actual sequential stdio bridge against a disposable HTTP peer.
/// A stalled catalog must release both tools/list and the following ping.
async fn catalog_peer(mode: &'static str) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut byte = [0];
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.unwrap();
            headers.push(byte[0]);
        }
        match mode {
            "headers" => std::future::pending::<()>().await,
            "body" => {
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\r\n{\"tools\":")
                    .await
                    .unwrap();
                std::future::pending::<()>().await;
            }
            _ => {
                let (status, body) = match mode {
                    "unauthorized" => ("401 Unauthorized", "{}"),
                    "malformed" => ("200 OK", "{"),
                    "catalog" => ("200 OK", "{\"tools\":[{\"name\":\"policy-specific\"}]}"),
                    "call" => {
                        // tools/call may legitimately await a human longer than
                        // the catalog budget. It must retain its own lifetime.
                        tokio::time::sleep(Duration::from_millis(3500)).await;
                        ("200 OK", "{\"approved\":true}")
                    }
                    _ => unreachable!(),
                };
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        }
    });
    let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .arg("mcp")
        .env("RELAY_AGENT_URL", format!("http://{address}"))
        .env("RELAY_AGENT_CAPABILITY", "disposable-fixture-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let method = if mode == "call" {
        "tools/call"
    } else {
        "tools/list"
    };
    let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"name":"list_agents","arguments":{}}});
    let mut stdin = bridge.stdin.take().unwrap();
    stdin
        .write_all(
            format!("{request}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}}\n").as_bytes(),
        )
        .await
        .unwrap();
    drop(stdin);
    let mut lines = BufReader::new(bridge.stdout.take().unwrap()).lines();
    let responses = tokio::time::timeout(Duration::from_secs(8), async {
        let first: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        let ping: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        (first, ping)
    })
    .await;
    peer.abort();
    bridge.kill().await.ok();
    bridge.wait().await.ok();
    let (a, b) = responses.expect("Catalog blocked the stdio bridge and following ping");
    let (first, ping) = if a["id"] == 1 { (a, b) } else { (b, a) };
    assert_eq!(first["id"], 1);
    assert_eq!(ping["id"], 2);
    assert_eq!(ping["result"], json!({}));
    if mode == "call" {
        assert_eq!(first["result"]["isError"], false);
        let content: Value =
            serde_json::from_str(first["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["approved"], true);
    } else {
        let tools = first["result"]["tools"].as_array().unwrap();
        if mode == "catalog" {
            assert_eq!(tools[0]["name"], "policy-specific");
        } else {
            for name in ["report_result", "list_agents", "spawn_agent"] {
                assert!(tools.iter().any(|t| t["name"] == name));
            }
        }
    }
}

#[tokio::test]
async fn catalog_stalled_headers_release_ping() {
    catalog_peer("headers").await;
}
#[tokio::test]
async fn catalog_stalled_body_release_ping() {
    catalog_peer("body").await;
}
#[tokio::test]
async fn catalog_fast_fallback_and_policy_success() {
    for mode in ["unauthorized", "malformed", "catalog"] {
        catalog_peer(mode).await;
    }
}
#[tokio::test]
async fn catalog_budget_does_not_limit_tool_calls() {
    catalog_peer("call").await;
}

struct Server {
    child: tokio::process::Child,
    endpoint: String,
    token: String,
    client: reqwest::Client,
    directory: tempfile::TempDir,
}
impl Server {
    async fn new(real_claude: bool) -> Self {
        Self::new_named(real_claude, "boundary").await
    }
    async fn new_named(real_claude: bool, case: &'static str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let token = uuid::Uuid::new_v4().to_string();
        let binary = env!("CARGO_BIN_EXE_relay-server");
        let fake = directory.path().join("fake-codex");
        if !real_claude {
            std::fs::write(&fake, r#"#!/bin/sh
set -eu
umask 077
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    model/list) printf '{"id":%s,"result":{"data":[{"id":"test-model","displayName":"Test model"}]}}\n' "$id" ;;
    skills/list|mcpServerStatus/list) printf '{"id":%s,"result":{"data":[]}}\n' "$id" ;;
    thread/start|thread/resume) printf '{"id":%s,"result":{"model":"test-model","thread":{"id":"fake-thread"}}}\n' "$id" ;;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"turn"}}}\n' "$id"
      printf '%s\n' '{"method":"turn/started","params":{"threadId":"fake-thread","turn":{"id":"turn"}}}' '{"method":"item/started","params":{"threadId":"fake-thread","item":{"id":"spawn-result","type":"mcpToolCall","server":"relay","tool":"spawn_agent","arguments":{"call_id":"boundary-spawn","title":"","task":"Verify"}}}}'
      if [ -e "$RELAY_BOUNDARY_DIR/outbox-probe" ]; then
        printf 'effect\n' >> "$RELAY_BOUNDARY_DIR/outbox-effects"
        while [ -e "$RELAY_BOUNDARY_DIR/hold-before-output" ]; do sleep 0.05; done
        printf '%s\n' '{"method":"item/completed","params":{"threadId":"fake-thread","item":{"id":"outbox-output","type":"agentMessage","text":"Lifecycle notice observed"}}}'
        while [ -e "$RELAY_BOUNDARY_DIR/hold-after-output" ]; do sleep 0.05; done
        printf '%s\n' '{"method":"turn/completed","params":{"threadId":"fake-thread","turn":{"id":"turn","status":"completed"}}}'
        continue
      fi
      # Neither the scoped secret nor the workspace credential is printed.
      test -z "${RELAY_TOKEN:-}" && test -z "${RELAY_TOKEN_FILE:-}"
      printf '%s' "$RELAY_AGENT_CAPABILITY" > "$RELAY_BOUNDARY_DIR/capability"
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' '{"jsonrpc":"2.0","id":3,"method":"ping"}' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}' '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"archive_agent","arguments":{"agent":"missing"}}}' '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"call_id":"boundary-spawn","title":"","task":"Verify"}}}' '{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"unarchive_agent","arguments":{"agent":"missing"}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/receipt"
      # Exercise strict arguments and real self-scope refusal through stdio/HTTP.
      caller=$(jq -r 'select(.id==4) | .result.content[0].text' "$RELAY_BOUNDARY_DIR/receipt" | jq -r '.agents[0].id')
      jq -nc --arg caller "$caller" '{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"unarchive_agent","arguments":{"agent":$caller}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/unarchive-self"
      printf '%s\n' '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"unarchive_agent","arguments":{"agent":"missing","subtree":false}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/unarchive-invalid"
      # The first session that lists a mentioned peer messages it once.
      peer=$(jq -r 'select(.id==4) | .result.content[0].text' "$RELAY_BOUNDARY_DIR/receipt" | jq -r '.agents[]? | select(.relationship=="peer") | .id' | head -n 1 || true)
      if [ -n "$peer" ] && [ ! -e "$RELAY_BOUNDARY_DIR/peer-list" ]; then
        cp "$RELAY_BOUNDARY_DIR/receipt" "$RELAY_BOUNDARY_DIR/peer-list"
        jq -nc --arg peer "$peer" '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"message_agent","arguments":{"agent":$peer,"text":"Hello peer"}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/peer-message"
      fi
      # A terminal harness event proves delivery; flushing the bridge alone does not.
      # Codex 0.161 transforms the bridge envelope: isError becomes the native
      # terminal status, not a property of McpToolCallResult.
      jq -nc --argjson result "$(jq -c 'select(.id==7) | .result' "$RELAY_BOUNDARY_DIR/receipt")" '{"method":"item/completed","params":{"threadId":"fake-thread","item":{"id":"spawn-result","type":"mcpToolCall","server":"relay","tool":"spawn_agent","status":(if $result.isError then "failed" else "completed" end),"arguments":{"call_id":"boundary-spawn","title":"","task":"Verify"},"result":{"content":$result.content,"_meta":{"fixture":true},"structuredContent":null}}}}'
      printf '%s\n' '{"method":"turn/started","params":{"threadId":"fake-thread","turn":{"id":"turn"}}}' '{"method":"turn/completed","params":{"threadId":"fake-thread","turn":{"id":"turn","status":"completed"}}}'
      exit ;;
  esac
done
"#).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let fake_claude = directory.path().join("fake-claude");
        if !real_claude {
            std::fs::write(&fake_claude, r#"#!/bin/sh
set -eu
umask 077
while IFS= read -r line; do
  type=$(printf '%s' "$line" | jq -r '.type')
  case "$type" in
    control_request)
      id=$(printf '%s' "$line" | jq -c '.request_id')
      printf '{"type":"control_response","response":{"request_id":%s,"subtype":"success","response":{"models":[{"value":"test-claude","displayName":"Test Claude"}],"commands":[],"mcpServers":[]}}}\n' "$id" ;;
    user)
      printf '%s\n' '{"type":"system","subtype":"init","session_id":"11111111-2222-4333-8444-555555555555","model":"test-claude"}'
      if [ -e "$RELAY_BOUNDARY_DIR/outbox-probe" ]; then
        printf 'effect\n' >> "$RELAY_BOUNDARY_DIR/outbox-effects"
        while [ -e "$RELAY_BOUNDARY_DIR/hold-before-output" ]; do sleep 0.05; done
        printf '%s\n' '{"type":"assistant","message":{"id":"outbox-output","content":[{"type":"text","text":"Lifecycle notice observed"}]}}'
        while [ -e "$RELAY_BOUNDARY_DIR/hold-after-output" ]; do sleep 0.05; done
        printf '%s\n' '{"type":"result","subtype":"success"}' '{"type":"system","subtype":"session_state_changed","state":"idle","sdk_host_only":true}'
        continue
      fi
      if [ ! -e "$RELAY_BOUNDARY_DIR/claude-initial" ]; then
        touch "$RELAY_BOUNDARY_DIR/claude-initial"
        for call in acknowledged lost; do
          jq -nc --arg call "$call" '{"type":"assistant","message":{"id":$call,"content":[{"type":"tool_use","id":$call,"name":"mcp__relay__spawn_agent","input":{"call_id":$call,"title":"","task":"Verify"}}]}}'
          jq -nc --arg call "$call" '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"call_id":$call,"title":"","task":"Verify"}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/claude-$call"
          if [ "$call" = acknowledged ]; then
            jq -nc --argjson result "$(jq -c '.result' "$RELAY_BOUNDARY_DIR/claude-$call")" '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"acknowledged","is_error":$result.isError,"content":$result.content}]}}'
          fi
        done
      else
        # Actual assistant output after the notice input establishes native
        # notice consumption, rather than mere reservation/process launch.
        printf '%s\n' '{"type":"assistant","message":{"id":"notice-read","content":[{"type":"text","text":"Recovery notice received"}]}}'
      fi
      printf '%s\n' '{"type":"result","subtype":"success"}' '{"type":"system","subtype":"session_state_changed","state":"idle","sdk_host_only":true}' ;;
  esac
done
"#).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
        }
        let mut command = tokio::process::Command::new(binary);
        if !real_claude {
            private_environment(&mut command, directory.path());
        }
        command
            .env("RELAY_BIND", "127.0.0.1:0")
            .env("RELAY_DATABASE", directory.path().join("db"))
            .env("RELAY_TOKEN", &token)
            .env_remove("RELAY_TOKEN_FILE")
            .env_remove("RELAY_GITHUB_REPO")
            .env_remove("RELAY_GITHUB_PROJECT_OWNER")
            .env_remove("RELAY_GITHUB_PROJECT_NUMBER")
            .env_remove("RELAY_REPO_PATH")
            .env_remove("RELAY_DEFAULT_PROFILE")
            .env_remove("RELAY_PUBLIC_ORIGIN")
            .env_remove("RELAY_SETUP_TOKEN_FILE")
            .env_remove("RELAY_WEB_ROOT")
            .env("RELAY_PROJECTS_DIR", directory.path().join("projects"))
            .env("RELAY_BOUNDARY_DIR", directory.path())
            .env("RELAY_BOUNDARY_SERVER", binary)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if !real_claude {
            command.env("RELAY_CODEX_BIN", fake);
            command.env("RELAY_CLAUDE_BIN", fake_claude);
        }
        let mut child = command.spawn().unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let endpoint = checked_startup(&mut child, &client, &token, case).await;
        Self {
            child,
            endpoint,
            token,
            client,
            directory,
        }
    }
    async fn snapshot(&self) -> Snapshot {
        self.client
            .get(format!("{}/v1/snapshot", self.endpoint))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn command(&self, command: Command) -> Snapshot {
        let phase = match &command {
            Command::CreateProject { .. } => "create_project",
            Command::UpdateAgentConfig { .. } => "update_agent_config",
            Command::StartRootAgent { .. } => "start_root_agent",
            Command::SubmitTurn { .. } => "submit_turn",
            Command::SendWorker { .. } => "send_worker",
            _ => "other_fixture_command",
        };
        let snapshot = self.snapshot().await;
        let envelope = CommandEnvelope {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: snapshot.revision,
            command,
        };
        let response = self
            .client
            .post(format!("{}/v1/commands", self.endpoint))
            .bearer_auth(&self.token)
            .header("x-relay-protocol", PROTOCOL_VERSION)
            .json(&envelope)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .unwrap();
        let status = response.status();
        if !status.is_success() {
            let error = response.json::<ApiError>().await.ok();
            let code = match error.as_ref().map(|e| e.code.as_str()) {
                Some("conflict") => "conflict",
                Some("invalid") => "invalid",
                Some("storage") => "storage",
                Some("shutdown") => "shutdown",
                Some("protocol") => "protocol",
                _ => "unclassified",
            };
            let reason = match error.as_ref().map(|e| e.message.as_str()) {
                Some("Workspace changed. Refresh and review your draft before saving again.") => {
                    "workspace_revision_changed"
                }
                Some("Choose a model before creating this worker") => "model_required",
                Some("Run is no longer active") => "run_inactive",
                _ => "unclassified",
            };
            // Print only allowlisted labels/numbers, never response bodies,
            // config values, raw prompts, capability or authentication headers.
            eprintln!(
                "fixture_command phase={phase} status={} code={code} reason={reason} expected_revision={}",
                status.as_u16(),
                envelope.expected_revision
            );
            let current = self.snapshot().await.revision;
            panic!(
                "Boundary command rejected phase={phase} status={} code={code} reason={reason} expected_revision={} current_revision={current}",
                status.as_u16(),
                envelope.expected_revision
            );
        }
        self.snapshot().await
    }
    async fn launch(&self, harness: Harness) -> String {
        self.start(harness, "MCP boundary", vec![]).await
    }
    /// Start a root agent in a new project; structured parts replace the default prompt.
    async fn start(&self, harness: Harness, name: &str, parts: Vec<Part>) -> String {
        let prepared = self.prepare(harness, name).await;
        self.start_prepared(prepared, parts).await
    }
    /// Configuration edits require a current revision. Prepare them before
    /// starting the agents whose events intentionally advance workspace state.
    async fn prepare(&self, harness: Harness, name: &str) -> (String, String) {
        let root = self.directory.path().join(name.replace(' ', "-"));
        let snapshot = self
            .command(Command::CreateProject {
                name: name.into(),
                root: root.display().to_string(),
                connections: vec![],
            })
            .await;
        let director = snapshot
            .agent_configs
            .iter()
            .find(|d| {
                snapshot
                    .projects
                    .iter()
                    .any(|p| p.id == d.project_id && p.name == name)
            })
            .unwrap();
        let project_id = director.project_id.clone();
        let agent_config_id = director.id.clone();
        let route = if harness == Harness::Codex {
            "codex"
        } else {
            "claude_code"
        };
        let catalog: HarnessCatalog = self
            .client
            .get(format!(
                "{}/v1/projects/{}/catalog/{route}",
                self.endpoint, director.project_id
            ))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let model = catalog
            .models
            .first()
            .expect("A boundary model is available")
            .id
            .clone();
        self.command(Command::UpdateAgentConfig {
            agent_config_id: agent_config_id.clone(),
            name: director.name.clone(),
            overrides: ProfileOverrides {
                harness: Some(harness),
                model: Some(model),
                ..Default::default()
            },
        })
        .await;
        (project_id, agent_config_id)
    }
    async fn start_prepared(&self, prepared: (String, String), parts: Vec<Part>) -> String {
        let (project_id, agent_config_id) = prepared;
        let prompt = if parts.is_empty() {
            "Call the Relay MCP tool mcp__relay__list_agents once, then reply with only the agent count. Do not use other tools or inspect environment variables.".into()
        } else {
            plain_text(&parts)
        };
        let snapshot = self
            .command(Command::StartRootAgent {
                issue_id: None,
                agent_config_id,
                prompt,
                parts,
                approve_implementation: true,
            })
            .await;
        snapshot
            .sessions
            .iter()
            .find(|s| !s.fixture && s.project_id == project_id)
            .unwrap()
            .id
            .clone()
    }
    async fn terminal(&self, session: &str) -> Snapshot {
        tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                let snapshot = self.snapshot().await;
                if snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session)
                    .unwrap()
                    .worker
                    .as_ref()
                    .is_some_and(|w| {
                        !matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued)
                    })
                {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("Harness boundary timed out")
    }
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
        let _ = self.child.wait().await;
    }

    async fn crash(&mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }

    async fn reopen(&mut self) {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"));
        private_environment(&mut command, self.directory.path());
        command
            .env("RELAY_BIND", "127.0.0.1:0")
            .env("RELAY_DATABASE", self.directory.path().join("db"))
            .env("RELAY_TOKEN", &self.token)
            .env_remove("RELAY_TOKEN_FILE")
            .env_remove("RELAY_GITHUB_REPO")
            .env_remove("RELAY_GITHUB_PROJECT_OWNER")
            .env_remove("RELAY_GITHUB_PROJECT_NUMBER")
            .env_remove("RELAY_REPO_PATH")
            .env_remove("RELAY_DEFAULT_PROFILE")
            .env("RELAY_PROJECTS_DIR", self.directory.path().join("projects"))
            .env("RELAY_BOUNDARY_DIR", self.directory.path())
            .env("RELAY_BOUNDARY_SERVER", env!("CARGO_BIN_EXE_relay-server"))
            .env("RELAY_CODEX_BIN", self.directory.path().join("fake-codex"))
            .env(
                "RELAY_CLAUDE_BIN",
                self.directory.path().join("fake-claude"),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        self.child = command.spawn().unwrap();
        self.endpoint = checked_startup(&mut self.child, &self.client, &self.token, "reopen").await;
    }
}

#[tokio::test]
async fn lifecycle_native_start_and_output_crash_never_replay_effects() {
    for harness in [Harness::Codex, Harness::ClaudeCode] {
        for phase in ["before", "after"] {
            let case = match (harness, phase) {
                (Harness::Codex, "before") => "lifecycle_codex_before",
                (Harness::Codex, _) => "lifecycle_codex_after",
                (Harness::ClaudeCode, "before") => "lifecycle_claude_before",
                _ => "lifecycle_claude_after",
            };
            let mut server = Server::new_named(false, case).await;
            let parent = server.launch(harness).await;
            server.terminal(&parent).await;
            // Let any existing spawn receipt notice finish before isolating
            // the lifecycle delivery under test.
            if harness == Harness::ClaudeCode {
                tokio::time::timeout(Duration::from_secs(15), async {
                    loop {
                        let s = server.snapshot().await;
                        if s.submissions.iter().any(|t| {
                            t.id.starts_with("relay-notice-spawn-")
                                && t.state == SubmissionState::Completed
                        }) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();
            }
            let mut snapshot = server.snapshot().await;
            server.crash().await;
            // Seed a committed terminal event in a private, offline fixture.
            // Unit tests separately establish terminal/event transactionality.
            let mut child = snapshot
                .sessions
                .iter()
                .find(|s| s.id == parent)
                .unwrap()
                .clone();
            let mut profile = snapshot.session_profile(&child).unwrap();
            profile
                .permissions
                .insert(Task::Implement, Permission::Allow);
            snapshot
                .sessions
                .iter_mut()
                .find(|s| s.id == parent)
                .unwrap()
                .profile = Some(profile.clone());
            child.profile = Some(profile);
            child.id = "synthetic-lifecycle-child".into();
            child.parent_session_id = Some(parent.clone());
            child.worker.as_mut().unwrap().status = WorkerStatus::Completed;
            let project = child.project_id.clone();
            for config in snapshot
                .agent_configs
                .iter_mut()
                .filter(|c| c.project_id == project)
            {
                config.overrides.permissions = Some([(Task::Implement, Permission::Allow)].into());
            }
            snapshot.sessions.push(child);
            snapshot.revision += 1;
            let mut db = rusqlite::Connection::open(server.directory.path().join("db")).unwrap();
            let tx = db.transaction().unwrap();
            tx.execute(
                "UPDATE workspace SET snapshot=?1",
                [serde_json::to_string(&snapshot).unwrap()],
            )
            .unwrap();
            tx.execute("INSERT INTO runs(session_id,run_id) VALUES('synthetic-lifecycle-child','synthetic-child-run')",[]).unwrap();
            tx.execute("INSERT INTO lifecycle_events(child_session,child_run,parent_session,disposition) VALUES('synthetic-lifecycle-child','synthetic-child-run',?1,'pending')",[&parent]).unwrap();
            tx.commit().unwrap();
            drop(db);
            std::fs::write(server.directory.path().join("outbox-probe"), b"").unwrap();
            std::fs::write(
                server.directory.path().join(format!("hold-{phase}-output")),
                b"",
            )
            .unwrap();
            server.reopen().await;
            let snapshot = server.snapshot().await;
            let notice = snapshot
                .submissions
                .iter()
                .find(|t| {
                    t.session_id == parent
                        && t.parts.iter().any(|p| {
                            plain_text(std::slice::from_ref(p))
                                .contains("synthetic-lifecycle-child")
                        })
                })
                .unwrap()
                .id
                .clone();
            assert_eq!(
                snapshot
                    .submissions
                    .iter()
                    .find(|t| t.id == notice)
                    .unwrap()
                    .state,
                SubmissionState::Paused
            );
            server
                .command(Command::ResumeQueue {
                    session_id: parent.clone(),
                    queue: None,
                })
                .await;
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    let snapshot = server.snapshot().await;
                    let output = snapshot
                        .messages
                        .iter()
                        .any(|m| m.session_id == parent && m.body == "Lifecycle notice observed");
                    let started = snapshot
                        .sessions
                        .iter()
                        .find(|s| s.id == parent)
                        .unwrap()
                        .worker
                        .as_ref()
                        .unwrap()
                        .thread_id
                        .is_some();
                    if server.directory.path().join("outbox-effects").exists()
                        && started
                        && (phase == "before" || output)
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap();
            server.crash().await;
            std::fs::remove_file(server.directory.path().join(format!("hold-{phase}-output")))
                .unwrap();
            for _ in 0..2 {
                server.reopen().await;
                let snapshot = server.snapshot().await;
                assert_eq!(
                    snapshot
                        .submissions
                        .iter()
                        .filter(|t| t.id == notice)
                        .count(),
                    1
                );
                assert_eq!(
                    snapshot
                        .submissions
                        .iter()
                        .find(|t| t.id == notice)
                        .unwrap()
                        .state,
                    SubmissionState::Interrupted
                );
                assert_eq!(
                    snapshot
                        .sessions
                        .iter()
                        .find(|s| s.id == parent)
                        .unwrap()
                        .worker
                        .as_ref()
                        .unwrap()
                        .status,
                    WorkerStatus::Interrupted
                );
                assert_eq!(
                    std::fs::read_to_string(server.directory.path().join("outbox-effects"))
                        .unwrap()
                        .lines()
                        .count(),
                    1
                );
                server.crash().await;
            }
        }
    }
}

#[tokio::test]
#[ignore = "Requires RELAY_OUTBOX_OLD_SERVER pointing to a retained schema29 binary"]
async fn schema29_process_rejects_schema30_without_writing_history() {
    let old =
        std::env::var_os("RELAY_OUTBOX_OLD_SERVER").expect("Retained schema29 binary required");
    let mut server = Server::new(false).await;
    server.crash().await;
    let path = server.directory.path().join("db");
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: String = db
        .query_row("SELECT snapshot FROM workspace", [], |r| r.get(0))
        .unwrap();
    drop(db);
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(old)
            .env_clear()
            .env("RELAY_BIND", "127.0.0.1:0")
            .env("RELAY_DATABASE", &path)
            .env("RELAY_TOKEN", &server.token)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("Database schema is newer than this Relay server")
    );
    // SQLite may touch WAL bookkeeping; logical source and schema must remain.
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        30
    );
    let after: String = db
        .query_row("SELECT snapshot FROM workspace", [], |r| r.get(0))
        .unwrap();
    assert!(after == before, "Old writer changed synthetic history");
}

#[tokio::test]
async fn stdio_http_run_capability_boundary() {
    let server = Server::new(false).await;
    let session = server.launch(Harness::Codex).await;
    let snapshot = server.terminal(&session).await;
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Completed
    );
    let receipt = std::fs::read_to_string(server.directory.path().join("receipt")).unwrap();
    let mut values = receipt
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    // Concurrent MCP replies correlate by ID, not completion order.
    values.sort_by_key(|value| value["id"].as_u64().unwrap());
    assert_eq!(values.len(), 7);
    assert_eq!(values[0]["result"]["capabilities"]["tools"], json!({}));
    let tools = values[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 9);
    assert!(tools.iter().any(|tool| tool["name"] == "stop_agent"));
    assert!(tools.iter().any(|tool| tool["name"] == "archive_agent"));
    assert!(tools.iter().any(|tool| tool["name"] == "unarchive_agent"));
    let unarchive_tool = tools
        .iter()
        .find(|tool| tool["name"] == "unarchive_agent")
        .unwrap();
    assert_eq!(unarchive_tool["inputSchema"]["required"], json!(["agent"]));
    assert_eq!(unarchive_tool["inputSchema"]["additionalProperties"], false);
    assert!(tools.iter().any(|tool| tool["name"] == "delete_agent"));
    assert_eq!(values[3]["result"]["isError"], false);
    let content: Value =
        serde_json::from_str(values[3]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["agents"][0]["id"], session);
    assert_eq!(content["agents"][0]["archived"], false);
    assert_eq!(values[4]["result"]["isError"], true);
    let archive: Value =
        serde_json::from_str(values[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(archive["message"], "Agent missing");
    assert_eq!(values[5]["result"]["isError"], true);
    let spawn: Value =
        serde_json::from_str(values[5]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(spawn["phase"], "rejected");
    assert_eq!(values[6]["result"]["isError"], true);
    let unarchive: Value =
        serde_json::from_str(values[6]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(unarchive["message"], "Agent missing");
    for (file, message) in [
        (
            "unarchive-self",
            "Only your own descendants can be archived",
        ),
        ("unarchive-invalid", "Invalid tool arguments"),
    ] {
        let receipt = std::fs::read(server.directory.path().join(file)).unwrap();
        let value: Value = serde_json::from_slice(&receipt).unwrap();
        assert_eq!(value["result"]["isError"], true);
        let content: Value =
            serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["message"], message);
    }
    assert!(spawn["spawn_attempt"].is_string());
    assert!(
        !snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == session && s.id.starts_with("relay-notice-spawn-"))
    );
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .filter(|s| s.parent_session_id.as_deref() == Some(&session))
            .count(),
        0
    );
    assert!(snapshot.messages.iter().any(|m| {
        m.session_id == session
            && m.tool
                .as_ref()
                .is_some_and(|t| t.kind == ToolKind::Spawn && t.state == ToolState::Failed)
    }));
    // Read the protected credential solely to verify revoked authentication; never format it.
    let secret = std::fs::read_to_string(server.directory.path().join("capability")).unwrap();
    for tool in ["list_agents", "unarchive_agent"] {
        for credential in [&secret, &server.token] {
            let response = server
                .client
                .post(format!("{}/v1/agent-tools/{tool}", server.endpoint))
                .bearer_auth(credential)
                .json(&json!({"agent":"missing"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        }
    }
    let mut mcp = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .arg("mcp")
        .env("RELAY_AGENT_URL", &server.endpoint)
        .env("RELAY_AGENT_CAPABILITY", "invalid-test-capability")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    mcp.stdin.take().unwrap().write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_agents\",\"arguments\":{}}}\n").await.unwrap();
    let output = mcp.wait_with_output().await.unwrap();
    assert!(serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"]["isError"] == true);
    server.stop().await;
}

/// Exercise the public process boundary without compiling the server's libtest.
/// Gates and credentials belong only to the scripted adapter, not production code.
struct UnarchiveBoundary {
    server: Server,
    parent: String,
    child: String,
    grandchild: String,
    capability: String,
}
impl UnarchiveBoundary {
    async fn new() -> Self {
        let server = Server::new(false).await;
        // No model runs: the same fake app-server protocol supplies finite history
        // and holds two selected turns while the test drives real HTTP requests.
        std::fs::write(
            server.directory.path().join("fake-codex"),
            r#"#!/bin/sh
set -eu
umask 077
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    model/list) printf '{"id":%s,"result":{"data":[{"id":"test-model","displayName":"Test model"}]}}\n' "$id" ;;
    skills/list|mcpServerStatus/list) printf '{"id":%s,"result":{"data":[]}}\n' "$id" ;;
    thread/start|thread/resume) printf '{"id":%s,"result":{"model":"test-model","thread":{"id":"unarchive-thread"}}}\n' "$id" ;;
    turn/start)
      printf 'turn\n' >> "$RELAY_BOUNDARY_DIR/unarchive-turns"
      printf '{"id":%s,"result":{"turn":{"id":"turn"}}}\n' "$id"
      printf '%s\n' '{"method":"turn/started","params":{"threadId":"unarchive-thread","turn":{"id":"turn"}}}'
      # Context and previous tasks may contain markers. Only an exact selected
      # text part of this turn may select a gate; never capture the full request.
      gate=$(printf '%s' "$line" | jq -r '[.params.input[]? | select(.type == "text") | .text] | if any(. == "UNARCHIVE_HOLD_CHILD") then "child" elif any(. == "UNARCHIVE_HOLD_PARENT") then "parent" else "none" end')
      case "$gate" in
        parent)
          test -z "${RELAY_TOKEN:-}" && test -z "${RELAY_TOKEN_FILE:-}"
          printf '%s' "$RELAY_AGENT_CAPABILITY" > "$RELAY_BOUNDARY_DIR/unarchive-capability.tmp"
          mv "$RELAY_BOUNDARY_DIR/unarchive-capability.tmp" "$RELAY_BOUNDARY_DIR/unarchive-capability"
          ;;
      esac
      # Only fixed phase names are retained: no input, headers or credentials.
      printf '%s\n' "$gate" >> "$RELAY_BOUNDARY_DIR/unarchive-gates"
      if [ "$gate" != none ]; then
        touch "$RELAY_BOUNDARY_DIR/unarchive-$gate-ready"
        attempts=0
        while [ ! -e "$RELAY_BOUNDARY_DIR/unarchive-$gate-release" ]; do
          attempts=$((attempts + 1))
          test "$attempts" -lt 1500 || exit 1
          sleep 0.02
        done
      fi
      printf '%s\n' '{"method":"item/completed","params":{"threadId":"unarchive-thread","item":{"id":"retained","type":"agentMessage","text":"Retained boundary transcript"}}}' '{"method":"turn/completed","params":{"threadId":"unarchive-thread","turn":{"id":"turn","status":"completed"}}}'
      exit ;;
  esac
done
"#,
        )
        .unwrap();
        let parent = server
            .start(
                Harness::Codex,
                "Unarchive boundary",
                vec![Part::text("UNARCHIVE_HOLD_PARENT")],
            )
            .await;
        // Keep the parent active throughout setup: normal child-ended notices
        // can queue but cannot start an incidental parent run before our caller.
        Self::wait_file(&server, "unarchive-parent-ready", &parent).await;
        let snapshot = server.snapshot().await;
        let parent_session = snapshot.sessions.iter().find(|s| s.id == parent).unwrap();
        assert!(
            !parent_session.workspaces.is_empty()
                && parent_session
                    .workspaces
                    .iter()
                    .all(|space| !space.repository),
            "Boundary fixture requires shared ordinary directories, not repository worktrees"
        );
        let child = Self::create_child(&server, &parent, "Child", "Seed child history").await;
        Self::completed(&server, &child, "seed-child").await;
        Self::notice(&server, &parent, &child, "seed-child-ended").await;
        let snapshot = server.snapshot().await;
        let notice = snapshot
            .submissions
            .iter()
            .find(|s| {
                s.session_id == parent
                    && s.id.starts_with("relay-notice-")
                    && s.state == SubmissionState::Queued
                    && plain_text(&s.parts).contains(&child)
            })
            .unwrap();
        // Consume this first batch explicitly so a later wait cannot mistake
        // its retained record for the resumed child's second completion.
        server
            .command(Command::CancelTurn {
                submission_id: notice.id.clone(),
            })
            .await;
        server
            .command(Command::SendWorker {
                session_id: child.clone(),
                prompt: "UNARCHIVE_HOLD_CHILD".into(),
                approve_implementation: true,
            })
            .await;
        Self::wait_file(&server, "unarchive-child-ready", &child).await;
        let grandchild =
            Self::create_child(&server, &child, "Grandchild", "Seed grandchild history").await;
        Self::completed(&server, &grandchild, "seed-grandchild").await;
        // Observe the real lifecycle batch before pausing the child's queue.
        // Otherwise its delayed delivery could mutate retention snapshots later.
        Self::notice(&server, &child, &grandchild, "grandchild-ended").await;
        // Explicitly settle the observed outcome before pausing later work.
        // The durable outbox correctly defers the child's own terminal notice
        // while a grandchild outcome still needs handling. Cancellation retains
        // its history/identity and must not launch another adapter turn.
        let observed = server.snapshot().await;
        let grandchild_notice = observed
            .submissions
            .iter()
            .find(|s| {
                s.session_id == child
                    && s.id.starts_with("relay-notice-")
                    && s.state == SubmissionState::Queued
                    && plain_text(&s.parts).contains(&grandchild)
            })
            .unwrap();
        let turns_before_cancel =
            std::fs::read_to_string(server.directory.path().join("unarchive-turns")).unwrap();
        let settled = server
            .command(Command::CancelTurn {
                submission_id: grandchild_notice.id.clone(),
            })
            .await;
        assert_eq!(
            settled
                .submissions
                .iter()
                .find(|s| s.id == grandchild_notice.id)
                .unwrap()
                .state,
            SubmissionState::Cancelled
        );
        assert_eq!(
            std::fs::read_to_string(server.directory.path().join("unarchive-turns")).unwrap(),
            turns_before_cancel
        );
        let sibling = Self::create_child(&server, &parent, "Sibling", "").await;
        // A real accepted human mention creates a grant and a queued follow-up
        // behind the child's held turn. Archive must prune it, not restore it.
        let parts = vec![
            Part::text("Paused follow-up must not execute"),
            Part {
                id: uuid::Uuid::new_v4().to_string(),
                kind: PartKind::SessionMention {
                    mention: SessionMention {
                        session_id: sibling.clone(),
                        label: "Unarchive boundary • Sibling".into(),
                    },
                },
            },
        ];
        let response = server
            .client
            .post(format!("{}/v1/drafts/{child}", server.endpoint))
            .bearer_auth(&server.token)
            .header("x-relay-protocol", PROTOCOL_VERSION)
            .json(&SaveDraft {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                parts: parts.clone(),
            })
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "Boundary draft rejected");
        let draft: Draft = response.json().await.unwrap();
        let queued = server
            .command(Command::SubmitTurn {
                session_id: child.clone(),
                draft_revision: draft.revision,
                parts,
                approve_implementation: true,
            })
            .await;
        assert!(queued.is_peer(&child, &sibling));
        assert!(
            queued
                .submissions
                .iter()
                .any(|s| s.session_id == child && s.state == SubmissionState::Queued)
        );
        server
            .command(Command::PauseQueue {
                session_id: child.clone(),
            })
            .await;
        std::fs::write(server.directory.path().join("unarchive-child-release"), "").unwrap();
        Self::completed(&server, &child, "release-child").await;
        Self::notice(&server, &parent, &child, "child-ended").await;
        server
            .command(Command::PauseQueue {
                session_id: parent.clone(),
            })
            .await;
        server
            .command(Command::SetSessionNetworkAccess {
                session_id: grandchild.clone(),
                network_access: Some(false),
            })
            .await;
        let archived = server
            .command(Command::SetSessionArchived {
                session_id: child.clone(),
                archived: true,
            })
            .await;
        assert!(!archived.is_peer(&child, &sibling));
        for id in [&child, &grandchild] {
            let session = archived.sessions.iter().find(|s| &s.id == id).unwrap();
            assert!(session.archived);
            assert_eq!(
                session.worker.as_ref().unwrap().status,
                WorkerStatus::Completed
            );
            assert!(session.worker.as_ref().unwrap().thread_id.is_some());
            assert!(
                archived
                    .messages
                    .iter()
                    .any(|m| &m.session_id == id && !m.body.is_empty())
            );
            for space in &session.workspaces {
                std::fs::write(
                    std::path::Path::new(&space.path).join("unarchive-evidence.txt"),
                    "Retained workspace evidence",
                )
                .unwrap();
            }
        }
        // Read this protected file solely for authenticated test requests.
        let path = server.directory.path().join("unarchive-capability");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let capability = std::fs::read_to_string(path).unwrap();
        Self {
            server,
            parent,
            child,
            grandchild,
            capability,
        }
    }
    async fn create_child(server: &Server, parent: &str, name: &str, prompt: &str) -> String {
        let snapshot = server.snapshot().await;
        let parent_session = snapshot.sessions.iter().find(|s| s.id == parent).unwrap();
        let profile = snapshot.session_profile(parent_session).unwrap();
        let snapshot = server
            .command(Command::StartChildAgent {
                parent_session_id: parent.into(),
                name: name.into(),
                profile,
                // Creation prepares an inactive child. Start its first turn
                // separately with a receipt the launch guard authorizes.
                prompt: String::new(),
                approve_implementation: true,
            })
            .await;
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.parent_session_id.as_deref() == Some(parent) && s.title == name)
            .unwrap();
        assert!(child.awaiting_first_message);
        assert_eq!(
            child.worker.as_ref().unwrap().status,
            WorkerStatus::Completed
        );
        let id = child.id.clone();
        if !prompt.is_empty() {
            server
                .command(Command::SendWorker {
                    session_id: id.clone(),
                    prompt: prompt.into(),
                    approve_implementation: true,
                })
                .await;
        }
        id
    }
    fn launch_error_category(error: Option<&str>) -> &'static str {
        // Match fixed source errors; never return any part of arbitrary errors.
        match error {
            None => "none",
            Some("Run receipt does not authorize a turn") => "run-receipt",
            Some("Run superseded" | "Run is no longer active") => "run-state",
            Some("Agent capability unavailable or expired") => "run-capability",
            Some(
                "Model is no longer available"
                | "Effort is not supported by this model"
                | "Fast mode is not supported by this model"
                | "Choose a model before creating this worker",
            ) => "model-selection",
            Some(
                "Cannot start harness discovery"
                | "Harness output unavailable"
                | "Harness discovery disconnected"
                | "Harness discovery timed out"
                | "Codex discovery disconnected"
                | "Invalid Codex discovery event"
                | "Codex discovery request unavailable",
            ) => "discovery",
            Some(
                "Session working directory unavailable"
                | "Session working directory missing"
                | "Session workspace unavailable"
                | "Cannot create session workspace"
                | "A recorded session workspace is unavailable; restore it before resuming"
                | "Recorded worktree is unavailable"
                | "Project root is unavailable",
            ) => "workspace-preparation",
            Some(
                "Child workspaces were not prepared"
                | "Child workspace identity changed"
                | "Primary fork workspace missing",
            ) => "fork-preparation",
            Some(
                "Current profile denies implementation"
                | "Implementation approval is required for this turn"
                | "Execution mode changed before launch; review and send again"
                | "Network access changed before launch; review and send again",
            ) => "execution-policy",
            Some(
                "Cannot launch agent harness; check its executable and authentication in Settings",
            ) => "process-launch",
            Some("Codex thread ID missing" | "Codex resumed a different thread") => "thread",
            Some("Workspace storage failed") => "storage",
            Some(error) if error.starts_with("Codex rejected ") => "adapter-rpc",
            Some(_) => "unclassified",
        }
    }
    fn launch_diagnostic(server: &Server, snapshot: &Snapshot, session: &str) -> String {
        let worker = snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .and_then(|s| s.worker.as_ref());
        let count = |state| {
            snapshot
                .submissions
                .iter()
                .filter(|s| s.session_id == session && s.state == state)
                .count()
        };
        let gates = std::fs::read_to_string(server.directory.path().join("unarchive-gates"))
            .unwrap_or_default();
        let gate_count = |name| gates.lines().filter(|line| *line == name).count();
        let error = worker.and_then(|w| w.error.as_deref());
        // Emit enums, counts and booleans only. Free-form errors may contain
        // provider data, so classify them without printing their contents.
        format!(
            "status={:?} thread={} error={} error_category={} policy_error={} workspace_error={} queued={} launching={} running={} paused={} pending_permissions={} gates(parent/child/none)={}/{}/{}",
            worker.map(|w| &w.status),
            worker.is_some_and(|w| w.thread_id.is_some()),
            error.is_some(),
            Self::launch_error_category(error),
            error.is_some_and(|e| e.contains("denies")
                || e.contains("approval")
                || e.contains("permission")),
            error.is_some_and(|e| e.contains("workspace")),
            count(SubmissionState::Queued),
            count(SubmissionState::Launching),
            count(SubmissionState::Running),
            count(SubmissionState::Paused),
            snapshot
                .tool_permissions
                .iter()
                .filter(|p| p.session_id == session && p.decision.is_none() && !p.expired)
                .count(),
            gate_count("parent"),
            gate_count("child"),
            gate_count("none"),
        )
    }
    async fn completed(server: &Server, session: &str, stage: &str) {
        let snapshot = server.terminal(session).await;
        let status = &snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .status;
        assert_eq!(
            *status,
            WorkerStatus::Completed,
            "Boundary stage {stage}: {}",
            Self::launch_diagnostic(server, &snapshot, session)
        );
    }
    async fn notice(server: &Server, target: &str, child: &str, stage: &str) {
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = server.snapshot().await;
                if snapshot.submissions.iter().any(|s| {
                    s.session_id == target
                        && s.id.starts_with("relay-notice-")
                        && s.state == SubmissionState::Queued
                        && plain_text(&s.parts).contains(child)
                }) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if result.is_err() {
            let snapshot = server.snapshot().await;
            panic!(
                "Boundary lifecycle stage {stage} timed out: {}",
                Self::launch_diagnostic(server, &snapshot, target)
            );
        }
    }
    async fn wait_file(server: &Server, name: &str, session: &str) {
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = server.snapshot().await;
                let worker = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session)
                    .and_then(|s| s.worker.as_ref());
                assert!(
                    worker.is_some_and(|w| matches!(
                        w.status,
                        WorkerStatus::Queued | WorkerStatus::Running
                    )),
                    "Boundary gate {name} ended before readiness: {}",
                    Self::launch_diagnostic(server, &snapshot, session)
                );
                if server.directory.path().join(name).exists() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if result.is_err() {
            let snapshot = server.snapshot().await;
            panic!(
                "Boundary gate {name} timed out: {}",
                Self::launch_diagnostic(server, &snapshot, session)
            );
        }
    }
    fn turns(&self) -> usize {
        std::fs::read_to_string(self.server.directory.path().join("unarchive-turns"))
            .unwrap()
            .lines()
            .count()
    }
    fn request(&self) -> reqwest::RequestBuilder {
        self.server
            .client
            .post(format!(
                "{}/v1/agent-tools/unarchive_agent",
                self.server.endpoint
            ))
            .bearer_auth(&self.capability)
            .timeout(Duration::from_secs(20))
            .json(&json!({"agent":self.child}))
    }
    async fn mcp_unarchive(&self) -> Value {
        // Exercise the shipped stdio bridge against the held caller's actual
        // run capability. No provider acknowledgement is implied by this call.
        let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_relay-server"))
            .arg("mcp")
            .env("RELAY_AGENT_URL", &self.server.endpoint)
            .env("RELAY_AGENT_CAPABILITY", &self.capability)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut request = serde_json::to_vec(&json!({
            "jsonrpc":"2.0", "id":41, "method":"tools/call",
            "params":{"name":"unarchive_agent","arguments":{"agent":self.child}}
        }))
        .unwrap();
        request.push(b'\n');
        bridge
            .stdin
            .take()
            .unwrap()
            .write_all(&request)
            .await
            .unwrap();
        let output = tokio::time::timeout(Duration::from_secs(20), bridge.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "MCP bridge failed");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn delegate(&self, permission: Permission) {
        let snapshot = self.server.snapshot().await;
        let parent = snapshot
            .sessions
            .iter()
            .find(|s| s.id == self.parent)
            .unwrap();
        let config = snapshot
            .agent_configs
            .iter()
            .find(|c| c.id == parent.agent_config_id)
            .unwrap();
        let mut overrides = config.overrides.clone();
        let mut permissions = snapshot.session_profile(parent).unwrap().permissions;
        permissions.insert(Task::Delegate, permission);
        overrides.permissions = Some(permissions);
        let snapshot = self
            .server
            .command(Command::UpdateAgentConfig {
                agent_config_id: config.id.clone(),
                name: config.name.clone(),
                overrides,
            })
            .await;
        let parent = snapshot
            .sessions
            .iter()
            .find(|s| s.id == self.parent)
            .unwrap();
        assert_eq!(
            snapshot.session_profile(parent).unwrap().permissions[&Task::Delegate],
            permission
        );
    }
    async fn pending(&self) -> ToolPermission {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = self.server.snapshot().await;
                if let Some(p) = snapshot.tool_permissions.iter().find(|p| {
                    p.session_id == self.parent
                        && p.tool == "mcp__relay__unarchive_agent"
                        && p.decision.is_none()
                        && !p.expired
                }) {
                    assert!(p.description.contains("entire inactive descendant branch"));
                    return p.clone();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("Unarchive did not request human approval")
    }
    async fn decide(&self, permission: ToolPermission, allow: bool) {
        self.server
            .command(Command::RespondPermission {
                permission_id: permission.id,
                run_id: permission.run_id,
                allow,
            })
            .await;
    }
    async fn retained(&self, before: &Snapshot, available: bool, turns: usize) {
        // A bounded quiet interval plus every observed adapter start guards
        // against a transient launch that a terminal snapshot alone could miss.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let after = self.server.snapshot().await;
        assert_eq!(after.sessions.len(), before.sessions.len());
        for original in &before.sessions {
            let mut expected = original.clone();
            if available && (original.id == self.child || original.id == self.grandchild) {
                expected.archived = false;
            }
            let actual = after.sessions.iter().find(|s| s.id == original.id).unwrap();
            assert_eq!(actual, &expected);
            assert_eq!(
                before.session_profile(original).unwrap(),
                after.session_profile(actual).unwrap()
            );
        }
        assert_eq!(after.messages, before.messages);
        assert_eq!(after.submissions, before.submissions);
        assert_eq!(after.session_peers, before.session_peers);
        assert!(
            after
                .submissions
                .iter()
                .any(|s| s.session_id == self.child && s.state == SubmissionState::Paused)
        );
        for id in [&self.child, &self.grandchild] {
            let session = after.sessions.iter().find(|s| &s.id == id).unwrap();
            for space in &session.workspaces {
                assert_eq!(
                    std::fs::read_to_string(
                        std::path::Path::new(&space.path).join("unarchive-evidence.txt")
                    )
                    .unwrap(),
                    "Retained workspace evidence"
                );
            }
        }
        assert_eq!(self.turns(), turns, "Unarchive started an adapter turn");
    }
    async fn stop(self) {
        std::fs::write(
            self.server
                .directory
                .path()
                .join("unarchive-parent-release"),
            "",
        )
        .unwrap();
        Self::completed(&self.server, &self.parent, "release-parent").await;
        self.server.stop().await;
    }
}

#[tokio::test]
async fn unarchive_http_mutation_approval_and_rechecks() {
    let f = UnarchiveBoundary::new().await;
    f.delegate(Permission::Allow).await;
    let before = f.server.snapshot().await;
    let turns = f.turns();
    // Delegate Allow: actual successful HTTP mutation of the whole branch,
    // then an idempotent repeat, retaining histories, paused work and policy.
    for _ in 0..2 {
        let response = f.request().send().await.unwrap();
        assert!(response.status().is_success(), "Allow unarchive rejected");
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"agent":f.child,"archived":false})
        );
        f.retained(&before, true, turns).await;
    }
    f.server
        .command(Command::SetSessionArchived {
            session_id: f.child.clone(),
            archived: true,
        })
        .await;
    // Positive native MCP mutation shares the same no-run/no-replay contract.
    let before = f.server.snapshot().await;
    let reply = f.mcp_unarchive().await;
    assert_eq!(reply["id"], 41);
    assert_eq!(reply["result"]["isError"], false);
    let restored: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(restored, json!({"agent":f.child,"archived":false}));
    f.retained(&before, true, turns).await;
    f.server
        .command(Command::SetSessionArchived {
            session_id: f.child.clone(),
            archived: true,
        })
        .await;
    // Scope and argument failures go through the authenticated production route.
    let before = f.server.snapshot().await;
    for args in [
        json!({"agent":f.parent}),
        json!({"agent":"missing"}),
        json!({"agent":f.child,"subtree":false}),
    ] {
        let response = f.request().json(&args).send().await.unwrap();
        assert!(!response.status().is_success());
        f.retained(&before, false, turns).await;
    }
    f.delegate(Permission::Deny).await;
    let before = f.server.snapshot().await;
    let response = f.request().send().await.unwrap();
    assert!(!response.status().is_success());
    assert_eq!(
        response.json::<Value>().await.unwrap()["message"],
        "Profile denies managing subagents"
    );
    assert_eq!(
        f.server.snapshot().await.tool_permissions.len(),
        before.tool_permissions.len()
    );
    f.retained(&before, false, turns).await;

    f.delegate(Permission::Ask).await;
    for allow in [false, true] {
        let request = f.request();
        let task = tokio::spawn(async move { request.send().await.unwrap() });
        let permission = f.pending().await;
        assert!(!task.is_finished(), "Ask bypassed human approval");
        let before = f.server.snapshot().await;
        assert!(
            before
                .sessions
                .iter()
                .filter(|s| s.id == f.child || s.id == f.grandchild)
                .all(|s| s.archived)
        );
        f.decide(permission, allow).await;
        let response = task.await.unwrap();
        assert_eq!(response.status().is_success(), allow);
        if allow {
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"agent":f.child,"archived":false})
            );
        } else {
            assert_eq!(
                response.json::<Value>().await.unwrap()["message"],
                "Human denied agent tool permission"
            );
        }
        f.retained(&before, allow, turns).await;
    }
    f.server
        .command(Command::SetSessionArchived {
            session_id: f.child.clone(),
            archived: true,
        })
        .await;
    // Deterministic approval races: mutate through the human HTTP endpoint
    // while the actual agent request waits, then approve its original plan.
    for mutation in ["caller-policy", "descendant-policy"] {
        f.delegate(Permission::Ask).await;
        let request = f.request();
        let task = tokio::spawn(async move { request.send().await.unwrap() });
        let permission = f.pending().await;
        assert!(!task.is_finished());
        if mutation == "caller-policy" {
            f.delegate(Permission::Allow).await;
        } else {
            f.server
                .command(Command::SetSessionNetworkAccess {
                    session_id: f.grandchild.clone(),
                    network_access: Some(true),
                })
                .await;
        }
        let before = f.server.snapshot().await;
        f.decide(permission, true).await;
        let response = task.await.unwrap();
        assert!(
            !response.status().is_success(),
            "Stale approval accepted: {mutation}"
        );
        let expected = if mutation == "caller-policy" {
            "Agent policy changed; retry"
        } else {
            "Unarchive branch or policy changed; review and retry"
        };
        assert_eq!(response.json::<Value>().await.unwrap()["message"], expected);
        f.retained(&before, false, turns).await;
    }
    f.stop().await;
}

#[tokio::test]
async fn claude_native_receipt_ack_and_lost_result_notice_consumption() {
    let server = Server::new(false).await;
    let session = server.launch(Harness::ClaudeCode).await;
    let snapshot = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let snapshot = server.snapshot().await;
            if snapshot.submissions.iter().any(|s| {
                s.session_id == session
                    && s.id.starts_with("relay-notice-spawn-")
                    && s.state == SubmissionState::Completed
            }) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Fake Claude did not consume the recovery notice");
    let notices = snapshot
        .submissions
        .iter()
        .filter(|s| s.session_id == session && s.id.starts_with("relay-notice-spawn-"))
        .collect::<Vec<_>>();
    assert_eq!(
        notices.len(),
        1,
        "The acknowledged receipt must not cause a second notice"
    );
    assert!(
        !snapshot
            .sessions
            .iter()
            .any(|s| s.parent_session_id.as_deref() == Some(&session)),
        "Rejected attempts created no children"
    );
    assert_eq!(snapshot.sessions.iter().filter(|s| !s.fixture).count(), 1);
    assert!(snapshot.messages.iter().any(|m| {
        m.session_id == session
            && m.author == "Claude Code"
            && m.tool.as_ref().is_some_and(|t| {
                t.name == "mcp__relay__spawn_agent" && t.state == ToolState::Failed
            })
    }));
    assert!(
        snapshot
            .messages
            .iter()
            .any(|m| m.session_id == session && m.body == "Recovery notice received")
    );
    let connection = rusqlite::Connection::open_with_flags(
        server.directory.path().join("db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let delivered: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM spawn_attempts WHERE caller=?1 AND delivered=1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    let undelivered: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM spawn_attempts WHERE caller=?1 AND delivered=0",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    let consumed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM spawn_notice_consumption WHERE caller=?1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((delivered, undelivered, consumed), (1, 1, 1));
    assert_eq!(
        snapshot
            .messages
            .iter()
            .filter(|m| m.id == format!("prompt-{}", notices[0].id))
            .count(),
        1
    );
    drop(connection);
    server.stop().await;
}

#[tokio::test]
async fn mentioned_peer_is_listed_and_messaged_through_stdio_mcp() {
    let server = Server::new(false).await;
    let first = server.launch(Harness::Codex).await;
    let snapshot = server.terminal(&first).await;
    let title = &snapshot
        .sessions
        .iter()
        .find(|s| s.id == first)
        .unwrap()
        .title;
    let mention = Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::SessionMention {
            mention: SessionMention {
                session_id: first.clone(),
                label: mention_label("MCP boundary", title),
            },
        },
    };
    let second = server
        .start(
            Harness::Codex,
            "Peer boundary",
            vec![Part::text("Message "), mention],
        )
        .await;
    let snapshot = server.terminal(&second).await;
    assert!(snapshot.is_peer(&first, &second));
    let list = std::fs::read_to_string(server.directory.path().join("peer-list")).unwrap();
    let list = list
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|value| value["id"] == 4)
        .unwrap();
    let content: Value =
        serde_json::from_str(list["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["agents"][0]["id"], second);
    let peer = content["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["relationship"] == "peer")
        .unwrap();
    assert_eq!(peer["id"], first);
    assert_eq!(peer["project"], "MCP boundary");
    assert!(peer.get("worktree").is_none() && peer.get("result").is_none());
    let sent = std::fs::read_to_string(server.directory.path().join("peer-message")).unwrap();
    let sent: Value = serde_json::from_str(sent.trim()).unwrap();
    assert_eq!(sent["result"]["isError"], false, "{sent}");
    let snapshot = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let snapshot = server.snapshot().await;
            if snapshot.submissions.iter().any(|s| {
                s.session_id == first
                    && s.sender
                        == Sender::Agent {
                            session_id: second.clone(),
                        }
            }) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("Peer message was not queued");
    assert!(snapshot.messages.iter().any(|m| {
        m.session_id == second
            && m.tool
                .as_ref()
                .is_some_and(|t| t.name.ends_with("message_agent"))
    }));
    server.terminal(&first).await;
    server.stop().await;
}

#[tokio::test]
#[ignore = "Requires authenticated Claude Code; intentionally invokes one model turn"]
async fn real_claude_lists_relay_agents() {
    let server = Server::new(true).await;
    let session = server.launch(Harness::ClaudeCode).await;
    let snapshot = server.terminal(&session).await;
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session)
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert!(
        worker.status == WorkerStatus::Completed,
        "Real Claude turn did not complete"
    );
    assert!(
        snapshot.messages.iter().any(|m| m.session_id == session
            && m.tool.as_ref().is_some_and(
                |t| t.name == "mcp__relay__list_agents" && t.state == ToolState::Completed
            )),
        "Real Claude did not complete Relay list_agents"
    );
    server.stop().await;
}

async fn control_http(server: &Server, envelope: &CommandEnvelope) -> reqwest::Response {
    server
        .client
        .post(format!("{}/v1/commands", server.endpoint))
        .bearer_auth(&server.token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .json(envelope)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap()
}

async fn control_permission(server: &Server, session: &str) -> ToolPermission {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(permission) = server
                .snapshot()
                .await
                .tool_permissions
                .into_iter()
                .find(|p| p.session_id == session && p.decision.is_none() && !p.expired)
            {
                return permission;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("scripted native approval must become pending")
}

fn control_phase_count(server: &Server, phase: &str) -> usize {
    std::fs::read(server.directory.path().join(phase))
        .map_or(0, |b| b.iter().filter(|b| **b == b'\n').count())
}

async fn control_terminal(server: &Server, session: &str, phase: &str) -> Snapshot {
    eprintln!(
        "control_stop phase={phase} waiting=true interrupts={}",
        control_phase_count(server, "control-interrupt")
    );
    let mut last = None;
    let terminal = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let snapshot = server.snapshot().await;
            let status = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session)
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .status
                .clone();
            last = Some(status.clone());
            if !matches!(status, WorkerStatus::Running | WorkerStatus::Queued) {
                assert_eq!(
                    status,
                    WorkerStatus::Stopped,
                    "accepted stop must reach Stopped"
                );
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    // Only fixed phase labels, enum status and synthetic adapter counters. Never
    // print snapshots, messages, capabilities, environment or native raw output.
    eprintln!(
        "control_stop phase={phase} status={last:?} accepted={} declined={} interrupts={}",
        control_phase_count(server, "control-accepted"),
        control_phase_count(server, "control-declined"),
        control_phase_count(server, "control-interrupt")
    );
    terminal.expect("accepted stop did not reach terminal state; synthetic phase counters above")
}

fn control_target(snapshot: &Snapshot, session: &str) -> serde_json::Value {
    // Ignore unrelated traffic but check the decision, worker, delivered prompts
    // and queue of the target. Rejection must not change any of them.
    serde_json::json!({
        "worker": snapshot.sessions.iter().find(|s| s.id == session).and_then(|s| s.worker.as_ref()),
        "permissions": snapshot.tool_permissions.iter().filter(|p| p.session_id == session).collect::<Vec<_>>(),
        "prompts": snapshot.messages.iter().filter(|m| m.session_id == session && m.kind == "prompt").collect::<Vec<_>>(),
        "submissions": snapshot.submissions.iter().filter(|s| s.session_id == session).collect::<Vec<_>>()
    })
}

async fn invalid_control(server: &Server, session: &str, request: &CommandEnvelope, message: &str) {
    let before = control_target(&server.snapshot().await, session);
    let response = control_http(server, request).await;
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let error: ApiError = response.json().await.unwrap();
    assert_eq!(error.code, "invalid");
    assert_eq!(error.message, message);
    assert_eq!(control_target(&server.snapshot().await, session), before);
}

#[tokio::test]
async fn controls_survive_agent_traffic_but_reject_changed_targets_and_edits() {
    let server = Server::new(false).await;
    // Scripted app-server traffic only: no provider/model or production state.
    std::fs::write(server.directory.path().join("fake-codex"), r#"#!/bin/sh
set -eu
umask 077
while IFS= read -r line; do
  method=$(printf '%s' "$line" | jq -r '.method // ""')
  id=$(printf '%s' "$line" | jq -c '.id')
  case "$method" in
    initialize) printf '{"id":%s,"result":{}}\n' "$id" ;;
    model/list) printf '{"id":%s,"result":{"data":[{"id":"test-model","displayName":"Fixture"}]}}\n' "$id" ;;
    skills/list|mcpServerStatus/list) printf '{"id":%s,"result":{"data":[]}}\n' "$id" ;;
    thread/start|thread/resume) printf '{"id":%s,"result":{"model":"test-model","thread":{"id":"control-thread"}}}\n' "$id" ;;
    turn/start)
      printf '{"id":%s,"result":{"turn":{"id":"control-turn"}}}\n' "$id"
      printf '%s\n' '{"method":"turn/started","params":{"threadId":"control-thread","turn":{"id":"control-turn"}}}'
      if printf '%s' "$line" | jq -e '[.params.input[]? | select(.type=="text") | .text] | any(. == "APPROVAL_FIXTURE")' >/dev/null; then
        printf '%s\n' '{"id":"control-approval","method":"item/commandExecution/requestApproval","params":{"threadId":"control-thread","command":"fixture only"}}'
      else
        printf '%s\n' '{"method":"item/started","params":{"threadId":"control-thread","item":{"id":"traffic","type":"agentMessage","text":""}}}'
        count=0
        while [ "$count" -lt 30 ]; do
          printf '%s\n' '{"method":"item/agentMessage/delta","params":{"threadId":"control-thread","itemId":"traffic","delta":"fixture-traffic "}}'
          count=$((count + 1))
          sleep 0.02
        done
      fi ;;
    turn/interrupt)
      if ! printf '%s' "$line" | jq -e '.params.threadId == "control-thread" and .params.turnId == "control-turn"' >/dev/null; then
        printf '{"id":%s,"error":{"code":-32602,"message":"Invalid fixture turn target"}}\n' "$id"
        continue
      fi
      printf '%s\n' 1 >> "$RELAY_BOUNDARY_DIR/control-interrupt"
      printf '{"id":%s,"result":{}}\n' "$id"
      printf '%s\n' '{"method":"turn/completed","params":{"threadId":"control-thread","turn":{"id":"control-turn","status":"interrupted"}}}'
      exit ;;
    "")
      if printf '%s' "$line" | jq -e '.id == "control-approval" and .result.decision == "accept"' >/dev/null; then
        printf '%s\n' 1 >> "$RELAY_BOUNDARY_DIR/control-accepted"
      elif printf '%s' "$line" | jq -e '.id == "control-approval" and .result.decision == "decline"' >/dev/null; then
        printf '%s\n' 1 >> "$RELAY_BOUNDARY_DIR/control-declined"
      fi ;;
  esac
done
"#).unwrap();
    // Configuration edits deliberately remain revision protected. Finish setup
    // before any agent publishes unrelated events; only the control commands
    // below are meant to cross the concurrent event stream.
    eprintln!("fixture_setup phase=prepare_controls");
    let controls_setup = server.prepare(Harness::Codex, "Controls").await;
    eprintln!("fixture_setup phase=prepare_traffic");
    let traffic_setup = server.prepare(Harness::Codex, "Traffic").await;
    eprintln!("fixture_setup phase=prepare_pending_stop");
    let blocked_setup = server
        .prepare(Harness::Codex, "Stop pending approval")
        .await;
    eprintln!("fixture_setup phase=start_controls");
    let parent = server
        .start_prepared(controls_setup, vec![Part::text("APPROVAL_FIXTURE")])
        .await;
    let permission = control_permission(&server, &parent).await;
    let rendered = server.snapshot().await;
    let request = |command| CommandEnvelope {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: rendered.revision,
        command,
    };
    eprintln!("fixture_setup phase=start_traffic");
    let traffic = server
        .start_prepared(traffic_setup, vec![Part::text("TRAFFIC_FIXTURE")])
        .await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = server.snapshot().await;
            if current
                .messages
                .iter()
                .any(|m| m.session_id == traffic && m.body.matches("fixture-traffic").count() >= 2)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("actual adapter events must advance state before control submission");
    // Stop while the app-server loop is inside the inline permission waiter.
    // The waiter may consume the stop notification before returning; the outer
    // loop must still observe its true value and issue the native interrupt.
    eprintln!("fixture_setup phase=start_pending_stop");
    let blocked = server
        .start_prepared(blocked_setup, vec![Part::text("APPROVAL_FIXTURE")])
        .await;
    let blocked_permission = control_permission(&server, &blocked).await;
    let interrupts = control_phase_count(&server, "control-interrupt");
    let declined = control_phase_count(&server, "control-declined");
    let blocked_stop = request(Command::StopWorkerRuns {
        session_id: blocked.clone(),
        include_descendants: false,
        runs: [(blocked.clone(), blocked_permission.run_id)].into(),
        branch: vec![blocked.clone()],
    });
    assert!(
        control_http(&server, &blocked_stop)
            .await
            .status()
            .is_success()
    );
    let stopped = control_terminal(&server, &blocked, "pending-approval").await;
    assert_eq!(
        control_phase_count(&server, "control-interrupt"),
        interrupts + 1
    );
    assert_eq!(
        control_phase_count(&server, "control-declined"),
        declined + 1
    );
    assert!(
        stopped
            .tool_permissions
            .iter()
            .any(|p| p.id == permission.id && p.decision.is_none() && !p.expired),
        "stopping another run cannot decide or expire the original permission"
    );
    let approve = request(Command::RespondPermission {
        permission_id: permission.id.clone(),
        run_id: permission.run_id.clone(),
        allow: true,
    });
    assert!(control_http(&server, &approve).await.status().is_success());
    assert!(
        control_http(&server, &approve).await.status().is_success(),
        "exact retry deduplicates even after decision"
    );
    let changed_identity = CommandEnvelope {
        command: Command::RespondPermission {
            permission_id: permission.id.clone(),
            run_id: permission.run_id.clone(),
            allow: false,
        },
        ..approve.clone()
    };
    invalid_control(
        &server,
        &parent,
        &changed_identity,
        "request_id was already used for a different command",
    )
    .await;
    let stale_permission = request(Command::RespondPermission {
        permission_id: permission.id,
        run_id: "superseded-run".into(),
        allow: true,
    });
    invalid_control(
        &server,
        &parent,
        &stale_permission,
        "Permission request is no longer pending",
    )
    .await;
    let project = rendered
        .sessions
        .iter()
        .find(|s| s.id == parent)
        .unwrap()
        .project_id
        .clone();
    let projects_before = serde_json::to_value(server.snapshot().await.projects).unwrap();
    let rejected_edit = control_http(
        &server,
        &request(Command::RenameProject {
            project_id: project,
            name: "stale edit".into(),
        }),
    )
    .await;
    assert_eq!(rejected_edit.status(), reqwest::StatusCode::CONFLICT);
    let error: ApiError = rejected_edit.json().await.unwrap();
    assert_eq!(error.code, "conflict");
    assert_eq!(
        error.message,
        "Workspace changed. Refresh and review your draft before saving again."
    );
    assert_eq!(
        serde_json::to_value(server.snapshot().await.projects).unwrap(),
        projects_before
    );
    let stop = |run: &str| {
        request(Command::StopWorkerRuns {
            session_id: parent.clone(),
            include_descendants: false,
            runs: [(parent.clone(), run.into())].into(),
            branch: vec![parent.clone()],
        })
    };
    invalid_control(
        &server,
        &parent,
        &stop("superseded-run"),
        "Active turns changed; review before stopping",
    )
    .await;
    let stop_first = stop(&permission.run_id);
    let mut wrong_branch = stop_first.clone();
    wrong_branch.request_id = uuid::Uuid::new_v4().to_string();
    if let Command::StopWorkerRuns { branch, .. } = &mut wrong_branch.command {
        branch.push("not-the-reviewed-branch".into());
    }
    invalid_control(
        &server,
        &parent,
        &wrong_branch,
        "Active turns changed; review before stopping",
    )
    .await;
    assert!(
        control_http(&server, &stop_first)
            .await
            .status()
            .is_success()
    );
    control_terminal(&server, &parent, "approved-run").await;
    let send = request(Command::SendWorker {
        session_id: parent.clone(),
        prompt: "APPROVAL_FIXTURE".into(),
        approve_implementation: true,
    });
    assert!(
        control_http(&server, &send).await.status().is_success(),
        "send validates current target despite old workspace revision"
    );
    assert!(control_http(&server, &send).await.status().is_success());
    let next = control_permission(&server, &parent).await;
    assert_ne!(next.run_id, permission.run_id);
    invalid_control(
        &server,
        &parent,
        &stop(&permission.run_id),
        "Active turns changed; review before stopping",
    )
    .await;
    let interrupts_before_receipt_retry = control_phase_count(&server, "control-interrupt");
    assert!(
        control_http(&server, &stop_first)
            .await
            .status()
            .is_success(),
        "old accepted stop retry is a receipt lookup, not another stop"
    );
    assert_eq!(
        control_phase_count(&server, "control-interrupt"),
        interrupts_before_receipt_retry
    );
    let current = server.snapshot().await;
    assert!(
        current
            .sessions
            .iter()
            .find(|s| s.id == parent)
            .unwrap()
            .worker
            .as_ref()
            .is_some_and(|w| matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued))
    );
    assert_eq!(
        current
            .messages
            .iter()
            .filter(|m| m.id == format!("prompt-{}", send.request_id))
            .count(),
        1
    );
    let draft = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        parts: vec![Part::text("draft one")],
    };
    let draft_url = format!("{}/v1/drafts/{parent}", server.endpoint);
    assert!(
        server
            .client
            .post(&draft_url)
            .bearer_auth(&server.token)
            .header("x-relay-protocol", PROTOCOL_VERSION)
            .json(&draft)
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let conflicting_draft = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        parts: vec![Part::text("draft two")],
        ..draft
    };
    let rejected_draft = server
        .client
        .post(draft_url)
        .bearer_auth(&server.token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .json(&conflicting_draft)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected_draft.status(), reqwest::StatusCode::CONFLICT);
    let error: ApiError = rejected_draft.json().await.unwrap();
    assert_eq!(error.code, "draft_conflict");
    assert_eq!(
        error.message,
        "Shared draft changed on another client; your local draft is retained"
    );
    let drafts: Vec<Draft> = server
        .client
        .get(format!("{}/v1/drafts", server.endpoint))
        .bearer_auth(&server.token)
        .header("x-relay-protocol", PROTOCOL_VERSION)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let retained = drafts.iter().find(|d| d.session_id == parent).unwrap();
    assert_eq!(retained.revision, 1);
    assert_eq!(plain_text(&retained.parts), "draft one");
    assert!(
        control_http(&server, &stop(&next.run_id))
            .await
            .status()
            .is_success()
    );
    control_terminal(&server, &parent, "replacement-run").await;
    // Corrupt/legacy Queued state is seeded only in this isolated fixture.
    // It must be a target rejection, never a 500 or a guessed cancellation.
    let mut queued = server.snapshot().await;
    queued
        .sessions
        .iter_mut()
        .find(|s| s.id == parent)
        .unwrap()
        .worker
        .as_mut()
        .unwrap()
        .status = WorkerStatus::Queued;
    queued.revision += 1;
    let db = rusqlite::Connection::open(server.directory.path().join("db")).unwrap();
    db.execute("DELETE FROM runs WHERE session_id=?1", [&parent])
        .unwrap();
    db.execute(
        "UPDATE workspace SET snapshot=?1 WHERE id=1",
        [serde_json::to_string(&queued).unwrap()],
    )
    .unwrap();
    invalid_control(
        &server,
        &parent,
        &stop(&next.run_id),
        "Active turn identity is unavailable; refresh before stopping",
    )
    .await;
    server.stop().await;
}
