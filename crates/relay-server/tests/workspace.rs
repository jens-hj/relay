use futures_util::StreamExt;
use relay_core::*;
use std::{path::Path, time::Duration};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

const TOKEN: &str = "relay-integration-test-token";
struct Server {
    endpoint: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start(path: &Path) -> Server {
    let app = relay_server::router(path, TOKEN.into(), DirectorProfile::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async {
        axum::serve(listener, app).await.unwrap();
    });
    Server { endpoint, task }
}
async fn snapshot(server: &Server) -> Snapshot {
    reqwest::Client::new()
        .get(format!("{}/v1/snapshot", server.endpoint))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}
async fn send(server: &Server, envelope: &CommandEnvelope) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/v1/commands", server.endpoint))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .json(envelope)
        .send()
        .await
        .unwrap()
}
fn envelope(revision: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: revision,
        command,
    }
}
fn comment() -> Command {
    Command::AddComment {
        message_id: "m2".into(),
        quote: Some("project defaults".into()),
        author: "Reviewer".into(),
        body: "Keep explicit overrides intact.".into(),
    }
}

#[tokio::test]
async fn protocol_guard_and_multiple_local_projects_round_trip_without_remote_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(&dir.path().join("db")).await;
    let initial = snapshot(&server).await;
    assert_eq!(initial.protocol_version, PROTOCOL_VERSION);
    let old = reqwest::Client::new()
        .post(format!("{}/v1/commands", server.endpoint))
        .bearer_auth(TOKEN)
        .json(&envelope(initial.revision, comment()))
        .send()
        .await
        .unwrap();
    assert_eq!(old.status(), 409);
    assert_eq!(
        old.json::<ApiError>().await.unwrap().code,
        "protocol_mismatch"
    );
    assert_eq!(snapshot(&server).await, initial);
    let mut current = initial;
    for name in ["One", "Two"] {
        let request = envelope(
            current.revision,
            Command::CreateProject {
                name: name.into(),
                root: dir.path().join(name).display().to_string(),
                connections: vec![],
            },
        );
        current = send(&server, &request)
            .await
            .json::<Snapshot>()
            .await
            .unwrap();
        assert_eq!(
            send(&server, &request)
                .await
                .json::<Snapshot>()
                .await
                .unwrap(),
            current
        );
    }
    assert_eq!(current.projects.iter().filter(|p| !p.fixture).count(), 2);
    let project = current
        .projects
        .iter()
        .find(|p| p.name == "One")
        .unwrap()
        .id
        .clone();
    let board = current
        .boards
        .iter()
        .find(|b| b.project_id == project)
        .unwrap()
        .id
        .clone();
    let request = envelope(
        current.revision,
        Command::CreateTask {
            board_id: board.clone(),
            title: "Local".into(),
            body: "No issue provider".into(),
            repository_connection_id: None,
        },
    );
    current = send(&server, &request).await.json().await.unwrap();
    let task = current
        .issues
        .iter()
        .find(|i| i.title == "Local")
        .unwrap()
        .id
        .clone();
    assert!(
        current
            .issues
            .iter()
            .find(|i| i.id == task)
            .unwrap()
            .reference
            .is_none()
    );
    let request = envelope(
        current.revision,
        Command::MoveTask {
            board_id: board.clone(),
            issue_id: task.clone(),
            column_id: "done".into(),
        },
    );
    current = send(&server, &request).await.json().await.unwrap();
    assert_eq!(
        current
            .memberships
            .iter()
            .find(|m| m.board_id == board && m.issue_id == task)
            .unwrap()
            .column_ids,
        vec!["done"]
    );
}

#[tokio::test]
async fn authentication_covers_reads_writes_and_event_upgrade() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    let client = reqwest::Client::new();
    for path in [
        "snapshot",
        "harnesses",
        "events",
        "drafts",
        "drafts/events",
        "assets/unknown",
    ] {
        assert_eq!(
            client
                .get(format!("{}/v1/{path}", server.endpoint))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        client
            .post(format!("{}/v1/harnesses/refresh", server.endpoint))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{}/v1/commands", server.endpoint))
            .json(&envelope(0, comment()))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{}/v1/snapshot", server.endpoint))
            .bearer_auth("wrong-token")
            .header("x-relay-protocol", "2")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(
        relay_server::router(
            db.path().join("bad.sqlite3"),
            "short".into(),
            DirectorProfile::default()
        )
        .is_err()
    );
}

#[tokio::test]
async fn saves_survive_restart_without_reseeding_or_duplicate_retries() {
    let db = tempfile::tempdir().unwrap();
    let path = db.path().join("relay.sqlite3");
    let server = start(&path).await;
    let request = envelope(0, comment());
    let saved: Snapshot = send(&server, &request).await.json().await.unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.comments.len(), 1);
    let repeated: Snapshot = send(&server, &request).await.json().await.unwrap();
    assert_eq!(repeated, saved);
    drop(server);
    let restarted = start(&path).await;
    assert_eq!(snapshot(&restarted).await, saved);
    let repeated: Snapshot = send(&restarted, &request).await.json().await.unwrap();
    assert_eq!(repeated.comments.len(), 1);
}

