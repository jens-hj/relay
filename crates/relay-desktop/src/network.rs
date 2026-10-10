#[cfg(not(target_arch = "wasm32"))]
use futures_util::{SinkExt, StreamExt};
use mosaic::prelude::StateSender;
use relay_core::{ApiError, CommandEnvelope, Snapshot};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use tokio::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

#[derive(Clone)]
pub struct Config {
    pub endpoint: reqwest::Url,
    #[cfg(not(target_arch = "wasm32"))]
    pub token: String,
}

impl Config {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_env() -> Result<Self, String> {
        let endpoint =
            std::env::var("RELAY_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:7331/".into());
        let endpoint = reqwest::Url::parse(&format!("{}/", endpoint.trim_end_matches('/')))
            .map_err(|_| "RELAY_ENDPOINT must be an HTTP(S) URL")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(
                "RELAY_ENDPOINT must be an HTTP(S) URL without credentials, a query, or a fragment"
                    .into(),
            );
        }
        let token = std::env::var("RELAY_TOKEN")
            .map_err(|_| "Set RELAY_TOKEN to the server's workspace token")?;
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "RELAY_TOKEN is not a valid header value")?;
        Ok(Self { endpoint, token })
    }
    #[cfg(target_arch = "wasm32")]
    pub fn from_env() -> Result<Self, String> {
        let origin = web_sys::window()
            .ok_or("Browser window unavailable")?
            .location()
            .origin()
            .map_err(|_| "Cannot read server origin")?;
        Ok(Self {
            endpoint: reqwest::Url::parse(&format!("{origin}/"))
                .map_err(|_| "Invalid server origin")?,
        })
    }
    pub(crate) fn url(&self, path: &str) -> reqwest::Url {
        self.endpoint
            .join(path)
            .expect("constant relative API path")
    }
}

#[derive(Clone, Default)]
pub struct NetworkState {
    pub server_build: Option<relay_core::BuildInfo>,
    pub harnesses: Vec<relay_core::HarnessStatus>,
    pub harness_error: String,
    pub snapshot: Snapshot,
    pub connected: bool,
    pub status: String,
    pub round_trip_ms: Option<u64>,
    pub outcome: Option<(String, Result<(), String>)>,
    pub outcome_serial: u64,
    pub outcome_conflict: bool,
    pub outcome_ambiguous: bool,
}

enum Event {
    Build(Option<relay_core::BuildInfo>),
    Harnesses(Result<Vec<relay_core::HarnessStatus>, String>),
    Snapshot(Snapshot),
    Status(bool, String),
    RoundTrip(u64),
    Outcome(String, Result<Snapshot, String>, bool, bool),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start(
    config: Config,
    ui: StateSender<NetworkState>,
) -> (
    mpsc::UnboundedSender<CommandEnvelope>,
    mpsc::UnboundedSender<()>,
) {
    let (commands, receiver) = mpsc::unbounded_channel();
    let (refresh, mut refreshes) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("network runtime");
        runtime.block_on(async move {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client");
            let (events, mut updates) = mpsc::unbounded_channel();
            let stream = tokio::spawn(stream(config.clone(), client.clone(), events.clone()));
            let status_config = config.clone();
            let status_client = client.clone();
            let status_events = events.clone();
            let status_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                loop {
                    let refresh = tokio::select! { _ = interval.tick() => false, Some(()) = refreshes.recv() => true };
                    let result = async {
                        let response = if refresh { status_client.post(status_config.url("v1/harnesses/refresh")) } else {status_client.get(status_config.url("v1/harnesses"))}
                            .bearer_auth(&status_config.token)
                            .header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string())
                            .send()
                            .await
                            .map_err(|_| "Harness status unavailable".to_owned())?;
                        if !response.status().is_success() {
                            return Err("Server could not check harness status".to_owned());
                        }
                        response
                            .json()
                            .await
                            .map_err(|_| "Incompatible harness status".to_owned())
                    }
                    .await;
                    if status_events.send(Event::Harnesses(result)).is_err() {
                        break;
                    }
                    let build = match status_client.get(status_config.url("v1/version"))
                        .bearer_auth(&status_config.token).send().await {
                        Ok(response) if response.status().is_success() => response.json().await.ok(),
                        _ => None,
                    };
                    if status_events.send(Event::Build(build)).is_err() { break; }
                }
            });
            let writer = tokio::spawn(write(config, client, receiver, events));
            let mut state = NetworkState {
                status: "Connecting…".into(),
                ..Default::default()
            };
            while let Some(event) = updates.recv().await {
                match event {
                    Event::Build(build) => state.server_build = build,
                    Event::Harnesses(result) => match result {
                        Ok(statuses) => {
                            state.harnesses = statuses;
                            state.harness_error.clear();
                        }
                        Err(error) => state.harness_error = error,
                    },
                    Event::Snapshot(snapshot) => replace_snapshot(&mut state, snapshot),
                    Event::Status(connected, status) => {
                        state.connected = connected;
                        state.status = status;
                        if !connected {
                            state.round_trip_ms = None;
                        }
                    }
                    Event::RoundTrip(round_trip_ms) => {
                        state.round_trip_ms = Some(round_trip_ms);
                    }
                    Event::Outcome(id, result, conflict, ambiguous) => {
                        state.outcome_conflict = conflict;
                        state.outcome_ambiguous = ambiguous;
                        state.outcome_serial += 1;
                        state.outcome = Some((
                            id,
                            match result {
                                Ok(snapshot) => {
                                    replace_snapshot(&mut state, snapshot);
                                    Ok(())
                                }
                                Err(error) => Err(error),
                            },
                        ));
                    }
                }
                if ui.send(state.clone()).is_err() {
                    break;
                }
            }
            stream.abort();
            writer.abort();
            status_task.abort();
        });
    });
    (commands, refresh)
}

