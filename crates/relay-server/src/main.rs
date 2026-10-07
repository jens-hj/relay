use relay_core::DirectorProfile;
use std::{env, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RELAY_TOKEN").map_err(
        |_| "Set RELAY_TOKEN to a random shared workspace token (at least 16 characters)",
    )?;
    let bind = env::var("RELAY_BIND").unwrap_or_else(|_| "127.0.0.1:7331".into());
    let path =
        PathBuf::from(env::var("RELAY_DATABASE").unwrap_or_else(|_| "data/relay.sqlite3".into()));
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let defaults = match env::var("RELAY_DEFAULT_PROFILE") {
        Ok(path) => DirectorProfile::from_toml(&std::fs::read_to_string(path)?)?,
        Err(_) => DirectorProfile::default(),
    };
    let (app, shutdown) = relay_server::router_with_shutdown(
        &path,
        token,
        defaults,
        relay_server::RuntimeConfig::from_env()?,
    )?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!(
        "Relay server listening on {} · database {}",
        listener.local_addr()?,
        path.display()
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            {
                if let Ok(mut terminate) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                {
                    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                } else {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            if let Err(error) = shutdown.shutdown().await {
                eprintln!("Worker shutdown failed: {error}");
            }
        })
        .await?;
    Ok(())
}
