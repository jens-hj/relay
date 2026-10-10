use super::*;
use crate::browser;
use wasm_bindgen_futures::spawn_local;

pub fn start(
    config: Config,
    sender: StateSender<DiscoveryUpdate>,
) -> mpsc::UnboundedSender<BoardSource> {
    let (requests, mut receiver) = mpsc::unbounded_channel::<BoardSource>();
    spawn_local(async move {
        while let Some(source) = receiver.recv().await {
            let result = post(&config, "v1/boards/discover", &source).await;
            if sender
                .send(DiscoveryUpdate {
                    source: Some(source),
                    result: Some(result),
                })
                .is_err()
            {
                break;
            }
        }
    });
    requests
}
pub fn start_recovery(
    config: Config,
    sender: StateSender<RecoveryUpdate>,
) -> mpsc::UnboundedSender<RecoveryRequest> {
    let (requests, mut receiver) = mpsc::unbounded_channel::<RecoveryRequest>();
    spawn_local(async move {
        while let Some(request) = receiver.recv().await {
            let result = post(&config, "v1/operations/reconcile", &request.input).await;
            if sender
                .send(RecoveryUpdate {
                    request: Some(request),
                    result: Some(result),
                })
                .is_err()
            {
                break;
            }
        }
    });
    requests
}
async fn post<T: serde::de::DeserializeOwned>(
    config: &Config,
    path: &str,
    body: &impl serde::Serialize,
) -> Result<T, String> {
    let response = browser::send(
        config,
        reqwest::Client::new().post(config.url(path)).json(body),
    )
    .await?;
    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|_| "Server returned incompatible metadata".into())
    } else {
        Err(response
            .json::<ApiError>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| "Server rejected the request".into()))
    }
}