#[tokio::test]
async fn stale_edits_and_invalid_comments_leave_storage_unchanged() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    assert_eq!(send(&server, &envelope(0, comment())).await.status(), 200);
    assert_eq!(send(&server, &envelope(0, comment())).await.status(), 409);
    let invalid = Command::AddComment {
        message_id: "m2".into(),
        quote: Some("not in the message".into()),
        author: "Reviewer".into(),
        body: "Feedback".into(),
    };
    assert_eq!(send(&server, &envelope(1, invalid)).await.status(), 422);
    let missing = Command::AddComment {
        message_id: "missing".into(),
        quote: None,
        author: "Reviewer".into(),
        body: "Feedback".into(),
    };
    assert_eq!(send(&server, &envelope(1, missing)).await.status(), 422);
    assert_eq!(snapshot(&server).await.comments.len(), 1);
    assert_eq!(snapshot(&server).await.revision, 1);
}

#[tokio::test]
async fn directors_inherit_updated_defaults_while_overrides_stay_specialized() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    let create = envelope(
        0,
        Command::CreateDirector {
            project_id: "demo".into(),
            name: "Release director".into(),
            overrides: ProfileOverrides {
                max_workers: Some(2),
                ..Default::default()
            },
        },
    );
    let created: Snapshot = send(&server, &create).await.json().await.unwrap();
    assert_eq!(created.directors.len(), 3);
    let defaults = DirectorProfile {
        max_workers: 8,
        harness: Harness::ClaudeCode,
        ..Default::default()
    };
    let updated: Snapshot = send(
        &server,
        &envelope(
            1,
            Command::UpdateProjectDefaults {
                project_id: "demo".into(),
                profile: defaults,
            },
        ),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        updated
            .effective_profile(&updated.directors[0])
            .unwrap()
            .max_workers,
        8
    );
    let specialized = updated.effective_profile(&updated.directors[2]).unwrap();
    assert_eq!(specialized.max_workers, 2);
    assert_eq!(specialized.harness, Harness::ClaudeCode);
    let invalid_scope = Command::UpdateDirector {
        director_id: updated.directors[2].id.clone(),
        name: "Release director".into(),
        overrides: ProfileOverrides {
            scope: Some(DirectorScope::Issues {
                issue_ids: vec!["elsewhere".into()],
            }),
            ..Default::default()
        },
    };
    assert_eq!(
        send(&server, &envelope(2, invalid_scope)).await.status(),
        422
    );
}

#[tokio::test]
async fn request_ids_cannot_be_reused_for_other_commands() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    let mut request = envelope(0, comment());
    assert_eq!(send(&server, &request).await.status(), 200);
    request.command = Command::CreateDirector {
        project_id: "demo".into(),
        name: "Different".into(),
        overrides: ProfileOverrides::default(),
    };
    assert_eq!(send(&server, &request).await.status(), 422);
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn connect(server: &Server) -> Socket {
    let mut request = format!("{}/v1/events", server.endpoint.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer relay-integration-test-token"),
    );
    tokio_tungstenite::connect_async(request).await.unwrap().0
}
async fn next(socket: &mut Socket) -> Snapshot {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(&message.into_text().unwrap()).unwrap()
}

#[tokio::test]
async fn shared_draft_revisions_are_independent_idempotent_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drafts.sqlite3");
    let server = start(&db).await;
    let client = reqwest::Client::new();
    let source = snapshot(&server)
        .await
        .messages
        .into_iter()
        .find(|m| m.id == "m2")
        .unwrap();
    let offset = source.body.find("project defaults").unwrap();
    let parts = vec![
        Part::text("Overall feedback\n"),
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Reply {
                anchor: Anchor {
                    message_id: source.id,
                    start: offset,
                    end: offset + 16,
                    quote: "project defaults".into(),
                },
                parts: vec![Part::text("Keep inheritance.")],
            },
        },
    ];
    let request = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        parts: parts.clone(),
    };
    let url = format!("{}/v1/drafts/session-plan", server.endpoint);
    let saved: Draft = client
        .post(&url)
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.parts, parts);
    assert_eq!(snapshot(&server).await.revision, 0);
    let duplicate: Draft = client
        .post(&url)
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved, duplicate);
    let stale = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        parts: vec![Part::text("Other client")],
    };
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .json(&stale)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let invalid = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 1,
        parts: vec![Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Reply {
                anchor: Anchor {
                    message_id: "m2".into(),
                    start: 0,
                    end: 1,
                    quote: "wrong".into(),
                },
                parts: vec![Part::text("No")],
            },
        }],
    };
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
    drop(server);
    let restarted = start(&db).await;
    let drafts: Vec<Draft> = client
        .get(format!("{}/v1/drafts", restarted.endpoint))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(drafts, vec![saved]);
}

