use super::*;
use crate::browser;
use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::Message;
use wasm_bindgen_futures::spawn_local;

pub fn start(
    config: Config,
    ui: StateSender<NetworkState>,
) -> (
    mpsc::UnboundedSender<CommandEnvelope>,
    mpsc::UnboundedSender<()>,
    mpsc::UnboundedSender<StreamRequest>,
) {
    let (commands, mut receiver) = mpsc::unbounded_channel::<CommandEnvelope>();
    let (requests, subscriptions) = mpsc::unbounded_channel();
    let (refresh, mut refreshes) = mpsc::unbounded_channel();
    let (events, mut updates) = mpsc::unbounded_channel();
    let stream_events = events.clone();
    let stream_config = config.clone();
    spawn_local(async move {
        stream(stream_config, stream_events, subscriptions).await;
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
            let build =
                match browser::send(&status_config, client.get(status_config.url("v1/version")))
                    .await
                {
                    Ok(response) if response.status().is_success() => response.json().await.ok(),
                    _ => None,
                };
            if status_events.send(Event::Build(build)).is_err() {
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
            let (result, conflict, ambiguous) = tokio::select! {
                result=async {
                match browser::send(&config, client.post(config.url(path)).json(&command)).await {
                    Ok(response) => {
                        let status = response.status();
                        let result = decode_ack(response).await.and_then(|ack| {
                            if ack.request_id == command.request_id {
                                Ok(ack)
                            } else {
                                Err("Invalid command acknowledgement".into())
                            }
                        });
                        let ambiguous =
                            result.is_err() && (status.is_success() || status.is_server_error());
                        (result, status == reqwest::StatusCode::CONFLICT, ambiguous)
                    }
                    Err(error) => (Err(error), false, true),
                }
                }=>result,
                _=browser::sleep(30_000)=>(Err("Acknowledgement timed out; exact retry preserves its ID".into()),false,true),
            };
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
            receive_event(&mut state, event);
            if ui.send(state.clone()).is_err() {
                break;
            }
        }
    });
    (commands, refresh, requests)
}
async fn decode_ack(response: reqwest::Response) -> Result<CommandAck, String> {
    if response.status().is_success() {
        let ack = response
            .json()
            .await
            .map_err(|_| "Server returned an incompatible acknowledgement")?;
        Ok(ack)
    } else {
        Err(response
            .json::<ApiError>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| "Server rejected the request".into()))
    }
}
async fn stream(
    config: Config,
    events: mpsc::UnboundedSender<Event>,
    mut requests: mpsc::UnboundedReceiver<StreamRequest>,
) {
    let probe_config = config.clone();
    let probe_events = events.clone();
    spawn_local(async move {
        let client = reqwest::Client::new();
        loop {
            let sent = crate::platform::Instant::now();
            let result = tokio::select! {
                result=async {
                    let response=browser::send(&probe_config,client.get(probe_config.url("v1/ping"))).await?;
                    if !response.status().is_success() {return Err("Probe failed".to_owned());}
                    response.bytes().await.map_err(|_|"Probe failed".to_owned())?;
                    Ok::<_,String>(())
                }=>result,
                _=browser::sleep(10_000)=>Err("Probe timed out".into()),
            };
            if result.is_ok()
                && probe_events
                    .send(Event::RoundTrip(sent.elapsed().as_millis() as u64))
                    .is_err()
            {
                break;
            }
            browser::sleep(20_000).await;
        }
    });
    let mut selected = None;
    let mut delay = 1;
    loop {
        let result:Result<(),String>=async {
            let mut socket=browser::connect(&config,"v1/events").await?;
            let initial=tokio::select! {
                initial=socket.next()=>initial.ok_or("Connection lost")?.map_err(|_|"Connection lost")?,
                _=browser::sleep(10_000)=>return Err("Workspace handshake timed out".into()),
            };
            let Message::Text(json)=initial else {return Err("Invalid workspace handshake".into());};
            let event:StreamEvent=serde_json::from_str(&json).map_err(|_|"Invalid server event")?;
            if !matches!(event,StreamEvent::Reset{..}) {return Err("Invalid workspace handshake".into());}
            let mut baseline=Snapshot::default();
            event.apply(&mut baseline)?;
            validate_protocol(&baseline)?;
            events.send(Event::Stream(event)).map_err(|_|"Client closed")?;
            events.send(Event::Status(true,"Connected".into())).map_err(|_|"Client closed")?;
            delay=1;
            while let Ok(request)=requests.try_recv() {if matches!(request,StreamRequest::Subscribe{..}) {selected=Some(request);}}
            if let Some(request)=&selected {socket.send(Message::Text(serde_json::to_string(request).map_err(|_|"Invalid subscription")?)).await.map_err(|_|"Connection lost")?;}
            loop {tokio::select! {
                request=requests.recv()=>if let Some(request)=request {
                    if matches!(request,StreamRequest::Subscribe{..}) {selected=Some(request.clone());}
                    socket.send(Message::Text(serde_json::to_string(&request).map_err(|_|"Invalid conversation request")?)).await.map_err(|_|"Connection lost")?;
                },
                message=socket.next()=>match message {
                    Some(Ok(Message::Text(json)))=> {
                        let event:StreamEvent=serde_json::from_str(&json).map_err(|_|"Invalid server event")?;
                        event.apply(&mut baseline)?;
                        events.send(Event::Stream(event)).map_err(|_|"Client closed")?;
                    },
                    Some(Ok(_))=>{},
                    _=>return Err("Connection lost".into()),
                },
            }}
        }.await;
        if result.is_err()
            && events
                .send(Event::RetryAt(Some(connection_now() + u64::from(delay))))
                .is_err()
        {
            break;
        }
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
