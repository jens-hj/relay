use super::*;
use crate::browser;
use futures_util::StreamExt;
use gloo_net::websocket::Message;
use wasm_bindgen_futures::spawn_local;
pub fn start(config: Config, sender: StateSender<Update>) -> RequestSender {
    let (
        requests,
        RequestReceiver {
            data: mut receiver,
            control: mut controls,
        },
    ) = request_channel();
    let (events, mut updates) = mpsc::channel(8);
    let watch_config = config.clone();
    let watch_events = events.clone();
    spawn_local(async move {
        watch(watch_config, watch_events).await;
    });
    spawn_local(async move {
        let client = reqwest::Client::new();
        let mut state = Update::default();
        let mut credits = BTreeMap::new();
        let mut pending: std::collections::VecDeque<Admitted> = std::collections::VecDeque::new();
        let mut active = std::collections::BTreeSet::new();
        let mut jobs = futures_util::stream::FuturesUnordered::new();
        loop {
            if let Ok(ids) = controls.try_recv() {
                forget(&mut state, &mut credits, ids);
            }
            tokio::select! {
                            _=browser::sleep(100)=>{},
                            event=updates.recv()=>match event {
                                Some(Event::Drafts(drafts))=>{state.drafts=drafts;state.initialized=true;},
                                Some(Event::Connected(connected))=>state.connected=connected,
                                None=>break,
                            },
                            Some((key, delta, credit)) = jobs.next(), if !jobs.is_empty() => {
                                active.remove(&key);
                                completed(&mut state, &mut credits, delta, credit);
                            },
                            control = controls.recv() => { if let Some(ids) = control { forget(&mut state, &mut credits, ids); } },
                            request=receiver.recv() =>match request {
                                Some(request) => pending.push_back(request),
            None=>break,
                            }
                        }
            while let Some(index) = pending
                .iter()
                .position(|r| can_dispatch(&r.request, &active))
            {
                let Admitted { request, credit } = pending.remove(index).unwrap();
                let key = io_key(&request);
                active.insert(key.clone());
                let client = client.clone();
                let config = config.clone();
                jobs.push(async move { (key, execute(client, config, request).await, credit) });
            }
            state.serial += 1;
            if sender.send(state.clone()).is_err() {
                break;
            }
        }
    });
    requests
}
async fn watch(config: Config, events: mpsc::Sender<Event>) {
    loop {
        let result:Result<(),String>=async {
            // HTTP also detects expired sessions: browsers hide the upgrade response status.
            let drafts=fetch_drafts(&config).await?;
            events.send(Event::Drafts(drafts)).await.map_err(|_|"Client closed")?;
            let mut socket=browser::connect(&config,"v1/drafts/events").await?;
            events.send(Event::Connected(true)).await.map_err(|_|"Client closed")?;
            loop {tokio::select! {
                incoming=socket.next()=>match incoming {
                    Some(Ok(Message::Text(json)))=>{let drafts=serde_json::from_str(&json).map_err(|_|"Invalid draft event")?;events.send(Event::Drafts(drafts)).await.map_err(|_|"Client closed")?;},
                    Some(Ok(_))=>{},_=>return Err("Draft connection lost".into()),
                },
                _ = browser::sleep(20_000) => { let drafts=fetch_drafts(&config).await?;
                    events.send(Event::Drafts(drafts)).await.map_err(|_|"Client closed")?;},
            }}
        }.await;
        if result.is_err() && events.send(Event::Connected(false)).await.is_err() {
            break;
        }
        browser::sleep(2_000).await;
    }
}

