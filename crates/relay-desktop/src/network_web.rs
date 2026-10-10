use super::*;
use crate::browser;
use futures_util::StreamExt;
use gloo_net::websocket::Message;
use wasm_bindgen_futures::spawn_local;

pub fn start(
    config: Config,
    ui: StateSender<NetworkState>,
) -> (
    mpsc::UnboundedSender<CommandEnvelope>,
    mpsc::UnboundedSender<()>,
) {
    let (commands, mut receiver) = mpsc::unbounded_channel::<CommandEnvelope>();
    let (refresh, mut refreshes) = mpsc::unbounded_channel();
    let (events, mut updates) = mpsc::unbounded_channel();
    let stream_events = events.clone();
    let stream_config = config.clone();
    spawn_local(async move {
        stream(stream_config, stream_events).await;
    });
    let status_events = events.clone();
    let status_config = config.clone();
    spawn_local(async move {
        let client = reqwest::Client::new();
        loop {
            let request = client.get(status_config.url("v1/harnesses"));
            let result = match browser::send(&status_config, request).await {
                Ok(response) if response.status().is_success() => response
                    .json()
                    .await
                    .map_err(|_| "Incompatible harness status".into()),
                _ => Err("Harness status unavailable".into()),
            };
            if status_events.send(Event::Harnesses(result)).is_err() {
                break;
            }
            tokio::select! {
                _ = browser::sleep(5_000) => {},
                refresh = refreshes.recv() => {
                    if refresh.is_none() { break; }
                    let _ = browser::send(&status_config, client.post(status_config.url("v1/harnesses/refresh"))).await;
                }
            }
        }
    });
    spawn_local(async move {
        let client = reqwest::Client::new();
        while let Some(command) = receiver.recv().await {
            let path = if matches!(
                command.command,
                relay_core::Command::SubmitTurn { .. } | relay_core::Command::EditQueuedTurn { .. }
            ) {
                "v1/conversation/commands"
            } else {
                "v1/commands"
            };
            let (result, conflict, ambiguous) =
                match browser::send(&config, client.post(config.url(path)).json(&command)).await {
                    Ok(response) => {
                        let status = response.status();
                        let result = decode(response).await;
                        let ambiguous =
                            result.is_err() && (status.is_success() || status.is_server_error());
                        (result, status == reqwest::StatusCode::CONFLICT, ambiguous)
                    }
                    Err(error) => (Err(error), false, true),
                };
            if result.is_err()
                && let Ok(snapshot) = read(&config).await
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
    });
    spawn_local(async move {
        let mut state = NetworkState {
            status: "Connecting…".into(),
            ..Default::default()
        };
        while let Some(event) = updates.recv().await {
            match event {
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
                Event::RoundTrip(ms) => state.round_trip_ms = Some(ms),
                Event::Outcome(id, result, conflict, ambiguous) => {
                    state.outcome_serial += 1;
                    state.outcome_conflict = conflict;
                    state.outcome_ambiguous = ambiguous;
                    state.outcome = Some((
                        id,
                        result.map(|snapshot| replace_snapshot(&mut state, snapshot)),
                    ));
                }
            }
            if ui.send(state.clone()).is_err() {
                break;
            }
        }
    });
    (commands, refresh)
}
async fn decode(response: reqwest::Response) -> Result<Snapshot, String> {
    if response.status().is_success() {
        let snapshot = response
            .json()
            .await
            .map_err(|_| "Server returned an incompatible snapshot")?;
        validate_protocol(&snapshot)?;
        Ok(snapshot)
    } else {
        Err(response
            .json::<ApiError>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| "Server rejected the request".into()))
    }
}
async fn read(config: &Config) -> Result<Snapshot, String> {
    decode(
        browser::send(
            config,
            reqwest::Client::new().get(config.url("v1/snapshot")),
        )
        .await?,
    )
    .await
}
async fn stream(config: Config, events: mpsc::UnboundedSender<Event>) {
    let mut delay = 1;
    loop {
        let result: Result<(),String> = async {
            let snapshot=read(&config).await?;
            events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
            let mut socket=browser::connect(&config,"v1/events").await?;
            delay=1;
            events.send(Event::Status(true,"Connected".into())).map_err(|_| "Client closed")?;
            loop { tokio::select! {
                message=socket.next()=>match message {
                    Some(Ok(Message::Text(json)))=> {
                        let snapshot=serde_json::from_str(&json).map_err(|_| "Incompatible event snapshot")?;
                        validate_protocol(&snapshot)?;
                        events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
                    },
                    Some(Ok(_))=>{},
                    _=>return Err("Connection lost".into()),
                },
                // Browsers cannot send WebSocket control pings. Probe authenticated HTTP instead.
                _=browser::sleep(20_000)=> {
                    let sent=crate::platform::Instant::now();
                    let snapshot=read(&config).await?;
                    events.send(Event::RoundTrip(sent.elapsed().as_millis() as u64)).map_err(|_| "Client closed")?;
                    events.send(Event::Snapshot(snapshot)).map_err(|_| "Client closed")?;
                }
            } }
        }.await;
        if let Err(error) = result
            && events
                .send(Event::Status(
                    false,
                    format!("{error} · retrying in {delay}s"),
                ))
                .is_err()
        {
            break;
        }
        browser::sleep(delay * 1000).await;
        delay = (delay * 2).min(15);
    }
}
