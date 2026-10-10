use crate::network::Config;
#[cfg(not(target_arch = "wasm32"))]
use futures_util::{SinkExt, StreamExt};
use mosaic::prelude::StateSender;
use relay_core::*;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

#[derive(Clone)]
pub enum Request {
    Catalog { session: String, force: bool },
    Save { session: String, request: SaveDraft },
    Upload { asset: Asset, bytes: Arc<Vec<u8>> },
    Fetch(String),
    Forget(Vec<String>),
}

#[derive(Clone)]
pub enum Outcome {
    Saved {
        session: String,
        request: SaveDraft,
        result: Result<Draft, (bool, String)>,
    },
    Fetched {
        id: String,
        result: Result<(), String>,
    },
    Uploaded {
        asset: Asset,
        result: Result<(), String>,
    },
}

#[derive(Clone, Default)]
pub struct Update {
    pub drafts: Vec<Draft>,
    pub catalogs: BTreeMap<String, Result<HarnessCatalog, String>>,
    pub blobs: BTreeMap<String, Arc<Vec<u8>>>,
    pub outcomes: BTreeMap<String, Outcome>,
    pub initialized: bool,
    pub connected: bool,
    pub serial: u64,
}

enum Event {
    Catalog(String, Result<HarnessCatalog, String>),
    Drafts(Vec<Draft>),
    Connected(bool),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start(config: Config, sender: StateSender<Update>) -> mpsc::UnboundedSender<Request> {
    let (requests, mut receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build().unwrap();
            let (events, mut updates) = mpsc::unbounded_channel();
            let watcher = tokio::spawn(watch(config.clone(), events.clone()));
            let mut state = Update::default();
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    _ = tick.tick() => {},
                    event = updates.recv() => match event {
                        Some(Event::Drafts(drafts)) => { state.drafts = drafts; state.initialized = true; },
                        Some(Event::Connected(connected)) => state.connected = connected,
                        Some(Event::Catalog(session,result))=>{state.catalogs.insert(session,result);},
                        None => break,
                    },
                    request = receiver.recv() => match request {
                        Some(Request::Catalog {session,force}) => {
                            let client=client.clone();let config=config.clone();let events=events.clone();
                            tokio::spawn(async move {
                            let path=format!("v1/sessions/{}/catalog",percent_encoding::utf8_percent_encode(&session,percent_encoding::NON_ALPHANUMERIC));
                            let request=if force {client.post(config.url(&path))}else{client.get(config.url(&path))};
                            let result=match request.bearer_auth(&config.token).header("X-Relay-Protocol",relay_core::PROTOCOL_VERSION.to_string()).send().await {
                                Ok(response) if response.status().is_success()=>response.json::<HarnessCatalog>().await.map_err(|_|"Invalid harness catalog".into()),
                                Ok(response)=>Err(response.json::<ApiError>().await.map(|e|e.message).unwrap_or_else(|_|"Harness discovery unavailable".into())),
                                Err(_)=>Err("Cannot discover harness commands".into()),
                            };
                            let _=events.send(Event::Catalog(session,result));
                            });
                        },
                        Some(Request::Forget(ids)) => { for id in ids { state.outcomes.remove(&id); } },
                        Some(Request::Save {session, request}) => {
                            let result = match client.post(config.url(&format!("v1/drafts/{}", percent_encoding::utf8_percent_encode(&session, percent_encoding::NON_ALPHANUMERIC)))).bearer_auth(&config.token).header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string()).json(&request).send().await {
                                Ok(response) => { let conflict = response.status() == reqwest::StatusCode::CONFLICT; if response.status().is_success() { response.json::<Draft>().await.map_err(|_| (false,"Draft save could not be confirmed; exact retry is retained".into())) } else { Err((conflict,response.json::<ApiError>().await.map(|e| e.message).unwrap_or_else(|_| "Draft save was rejected".into()))) } },
                                Err(_) => Err((false,"Cannot confirm draft save; local content is retained".into())),
                            };
                            if let Ok(draft) = &result { if let Some(old) = state.drafts.iter_mut().find(|d| d.session_id == session) { if draft.revision >= old.revision { *old = draft.clone(); } } else { state.drafts.push(draft.clone()); } }
                            state.outcomes.insert(request.request_id.clone(), Outcome::Saved {session,request,result});
                        },
                        Some(Request::Upload {asset, bytes}) => {
                            let result = match client.post(config.url(&format!("v1/assets/{}",asset.id))).bearer_auth(&config.token).header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string()).header("content-type", &asset.media_type).header("x-relay-filename",percent_encoding::utf8_percent_encode(&asset.name, percent_encoding::NON_ALPHANUMERIC).to_string()).body(bytes.as_ref().clone()).send().await {
                                Ok(response) if response.status().is_success() => response.json::<Asset>().await.map_err(|_| "File upload acknowledgement was invalid".into()).and_then(|saved| if saved == asset { Ok(()) } else { Err("Uploaded file metadata changed".into()) }),
                                Ok(response) => Err(response.json::<ApiError>().await.map(|e| e.message).unwrap_or_else(|_| "File upload failed".into())),
                                Err(_) => Err("File upload could not be confirmed; retry retains its ID".into()),
                            };
                            state.blobs.insert(asset.id.clone(),bytes);
                            state.outcomes.insert(asset.id.clone(), Outcome::Uploaded {asset,result});
                        },
                        Some(Request::Fetch(id)) => {
                            let result = match fetch_bytes(&client,&config,&id).await {Ok(bytes)=>{state.blobs.insert(id.clone(),Arc::new(bytes));Ok(())},Err(error)=>Err(error)};
                            state.outcomes.insert(format!("fetch-{id}"),Outcome::Fetched{id,result});
                        },
                        None => break,
                    }
                }
                state.serial += 1;
                if sender.send(state.clone()).is_err() { break; }
            }
            watcher.abort();
        });
    });
    requests
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_bytes(
    client: &reqwest::Client,
    config: &Config,
    id: &str,
) -> Result<Vec<u8>, String> {
    let error = || "Cannot load inline file; click its preview to retry".to_owned();
    let mut response = client
        .get(config.url(&format!("v1/assets/{id}")))
        .bearer_auth(&config.token)
        .send()
        .await
        .map_err(|_| error())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > ASSET_LIMIT as u64)
    {
        return Err(error());
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| error())? {
        if bytes.len() + chunk.len() > ASSET_LIMIT {
            return Err(error());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
async fn watch(config: Config, events: mpsc::UnboundedSender<Event>) {
    loop {
        let result: Result<(), ()> = async {
            let mut url = config.url("v1/drafts/events");
            url.set_scheme(if config.endpoint.scheme() == "https" { "wss" } else { "ws" }).map_err(|_| ())?;
            let mut request = url.as_str().into_client_request().map_err(|_| ())?;
            request.headers_mut().insert("authorization", HeaderValue::from_str(&format!("Bearer {}",config.token)).map_err(|_| ())?);
            let (mut socket,_) = tokio::time::timeout(Duration::from_secs(10),tokio_tungstenite::connect_async(request)).await.map_err(|_| ())?.map_err(|_| ())?;
            events.send(Event::Connected(true)).map_err(|_| ())?;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
            let mut last = tokio::time::Instant::now();
            loop { tokio::select! {
                _ = heartbeat.tick() => { if last.elapsed() > Duration::from_secs(45) { return Err(()); } socket.send(Message::Ping(vec![].into())).await.map_err(|_| ())?; },
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Text(json))) => { last = tokio::time::Instant::now(); events.send(Event::Drafts(serde_json::from_str(&json).map_err(|_| ())?)).map_err(|_| ())?; },
                    Some(Ok(Message::Ping(data))) => { last = tokio::time::Instant::now(); socket.send(Message::Pong(data)).await.map_err(|_| ())?; },
                    Some(Ok(Message::Pong(_))) => last = tokio::time::Instant::now(),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err(()),
                    _ => {},
                }
            } }
        }.await;
        if result.is_err() && events.send(Event::Connected(false)).is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(target_arch = "wasm32")]
#[path = "buffer_network_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