async fn execute_inner(client: reqwest::Client, config: Config, request: Request) -> Update {
    let mut state = Update::default();
    match request {
        request @ (Request::Catalog { .. } | Request::ProfileCatalog { .. }) => {
            let (key, path, force) = request.catalog_route();
            let request = if force {
                client.post(config.url(&path))
            } else {
                client.get(config.url(&path))
            };
            let result = match browser::send(&config, request).await {
                Ok(response) if response.status().is_success() => response
                    .json::<HarnessCatalog>()
                    .await
                    .map_err(|_| "Invalid harness catalog".into()),
                _ => Err("Cannot discover harness commands".into()),
            };
            state.catalogs.insert(key, result);
        }
        Request::Save {
            session,
            request,
            generation,
        } => {
            let path = format!(
                "v1/drafts/{}",
                percent_encoding::utf8_percent_encode(&session, percent_encoding::NON_ALPHANUMERIC)
            );
            let result = match browser::send(&config, client.post(config.url(&path)).json(&request))
                .await
            {
                Ok(response) => {
                    let conflict = save_failure(response.status());
                    if response.status().is_success() {
                        response.json::<Draft>().await.map_err(|_| {
                            (
                                SaveFailure::Retryable,
                                "Draft save could not be confirmed; exact retry is retained".into(),
                            )
                        })
                    } else if conflict != SaveFailure::Retryable {
                        Err((
                            conflict,
                            if conflict == SaveFailure::Conflict {
                                "Shared draft changed; local content is retained".into()
                            } else {
                                "Draft save refused; check access and whether the chat still exists"
                                    .into()
                            },
                        ))
                    } else {
                        Err((
                            conflict,
                            response
                                .json::<ApiError>()
                                .await
                                .map(|e| e.message)
                                .unwrap_or_else(|_| "Draft save was rejected".into()),
                        ))
                    }
                }
                Err(error) => {
                    let kind = if error == "Login required" {
                        SaveFailure::Permanent
                    } else {
                        SaveFailure::Retryable
                    };
                    Err((kind, error))
                }
            };
            if let Ok(draft) = &result {
                if let Some(old) = state.drafts.iter_mut().find(|d| d.session_id == session) {
                    if draft.revision >= old.revision {
                        *old = draft.clone();
                    }
                } else {
                    state.drafts.push(draft.clone());
                }
            }
            state.outcomes.insert(
                format!("{}:{generation}", request.request_id),
                Outcome::Saved {
                    session,
                    request,
                    generation,
                    result,
                },
            );
        }
        Request::Upload { asset, bytes } => {
            let result = match browser::send(
                &config,
                client
                    .post(config.url(&format!("v1/assets/{}", asset.id)))
                    .header("content-type", &asset.media_type)
                    .header(
                        "x-relay-filename",
                        percent_encoding::utf8_percent_encode(
                            &asset.name,
                            percent_encoding::NON_ALPHANUMERIC,
                        )
                        .to_string(),
                    )
                    .body(bytes.as_ref().clone()),
            )
            .await
            {
                Ok(response) if response.status().is_success() => response
                    .json::<Asset>()
                    .await
                    .map_err(|_| "File upload acknowledgement was invalid".into())
                    .and_then(|saved| {
                        if saved == asset {
                            Ok(())
                        } else {
                            Err("Uploaded file metadata changed".into())
                        }
                    }),
                Ok(response) => Err(response
                    .json::<ApiError>()
                    .await
                    .map(|e| e.message)
                    .unwrap_or_else(|_| "File upload failed".into())),
                Err(error) => Err(error),
            };
            state.blobs.insert(asset.id.clone(), bytes);
            state
                .outcomes
                .insert(asset.id.clone(), Outcome::Uploaded { asset, result });
        }
        Request::Fetch(id) => {
            let result =
                match browser::send(&config, client.get(config.url(&format!("v1/assets/{id}"))))
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        match response.bytes().await {
                            Ok(bytes) if bytes.len() <= ASSET_LIMIT => {
                                state.blobs.insert(id.clone(), Arc::new(bytes.to_vec()));
                                Ok(())
                            }
                            _ => Err("Inline file is invalid or exceeds 20 MiB".into()),
                        }
                    }
                    _ => Err("Cannot load inline file; click its preview to retry".into()),
                };
            state
                .outcomes
                .insert(format!("fetch-{id}"), Outcome::Fetched { id, result });
        }
        _ => unreachable!("control request dispatched as I/O"),
    }
    state
}

async fn execute(client: reqwest::Client, config: Config, request: Request) -> Update {
    execute_bounded(client, config, request, 30_000).await
}
async fn execute_bounded(
    client: reqwest::Client,
    config: Config,
    request: Request,
    deadline_ms: u32,
) -> Update {
    let backup = request.clone();
    let job = execute_inner(client, config, request);
    tokio::select! {
        result = job => result,
        _ = io_deadline(deadline_ms) => {
            let mut state = Update::default();
            let message = "Request timed out; exact retry preserves its ID".to_owned();
            let (id, outcome) = match backup {
                Request::Save { session, request, generation } => (format!("{}:{generation}", request.request_id), Outcome::Saved {session, request, generation, result: Err((SaveFailure::Retryable, message))}),
                Request::Upload {asset, ..} => (asset.id.clone(), Outcome::Uploaded {asset, result: Err(message)}),
                Request::Fetch(id) => (format!("fetch-{id}"), Outcome::Fetched {id, result: Err(message)}),
                request @ (Request::Catalog {..} | Request::ProfileCatalog {..}) => {
                    state.catalogs.insert(request.catalog_route().0, Err(message));
                    return state;
                },
                _ => unreachable!(),
            };
            state.outcomes.insert(id, outcome);
            state
        }
    }
}
async fn io_deadline(deadline_ms: u32) {
    browser::sleep(deadline_ms).await;
}

async fn fetch_drafts(config: &Config) -> Result<Vec<Draft>, String> {
    tokio::select! {
        result = async {
            let response = browser::send(config, reqwest::Client::new().get(config.url("v1/drafts"))).await?;
            if !response.status().is_success() { return Err("Cannot read shared drafts".into()); }
            response.json().await.map_err(|_| "Invalid shared drafts".into())
        } => result,
        _ = browser::sleep(30_000) => Err("Shared draft response timed out".into()),
    }
}
