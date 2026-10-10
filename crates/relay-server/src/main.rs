use relay_core::DirectorProfile;
use std::{env, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "--initialize-setup-code" {
        relay_server::initialize_setup_code(std::path::Path::new(&args[1]))?;
        return Ok(());
    }
    if !args.is_empty() {
        return Err("Usage: relay-server [--initialize-setup-code FILE]".into());
    }
    let token = match (
        env::var("RELAY_TOKEN").ok(),
        env::var_os("RELAY_TOKEN_FILE"),
    ) {
        (Some(_), Some(_)) => return Err("RELAY_TOKEN and RELAY_TOKEN_FILE are exclusive".into()),
        (Some(token), None) => token,
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|_| "Cannot read RELAY_TOKEN_FILE")?
            .trim()
            .to_owned(),
        (None, None) => return Err("Set RELAY_TOKEN or RELAY_TOKEN_FILE".into()),
    };
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
    let (app, shutdown) = relay_server::router_with_browser(
        &path,
        token,
        defaults,
        relay_server::RuntimeConfig::from_env()?,
        relay_server::BrowserConfig::from_env()?,
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
