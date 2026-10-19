//! Named workspaces, independent resources, and board membership.
use crate::*;

pub const PROTOCOL_VERSION: u32 = 14;

/// The most letters and digits in an automatically placed project's directory.
const PROJECT_DIRECTORY_LIMIT: usize = 64;

/// The directory name for a project created without an explicit root: the
/// name in lowercase, each run of characters other than letters, digits and
/// `_` collapsed to one `-`, with no leading or trailing `-`. The result has
/// no `/` or `.`, so it cannot name a parent or hidden directory. `None` when
/// the name has no letter or digit.
pub fn project_directory(name: &str) -> Option<String> {
    let mut directory = String::new();
    let mut count = 0;
    let mut separate = false;
    for c in name.trim().chars() {
        if !(c.is_alphanumeric() || c == '_') {
            separate = true;
            continue;
        }
        if count == PROJECT_DIRECTORY_LIMIT {
            break;
        }
        if separate && !directory.is_empty() {
            directory.push('-');
        }
        separate = false;
        directory.extend(c.to_lowercase());
        count += 1;
    }
    (!directory.is_empty()).then_some(directory)
}

/// Where a project named `name` lives when created without a root: below the
/// server's absolute `projects_root`. `None` when the base is not absolute or
/// the name yields no directory.
pub fn default_project_root(projects_root: &str, name: &str) -> Option<String> {
    let base = projects_root.trim();
    if !base.starts_with('/') {
        return None;
    }
    Some(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        project_directory(name)?
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardDiscovery {
    pub source: BoardSource,
    pub name: String,
    pub columns: Vec<BoardColumn>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationInput {
    pub operation_id: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationResult {
    pub key: String,
    pub result: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoardSource {
    Local,
    Github {
        owner: String,
        number: u64,
        url: String,
    },
    Gitlab {
        host: String,
        group: bool,
        path: String,
        number: u64,
        url: String,
    },
}

impl BoardSource {
    pub fn same_board(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local, Self::Local) => true,
            (
                Self::Github {
                    owner: a,
                    number: an,
                    ..
                },
                Self::Github {
                    owner: b,
                    number: bn,
                    ..
                },
            ) => a.eq_ignore_ascii_case(b) && an == bn,
            (
                Self::Gitlab {
                    host: a,
                    group: ag,
                    path: ap,
                    number: an,
                    ..
                },
                Self::Gitlab {
                    host: b,
                    group: bg,
                    path: bp,
                    number: bn,
                    ..
                },
            ) => a.eq_ignore_ascii_case(b) && ag == bg && ap == bp && an == bn,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub source: BoardSource,
    pub columns: Vec<BoardColumn>,
    pub last_synced_at: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardMembership {
    pub board_id: String,
    pub issue_id: String,
    pub column_ids: Vec<String>,
    pub remote_item_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectionInput {
    Repository { remote: String },
    Directory { path: String },
    Board { source: BoardSource },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectionKind {
    Repository {
        remote: String,
        checkout: Option<String>,
        owned: bool,
    },
    Directory {
        path: String,
    },
    Board {
        board_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Pending,
    Ready,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConnection {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub enabled: bool,
    pub state: ConnectionState,
    pub error: Option<String>,
    pub kind: ConnectionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWorkspace {
    pub connection_id: String,
    pub path: String,
    pub repository: bool,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub changes: Option<ChangeSet>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    pub local_id: String,
    pub remote_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPublication {
    pub issue_id: String,
    pub repository_connection_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishTarget {
    /// number == 0 requests a new remote board. Otherwise use that board.
    pub source: BoardSource,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    Clone {
        connection_id: String,
    },
    Sync {
        board_id: String,
    },
    Publish {
        board_id: String,
        target: PublishTarget,
        columns: Vec<ColumnMapping>,
        tasks: Vec<TaskPublication>,
    },
    EditTask {
        issue_id: String,
        title: String,
        body: String,
    },
    MoveTask {
        board_id: String,
        issue_id: String,
        column_id: String,
    },
    CreateTask {
        board_id: String,
        issue_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
    NeedsReconciliation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOperation {
    pub id: String,
    pub project_id: String,
    pub kind: OperationKind,
    pub state: OperationState,
    pub error: Option<String>,
    /// Durable provider results keyed by step; written before the next request.
    pub results: std::collections::BTreeMap<String, String>,
}

impl Issue {
    pub fn number(&self) -> u64 {
        self.reference.as_ref().map_or(0, |r| r.number)
    }
    pub fn label(&self) -> String {
        self.reference.as_ref().map_or_else(
            || "Local task".into(),
            |r| format!("{} #{}", r.repository, r.number),
        )
    }
}

pub fn local_columns() -> Vec<BoardColumn> {
    [
        ("backlog", "Backlog"),
        ("in_progress", "In Progress"),
        ("done", "Done"),
    ]
    .into_iter()
    .map(|(id, title)| BoardColumn {
        id: id.into(),
        title: title.into(),
    })
    .collect()
}

impl Snapshot {
    pub fn canonical_board_id<'a>(&'a self, id: &'a str) -> &'a str {
        resolve_alias(&self.board_aliases, id)
    }
    pub fn canonical_issue_id<'a>(&'a self, id: &'a str) -> &'a str {
        resolve_alias(&self.issue_aliases, id)
    }
    pub fn issue(&self, id: &str) -> Result<&Issue, String> {
        let id = self.canonical_issue_id(id);
        self.issues
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| "Task not found".into())
    }
    pub fn board(&self, id: &str) -> Result<&Board, String> {
        let id = self.canonical_board_id(id);
        self.boards
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| "Board not found".into())
    }
    pub fn board_active(&self, board_id: &str) -> bool {
        if self.canonical_board_id(board_id) != board_id {
            return false;
        }
        self.connections
            .iter()
            .find(|c| matches!(&c.kind,ConnectionKind::Board{board_id:id} if id==board_id))
            .is_none_or(|c| c.enabled)
    }
    pub fn visible_task(&self, issue_id: &str) -> bool {
        let issue_id = self.canonical_issue_id(issue_id);
        let Some(issue) = self.issues.iter().find(|i| i.id == issue_id) else {
            return false;
        };
        if self.boards.iter().any(|b| b.project_id == issue.project_id) {
            self.memberships.iter().any(|m| {
                m.issue_id == issue_id
                    && self.board_active(&m.board_id)
                    && self.boards.iter().any(|b| {
                        b.id == m.board_id
                            && m.column_ids
                                .iter()
                                .any(|c| b.columns.iter().any(|v| &v.id == c))
                    })
            })
        } else {
            self.projects.iter().any(|p| {
                p.id == issue.project_id && p.columns.iter().any(|c| c.id == issue.column_id)
            })
        }
    }
    /// Idempotent legacy upgrade. Existing keys and filesystem locations survive.
    pub fn migrate_projects(&mut self) {
        self.protocol_version = PROTOCOL_VERSION;
        for p in &mut self.projects {
            if self.boards.iter().any(|b| b.project_id == p.id) {
                continue;
            }
            let board_id = format!("board-{}", p.id);
            let source = p
                .github
                .as_ref()
                .map_or(BoardSource::Local, |g| BoardSource::Github {
                    owner: g.owner.clone(),
                    number: g.number,
                    url: g.url.clone(),
                });
            self.boards.push(Board {
                id: board_id.clone(),
                project_id: p.id.clone(),
                name: p.name.clone(),
                source,
                columns: p.columns.clone(),
                last_synced_at: p.github.as_ref().and_then(|g| g.last_synced_at),
                error: p.github.as_ref().and_then(|g| g.sync_error.clone()),
            });
            for i in self
                .issues
                .iter()
                .filter(|i| i.project_id == p.id && p.columns.iter().any(|c| c.id == i.column_id))
            {
                self.memberships.push(BoardMembership {
                    board_id: board_id.clone(),
                    issue_id: i.id.clone(),
                    column_ids: vec![i.column_id.clone()],
                    remote_item_id: None,
                });
            }
            if p.fixture {
                continue;
            }
            self.connections.push(ProjectConnection {
                id: format!("connection-{board_id}"),
                project_id: p.id.clone(),
                name: "Board".into(),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Board { board_id },
            });
            if let Some(binding) = self.bindings.iter().find(|b| {
                b.repository == p.repository
                    && p.github
                        .as_ref()
                        .is_some_and(|g| g.owner == b.owner && g.number == b.number)
            }) {
                let checkout = std::path::Path::new(&binding.checkout);
                p.root = checkout.parent().map(|r| r.to_string_lossy().into_owned());
                self.connections.push(ProjectConnection {
                    id: format!("repository-{}", p.id),
                    project_id: p.id.clone(),
                    name: p.repository.clone(),
                    enabled: true,
                    state: ConnectionState::Ready,
                    error: None,
                    kind: ConnectionKind::Repository {
                        remote: format!("git@github.com:{}.git", binding.repository),
                        checkout: Some(binding.checkout.clone()),
                        owned: false,
                    },
                });
            }
        }
    }
}

// Bound traversal even for malformed persisted data. Writers only create
// redirects within a project and retain the original records for history.
fn resolve_alias<'a>(
    aliases: &'a std::collections::BTreeMap<String, String>,
    id: &'a str,
) -> &'a str {
    let original = id;
    let mut current = id;
    for _ in 0..=aliases.len() {
        let Some(next) = aliases.get(current) else {
            return current;
        };
        current = next;
    }
    original
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    #[test]
    fn publication_aliases_keep_history_and_resolve_active_membership() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        snapshot.migrate_projects();
        let mut historical = snapshot.issues[0].clone();
        let canonical = historical.id.clone();
        historical.id = "historical-task".into();
        snapshot.issues.push(historical);
        snapshot
            .issue_aliases
            .insert("historical-task".into(), canonical.clone());
        assert!(snapshot.visible_task("historical-task"));
        assert_eq!(snapshot.canonical_issue_id("historical-task"), canonical);
        let mut board = snapshot.boards[0].clone();
        let canonical_board = board.id.clone();
        board.id = "historical-board".into();
        snapshot.boards.push(board);
        snapshot
            .board_aliases
            .insert("historical-board".into(), canonical_board.clone());
        assert!(!snapshot.board_active("historical-board"));
        assert_eq!(
            snapshot.board("historical-board").unwrap().id,
            canonical_board
        );
        assert_eq!(snapshot.issues.last().unwrap().id, "historical-task");
        let restored: Snapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(restored.issue_aliases, snapshot.issue_aliases);
    }

    #[test]
    fn malformed_alias_cycle_does_not_loop_or_select_an_arbitrary_task() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        snapshot
            .issue_aliases
            .extend([("a".into(), "b".into()), ("b".into(), "a".into())]);
        assert_eq!(snapshot.canonical_issue_id("a"), "a");
    }
}

#[cfg(test)]
mod project_directory_tests {
    use super::*;

    #[test]
    fn names_become_one_safe_lowercase_directory() {
        assert_eq!(project_directory("mosaic").as_deref(), Some("mosaic"));
        assert_eq!(
            project_directory("  My Project: v2 ").as_deref(),
            Some("my-project-v2")
        );
        assert_eq!(project_directory("a__b").as_deref(), Some("a__b"));
        for unsafe_name in ["../..", "..", ".", "/", "/etc/passwd ", "a/../b"] {
            let directory = project_directory(unsafe_name);
            assert!(
                directory
                    .as_deref()
                    .is_none_or(|d| !d.contains(['/', '.']) && !d.starts_with('-')),
                "{unsafe_name}: {directory:?}"
            );
        }
        assert_eq!(project_directory("../etc").as_deref(), Some("etc"));
        assert_eq!(project_directory("").as_deref(), None);
        assert_eq!(project_directory(" -- ").as_deref(), None);
        assert_eq!(project_directory(&"a".repeat(200)).unwrap().len(), 64);
    }

    #[test]
    fn the_default_root_needs_an_absolute_base_and_a_usable_name() {
        let base = "/srv/relay/projects";
        assert_eq!(
            default_project_root(base, "Mosaic").as_deref(),
            Some("/srv/relay/projects/mosaic")
        );
        assert_eq!(
            default_project_root("/srv/relay/projects/", "Mosaic").as_deref(),
            Some("/srv/relay/projects/mosaic")
        );
        assert_eq!(default_project_root("", "Mosaic"), None);
        assert_eq!(default_project_root("projects", "Mosaic"), None);
        assert_eq!(default_project_root(base, "!!!"), None);
    }

    #[test]
    fn build_info_without_a_projects_root_keeps_its_old_shape() {
        let build = BuildInfo::current();
        let json = serde_json::to_value(&build).unwrap();
        assert!(json.get("projects_root").is_none());
        let with_root = BuildInfo {
            projects_root: "/srv/projects".into(),
            ..build.clone()
        };
        let round: BuildInfo =
            serde_json::from_value(serde_json::to_value(&with_root).unwrap()).unwrap();
        assert_eq!(round, with_root);
        let old: BuildInfo = serde_json::from_value(json).unwrap();
        assert_eq!(old, build);
    }
}