fn replace_snapshot(state: &mut NetworkState, snapshot: Snapshot) {
    if snapshot.revision >= state.snapshot.revision {
        state.snapshot = snapshot;
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn read(client: &reqwest::Client, config: &Config) -> Result<Snapshot, String> {
    decode(
        client
            .get(config.url("v1/snapshot"))
            .bearer_auth(&config.token)
            .send()
            .await
            .map_err(|_| "Cannot reach the Relay server".to_string())?,
    )
    .await
}

fn validate_protocol(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.protocol_version != relay_core::PROTOCOL_VERSION {
        return Err(format!(
            "Incompatible Relay server protocol {}. This client requires protocol {}. Update the server before making changes.",
            snapshot.protocol_version,
            relay_core::PROTOCOL_VERSION
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn decode(response: reqwest::Response) -> Result<Snapshot, String> {
    if response.status().is_success() {
        let snapshot: Snapshot = response
            .json()
            .await
            .map_err(|_| "Server returned an incompatible snapshot".to_owned())?;
        validate_protocol(&snapshot)?;
        return Ok(snapshot);
    }
    let status = response.status();
    Err(response
        .json::<ApiError>()
        .await
        .map(|e| e.message)
        .unwrap_or_else(|_| format!("Server rejected the request ({status})")))
}

#[cfg(not(target_arch = "wasm32"))]
async fn stream(config: Config, client: reqwest::Client, events: mpsc::UnboundedSender<Event>) {
    let mut delay = 1;
    loop {
        let result = async {
            let snapshot = read(&client, &config).await?;
            events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
            let mut url = config.url("v1/events");
            let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
            url.set_scheme(scheme).map_err(|_| "Invalid event endpoint")?;
            let mut request = url.as_str().into_client_request().map_err(|_| "Invalid event request")?;
            request.headers_mut().insert("authorization", HeaderValue::from_str(&format!("Bearer {}", config.token)).map_err(|_| "Invalid token")?);
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(request)).await.map_err(|_| "Event connection timed out")?.map_err(|_| "Event connection failed")?;
            delay = 1;
            events.send(Event::Status(true, "Connected".into())).map_err(|_| "Client closed")?;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
            let mut last_response = tokio::time::Instant::now();
            let mut ping_sent = None;
            loop {
                tokio::select! {
                    incoming = socket.next() => match incoming {
                        Some(Ok(Message::Text(json))) => {
                            last_response = tokio::time::Instant::now();
                            let snapshot = serde_json::from_str(&json).map_err(|_| "Invalid server event")?;
                            validate_protocol(&snapshot)?;
                            events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
                        },
                        Some(Ok(Message::Ping(data))) => { last_response = tokio::time::Instant::now(); socket.send(Message::Pong(data)).await.map_err(|_| "Event connection lost")?; },
                        Some(Ok(Message::Pong(_))) => {
                            let received = tokio::time::Instant::now();
                            last_response = received;
                            if let Some(sent) = ping_sent.take() {
                                let round_trip_ms = received.duration_since(sent).as_millis() as u64;
                                events.send(Event::RoundTrip(round_trip_ms)).map_err(|_| "Client closed")?;
                            }
                        },
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err::<(), String>("Connection lost".into()),
                        _ => {},
                    },
                    _ = heartbeat.tick() => {
                        if last_response.elapsed() > Duration::from_secs(45) { return Err("Event connection stopped responding".into()); }
                        if ping_sent.is_none() {
                            socket.send(Message::Ping(Vec::new().into())).await.map_err(|_| "Event connection lost")?;
                            ping_sent = Some(tokio::time::Instant::now());
                        }
                    },
                }
            }
        }.await;
        if let Err(message) = result
            && events
                .send(Event::Status(
                    false,
                    format!("{message} · retrying in {delay}s"),
                ))
                .is_err()
        {
            break;
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(15);
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn write(
    config: Config,
    client: reqwest::Client,
    mut commands: mpsc::UnboundedReceiver<CommandEnvelope>,
    events: mpsc::UnboundedSender<Event>,
) {
    while let Some(command) = commands.recv().await {
        let endpoint = if matches!(
            command.command,
            relay_core::Command::SubmitTurn { .. } | relay_core::Command::EditQueuedTurn { .. }
        ) {
            "v1/conversation/commands"
        } else {
            "v1/commands"
        };
        let (result, conflict, ambiguous) = match client.post(config.url(endpoint)).bearer_auth(&config.token).header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string()).json(&command).send().await {
            Ok(response) => {
                let status = response.status();
                let result = decode(response).await;
                let ambiguous = result.is_err() && (status.is_success() || status.is_server_error());
                (result, status == reqwest::StatusCode::CONFLICT, ambiguous)
            }
            Err(_) => (Err("Request could not be confirmed. Your draft is retained; exact retry preserves the original request ID and revision.".into()), false, true),
        };
        if result.is_err()
            && let Ok(snapshot) = read(&client, &config).await
        {
            let _ = events.send(Event::Snapshot(snapshot));
        }
        if events
            .send(Event::Outcome(
                command.request_id,
                result,
                conflict,
                ambiguous,
            ))
            .is_err()
        {
            break;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn websocket_heartbeat_publishes_round_trip_time() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut http, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut buffer = [0; 1024];
                let length = http.read(&mut buffer).await.unwrap();
                assert_ne!(length, 0);
                request.extend_from_slice(&buffer[..length]);
            }
            let body = serde_json::to_string(&Snapshot {
                protocol_version: relay_core::PROTOCOL_VERSION,
                ..Default::default()
            })
            .unwrap();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();

            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let Some(Ok(Message::Ping(data))) = socket.next().await else {
                panic!("expected client heartbeat ping");
            };
            socket.send(Message::Pong(data)).await.unwrap();
        });

        let config = Config {
            endpoint: reqwest::Url::parse(&format!("http://{address}/")).unwrap(),
            token: "test-only".into(),
        };
        let client = reqwest::Client::new();
        let (events, mut updates) = mpsc::unbounded_channel();
        let task = tokio::spawn(stream(config, client, events));
        let round_trip = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(Event::RoundTrip(round_trip)) = updates.recv().await {
                    break round_trip;
                }
            }
        })
        .await
        .expect("heartbeat round trip should arrive");
        assert!(round_trip < 2_000);
        task.abort();
        server.await.unwrap();
    }

    #[test]
    fn rejects_old_servers_without_rejecting_current_protocol() {
        assert!(
            validate_protocol(&Snapshot::default())
                .unwrap_err()
                .contains("requires protocol 3")
        );
        assert!(
            validate_protocol(&Snapshot {
                protocol_version: relay_core::PROTOCOL_VERSION,
                ..Default::default()
            })
            .is_ok()
        );
    }
    #[test]
    fn late_responses_never_replace_newer_state() {
        let mut state = NetworkState {
            snapshot: Snapshot {
                revision: 8,
                ..Default::default()
            },
            ..Default::default()
        };
        replace_snapshot(
            &mut state,
            Snapshot {
                revision: 7,
                ..Default::default()
            },
        );
        assert_eq!(state.snapshot.revision, 8);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod outcome_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[tokio::test]
    async fn writer_distinguishes_conflict_rejection_and_ambiguous_acknowledgements() {
        for (status, conflict, ambiguous) in [
            (Some(409), true, false),
            (Some(400), false, false),
            (Some(500), false, true),
            (Some(200), false, true),
            (None, false, true),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                for first in [true, false] {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let mut buffer = [0; 1024];
                        let length = socket.read(&mut buffer).unwrap();
                        assert_ne!(length, 0);
                        request.extend_from_slice(&buffer[..length]);
                    }
                    if first {
                        assert!(
                            String::from_utf8_lossy(&request)
                                .to_lowercase()
                                .contains("x-relay-protocol: 3")
                        );
                    }
                    if first && status.is_none() {
                        continue;
                    }
                    let code = if first { status.unwrap() } else { 200 };
                    let body = if first {
                        "invalid or rejected response".into()
                    } else {
                        serde_json::to_string(&Snapshot {
                            protocol_version: relay_core::PROTOCOL_VERSION,
                            ..Default::default()
                        })
                        .unwrap()
                    };
                    write!(socket, "HTTP/1.1 {code} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
            });
            let config = Config {
                endpoint: reqwest::Url::parse(&format!("http://{address}/")).unwrap(),
                token: "test-only".into(),
            };
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            let (commands, receiver) = mpsc::unbounded_channel();
            commands
                .send(CommandEnvelope {
                    request_id: "request-test".into(),
                    expected_revision: 4,
                    command: relay_core::Command::SyncProject {
                        project_id: "project-test".into(),
                    },
                })
                .unwrap();
            drop(commands);
            let (events, mut updates) = mpsc::unbounded_channel();
            write(config, client, receiver, events).await;
            assert!(matches!(updates.recv().await, Some(Event::Snapshot(_))));
            let Some(Event::Outcome(id, result, actual_conflict, actual_ambiguous)) =
                updates.recv().await
            else {
                panic!("Missing classified outcome")
            };
            assert_eq!(id, "request-test");
            assert!(result.is_err());
            assert_eq!((actual_conflict, actual_ambiguous), (conflict, ambiguous));
            server.join().unwrap();
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[path = "network_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
