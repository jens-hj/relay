//! Build output optionally shared across session worktrees and reclaimed when sessions are archived.
use crate::Error;
use relay_core::Snapshot;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

/// Set to `1`, `true`, `on`, or `yes` to share one Cargo target directory across a
/// project's worktrees. Off by default: concurrent builds of the same crates in
/// different worktrees overwrite each other's binaries in a shared directory.
pub(crate) const SHARED_CACHE_ENV: &str = "RELAY_SHARED_BUILD_CACHE";
const CARGO_TARGET_DIR: &str = "CARGO_TARGET_DIR";

/// The per-project Cargo target directory when the shared cache is enabled and the
/// harness environment does not already choose one.
pub(crate) fn shared_target(
    project_root: &Path,
    existing: Option<&OsStr>,
    setting: Option<&OsStr>,
) -> Option<PathBuf> {
    if existing.is_some_and(|value| !value.is_empty()) {
        return None;
    }
    if !setting.is_some_and(|value| {
        matches!(
            value.to_string_lossy().trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        )
    }) {
        return None;
    }
    Some(
        project_root
            .join(".relay")
            .join("cache")
            .join("cargo-target"),
    )
}

/// When enabled, points a harness at the project's shared target directory and returns it so the
/// sandbox can make it writable. Failure to create it leaves the default behaviour.
pub(crate) fn configure(
    cmd: &mut tokio::process::Command,
    project_root: Option<&str>,
    existing: Option<&OsStr>,
    setting: Option<&OsStr>,
) -> Option<String> {
    let root = Path::new(project_root?).canonicalize().ok()?;
    let target = shared_target(&root, existing, setting)?;
    std::fs::create_dir_all(&target).ok()?;
    cmd.env(CARGO_TARGET_DIR, &target);
    Some(target.display().to_string())
}

/// Project roots and repository worktrees whose build output an archive may reclaim.
pub(crate) fn archived_worktrees(snapshot: &Snapshot, session_id: &str) -> Vec<(PathBuf, PathBuf)> {
    let branch = snapshot.session_branch_ids(session_id);
    let mut targets = Vec::new();
    for session in snapshot
        .sessions
        .iter()
        .filter(|s| branch.contains(&s.id) && s.archived && !s.fixture)
    {
        let Some(root) = snapshot
            .projects
            .iter()
            .find(|p| p.id == session.project_id)
            .and_then(|p| p.root.as_ref())
        else {
            continue;
        };
        for space in session.workspaces.iter().filter(|w| w.repository) {
            let entry = (PathBuf::from(root), PathBuf::from(&space.path));
            if !targets.contains(&entry) {
                targets.push(entry);
            }
        }
    }
    targets
}

/// Removes wholly ignored `target/` directories from a Relay-owned session worktree.
/// Tracked files, untracked source files, the worktree, and its branch are kept.
/// Missing paths and repeated calls are no-ops.
pub(crate) fn reclaim(project_root: &Path, worktree: &Path) -> Result<Vec<PathBuf>, Error> {
    let (Ok(root), Ok(worktree)) = (project_root.canonicalize(), worktree.canonicalize()) else {
        return Ok(vec![]);
    };
    // Connected directories and checkouts outside Relay's session directories are never touched.
    if !worktree.starts_with(root.join(".relay").join("sessions"))
        || !worktree.join(".git").is_file()
    {
        return Ok(vec![]);
    }
    let output = crate::process::capture(
        ProcessCommand::new("git").arg("-C").arg(&worktree).args([
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ]),
        1024 * 1024,
        std::time::Duration::from_secs(30),
        "Git build output scan",
    )?;
    if output.truncated || !output.status.success() {
        return Err(Error::invalid("Cannot list ignored build output"));
    }
    let mut removed = Vec::new();
    for entry in output.bytes.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let Ok(relative) = std::str::from_utf8(entry) else {
            continue;
        };
        // `--directory` only collapses directories with no tracked or unignored files.
        let Some(relative) = relative.strip_suffix('/') else {
            continue;
        };
        let relative = Path::new(relative);
        if relative.file_name() != Some(OsStr::new("target"))
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            continue;
        }
        let path = worktree.join(relative);
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => removed.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::internal(error)),
        }
    }
    Ok(removed)
}

/// Reclaims archived worktrees off the request path; failures only leave disk in use.
pub(crate) fn reclaim_archived(snapshot: &Snapshot, session_id: &str) {
    let targets = archived_worktrees(snapshot, session_id);
    if targets.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for (root, worktree) in targets {
            if let Err(error) = reclaim(&root, &worktree) {
                eprintln!(
                    "Cannot reclaim build output in {}: {}",
                    worktree.display(),
                    error.message
                );
            }
        }
    });
}
