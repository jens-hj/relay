//! Harness discovery and the server-owned project/execution configuration.
use crate::{Error, RemoteConfig, RuntimeConfig, Workspace, now};
use axum::{Json, extract::State};
use relay_core::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) fn configuration(
    snapshot: &Snapshot,
    project: &str,
    base: &RuntimeConfig,
) -> RuntimeConfig {
    let mut config = base.clone();
    if let Some(binding) = snapshot.bindings.iter().find(|b| {
        b.project_id() == project
            || snapshot
                .projects
                .iter()
                .find(|p| p.id == project)
                .is_some_and(|p| {
                    p.repository == b.repository
                        && p.github
                            .as_ref()
                            .is_some_and(|g| g.owner == b.owner && g.number == b.number)
                })
    }) {
        config.remote = Some(RemoteConfig {
            repository: binding.repository.clone(),
            owner: binding.owner.clone(),
            number: binding.number,
        });
        config.repository = Some(binding.checkout.clone().into());
    } else if !snapshot.bindings.is_empty() {
        config.remote = None;
        config.repository = None;
    }
    for installation in &snapshot.installations {
        match installation.harness {
            Harness::Codex => config.codex = installation.executable.clone().into(),
            Harness::ClaudeCode => config.claude = installation.executable.clone().into(),
        }
    }
    config
}
pub(super) fn mode(snapshot: &Snapshot, session: &str) -> Result<ApprovalMode, Error> {
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session)
        .ok_or_else(|| Error::invalid("Session not found"))?;
    let director = snapshot
        .directors
        .iter()
        .find(|d| d.id == session.director_id)
        .ok_or_else(|| Error::invalid("Director not found"))?;
    let profile = snapshot
        .effective_profile(director)
        .map_err(Error::invalid)?;
    Ok(session
        .worker
        .as_ref()
        .and_then(|w| w.execution.as_ref())
        .unwrap_or(&profile.execution)
        .approval)
}
pub(super) fn expire(snapshot: &mut Snapshot, session: &str, run: &str) {
    for permission in &mut snapshot.tool_permissions {
        if permission.session_id == session
            && permission.run_id == run
            && permission.decision.is_none()
        {
            permission.expired = true;
        }
    }
}
pub(super) async fn permission(
    workspace: &Workspace,
    session: &str,
    run: &str,
    tool: &str,
    description: &str,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<bool, Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let mut end = description.len().min(16 * 1024);
    while !description.is_char_boundary(end) {
        end -= 1;
    }
    workspace.update_run(session, run, |snapshot| {
        snapshot.tool_permissions.push(ToolPermission {
            id: id.clone(),
            session_id: session.into(),
            run_id: run.into(),
            tool: tool.chars().take(256).collect(),
            description: description[..end].into(),
            decision: None,
            expired: false,
        });
        Ok(())
    })?;
    let mut events = workspace.snapshots.subscribe();
    loop {
        if *stop.borrow() {
            return Ok(false);
        }
        let snapshot = events.borrow().clone();
        let request = snapshot
            .tool_permissions
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::invalid("Permission request unavailable"))?;
        if request.expired {
            return Ok(false);
        }
        if let Some(decision) = request.decision {
            return Ok(decision);
        }
        tokio::select! { _ = stop.changed() => return Ok(false), changed = events.changed() => {changed.map_err(|_| Error::invalid("Server stopped"))?;} }
    }
}
pub(super) fn add_project(
    snapshot: &mut Snapshot,
    binding: ProjectBinding,
    defaults: DirectorProfile,
) -> Result<String, Error> {
    validate_binding(&binding)?;
    let binding_id = binding.project_id();
    let id = snapshot
        .projects
        .iter()
        .find(|p| {
            !p.fixture
                && p.repository == binding.repository
                && p.github
                    .as_ref()
                    .is_some_and(|g| g.owner == binding.owner && g.number == binding.number)
        })
        .map(|p| p.id.clone())
        .unwrap_or_else(|| binding_id.clone());
    if let Some(existing) = snapshot
        .bindings
        .iter()
        .find(|b| b.project_id() == binding_id)
        && existing.checkout != binding.checkout
        && snapshot
            .sessions
            .iter()
            .any(|s| s.project_id == id && s.worker.is_some())
    {
        return Err(Error::invalid(
            "This project has workers bound to its saved checkout; keep that checkout available",
        ));
    }
    if let Some(project) = snapshot.projects.iter().find(|p| p.id == id) {
        if project.fixture
            || project.repository != binding.repository
            || project
                .github
                .as_ref()
                .is_none_or(|g| g.owner != binding.owner || g.number != binding.number)
        {
            return Err(Error::invalid(
                "Project identity conflicts with saved history",
            ));
        }
    } else {
        snapshot.projects.push(Project {
            id: id.clone(),
            name: format!("{} · GitHub project {}", binding.repository, binding.number),
            repository: binding.repository.clone(),
            fixture: false,
            columns: vec![BoardColumn {
                id: "github-no-status".into(),
                title: "No status".into(),
            }],
            defaults,
            github: Some(GitHubProject {
                owner: binding.owner.clone(),
                number: binding.number,
                url: format!(
                    "https://github.com/users/{}/projects/{}",
                    binding.owner, binding.number
                ),
                last_synced_at: None,
                sync_error: None,
            }),
        });
        snapshot.directors.push(Director {
            id: format!("director-{id}"),
            project_id: id.clone(),
            name: "Project director".into(),
            overrides: ProfileOverrides::default(),
        });
    }
    snapshot.bindings.retain(|b| b.project_id() != binding_id);
    snapshot.bindings.push(binding);
    let index = snapshot.projects.iter().position(|p| p.id == id).unwrap();
    let project = snapshot.projects.remove(index);
    snapshot.projects.insert(0, project);
    Ok(id)
}
fn validate_binding(binding: &ProjectBinding) -> Result<(), Error> {
    if binding.repository.split('/').count() != 2
        || binding
            .repository
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || !binding
            .repository
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
        || binding.owner.is_empty()
        || !binding
            .owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || binding.number == 0
    {
        return Err(Error::invalid(
            "Enter a GitHub owner/repository, project owner, and positive board number",
        ));
    }
    let path = Path::new(&binding.checkout);
    if !path.is_absolute() || !path.is_dir() || binding.checkout.len() > 4096 {
        return Err(Error::invalid(
            "Checkout must be an existing absolute Git path on the server",
        ));
    }
    let output = crate::process::capture(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["remote", "get-url", "origin"]),
        16 * 1024,
        Duration::from_secs(10),
        "Git",
    )?;
    let origin = String::from_utf8_lossy(&output.bytes);
    let origin = origin.trim().trim_end_matches('/').trim_end_matches(".git");
    if !output.status.success()
        || ![
            format!("git@github.com:{}", binding.repository),
            format!("https://github.com/{}", binding.repository),
            format!("ssh://git@github.com/{}", binding.repository),
        ]
        .contains(&origin.to_owned())
    {
        return Err(Error::invalid(
            "Checkout origin must match the configured GitHub repository",
        ));
    }
    Ok(())
}
fn probe(binary: &Path, harness: Harness) -> HarnessStatus {
    let mut status = HarnessStatus {
        harness,
        executable: binary.display().to_string(),
        version: None,
        state: "missing".into(),
        detail: "Executable unavailable. Install the harness on the server or set its executable."
            .into(),
        checked_at: now(),
    };
    let capture = |args: &[&str]| {
        crate::process::capture(
            Command::new(binary).args(args),
            128 * 1024,
            Duration::from_secs(5),
            "Harness check",
        )
    };
    let Ok(version) = capture(&["--version"]) else {
        return status;
    };
    if !version.status.success() || version.truncated {
        return status;
    }
    status.version = Some(
        String::from_utf8_lossy(&version.bytes)
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect(),
    );
    let Ok(help) = capture(&["--help"]) else {
        status.state = "incompatible".into();
        status.detail = "Cannot check the executable interface".into();
        return status;
    };
    let help = String::from_utf8_lossy(&help.bytes);
    let compatible = match harness {
        Harness::Codex => help.contains("app-server"),
        Harness::ClaudeCode => {
            help.contains("--input-format")
                && help.contains("--include-partial-messages")
                && help.contains("--permission-mode")
        }
    };
    if !compatible {
        status.state = "incompatible".into();
        status.detail = "Update the harness: required streaming interface is unavailable".into();
        return status;
    }
    let args: &[&str] = match harness {
        Harness::Codex => &["login", "status"],
        Harness::ClaudeCode => &["auth", "status", "--json"],
    };
    status.state = "unknown".into();
    status.detail =
        "Authentication could not be checked; run the harness login command on the server".into();
    if let Ok(auth) = capture(args) {
        let logged_in = match harness {
            Harness::ClaudeCode => serde_json::from_slice::<serde_json::Value>(&auth.bytes)
                .ok()
                .and_then(|v| v["loggedIn"].as_bool()),
            Harness::Codex => Some(auth.status.success()),
        };
        match logged_in {
            Some(true) => {
                status.state = "ready".into();
                status.detail =
                    "Installed and authenticated. Model access is verified when a turn starts."
                        .into();
            }
            Some(false) => {
                status.state = "signed_out".into();
                status.detail = match harness {
                    Harness::Codex => "Run codex login on the server",
                    Harness::ClaudeCode => "Run claude auth login on the server",
                }
                .into();
            }
            _ => {}
        }
    }
    status
}
pub(super) async fn statuses(
    State(workspace): State<Workspace>,
) -> Result<Json<Vec<HarnessStatus>>, Error> {
    let _probe = workspace.harness_probe.lock().await;
    let snapshot = workspace.snapshots.borrow().clone();
    let config = configuration(&snapshot, "", &workspace.config);
    let cached = workspace
        .harness_status
        .lock()
        .map_err(Error::internal)?
        .clone();
    let paths = [config.codex.clone(), config.claude.clone()];
    if cached.len() == 2
        && cached.iter().zip(&paths).all(|(status, path)| {
            status.executable == path.display().to_string()
                && now().saturating_sub(status.checked_at) < 60
        })
    {
        return Ok(Json(cached));
    }
    let codex = tokio::task::spawn_blocking(move || probe(&paths[0], Harness::Codex));
    let claude = tokio::task::spawn_blocking(move || probe(&config.claude, Harness::ClaudeCode));
    let result = vec![
        codex.await.map_err(Error::internal)?,
        claude.await.map_err(Error::internal)?,
    ];
    *workspace.harness_status.lock().map_err(Error::internal)? = result.clone();
    Ok(Json(result))
}
pub(super) async fn refresh(
    State(workspace): State<Workspace>,
) -> Result<Json<Vec<HarnessStatus>>, Error> {
    workspace
        .harness_status
        .lock()
        .map_err(Error::internal)?
        .clear();
    statuses(State(workspace)).await
}
pub(super) fn selected_binary(
    snapshot: &Snapshot,
    harness: Harness,
    base: &RuntimeConfig,
) -> PathBuf {
    let config = configuration(snapshot, "", base);
    match harness {
        Harness::Codex => config.codex,
        Harness::ClaudeCode => config.claude,
    }
}
