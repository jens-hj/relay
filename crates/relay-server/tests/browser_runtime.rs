//! Exercise the shipped executable's runtime-file contract, not only its router.
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
const TOKEN: &str = "runtime-file-secret-01234567890123456789";
#[tokio::test]
async fn executable_reads_token_file_and_setup_command_never_prints_secret() {
    let dir = tempfile::tempdir().unwrap();
    let setup = dir.path().join("setup-code");
    let result = Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .env_clear()
        .arg("--initialize-setup-code")
        .arg(&setup)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let code = std::fs::read_to_string(&setup).unwrap();
    assert_eq!(code.len(), 43);
    let repeated = Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .env_clear()
        .arg("--initialize-setup-code")
        .arg(&setup)
        .output()
        .unwrap();
    assert!(!repeated.status.success());
    assert_eq!(std::fs::read_to_string(&setup).unwrap(), code);
    let token = dir.path().join("token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let exclusive = Command::new(env!("CARGO_BIN_EXE_relay-server"))
        .env_clear()
        .env("RELAY_TOKEN", TOKEN)
        .env("RELAY_TOKEN_FILE", &token)
        .output()
        .unwrap();
    assert!(!exclusive.status.success());
    let error = String::from_utf8_lossy(&exclusive.stderr);
    assert!(error.contains("exclusive"));
    assert!(!error.contains(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_relay-server"))
            .env_clear()
            .env("RELAY_TOKEN_FILE", &token)
            .env("RELAY_DATABASE", dir.path().join("db"))
            .env("RELAY_BIND", address.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let url = format!("http://{address}/v1/snapshot");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                server.0.try_wait().unwrap().is_none(),
                "server exited before accepting HTTP"
            );
            if let Ok(response) = client.get(&url).bearer_auth(TOKEN).send().await {
                assert_eq!(response.status(), 200);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    assert_eq!(
        client
            .get(format!("http://{address}/auth/session"))
            .header("cookie", "__Host-relay_session=unknown")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}
