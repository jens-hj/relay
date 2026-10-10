use crate::network::Config;
use mosaic::prelude::StateSender;
use relay_core::{ApiError, BoardDiscovery, BoardSource};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Default)]
pub struct DiscoveryUpdate {
    pub source: Option<BoardSource>,
    pub result: Option<Result<BoardDiscovery, String>>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start(
    config: Config,
    sender: StateSender<DiscoveryUpdate>,
) -> mpsc::UnboundedSender<BoardSource> {
    let (requests, mut receiver) = mpsc::unbounded_channel::<BoardSource>();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap();
                while let Some(source) = receiver.recv().await {
                    let result = discover(&client, &config, &source).await;
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
    });
    requests
}

#[cfg(not(target_arch = "wasm32"))]
async fn discover(
    client: &reqwest::Client,
    config: &Config,
    source: &BoardSource,
) -> Result<BoardDiscovery, String> {
    let response = client
        .post(config.url("v1/boards/discover"))
        .bearer_auth(&config.token)
        .header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string())
        .json(source)
        .send()
        .await
        .map_err(|_| {
            "Cannot read destination metadata. Check the server connection and retry.".to_owned()
        })?;
    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|_| "Server returned incompatible destination metadata".into())
    } else {
        let status = response.status();
        Err(response
            .json::<ApiError>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| format!("Destination discovery failed ({status})")))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[tokio::test]
    async fn discovery_posts_the_requested_source_and_preserves_real_status_ids() {
        let source = BoardSource::Github {
            owner: "team".into(),
            number: 7,
            url: String::new(),
        };
        let metadata = BoardDiscovery {
            source: source.clone(),
            name: "Board".into(),
            columns: vec![relay_core::BoardColumn {
                id: "remote-status".into(),
                title: "Ready".into(),
            }],
        };
        let expected = serde_json::to_vec(&source).unwrap();
        let body = serde_json::to_string(&metadata).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(expected.len()).any(|b| b == expected) {
                let mut chunk = [0; 2048];
                let count = socket.read(&mut chunk).unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8_lossy(&bytes).to_lowercase();
            assert!(request.starts_with("post /v1/boards/discover "));
            assert!(request.contains("x-relay-protocol: 3"));
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        let config = Config {
            endpoint: reqwest::Url::parse(&format!("http://{address}/")).unwrap(),
            token: "test".into(),
        };
        assert_eq!(
            discover(&reqwest::Client::new(), &config, &source)
                .await
                .unwrap(),
            metadata
        );
        server.join().unwrap();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryRequest {
    pub input: relay_core::ReconciliationInput,
    pub pending: String,
    pub id: String,
}
#[derive(Clone, Default)]
pub struct RecoveryUpdate {
    pub request: Option<RecoveryRequest>,
    pub result: Option<Result<relay_core::ReconciliationResult, String>>,
}
#[cfg(not(target_arch = "wasm32"))]
pub fn start_recovery(
    config: Config,
    sender: StateSender<RecoveryUpdate>,
) -> mpsc::UnboundedSender<RecoveryRequest> {
    let (requests, mut receiver) = mpsc::unbounded_channel::<RecoveryRequest>();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap();
                while let Some(request) = receiver.recv().await {
                    let result = recovery(&client, &config, &request.input).await;
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
    });
    requests
}
#[cfg(not(target_arch = "wasm32"))]
async fn recovery(
    client: &reqwest::Client,
    config: &Config,
    input: &relay_core::ReconciliationInput,
) -> Result<relay_core::ReconciliationResult, String> {
    let response = client
        .post(config.url("v1/operations/reconcile"))
        .bearer_auth(&config.token)
        .header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string())
        .json(input)
        .send()
        .await
        .map_err(|_| {
            "Cannot check the provider result. Check the server connection and try again."
                .to_owned()
        })?;
    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|_| "Server returned incompatible recovery metadata.".into())
    } else {
        let status = response.status();
        Err(response
            .json::<ApiError>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| format!("Provider result lookup failed ({status})")))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod recovery_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[tokio::test]
    async fn recovery_lookup_posts_authenticated_url_and_decodes_readable_result() {
        let input = relay_core::ReconciliationInput {
            operation_id: "op-1".into(),
            url: "https://github.com/orgs/team/projects/9".into(),
        };
        let expected = serde_json::to_vec(&input).unwrap();
        let result = relay_core::ReconciliationResult {
            key: "board".into(),
            result: "typed-provider-result".into(),
            description: "Found board Work".into(),
        };
        let body = serde_json::to_string(&result).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(expected.len()).any(|b| b == expected) {
                let mut chunk = [0; 2048];
                let count = socket.read(&mut chunk).unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8_lossy(&bytes).to_lowercase();
            assert!(request.starts_with("post /v1/operations/reconcile "));
            assert!(request.contains("authorization: bearer test"));
            assert!(request.contains("x-relay-protocol: 3"));
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        let config = Config {
            endpoint: reqwest::Url::parse(&format!("http://{address}/")).unwrap(),
            token: "test".into(),
        };
        assert_eq!(
            recovery(&reqwest::Client::new(), &config, &input)
                .await
                .unwrap(),
            result
        );
        server.join().unwrap();
    }
}

#[cfg(target_arch = "wasm32")]
#[path = "project_network_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
