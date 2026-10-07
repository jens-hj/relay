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
async fn authentication_covers_reads_writes_and_event_upgrade() {
    let db = tempfile::tempdir().unwrap();
    let server = start(&db.path().join("relay.sqlite3")).await;
    let client = reqwest::Client::new();
    for path in ["snapshot", "events"] {
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
