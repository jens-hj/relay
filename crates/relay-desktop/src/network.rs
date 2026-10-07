use futures_util::{SinkExt, StreamExt};
use mosaic::prelude::StateSender;
use relay_core::{ApiError, CommandEnvelope, Snapshot};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

#[derive(Clone)]
pub struct Config {
    pub endpoint: reqwest::Url,
    pub token: String,
}

impl Config {
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
    fn url(&self, path: &str) -> reqwest::Url {
        self.endpoint
            .join(path)
            .expect("constant relative API path")
    }
}

#[derive(Clone, Default)]
pub struct NetworkState {
    pub snapshot: Snapshot,
    pub connected: bool,
    pub status: String,
    pub outcome: Option<(String, Result<(), String>)>,
    pub outcome_serial: u64,
}

enum Event {
    Snapshot(Snapshot),
    Status(bool, String),
    Outcome(String, Result<Snapshot, String>),
}

pub fn start(
    config: Config,
    ui: StateSender<NetworkState>,
) -> mpsc::UnboundedSender<CommandEnvelope> {
    let (commands, receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("network runtime");
        runtime.block_on(async move {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .build()
                .expect("HTTP client");
            let (events, mut updates) = mpsc::unbounded_channel();
            let stream = tokio::spawn(stream(config.clone(), client.clone(), events.clone()));
            let writer = tokio::spawn(write(config, client, receiver, events));
            let mut state = NetworkState {
                status: "Connecting…".into(),
                ..Default::default()
            };
            while let Some(event) = updates.recv().await {
                match event {
                    Event::Snapshot(snapshot) => replace_snapshot(&mut state, snapshot),
                    Event::Status(connected, status) => {
                        state.connected = connected;
                        state.status = status;
                    }
                    Event::Outcome(id, result) => {
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
        });
    });
    commands
}

fn replace_snapshot(state: &mut NetworkState, snapshot: Snapshot) {
    if snapshot.revision >= state.snapshot.revision {
        state.snapshot = snapshot;
    }
}

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

async fn decode(response: reqwest::Response) -> Result<Snapshot, String> {
    if response.status().is_success() {
        return response
            .json()
            .await
            .map_err(|_| "Server returned an incompatible snapshot".into());
    }
    let status = response.status();
    Err(response
        .json::<ApiError>()
        .await
        .map(|e| e.message)
        .unwrap_or_else(|_| format!("Server rejected the request ({status})")))
}

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
            loop {
                tokio::select! {
                    incoming = socket.next() => match incoming {
                        Some(Ok(Message::Text(json))) => {
                            last_response = tokio::time::Instant::now();
                            let snapshot = serde_json::from_str(&json).map_err(|_| "Invalid server event")?;
                            events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
                        },
                        Some(Ok(Message::Ping(data))) => { last_response = tokio::time::Instant::now(); socket.send(Message::Pong(data)).await.map_err(|_| "Event connection lost")?; },
                        Some(Ok(Message::Pong(_))) => { last_response = tokio::time::Instant::now(); },
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err::<(), String>("Connection lost".into()),
                        _ => {},
                    },
                    _ = heartbeat.tick() => {
                        if last_response.elapsed() > Duration::from_secs(45) { return Err("Event connection stopped responding".into()); }
                        socket.send(Message::Ping(Vec::new().into())).await.map_err(|_| "Event connection lost")?;
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

async fn write(
    config: Config,
    client: reqwest::Client,
    mut commands: mpsc::UnboundedReceiver<CommandEnvelope>,
    events: mpsc::UnboundedSender<Event>,
) {
    while let Some(command) = commands.recv().await {
        let result = match client.post(config.url("v1/commands")).bearer_auth(&config.token).json(&command).send().await {
            Ok(response) => decode(response).await,
            Err(_) => Err("Save could not be confirmed. Your draft is retained; retry uses the same request ID.".into()),
        };
        if result.is_err()
            && let Ok(snapshot) = read(&client, &config).await
        {
            let _ = events.send(Event::Snapshot(snapshot));
        }
        if events
            .send(Event::Outcome(command.request_id, result))
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