#[tokio::test]
async fn inline_assets_are_authenticated_bounded_immutable_and_validated_in_drafts() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(&dir.path().join("assets.sqlite3")).await;
    let client = reqwest::Client::new();
    let id = uuid::Uuid::new_v4().to_string();
    let url = format!("{}/v1/assets/{id}", server.endpoint);
    let bytes = b"hello inline context";
    assert_eq!(
        client
            .post(&url)
            .body(bytes.as_slice())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let asset: Asset = client
        .post(&url)
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .header("x-relay-filename", "notes%20%CE%BB.txt")
        .header("content-type", "application/octet-stream")
        .body(bytes.as_slice())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(asset.name, "notes λ.txt");
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        bytes
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .header("x-relay-filename", "notes%20%CE%BB.txt")
            .body("different")
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
    let mut tampered = asset.clone();
    tampered.name = "other.txt".into();
    let request = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        parts: vec![
            Part::text("Before "),
            Part {
                id: uuid::Uuid::new_v4().to_string(),
                kind: PartKind::Asset { asset: tampered },
            },
            Part::text(" after"),
        ],
    };
    let draft_url = format!("{}/v1/drafts/session-plan", server.endpoint);
    assert_eq!(
        client
            .post(&draft_url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
    let mut valid = request;
    valid.request_id = uuid::Uuid::new_v4().to_string();
    valid.parts[1].kind = PartKind::Asset { asset };
    assert_eq!(
        client
            .post(&draft_url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .json(&valid)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let large_url = format!("{}/v1/assets/{}", server.endpoint, uuid::Uuid::new_v4());
    assert_eq!(
        client
            .post(large_url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .body(vec![0; ASSET_LIMIT + 1])
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let image_url = format!("{}/v1/assets/{}", server.endpoint, uuid::Uuid::new_v4());
    assert_eq!(
        client
            .post(image_url)
            .bearer_auth(TOKEN)
            .header("x-relay-protocol", "2")
            .header("content-type", "image/png")
            .body("invalid PNG")
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
}

#[tokio::test]
async fn two_clients_receive_committed_changes_and_reconnect_to_latest_state() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    let mut first = connect(&server).await;
    let mut second = connect(&server).await;
    assert_eq!(next(&mut first).await.revision, 0);
    assert_eq!(next(&mut second).await.revision, 0);
    assert_eq!(send(&server, &envelope(0, comment())).await.status(), 200);
    let a = next(&mut first).await;
    assert_eq!(a, next(&mut second).await);
    assert_eq!(a.comments.len(), 1);
    drop(first);
    let mut reconnect = connect(&server).await;
    assert_eq!(next(&mut reconnect).await, a);
}

#[tokio::test]
async fn shared_draft_websocket_publishes_and_reconnects_without_workspace_revision_changes() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(&dir.path().join("draft-events.sqlite3")).await;
    let mut upgrade = format!(
        "{}/v1/drafts/events",
        server.endpoint.replace("http://", "ws://")
    )
    .into_client_request()
    .unwrap();
    upgrade.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {TOKEN}")).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(upgrade.clone())
        .await
        .unwrap();
    let initial = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Draft>>(&initial.into_text().unwrap()).unwrap(),
        vec![]
    );
    let parts = vec![Part::text("Shared next turn")];
    let request = SaveDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        parts: parts.clone(),
    };
    let saved: Draft = reqwest::Client::new()
        .post(format!("{}/v1/drafts/session-plan", server.endpoint))
        .bearer_auth(TOKEN)
        .header("x-relay-protocol", "2")
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Draft>>(&event.into_text().unwrap()).unwrap(),
        vec![saved.clone()]
    );
    assert_eq!(snapshot(&server).await.revision, 0);
    socket.close(None).await.unwrap();
    let (mut reopened, _) = tokio_tungstenite::connect_async(upgrade).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), reopened.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Draft>>(&event.into_text().unwrap()).unwrap(),
        vec![saved]
    );
    reopened.close(None).await.unwrap();
}
