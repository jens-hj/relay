use relay_core::AgentProfile;
use std::{env, path::PathBuf};

/// glibc gives each thread that allocates its own arena. Store sections run on
/// whichever runtime worker `block_in_place` lands on, so large transient
/// allocations scatter across up to 8×cores arenas that are rarely returned to
/// the OS. Two arenas keep RSS close to live memory. Set in-process rather than
/// via `MALLOC_ARENA_MAX` so harness processes do not inherit it.
fn limit_malloc_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: called before the runtime starts any thread; mallopt only
    // adjusts allocator tuning.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    limit_malloc_arenas();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "mcp" {
        return relay_server::mcp_stdio().await;
    }
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
        Ok(path) => AgentProfile::from_toml(&std::fs::read_to_string(path)?)?,
        Err(_) => AgentProfile::default(),
    };
    let (app, shutdown) = relay_server::router_with_browser(
        &path,
        token,
        defaults,
        relay_server::RuntimeConfig::from_env()?,
        relay_server::BrowserConfig::from_env()?,
    )?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let address = listener.local_addr()?;
    let ip = if address.ip().is_unspecified() {
        "127.0.0.1".parse()?
    } else {
        address.ip()
    };
    shutdown.set_agent_endpoint(format!(
        "http://{}",
        std::net::SocketAddr::new(ip, address.port())
    ));
    shutdown.start_window_start();
    shutdown.start_usage_reset_notifications();
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
