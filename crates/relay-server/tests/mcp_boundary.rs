//! Process boundary coverage: real binary, HTTP server, fake harness and stdio MCP.
use relay_core::*;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

struct Server {
    child: tokio::process::Child,
    endpoint: String,
    token: String,
    client: reqwest::Client,
    directory: tempfile::TempDir,
}
impl Server {
    async fn new(real_claude: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
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
      # Neither the scoped secret nor the workspace credential is printed.
      test -z "${RELAY_TOKEN:-}" && test -z "${RELAY_TOKEN_FILE:-}"
      printf '%s' "$RELAY_AGENT_CAPABILITY" > "$RELAY_BOUNDARY_DIR/capability"
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' '{"jsonrpc":"2.0","id":3,"method":"ping"}' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}' '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"archive_agent","arguments":{"agent":"missing"}}}' | "$RELAY_BOUNDARY_SERVER" mcp > "$RELAY_BOUNDARY_DIR/receipt"
      printf '{"id":%s,"result":{"turn":{"id":"turn"}}}\n' "$id"
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
        let mut command = tokio::process::Command::new(binary);
        command
            .env("RELAY_BIND", address.to_string())
            .env("RELAY_DATABASE", directory.path().join("db"))
            .env("RELAY_TOKEN", &token)
            .env_remove("RELAY_TOKEN_FILE")
            .env_remove("RELAY_GITHUB_REPO")
            .env_remove("RELAY_GITHUB_PROJECT_OWNER")
            .env_remove("RELAY_GITHUB_PROJECT_NUMBER")
            .env_remove("RELAY_REPO_PATH")
            .env_remove("RELAY_DEFAULT_PROFILE")
            .env("RELAY_BOUNDARY_DIR", directory.path())
            .env("RELAY_BOUNDARY_SERVER", binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if !real_claude {
            command.env("RELAY_CODEX_BIN", fake);
        }
        let child = command.spawn().unwrap();
        let server = Self {
            child,
            endpoint: format!("http://{address}"),
            token,
            client: reqwest::Client::new(),
            directory,
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if server
                    .client
                    .get(format!("{}/v1/ping", server.endpoint))
                    .bearer_auth(&server.token)
                    .send()
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        server
    }
    async fn snapshot(&self) -> Snapshot {
        self.client
            .get(format!("{}/v1/snapshot", self.endpoint))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn command(&self, command: Command) -> Snapshot {
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
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "Boundary command rejected");
        self.snapshot().await
    }
    async fn launch(&self, harness: Harness) -> String {
        let root = self.directory.path().join("project");
        let snapshot = self
            .command(Command::CreateProject {
                name: "MCP boundary".into(),
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
                    .any(|p| p.id == d.project_id && p.name == "MCP boundary")
            })
            .unwrap();
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
        let snapshot = self.command(Command::StartRootAgent { issue_id:None,agent_config_id,prompt:"Call the Relay MCP tool mcp__relay__list_agents once, then reply with only the agent count. Do not use other tools or inspect environment variables.".into(),parts:vec![],approve_implementation:true }).await;
        snapshot
            .sessions
            .iter()
            .find(|s| !s.fixture)
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
    let values = receipt
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 5);
    assert_eq!(values[0]["result"]["capabilities"]["tools"], json!({}));
    let tools = values[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);
    assert!(tools.iter().any(|tool| tool["name"] == "stop_agent"));
    assert!(tools.iter().any(|tool| tool["name"] == "archive_agent"));
    assert_eq!(values[3]["result"]["isError"], false);
    let content: Value =
        serde_json::from_str(values[3]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["agents"][0]["id"], session);
    assert_eq!(values[4]["result"]["isError"], true);
    let archive: Value =
        serde_json::from_str(values[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(archive["message"], "Agent missing");
    // Read the protected credential solely to verify revoked authentication; never format it.
    let secret = std::fs::read_to_string(server.directory.path().join("capability")).unwrap();
    let response = server
        .client
        .post(format!("{}/v1/agent-tools/list_agents", server.endpoint))
        .bearer_auth(secret)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let response = server
        .client
        .post(format!("{}/v1/agent-tools/list_agents", server.endpoint))
        .bearer_auth(&server.token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
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
