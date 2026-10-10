use super::*;
use crate::browser;
use futures_util::StreamExt;
use gloo_net::websocket::Message;
use wasm_bindgen_futures::spawn_local;
pub fn start(config: Config, sender: StateSender<Update>) -> mpsc::UnboundedSender<Request> {
    let (requests, mut receiver) = mpsc::unbounded_channel();
    let (events, mut updates) = mpsc::unbounded_channel();
    let watch_config = config.clone();
    spawn_local(async move {
        watch(watch_config, events).await;
    });
    spawn_local(async move {
        let client = reqwest::Client::new();
        let mut state = Update::default();
        loop {
            tokio::select! {
                _=browser::sleep(100)=>{},
                event=updates.recv()=>match event {
                    Some(Event::Drafts(drafts))=>{state.drafts=drafts;state.initialized=true;},
                    Some(Event::Connected(connected))=>state.connected=connected,
                    None=>break,
                },
                request=receiver.recv()=>match request {
                    Some(Request::Forget(ids))=>{for id in ids {state.outcomes.remove(&id);}},
                    Some(Request::Save{session,request})=>{
                        let path=format!("v1/drafts/{}",percent_encoding::utf8_percent_encode(&session,percent_encoding::NON_ALPHANUMERIC));
                        let result=match browser::send(&config,client.post(config.url(&path)).json(&request)).await {
                            Ok(response)=>{let conflict=response.status()==reqwest::StatusCode::CONFLICT;
                                if response.status().is_success() {response.json::<Draft>().await.map_err(|_|(false,"Draft save could not be confirmed; exact retry is retained".into()))}
                                else {Err((conflict,response.json::<ApiError>().await.map(|e|e.message).unwrap_or_else(|_|"Draft save was rejected".into()))) }
                            },Err(error)=>Err((false,error)),
                        };
                        if let Ok(draft)=&result {
                            if let Some(old)=state.drafts.iter_mut().find(|d|d.session_id==session) {if draft.revision>=old.revision {*old=draft.clone();}}
                            else {state.drafts.push(draft.clone());}
                        }
                        state.outcomes.insert(request.request_id.clone(),Outcome::Saved{session,request,result});
                    },
                    Some(Request::Upload{asset,bytes})=>{
                        let result=match browser::send(&config,client.post(config.url(&format!("v1/assets/{}",asset.id)))
                            .header("content-type",&asset.media_type).header("x-relay-filename",percent_encoding::utf8_percent_encode(&asset.name,percent_encoding::NON_ALPHANUMERIC).to_string()).body(bytes.as_ref().clone())).await {
                            Ok(response) if response.status().is_success()=>response.json::<Asset>().await.map_err(|_|"File upload acknowledgement was invalid".into()).and_then(|saved|if saved==asset {Ok(())}else{Err("Uploaded file metadata changed".into())}),
                            Ok(response)=>Err(response.json::<ApiError>().await.map(|e|e.message).unwrap_or_else(|_|"File upload failed".into())),
                            Err(error)=>Err(error),
                        };
                        state.blobs.insert(asset.id.clone(),bytes);state.outcomes.insert(asset.id.clone(),Outcome::Uploaded{asset,result});
                    },
                    Some(Request::Fetch(id))=>{
                        let result=match browser::send(&config,client.get(config.url(&format!("v1/assets/{id}")))).await {
                            Ok(response) if response.status().is_success()=>match response.bytes().await {
                                Ok(bytes) if bytes.len()<=ASSET_LIMIT=>{state.blobs.insert(id.clone(),Arc::new(bytes.to_vec()));Ok(())},
                                _=>Err("Inline file is invalid or exceeds 20 MiB".into()),
                            },_=>Err("Cannot load inline file; click its preview to retry".into()),
                        };
                        state.outcomes.insert(format!("fetch-{id}"),Outcome::Fetched{id,result});
                    },None=>break,
                }
            }
            state.serial += 1;
            if sender.send(state.clone()).is_err() {
                break;
            }
        }
    });
    requests
}
async fn watch(config: Config, events: mpsc::UnboundedSender<Event>) {
    loop {
        let result:Result<(),String>=async {
            // HTTP also detects expired sessions: browsers hide the upgrade response status.
            let response=browser::send(&config,reqwest::Client::new().get(config.url("v1/drafts"))).await?;
            if !response.status().is_success() {return Err("Cannot read shared drafts".into());}
            let drafts=response.json().await.map_err(|_|"Invalid shared drafts")?;
            events.send(Event::Drafts(drafts)).map_err(|_|"Client closed")?;
            let mut socket=browser::connect(&config,"v1/drafts/events").await?;
            events.send(Event::Connected(true)).map_err(|_|"Client closed")?;
            loop {tokio::select! {
                incoming=socket.next()=>match incoming {
                    Some(Ok(Message::Text(json)))=>{let drafts=serde_json::from_str(&json).map_err(|_|"Invalid draft event")?;events.send(Event::Drafts(drafts)).map_err(|_|"Client closed")?;},
                    Some(Ok(_))=>{},_=>return Err("Draft connection lost".into()),
                },
                _=browser::sleep(20_000)=>{let response=browser::send(&config,reqwest::Client::new().get(config.url("v1/drafts"))).await?;
                    if !response.status().is_success() {return Err("Cannot read shared drafts".into());}
                    let drafts=response.json().await.map_err(|_|"Invalid shared drafts")?;
                    events.send(Event::Drafts(drafts)).map_err(|_|"Client closed")?;},
            }}
        }.await;
        if result.is_err() && events.send(Event::Connected(false)).is_err() {
            break;
        }
        browser::sleep(2_000).await;
    }
}
