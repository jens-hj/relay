//! Fixtures exercise the real bounded CLI subprocess boundary, not a mock request
//! function. Each process verifies its arguments and returns one provider response.
use super::*;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

struct Fake {
    directory: TempDir,
    executable: PathBuf,
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
impl Fake {
    fn new(steps: Vec<(&str, Value, i32)>) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fake-provider");
        let count = directory.path().join("count");
        let log = directory.path().join("log");
        let mut script = format!(
            "#!/bin/sh\n[ -z \"${{RELAY_TOKEN+x}}\" ] || exit 99\nn=0\n[ ! -f {0} ] || n=$(cat {0})\nn=$((n+1))\nprintf '%s' \"$n\" > {0}\nprintf '%s\\n' \"$*\" >> {1}\ncase $n in\n",
            quote(&count.to_string_lossy()),
            quote(&log.to_string_lossy())
        );
        for (index, (expected, value, status)) in steps.into_iter().enumerate() {
            script.push_str(&format!(
                "{})\ncase \"$*\" in *{}*) ;; *) exit 98;; esac\nprintf '%s' {}\nexit {};;\n",
                index + 1,
                quote(expected),
                quote(&value.to_string()),
                status
            ));
        }
        script.push_str("*) exit 97;;\nesac\n");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            executable,
        }
    }
    fn config(&self) -> RuntimeConfig {
        RuntimeConfig {
            gh: self.executable.clone(),
            glab: self.executable.clone(),
            ..RuntimeConfig::default()
        }
    }
    fn calls(&self) -> String {
        fs::read_to_string(self.directory.path().join("log")).unwrap_or_default()
    }
}
fn gh_source() -> BoardSource {
    BoardSource::Github {
        owner: "owner".into(),
        number: 1,
        url: String::new(),
    }
}
fn gl_source(group: bool) -> BoardSource {
    BoardSource::Gitlab {
        host: "gitlab.example".into(),
        group,
        path: if group { "group" } else { "group/repo" }.into(),
        number: 2,
        url: String::new(),
    }
}
fn page(nodes: Value, next: Option<&str>) -> Value {
    json!({"nodes":nodes,"pageInfo":{"hasNextPage":next.is_some(),"endCursor":next}})
}
fn gh_metadata() -> Vec<(&'static str, Value, i32)> {
    vec![
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        (
            "projectV2(number",
            json!({"data":{"user":{"projectV2":{"id":"BOARD","title":"Remote","url":"https://github.com/users/owner/projects/1","viewerCanUpdate":true}}}}),
            0,
        ),
        (
            "fields(first",
            json!({"data":{"node":{"fields":page(json!([{"id":"FIELD","name":"Status","options":[{"id":"todo","name":"Todo"},{"id":"done","name":"Done"}]}]),None)}}}),
            0,
        ),
    ]
}
fn gh_issue(number: u64) -> Value {
    json!({"id":format!("NODE{number}"),"number":number,"title":format!("Task {number}"),"body":"Untrusted $(command) `text`","url":format!("https://github.com/elsewhere/repo/issues/{number}"),"repository":{"nameWithOwner":"elsewhere/repo"},"__typename":"Issue"})
}
fn gh_item(number: u64, status: Option<&str>) -> Value {
    json!({"id":format!("ITEM{number}"),"type":"ISSUE","content":gh_issue(number),"fieldValueByName":{"optionId":status}})
}
fn gl_metadata() -> Vec<(&'static str, Value, i32)> {
    vec![
        (
            "boards/2",
            json!({"id":2,"name":"GitLab","hide_backlog_list":false,"hide_closed_list":false,"milestone":null,"labels":[],"weight":null}),
            0,
        ),
        (
            "lists?per_page=100&page=1",
            json!([{"id":3,"label":{"name":"First"}},{"id":4,"label":{"name":"Second"}}]),
            0,
        ),
        (
            "--method GET",
            json!({"permissions":{"project_access":{"access_level":40}}}),
            0,
        ),
    ]
}
fn gl_issue(iid: u64, state: &str, labels: &[&str]) -> Value {
    json!({"id":iid+100,"iid":iid,"state":state,"title":"Issue","description":null,"labels":labels,"web_url":format!("https://gitlab.example/group/repo/-/issues/{iid}")})
}

#[test]
fn github_paginates_fields_items_and_labels_including_external_repositories_and_empty_status() {
    let mut steps = gh_metadata();
    steps[2].1["data"]["node"]["fields"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"FIELDS2"});
    steps.push((
        "after=FIELDS2",
        json!({"data":{"node":{"fields":page(json!([]),None)}}}),
        0,
    ));
    steps.push((
        "items(first",
        json!({"data":{"node":{"items":page(json!([gh_item(1,None)]),Some("ITEMS2"))}}}),
        0,
    ));
    steps.push((
        "labels(first",
        json!({"data":{"node":{"labels":page(json!([{"name":"one"}]),Some("LABELS2"))}}}),
        0,
    ));
    steps.push((
        "after=LABELS2",
        json!({"data":{"node":{"labels":page(json!([{"name":"two"}]),None)}}}),
        0,
    ));
    steps.push((
        "after=ITEMS2",
        json!({"data":{"node":{"items":page(json!([gh_item(2,Some("todo"))]),None)}}}),
        0,
    ));
    steps.push((
        "labels(first",
        json!({"data":{"node":{"labels":page(json!([]),None)}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let remote = metadata(&fake.config(), &gh_source()).unwrap();
    let tasks = tasks(&fake.config(), &remote).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].reference.repository, "elsewhere/repo");
    assert_eq!(tasks[0].columns, [NO_STATUS]);
    assert_eq!(tasks[0].labels, ["one", "two"]);
    assert_eq!(fake.calls().lines().count(), 9);
}
#[test]
fn missing_github_status_field_exposes_no_status_column() {
    let mut steps = gh_metadata();
    steps[2].1 = json!({"data":{"node":{"fields":page(json!([]),None)}}});
    let fake = Fake::new(steps);
    let board = discover(&fake.config(), &gh_source()).unwrap();
    assert_eq!(
        board.columns,
        vec![BoardColumn {
            id: NO_STATUS.into(),
            title: "No status".into()
        }]
    );
}
#[test]
fn repeated_github_cursor_and_auth_failure_are_errors() {
    let mut steps = gh_metadata();
    steps[2].1["data"]["node"]["fields"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"same"});
    steps.push(("after=same", steps[2].1.clone(), 0));
    let fake = Fake::new(steps);
    assert!(metadata(&fake.config(), &gh_source()).is_err());
    let fake = Fake::new(vec![("users/owner", json!({}), 1)]);
    assert!(
        metadata(&fake.config(), &gh_source())
            .unwrap_err()
            .to_string()
            .contains("authentication")
    );
}
#[test]
fn same_issue_multiple_boards_preserves_task_id_results_and_removed_history() {
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.boards.clear();
    snapshot.memberships.clear();
    let mut steps = gh_metadata();
    steps.extend(gh_metadata());
    let fake = Fake::new(steps);
    let mut first = metadata(&fake.config(), &gh_source()).unwrap();
    let mut second = metadata(&fake.config(), &gh_source()).unwrap();
    first.board.project_id = "project".into();
    first.board.id = "one".into();
    second.board.project_id = "project".into();
    second.board.id = "two".into();
    snapshot
        .boards
        .extend([first.board.clone(), second.board.clone()]);
    let incoming = || RemoteTask {
        reference: reference_for(&gh_source(), "elsewhere/repo", 1),
        title: "Task".into(),
        body: "body".into(),
        labels: vec![],
        columns: vec!["todo".into()],
        item_id: "item".into(),
    };
    merge(&mut snapshot, "one", &first, vec![incoming()]).unwrap();
    let issue_id = snapshot.memberships[0].issue_id.clone();
    snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == issue_id)
        .unwrap()
        .result = Some("history".into());
    merge(&mut snapshot, "two", &second, vec![incoming()]).unwrap();
    assert_eq!(snapshot.memberships[1].issue_id, issue_id);
    merge(&mut snapshot, "one", &first, vec![]).unwrap();
    assert_eq!(snapshot.memberships.len(), 1);
    assert_eq!(
        snapshot
            .issues
            .iter()
            .find(|i| i.id == issue_id)
            .unwrap()
            .result
            .as_deref(),
        Some("history")
    );
    let mut different_host = incoming();
    different_host.reference.provider = Provider::Gitlab;
    different_host.reference.url = "https://another.host/elsewhere/repo/-/issues/1".into();
    merge(&mut snapshot, "one", &first, vec![different_host]).unwrap();
    assert_ne!(snapshot.memberships[1].issue_id, issue_id);
}
#[test]
fn gitlab_group_board_imports_multiple_label_memberships_closed_and_open() {
    let mut steps = gl_metadata();
    steps.push((
        "groups/group/issues?state=all",
        json!([
            gl_issue(1, "opened", &["First", "Second"]),
            gl_issue(2, "closed", &["First"]),
            gl_issue(3, "opened", &[])
        ]),
        0,
    ));
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gl_source(true)).unwrap();
    assert!(
        matches!(&board.board.source, BoardSource::Gitlab { url, .. } if url == "https://gitlab.example/groups/group/-/boards/2")
    );
    let tasks = tasks(&fake.config(), &board).unwrap();
    assert_eq!(tasks[0].columns, ["gitlab-list-3", "gitlab-list-4"]);
    assert_eq!(tasks[1].columns, ["gitlab-closed"]);
    assert_eq!(tasks[2].columns, ["gitlab-open"]);
    assert_eq!(tasks[0].body, "");
}
#[test]
fn gitlab_full_issue_page_requires_second_page() {
    let mut steps = gl_metadata();
    steps.push((
        "page=1",
        json!(
            (1..=100)
                .map(|n| gl_issue(n, "opened", &[]))
                .collect::<Vec<_>>()
        ),
        0,
    ));
    steps.push(("page=2", json!([gl_issue(101, "closed", &[])]), 0));
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gl_source(false)).unwrap();
    assert_eq!(tasks(&fake.config(), &board).unwrap().len(), 101);
}
#[test]
fn gitlab_unsupported_scopes_and_list_types_fail_explicitly() {
    let fake = Fake::new(vec![("boards/2", json!({"milestone":{"id":3}}), 0)]);
    assert!(
        metadata(&fake.config(), &gl_source(false))
            .unwrap_err()
            .to_string()
            .contains("scope")
    );
    let mut steps = gl_metadata();
    steps[1].1 = json!([{"id":1,"assignee":{"id":7},"list_type":"assignee"}]);
    let fake = Fake::new(steps);
    assert!(
        metadata(&fake.config(), &gl_source(false))
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
}
#[test]
fn gitlab_move_preserves_unrelated_labels_and_uses_explicit_state() {
    let mut steps = gl_metadata();
    steps.push(("issues/1", gl_issue(1, "opened", &["First", "other"]), 0));
    steps.push(("add_labels=Second", json!({}), 0));
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gl_source(false)).unwrap();
    gitlab::move_task(
        &fake.config(),
        &board,
        &reference_for(&gl_source(false), "group/repo", 1),
        "gitlab-list-4",
    )
    .unwrap();
    let calls = fake.calls();
    assert!(calls.contains("remove_labels=First"));
    assert!(!calls.contains("remove_labels=other"));
    assert!(calls.contains("state_event=reopen"));
}
#[test]
fn github_remote_edit_uses_api_fields_without_shell_interpretation() {
    let fake = Fake::new(vec![("--method PATCH", json!({}), 0)]);
    edit(
        &fake.config(),
        &reference_for(&gh_source(), "owner/repo", 1),
        "$(echo bad)",
        "`body`",
    )
    .unwrap();
    assert!(fake.calls().contains("title=$(echo bad)"));
    assert!(fake.calls().contains("body=`body`"));
}
#[test]
fn gitlab_edit_uses_correct_host_repository_and_description_field() {
    let fake = Fake::new(vec![("description=New body", json!({}), 0)]);
    edit(
        &fake.config(),
        &reference_for(&gl_source(false), "group/repo", 1),
        "New title",
        "New body",
    )
    .unwrap();
    assert!(
        fake.calls()
            .contains("--hostname gitlab.example projects/group%2Frepo/issues/1 --method PUT")
    );
}

async fn workspace(config: RuntimeConfig) -> (TempDir, Workspace) {
    let temp = tempfile::tempdir().unwrap();
    let (_, shutdown) = crate::router_with_shutdown(
        temp.path().join("db"),
        "test-provider-token".into(),
        DirectorProfile::default(),
        config,
    )
    .unwrap();
    (temp, shutdown.workspace)
}
fn sample_operation(kind: OperationKind) -> ProjectOperation {
    ProjectOperation {
        id: "operation".into(),
        project_id: "project".into(),
        kind,
        state: OperationState::Running,
        error: None,
        results: Default::default(),
    }
}
#[tokio::test]
async fn unknown_write_outcome_does_not_replay_and_known_result_resumes() {
    let fake = Fake::new(vec![("repos/owner/repo/issues", json!({}), 1)]);
    let (_temp, workspace) = workspace(fake.config()).await;
    workspace
        .update_project(|s| {
            s.operations.push(sample_operation(OperationKind::Sync {
                board_id: "unused".into(),
            }));
            Ok(())
        })
        .unwrap();
    let create = || {
        github::rest(
            &workspace.config,
            "repos/owner/repo/issues",
            "POST",
            json!({"title":"Title"}),
        )?;
        Ok("created".into())
    };
    assert_eq!(
        step(&workspace, "operation", "issue/task", create)
            .unwrap_err()
            .code,
        "needs_reconciliation"
    );
    assert_eq!(
        step(&workspace, "operation", "issue/task", || panic!(
            "must not replay"
        ))
        .unwrap_err()
        .code,
        "needs_reconciliation"
    );
    journal(&workspace, "operation", "issue/task", "known".into()).unwrap();
    assert_eq!(
        step(&workspace, "operation", "issue/task", || panic!(
            "must not duplicate"
        ))
        .unwrap(),
        "known"
    );
    assert_eq!(fake.calls().lines().count(), 1);
}
#[tokio::test]
async fn failed_sync_keeps_last_good_data_and_reports_error() {
    let fake = Fake::new(vec![("users/owner", json!({}), 1)]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let board = Board {
        id: "board".into(),
        project_id: "project".into(),
        name: "Good".into(),
        source: gh_source(),
        columns: local_columns(),
        last_synced_at: Some(42),
        error: None,
    };
    workspace
        .update_project(|s| {
            s.boards.push(board.clone());
            s.operations.push(sample_operation(OperationKind::Sync {
                board_id: "board".into(),
            }));
            Ok(())
        })
        .unwrap();
    assert!(execute(&workspace, "operation").is_err());
    let snapshot = workspace.snapshots.borrow();
    let retained = snapshot.board("board").unwrap();
    assert_eq!(retained.columns, board.columns);
    assert_eq!(retained.last_synced_at, Some(42));
    assert_eq!(retained.name, "Good");
    assert!(retained.error.is_some());
    assert_eq!(
        operation(&snapshot, "operation").unwrap().state,
        OperationState::Failed
    );
}
#[tokio::test]
async fn publish_validates_every_mapping_before_any_write() {
    let fake = Fake::new(gh_metadata());
    let (_temp, workspace) = workspace(fake.config()).await;
    let op = sample_operation(OperationKind::Publish {
        board_id: "local".into(),
        target: PublishTarget {
            source: gh_source(),
            name: "Destination".into(),
        },
        columns: vec![],
        tasks: vec![],
    });
    workspace
        .update_project(|s| {
            s.boards.push(Board {
                id: "local".into(),
                project_id: "project".into(),
                name: "Local".into(),
                source: BoardSource::Local,
                columns: local_columns(),
                last_synced_at: None,
                error: None,
            });
            s.operations.push(op.clone());
            Ok(())
        })
        .unwrap();
    assert!(
        execute(&workspace, "operation")
            .unwrap_err()
            .to_string()
            .contains("every local column")
    );
    assert!(!fake.calls().contains("mutation"));
    assert_eq!(
        workspace.snapshots.borrow().board("local").unwrap().source,
        BoardSource::Local
    );
}
#[test]
fn repository_host_project_scope_and_reference_url_are_validated() {
    let fake = Fake::new(vec![]);
    let reference = reference_for(&gl_source(false), "outside/repo", 1);
    assert!(
        check_destination(&fake.config(), &gl_source(false), &reference)
            .unwrap_err()
            .to_string()
            .contains("outside")
    );
    assert!(
        check_destination(&fake.config(), &gh_source(), &reference)
            .unwrap_err()
            .to_string()
            .contains("provider/host")
    );
    let mut bad = reference_for(&gh_source(), "owner/repo", 1);
    bad.url = "https://github.com/other/repo/issues/1".into();
    assert!(validate_reference(&bad).is_err());
    assert!(fake.calls().is_empty());
}

fn pending_task() -> Issue {
    Issue {
        id: "task".into(),
        project_id: "project".into(),
        reference: None,
        title: "Local title".into(),
        body: "Only explicit task body".into(),
        column_id: "todo".into(),
        labels: vec![],
        result: Some("Private agent result".into()),
        repository_connection_id: Some("repository".into()),
    }
}

fn reconciliation_snapshot(kind: OperationKind, key: &str) -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.issues.push(pending_task());
    snapshot.boards.push(board_record(gh_source()));
    snapshot.connections.push(repository_connection());
    let mut op = sample_operation(kind);
    op.state = OperationState::NeedsReconciliation;
    op.error = Some("Remote acceptance unknown".into());
    op.results.insert("pending".into(), key.into());
    snapshot.operations.push(op);
    snapshot
}

fn reconcile(
    snapshot: &mut Snapshot,
    key: &str,
    result: &str,
) -> Result<Option<crate::Action>, Error> {
    crate::projects::apply(
        snapshot,
        relay_core::Command::ReconcileOperation {
            operation_id: "operation".into(),
            key: key.into(),
            result: result.into(),
        },
        "recovery-request",
        DirectorProfile::default(),
        &RuntimeConfig::default(),
    )
}

fn creation() -> OperationKind {
    OperationKind::CreateTask {
        board_id: "board".into(),
        issue_id: "task".into(),
    }
}

fn new_publication() -> OperationKind {
    let mut source = gh_source();
    if let BoardSource::Github { number, .. } = &mut source {
        *number = 0;
    }
    OperationKind::Publish {
        board_id: "local".into(),
        target: PublishTarget {
            source,
            name: "New board".into(),
        },
        tasks: vec![],
        columns: vec![],
    }
}

#[test]
fn reconciliation_rejects_untyped_or_unrelated_results_without_changing_pending_history() {
    let issue = encode(&reference_for(&gh_source(), "elsewhere/repo", 1)).unwrap();
    let wrong_repo = encode(&reference_for(&gh_source(), "other/repo", 1)).unwrap();
    let wrong_provider = encode(&reference_for(&gl_source(false), "group/repo", 1)).unwrap();
    let cases = vec![
        (creation(), "issue/task", "pending", "issue/task".into()),
        (creation(), "issue/task", "issue/task", "not JSON".into()),
        (creation(), "issue/task", "issue/task", wrong_repo),
        (creation(), "issue/task", "issue/task", wrong_provider),
        (creation(), "issue/task", "status/task", "confirmed".into()),
        (creation(), "issue/task", "issue/other-task", issue),
        (
            creation(),
            "column/todo",
            "column/todo",
            "remote-status".into(),
        ),
        (
            creation(),
            "unknown/task",
            "unknown/task",
            "confirmed".into(),
        ),
        (creation(), "edit/task", "edit/task", "confirmed".into()),
        (creation(), "status/task", "status/task", "success".into()),
        (creation(), "membership/task", "membership/task", " ".into()),
        (creation(), "board", "board", encode(&gh_source()).unwrap()),
        (
            new_publication(),
            "board",
            "board",
            encode(&BoardSource::Local).unwrap(),
        ),
        (
            new_publication(),
            "board",
            "board",
            encode(&gl_source(false)).unwrap(),
        ),
        (
            new_publication(),
            "board",
            "board",
            encode(&BoardSource::Github {
                owner: "other".into(),
                number: 2,
                url: String::new(),
            })
            .unwrap(),
        ),
        (
            new_publication(),
            "board",
            "board",
            encode(&BoardSource::Github {
                owner: "owner".into(),
                number: 0,
                url: String::new(),
            })
            .unwrap(),
        ),
    ];
    for (kind, pending, key, value) in cases {
        let mut snapshot = reconciliation_snapshot(kind, pending);
        let before = serde_json::to_value(&snapshot).unwrap();
        assert!(
            reconcile(&mut snapshot, key, &value).is_err(),
            "accepted {key}: {value}"
        );
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            before,
            "mutated {key}"
        );
    }
}

#[test]
fn reconciliation_accepts_only_typed_results_for_the_exact_operation_step() {
    let cases = vec![
        (
            creation(),
            "issue/task",
            encode(&reference_for(&gh_source(), "elsewhere/repo", 1)).unwrap(),
        ),
        (creation(), "membership/task", "ITEM1".into()),
        (creation(), "status/task", "confirmed".into()),
        (
            OperationKind::MoveTask {
                board_id: "board".into(),
                issue_id: "task".into(),
                column_id: "todo".into(),
            },
            "status/task",
            "confirmed".into(),
        ),
        (
            OperationKind::EditTask {
                issue_id: "task".into(),
                title: "Title".into(),
                body: "Body".into(),
            },
            "edit/task",
            "confirmed".into(),
        ),
        (new_publication(), "board", encode(&gh_source()).unwrap()),
    ];
    for (kind, key, value) in cases {
        let mut snapshot = reconciliation_snapshot(kind, key);
        let action = reconcile(&mut snapshot, key, &value).unwrap();
        assert!(matches!(action, Some(crate::Action::Operations(ids)) if ids == ["operation"]));
        let operation = &snapshot.operations[0];
        assert_eq!(operation.state, OperationState::Pending);
        assert_eq!(operation.error, None);
        assert_eq!(operation.results.get(key), Some(&value));
        assert_eq!(
            operation.results.get("pending").map(String::as_str),
            Some(key)
        );
        // A second submission cannot change the accepted result or restart the job.
        let before = serde_json::to_value(&snapshot).unwrap();
        assert!(reconcile(&mut snapshot, key, "different").is_err());
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), before);
        snapshot.operations[0].state = OperationState::NeedsReconciliation;
        let before = serde_json::to_value(&snapshot).unwrap();
        assert!(reconcile(&mut snapshot, key, &value).is_err());
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), before);
    }
}
fn repository_connection() -> ProjectConnection {
    ProjectConnection {
        id: "repository".into(),
        project_id: "project".into(),
        name: "Repository".into(),
        enabled: true,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Repository {
            remote: "git@github.com:elsewhere/repo.git".into(),
            checkout: Some("/unused".into()),
            owned: false,
        },
    }
}
fn board_record(source: BoardSource) -> Board {
    Board {
        id: "board".into(),
        project_id: "project".into(),
        name: "Local".into(),
        source,
        columns: vec![BoardColumn {
            id: "todo".into(),
            title: "Todo".into(),
        }],
        last_synced_at: Some(5),
        error: None,
    }
}
fn membership_record() -> BoardMembership {
    BoardMembership {
        board_id: "board".into(),
        issue_id: "task".into(),
        column_ids: vec!["todo".into()],
        remote_item_id: None,
    }
}
fn writable_repo() -> Value {
    json!({"permissions":{"push":true},"has_issues":true})
}
fn install_task(workspace: &Workspace, op: ProjectOperation, task: Issue, board: Board) {
    workspace
        .update_project(|s| {
            s.boards.push(board);
            s.connections.push(repository_connection());
            s.memberships.push(membership_record());
            s.issues.push(task);
            s.operations.push(op);
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn create_task_reconciles_unknown_issue_creation_without_duplicate_and_preserves_local_id() {
    let mut steps = gh_metadata();
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push(("--method POST", json!({}), 1)); // unknown acceptance
    steps.extend(gh_metadata());
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push((
        "repos/elsewhere/repo/issues/1",
        json!({"node_id":"NODE1"}),
        0,
    ));
    steps.push((
        "addProjectV2ItemById",
        json!({"data":{"addProjectV2ItemById":{"item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push((
        "updateProjectV2ItemFieldValue",
        json!({"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push((
        "items(first",
        json!({"data":{"node":{"items":page(json!([gh_item(1,Some("todo"))]),None)}}}),
        0,
    ));
    steps.push((
        "labels(first",
        json!({"data":{"node":{"labels":page(json!([]),None)}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    install_task(
        &workspace,
        sample_operation(OperationKind::CreateTask {
            board_id: "board".into(),
            issue_id: "task".into(),
        }),
        pending_task(),
        board_record(gh_source()),
    );
    assert!(execute(&workspace, "operation").is_err());
    assert_eq!(
        operation(&workspace.snapshots.borrow(), "operation")
            .unwrap()
            .state,
        OperationState::NeedsReconciliation
    );
    assert!(
        workspace
            .snapshots
            .borrow()
            .issues
            .iter()
            .find(|i| i.id == "task")
            .unwrap()
            .reference
            .is_none()
    );
    workspace
        .update_project(|snapshot| {
            let action = reconcile(
                snapshot,
                "issue/task",
                &encode(&reference_for(&gh_source(), "elsewhere/repo", 1))?,
            )?;
            assert!(matches!(action, Some(crate::Action::Operations(_))));
            Ok(())
        })
        .unwrap();
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    let task = snapshot.issues.iter().find(|i| i.id == "task").unwrap();
    assert_eq!(task.reference.as_ref().unwrap().number, 1);
    assert_eq!(task.result.as_deref(), Some("Private agent result"));
    assert_eq!(
        snapshot
            .memberships
            .iter()
            .find(|m| m.issue_id == "task")
            .unwrap()
            .remote_item_id
            .as_deref(),
        Some("ITEM1")
    );
    assert_eq!(
        operation(&snapshot, "operation").unwrap().state,
        OperationState::Completed
    );
    assert_eq!(
        fake.calls()
            .lines()
            .filter(|s| s.contains("--method POST"))
            .count(),
        1
    );
    assert!(!fake.calls().contains("Private agent result"));
    assert!(fake.calls().contains("relay-operation:operation:task:task"));
}
#[tokio::test]
async fn publication_reuses_existing_issue_and_fetches_preexisting_destination_tasks() {
    let mut steps = gh_metadata();
    for _ in 0..3 {
        steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    }
    steps.push((
        "repos/elsewhere/repo/issues/1",
        json!({"node_id":"NODE1"}),
        0,
    ));
    steps.push((
        "addProjectV2ItemById",
        json!({"data":{"addProjectV2ItemById":{"item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push((
        "updateProjectV2ItemFieldValue",
        json!({"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push(("items(first",json!({"data":{"node":{"items":page(json!([gh_item(1,Some("todo")),gh_item(2,None)]),None)}}}),0));
    for _ in 0..2 {
        steps.push((
            "labels(first",
            json!({"data":{"node":{"labels":page(json!([]),None)}}}),
            0,
        ));
    }
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    let op = sample_operation(OperationKind::Publish {
        board_id: "board".into(),
        target: PublishTarget {
            source: gh_source(),
            name: "Remote".into(),
        },
        columns: vec![ColumnMapping {
            local_id: "todo".into(),
            remote_id: "todo".into(),
        }],
        tasks: vec![TaskPublication {
            issue_id: "task".into(),
            repository_connection_id: "repository".into(),
        }],
    });
    install_task(&workspace, op, task, board_record(BoardSource::Local));
    workspace
        .update_project(|s| {
            let mut imported = s.issues.iter().find(|i| i.id == "task").unwrap().clone();
            imported.id = "imported-history".into();
            imported.result = Some("Historical result".into());
            s.issues.insert(0, imported);
            let mut existing = board_record(gh_source());
            existing.id = "existing-destination".into();
            s.boards.push(existing);
            s.connections.push(ProjectConnection {
                id: "existing-board-connection".into(),
                project_id: "project".into(),
                name: "Remote".into(),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Board {
                    board_id: "existing-destination".into(),
                },
            });
            s.memberships.push(BoardMembership {
                board_id: "existing-destination".into(),
                issue_id: "imported-history".into(),
                column_ids: vec!["todo".into()],
                remote_item_id: Some("ITEM1".into()),
            });
            s.sessions[0].issue_id = Some("imported-history".into());
            s.directors[0].overrides.scope = Some(DirectorScope::Issues {
                issue_ids: vec!["imported-history".into()],
            });
            Ok(())
        })
        .unwrap();
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    assert!(matches!(
        snapshot.board("board").unwrap().source,
        BoardSource::Github { .. }
    ));
    assert_eq!(
        snapshot
            .memberships
            .iter()
            .filter(|m| m.board_id == "board")
            .count(),
        2
    );
    assert_eq!(
        snapshot
            .issues
            .iter()
            .find(|i| i.id == "task")
            .unwrap()
            .result
            .as_deref(),
        Some("Private agent result")
    );
    assert!(!fake.calls().contains("--method POST"));
    assert!(
        operation(&snapshot, "operation")
            .unwrap()
            .results
            .contains_key("membership/task")
    );
    drop(snapshot);
    execute(&workspace, "operation").unwrap(); // completed publication does not replay
    let snapshot = workspace.snapshots.borrow();
    assert_eq!(snapshot.canonical_board_id("existing-destination"), "board");
    assert!(!snapshot.board_active("existing-destination"));
    assert_eq!(snapshot.canonical_issue_id("imported-history"), "task");
    assert_eq!(
        snapshot.sessions[0].issue_id.as_deref(),
        Some("imported-history")
    );
    assert_eq!(
        snapshot
            .issues
            .iter()
            .find(|i| i.id == "imported-history")
            .unwrap()
            .result
            .as_deref(),
        Some("Historical result")
    );
    assert_eq!(
        snapshot.directors[0].overrides.scope,
        Some(DirectorScope::Issues {
            issue_ids: vec!["imported-history".into()]
        })
    );
    assert_eq!(
        snapshot
            .connections
            .iter()
            .filter(|c| c.enabled
                && matches!(&c.kind, ConnectionKind::Board { board_id } if board_id == "board"))
            .count(),
        1
    );
    assert!(
        snapshot
            .connections
            .iter()
            .any(|c| c.id == "existing-board-connection"
                && matches!(&c.kind, ConnectionKind::Board { board_id } if board_id == "board"))
    );
    assert!(
        snapshot
            .memberships
            .iter()
            .filter(|m| m.board_id == "board")
            .all(|m| m.issue_id != "imported-history")
    );
    let restored = crate::Store::open(&_temp.path().join("db"), DirectorProfile::default())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(restored.issue_aliases, snapshot.issue_aliases);
    assert_eq!(restored.board_aliases, snapshot.board_aliases);
    assert_eq!(
        restored
            .operations
            .iter()
            .find(|o| o.id == "operation")
            .unwrap()
            .results,
        snapshot
            .operations
            .iter()
            .find(|o| o.id == "operation")
            .unwrap()
            .results
    );
}

#[test]
fn gitlab_label_columns_follow_remote_positions() {
    let mut steps = gl_metadata();
    steps[1].1 = json!([
        {"id":4,"position":2,"label":{"name":"Review"}},
        {"id":3,"position":1,"label":{"name":"Working"}}
    ]);
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gl_source(false)).unwrap();
    assert_eq!(
        board
            .board
            .columns
            .iter()
            .map(|c| c.title.as_str())
            .collect::<Vec<_>>(),
        ["Open", "Working", "Review", "Closed"]
    );
}

#[test]
fn successful_publication_creates_one_ready_connection_and_is_idempotent() {
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.boards.push(board_record(gh_source()));
    preserve_board_identity(&mut snapshot, "board").unwrap();
    preserve_board_identity(&mut snapshot, "board").unwrap();
    let connections: Vec<_> = snapshot
        .connections
        .iter()
        .filter(|c| c.project_id == "project")
        .collect();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].state, ConnectionState::Ready);
    assert!(connections[0].enabled);
}
#[tokio::test]
async fn failed_explicit_edit_retains_confirmed_task_and_visible_unknown_result() {
    let fake = Fake::new(vec![
        ("repos/elsewhere/repo --method GET", writable_repo(), 0),
        ("--method PATCH", json!({}), 1),
    ]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    install_task(
        &workspace,
        sample_operation(OperationKind::EditTask {
            issue_id: "task".into(),
            title: "Unconfirmed".into(),
            body: "Unconfirmed".into(),
        }),
        task,
        board_record(gh_source()),
    );
    assert!(execute(&workspace, "operation").is_err());
    let snapshot = workspace.snapshots.borrow();
    let task = snapshot.issues.iter().find(|i| i.id == "task").unwrap();
    assert_eq!(task.title, "Local title");
    assert_eq!(task.body, "Only explicit task body");
    let op = operation(&snapshot, "operation").unwrap();
    assert_eq!(op.state, OperationState::NeedsReconciliation);
    assert!(op.error.is_some());
}
#[tokio::test]
async fn successful_explicit_edit_reads_authoritative_fields_before_local_commit() {
    let fake = Fake::new(vec![
        ("repos/elsewhere/repo --method GET", writable_repo(), 0),
        ("--method PATCH", json!({}), 0),
        (
            "issues/1 --method GET",
            json!({"title":"Authoritative title","body":"Authoritative body"}),
            0,
        ),
    ]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    install_task(
        &workspace,
        sample_operation(OperationKind::EditTask {
            issue_id: "task".into(),
            title: "Requested title".into(),
            body: "Requested body".into(),
        }),
        task,
        board_record(gh_source()),
    );
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    let task = snapshot.issues.iter().find(|i| i.id == "task").unwrap();
    assert_eq!(task.title, "Authoritative title");
    assert_eq!(task.body, "Authoritative body");
    assert_eq!(
        operation(&snapshot, "operation")
            .unwrap()
            .results
            .get("edit/task")
            .map(String::as_str),
        Some("confirmed")
    );
}
#[tokio::test]
async fn failed_explicit_move_retains_confirmed_membership() {
    let mut steps = gh_metadata();
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push(("updateProjectV2ItemFieldValue", json!({}), 1));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    install_task(
        &workspace,
        sample_operation(OperationKind::MoveTask {
            board_id: "board".into(),
            issue_id: "task".into(),
            column_id: "done".into(),
        }),
        task,
        board_record(gh_source()),
    );
    workspace
        .update_project(|s| {
            s.memberships
                .iter_mut()
                .find(|m| m.issue_id == "task")
                .unwrap()
                .remote_item_id = Some("ITEM1".into());
            Ok(())
        })
        .unwrap();
    assert!(execute(&workspace, "operation").is_err());
    let snapshot = workspace.snapshots.borrow();
    assert_eq!(
        snapshot
            .memberships
            .iter()
            .find(|m| m.issue_id == "task")
            .unwrap()
            .column_ids,
        ["todo"]
    );
}

#[tokio::test]
async fn new_github_board_resolves_named_columns_and_journals_destination_before_sync() {
    let mut steps = vec![
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        ("user --method GET", json!({"login":"owner"}), 0),
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        (
            "createProjectV2",
            json!({"data":{"createProjectV2":{"projectV2":{"id":"BOARD","number":1,"url":"https://github.com/users/owner/projects/1"}}}}),
            0,
        ),
    ];
    steps.extend(gh_metadata());
    steps.push((
        "items(first",
        json!({"data":{"node":{"items":page(json!([]),None)}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut source = gh_source();
    if let BoardSource::Github { number, .. } = &mut source {
        *number = 0;
    }
    workspace
        .update_project(|s| {
            s.boards.push(board_record(BoardSource::Local));
            s.operations.push(sample_operation(OperationKind::Publish {
                board_id: "board".into(),
                target: PublishTarget {
                    source,
                    name: "Created".into(),
                },
                columns: vec![ColumnMapping {
                    local_id: "todo".into(),
                    remote_id: "name:Todo".into(),
                }],
                tasks: vec![],
            }));
            Ok(())
        })
        .unwrap();
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    let op = operation(&snapshot, "operation").unwrap();
    assert!(matches!(
        decode::<BoardSource>(op.results.get("board").unwrap()).unwrap(),
        BoardSource::Github { number: 1, .. }
    ));
    assert_eq!(
        op.results.get("column/todo").map(String::as_str),
        Some("todo")
    );
    assert_eq!(op.state, OperationState::Completed);
}
#[tokio::test]
async fn new_gitlab_board_uses_open_closed_without_creating_labels_implicitly() {
    let fake = Fake::new(vec![
        (
            "projects/group%2Frepo --method GET",
            json!({"permissions":{"project_access":{"access_level":40}}}),
            0,
        ),
        (
            "projects/group%2Frepo/boards --method POST",
            json!({"id":3}),
            0,
        ),
        ("boards/3 --method GET", json!({"id":3,"name":"Created"}), 0),
        ("boards/3/lists", json!([]), 0),
        (
            "projects/group%2Frepo --method GET",
            json!({"permissions":{"project_access":{"access_level":40}}}),
            0,
        ),
        ("issues?state=all", json!([]), 0),
    ]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut source = gl_source(false);
    if let BoardSource::Gitlab { number, .. } = &mut source {
        *number = 0;
    }
    workspace
        .update_project(|s| {
            s.boards.push(board_record(BoardSource::Local));
            s.operations.push(sample_operation(OperationKind::Publish {
                board_id: "board".into(),
                target: PublishTarget {
                    source,
                    name: "Created".into(),
                },
                columns: vec![ColumnMapping {
                    local_id: "todo".into(),
                    remote_id: "gitlab-open".into(),
                }],
                tasks: vec![],
            }));
            Ok(())
        })
        .unwrap();
    execute(&workspace, "operation").unwrap();
    assert!(matches!(
        workspace.snapshots.borrow().board("board").unwrap().source,
        BoardSource::Gitlab { number: 3, .. }
    ));
    assert!(!fake.calls().contains("labels --method POST"));
}
#[tokio::test]
async fn uncertain_board_creation_cannot_replay_until_known_destination_is_supplied() {
    let fake = Fake::new(vec![
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        ("user --method GET", json!({"login":"owner"}), 0),
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        ("createProjectV2", json!({}), 1),
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        ("user --method GET", json!({"login":"owner"}), 0),
    ]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut source = gh_source();
    if let BoardSource::Github { number, .. } = &mut source {
        *number = 0;
    }
    workspace
        .update_project(|s| {
            s.boards.push(board_record(BoardSource::Local));
            s.operations.push(sample_operation(OperationKind::Publish {
                board_id: "board".into(),
                target: PublishTarget {
                    source,
                    name: "Created".into(),
                },
                columns: vec![ColumnMapping {
                    local_id: "todo".into(),
                    remote_id: "name:Todo".into(),
                }],
                tasks: vec![],
            }));
            Ok(())
        })
        .unwrap();
    assert!(execute(&workspace, "operation").is_err());
    assert!(execute(&workspace, "operation").is_err());
    assert_eq!(
        fake.calls()
            .lines()
            .filter(|s| s.contains("mutation"))
            .count(),
        1
    );
    assert_eq!(
        workspace.snapshots.borrow().board("board").unwrap().source,
        BoardSource::Local
    );
    assert_eq!(
        operation(&workspace.snapshots.borrow(), "operation")
            .unwrap()
            .results
            .get("pending")
            .map(String::as_str),
        Some("board")
    );
}
#[test]
fn status_clear_uses_clear_mutation_and_empty_status_field_is_explicit() {
    let mut steps = gh_metadata();
    steps.push((
        "clearProjectV2ItemFieldValue",
        json!({"data":{"clearProjectV2ItemFieldValue":{"projectV2Item":{"id":"ITEM1"}}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gh_source()).unwrap();
    github::move_task(&fake.config(), &board, "ITEM1", NO_STATUS).unwrap();
}

#[test]
fn provider_subprocess_removes_relay_bearer_from_environment() {
    let fake = Fake::new(vec![("--method GET", json!({}), 0)]);
    let mut command = Command::new(&fake.executable);
    command
        .args(["api", "--method", "GET"])
        .env("RELAY_TOKEN", "sentinel-secret");
    request(command).unwrap();
}
#[test]
fn oversized_provider_output_is_rejected_before_parsing() {
    let fake = Fake::new(vec![]);
    fs::write(
        &fake.executable,
        format!("#!/bin/sh\nhead -c {} /dev/zero\n", LIMIT + 1),
    )
    .unwrap();
    let error = request(Command::new(&fake.executable)).unwrap_err();
    assert!(error.to_string().contains("16 MiB"));
}
#[test]
fn provider_capture_has_a_real_subprocess_deadline() {
    let fake = Fake::new(vec![]);
    fs::write(&fake.executable, "#!/bin/sh\nsleep 10\n").unwrap();
    let started = std::time::Instant::now();
    assert!(
        crate::process::capture(
            &mut Command::new(&fake.executable),
            1024,
            Duration::from_millis(30),
            "Provider test"
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn inaccessible_github_issues_retain_last_good_mirror_instead_of_silent_removal() {
    let mut steps = gh_metadata();
    steps.push(("items(first",json!({"data":{"node":{"items":page(json!([{"id":"ITEM","type":"ISSUE","content":null}]),None)}}}),0));
    let fake = Fake::new(steps);
    let board = metadata(&fake.config(), &gh_source()).unwrap();
    assert!(
        tasks(&fake.config(), &board)
            .unwrap_err()
            .to_string()
            .contains("inaccessible")
    );
}
#[test]
fn paginated_collection_budget_is_bounded_across_responses() {
    let mut budget = ReadBudget::new();
    budget.bytes = LIMIT;
    assert!(budget.include(&json!("over")).is_err());
    let mut budget = ReadBudget::new();
    budget.started = std::time::Instant::now() - Duration::from_secs(121);
    assert!(budget.check().is_err());
}

#[tokio::test]
async fn create_task_keeps_accepted_reference_and_membership_when_status_outcome_is_unknown() {
    let mut steps = gh_metadata();
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push((
        "--method POST",
        json!({"number":1,"html_url":"https://github.com/elsewhere/repo/issues/1"}),
        0,
    ));
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push(("issues/1", json!({"node_id":"NODE1"}), 0));
    steps.push((
        "addProjectV2ItemById",
        json!({"data":{"addProjectV2ItemById":{"item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push(("updateProjectV2ItemFieldValue", json!({}), 1));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    install_task(
        &workspace,
        sample_operation(OperationKind::CreateTask {
            board_id: "board".into(),
            issue_id: "task".into(),
        }),
        pending_task(),
        board_record(gh_source()),
    );
    workspace
        .update_project(|s| {
            s.memberships.retain(|m| m.issue_id != "task");
            Ok(())
        })
        .unwrap();
    assert!(execute(&workspace, "operation").is_err());
    let snapshot = workspace.snapshots.borrow();
    assert!(
        snapshot
            .issues
            .iter()
            .find(|i| i.id == "task")
            .unwrap()
            .reference
            .is_some()
    );
    let member = snapshot
        .memberships
        .iter()
        .find(|m| m.issue_id == "task")
        .unwrap();
    assert_eq!(member.remote_item_id.as_deref(), Some("ITEM1"));
    assert!(member.column_ids.is_empty());
    assert_eq!(
        operation(&snapshot, "operation").unwrap().state,
        OperationState::NeedsReconciliation
    );
}
#[test]
fn authoritative_merge_returns_board_connection_to_ready() {
    let fake = Fake::new(gh_metadata());
    let mut remote = metadata(&fake.config(), &gh_source()).unwrap();
    remote.board.id = "board".into();
    remote.board.project_id = "project".into();
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.boards.push(remote.board.clone());
    snapshot.connections.push(ProjectConnection {
        id: "board-connection".into(),
        project_id: "project".into(),
        name: "Board".into(),
        kind: ConnectionKind::Board {
            board_id: "board".into(),
        },
        enabled: true,
        state: ConnectionState::Pending,
        error: Some("Old failure".into()),
    });
    merge(&mut snapshot, "board", &remote, vec![]).unwrap();
    let connection = snapshot
        .connections
        .iter()
        .find(|c| c.id == "board-connection")
        .unwrap();
    assert_eq!(connection.state, ConnectionState::Ready);
    assert_eq!(connection.error, None);
}

#[tokio::test]
async fn gitlab_explicit_edit_accepts_authoritative_null_description_as_empty_body() {
    let fake = Fake::new(vec![
        (
            "projects/group%2Frepo --method GET",
            json!({"permissions":{"project_access":{"access_level":40}}}),
            0,
        ),
        (
            "projects/group%2Frepo --method GET",
            json!({"issues_enabled":true}),
            0,
        ),
        ("description=", json!({}), 0),
        (
            "issues/1 --method GET",
            json!({"title":"Accepted","description":null}),
            0,
        ),
    ]);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gl_source(false), "group/repo", 1));
    install_task(
        &workspace,
        sample_operation(OperationKind::EditTask {
            issue_id: "task".into(),
            title: "Accepted".into(),
            body: String::new(),
        }),
        task,
        board_record(gl_source(false)),
    );
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    let task = snapshot.issues.iter().find(|i| i.id == "task").unwrap();
    assert_eq!(task.title, "Accepted");
    assert_eq!(task.body, "");
}
#[test]
fn gitlab_hosts_cannot_be_cli_options_or_embedded_credentials() {
    for host in ["--hostname", "-H", "user@host", "host:443", "https://host"] {
        let mut source = gl_source(false);
        if let BoardSource::Gitlab {
            host: source_host, ..
        } = &mut source
        {
            *source_host = host.into();
        }
        assert!(validate_source(&source).is_err());
    }
}

#[test]
fn new_board_discovery_is_read_only_and_returns_supported_mapping_ids() {
    let fake = Fake::new(vec![
        ("users/owner", json!({"type":"User","node_id":"OWNER"}), 0),
        ("user --method GET", json!({"login":"owner"}), 0),
    ]);
    let mut source = gh_source();
    if let BoardSource::Github { number, .. } = &mut source {
        *number = 0;
    }
    let board = discover(&fake.config(), &source).unwrap();
    assert_eq!(board.source, source);
    assert_eq!(
        board
            .columns
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        [
            "name:Todo",
            "name:In Progress",
            "name:Done",
            "name:No status"
        ]
    );
    assert!(!fake.calls().contains("mutation"));
    let fake = Fake::new(vec![(
        "projects/group%2Frepo --method GET",
        json!({"permissions":{"project_access":{"access_level":40}}}),
        0,
    )]);
    let mut source = gl_source(false);
    if let BoardSource::Gitlab { number, .. } = &mut source {
        *number = 0;
    }
    let board = discover(&fake.config(), &source).unwrap();
    assert_eq!(
        board
            .columns
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["gitlab-open", "gitlab-closed"]
    );
    assert!(!fake.calls().contains("POST"));
}

fn recovery_snapshot(
    kind: OperationKind,
    task: Issue,
    source: BoardSource,
    pending: &str,
) -> Snapshot {
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.boards.push(board_record(source));
    snapshot.issues.push(task);
    snapshot.memberships.push(membership_record());
    snapshot.connections.push(repository_connection());
    let mut op = sample_operation(kind);
    op.state = OperationState::NeedsReconciliation;
    op.results.insert("pending".into(), pending.into());
    snapshot.operations.push(op);
    snapshot
}
#[test]
fn recovery_lookup_reads_marked_issue_and_returns_typed_known_result_without_writes() {
    let fake = Fake::new(vec![(
        "issues/1 --method GET",
        json!({"number":1,"html_url":"https://github.com/elsewhere/repo/issues/1","body":"body\n<!-- relay-operation:operation:task:task -->"}),
        0,
    )]);
    let snapshot = recovery_snapshot(
        OperationKind::CreateTask {
            board_id: "board".into(),
            issue_id: "task".into(),
        },
        pending_task(),
        gh_source(),
        "issue/task",
    );
    let (key, result, description) = lookup_reconciliation(
        &fake.config(),
        &snapshot,
        "operation",
        "https://github.com/elsewhere/repo/issues/1",
    )
    .unwrap();
    assert_eq!(key, "issue/task");
    assert_eq!(decode::<IssueRef>(&result).unwrap().number, 1);
    assert!(description.contains("elsewhere/repo"));
    assert!(!fake.calls().contains("POST"));
    assert!(
        snapshot
            .issues
            .iter()
            .find(|i| i.id == "task")
            .unwrap()
            .reference
            .is_none()
    );
}
#[test]
fn recovery_issue_lookup_rejects_wrong_repository_host_and_missing_marker() {
    let snapshot = recovery_snapshot(
        OperationKind::CreateTask {
            board_id: "board".into(),
            issue_id: "task".into(),
        },
        pending_task(),
        gh_source(),
        "issue/task",
    );
    let fake = Fake::new(vec![]);
    assert!(
        lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://wrong.host/elsewhere/repo/issues/1"
        )
        .is_err()
    );
    assert!(
        lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://github.com/wrong/repo/issues/1"
        )
        .is_err()
    );
    assert!(fake.calls().is_empty());
    let fake = Fake::new(vec![(
        "issues/1 --method GET",
        json!({"number":1,"html_url":"https://github.com/elsewhere/repo/issues/1","body":"No operation marker"}),
        0,
    )]);
    assert!(
        lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://github.com/elsewhere/repo/issues/1"
        )
        .unwrap_err()
        .to_string()
        .contains("marker")
    );
}
#[test]
fn recovery_board_lookup_requires_explicit_scope_matched_destination() {
    let fake = Fake::new(gh_metadata());
    let mut source = gh_source();
    if let BoardSource::Github { number, .. } = &mut source {
        *number = 0;
    }
    let snapshot = recovery_snapshot(
        OperationKind::Publish {
            board_id: "board".into(),
            target: PublishTarget {
                source,
                name: "Requested".into(),
            },
            columns: vec![],
            tasks: vec![],
        },
        pending_task(),
        BoardSource::Local,
        "board",
    );
    let (key, result, _) = lookup_reconciliation(
        &fake.config(),
        &snapshot,
        "operation",
        "https://github.com/users/owner/projects/1",
    )
    .unwrap();
    assert_eq!(key, "board");
    assert!(matches!(
        decode::<BoardSource>(&result).unwrap(),
        BoardSource::Github { number: 1, .. }
    ));
    assert!(!fake.calls().contains("mutation"));
    assert!(
        lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://github.com/users/wrong/projects/1"
        )
        .is_err()
    );
}
#[test]
fn recovery_edit_checks_actual_title_and_body_before_returning_confirmed() {
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    let snapshot = recovery_snapshot(
        OperationKind::EditTask {
            issue_id: "task".into(),
            title: "Requested".into(),
            body: "Body".into(),
        },
        task,
        gh_source(),
        "edit/task",
    );
    let fake = Fake::new(vec![(
        "issues/1 --method GET",
        json!({"title":"Requested","body":"Body"}),
        0,
    )]);
    let (_, result, _) = lookup_reconciliation(
        &fake.config(),
        &snapshot,
        "operation",
        "https://github.com/elsewhere/repo/issues/1",
    )
    .unwrap();
    assert_eq!(result, "confirmed");
    let fake = Fake::new(vec![(
        "issues/1 --method GET",
        json!({"title":"Other","body":"Body"}),
        0,
    )]);
    assert!(
        lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://github.com/elsewhere/repo/issues/1"
        )
        .is_err()
    );
}
#[test]
fn recovery_membership_and_status_require_confirmed_authoritative_board_state() {
    for (key, kind) in [
        (
            "membership/task",
            OperationKind::CreateTask {
                board_id: "board".into(),
                issue_id: "task".into(),
            },
        ),
        (
            "status/task",
            OperationKind::MoveTask {
                board_id: "board".into(),
                issue_id: "task".into(),
                column_id: "done".into(),
            },
        ),
    ] {
        let mut task = pending_task();
        task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
        let snapshot = recovery_snapshot(kind, task, gh_source(), key);
        let mut steps = gh_metadata();
        steps.push((
            "items(first",
            json!({"data":{"node":{"items":page(json!([gh_item(1,Some("done"))]),None)}}}),
            0,
        ));
        steps.push((
            "labels(first",
            json!({"data":{"node":{"labels":page(json!([]),None)}}}),
            0,
        ));
        let fake = Fake::new(steps);
        let (_, result, _) = lookup_reconciliation(
            &fake.config(),
            &snapshot,
            "operation",
            "https://github.com/elsewhere/repo/issues/1",
        )
        .unwrap();
        assert_eq!(
            result,
            if key.starts_with("membership") {
                "ITEM1"
            } else {
                "confirmed"
            }
        );
        assert!(!fake.calls().contains("mutation"));
    }
}

#[test]
fn same_gitlab_repository_and_issue_number_on_distinct_hosts_remain_distinct_tasks() {
    let fake = Fake::new(gl_metadata());
    let mut first = metadata(&fake.config(), &gl_source(false)).unwrap();
    first.board.id = "host-one".into();
    first.board.project_id = "project".into();
    let mut second = first.clone();
    second.board.id = "host-two".into();
    second.board.source = BoardSource::Gitlab {
        host: "other.example".into(),
        group: false,
        path: "group/repo".into(),
        number: 2,
        url: "https://other.example/group/repo/-/boards/2".into(),
    };
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot
        .boards
        .extend([first.board.clone(), second.board.clone()]);
    let incoming = |source: &BoardSource| RemoteTask {
        reference: reference_for(source, "group/repo", 1),
        title: "Issue".into(),
        body: String::new(),
        labels: vec![],
        columns: vec!["gitlab-open".into()],
        item_id: "101".into(),
    };
    merge(
        &mut snapshot,
        "host-one",
        &first,
        vec![incoming(&first.board.source)],
    )
    .unwrap();
    merge(
        &mut snapshot,
        "host-two",
        &second,
        vec![incoming(&second.board.source)],
    )
    .unwrap();
    let one = snapshot
        .memberships
        .iter()
        .find(|m| m.board_id == "host-one")
        .unwrap()
        .issue_id
        .clone();
    let two = snapshot
        .memberships
        .iter()
        .find(|m| m.board_id == "host-two")
        .unwrap()
        .issue_id
        .clone();
    assert_ne!(one, two);
    merge(
        &mut snapshot,
        "host-one",
        &first,
        vec![incoming(&first.board.source)],
    )
    .unwrap();
    assert_eq!(
        snapshot
            .memberships
            .iter()
            .find(|m| m.board_id == "host-one")
            .unwrap()
            .issue_id,
        one
    );
}

#[tokio::test]
async fn github_project_status_move_does_not_require_write_permission_on_external_repository() {
    let mut steps = gh_metadata();
    steps.push((
        "repos/elsewhere/repo --method GET",
        json!({"permissions":{"push":false,"pull":true}}),
        0,
    ));
    steps.push((
        "updateProjectV2ItemFieldValue",
        json!({"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push((
        "items(first",
        json!({"data":{"node":{"items":page(json!([gh_item(1,Some("done"))]),None)}}}),
        0,
    ));
    steps.push((
        "labels(first",
        json!({"data":{"node":{"labels":page(json!([]),None)}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    let mut task = pending_task();
    task.reference = Some(reference_for(&gh_source(), "elsewhere/repo", 1));
    install_task(
        &workspace,
        sample_operation(OperationKind::MoveTask {
            board_id: "board".into(),
            issue_id: "task".into(),
            column_id: "done".into(),
        }),
        task,
        board_record(gh_source()),
    );
    workspace
        .update_project(|s| {
            s.memberships
                .iter_mut()
                .find(|m| m.issue_id == "task")
                .unwrap()
                .remote_item_id = Some("ITEM1".into());
            Ok(())
        })
        .unwrap();
    execute(&workspace, "operation").unwrap();
    assert_eq!(
        workspace
            .snapshots
            .borrow()
            .memberships
            .iter()
            .find(|m| m.issue_id == "task")
            .unwrap()
            .column_ids,
        ["done"]
    );
}

#[tokio::test]
async fn github_created_reference_uses_canonical_url_casing_and_retains_task_id_on_mirror() {
    let mut steps = gh_metadata();
    steps.push(("repos/elsewhere/repo --method GET", writable_repo(), 0));
    steps.push((
        "--method POST",
        json!({"number":1,"html_url":"https://github.com/ElseWhere/Repo/issues/1"}),
        0,
    ));
    steps.push(("repos/ElseWhere/Repo --method GET", writable_repo(), 0));
    steps.push((
        "repos/ElseWhere/Repo/issues/1",
        json!({"node_id":"NODE1"}),
        0,
    ));
    steps.push((
        "addProjectV2ItemById",
        json!({"data":{"addProjectV2ItemById":{"item":{"id":"ITEM1"}}}}),
        0,
    ));
    steps.push((
        "updateProjectV2ItemFieldValue",
        json!({"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"ITEM1"}}}}),
        0,
    ));
    let mut item = gh_item(1, Some("todo"));
    item["content"]["repository"]["nameWithOwner"] = json!("ElseWhere/Repo");
    item["content"]["url"] = json!("https://github.com/ElseWhere/Repo/issues/1");
    steps.push((
        "items(first",
        json!({"data":{"node":{"items":page(json!([item]),None)}}}),
        0,
    ));
    steps.push((
        "labels(first",
        json!({"data":{"node":{"labels":page(json!([]),None)}}}),
        0,
    ));
    let fake = Fake::new(steps);
    let (_temp, workspace) = workspace(fake.config()).await;
    install_task(
        &workspace,
        sample_operation(OperationKind::CreateTask {
            board_id: "board".into(),
            issue_id: "task".into(),
        }),
        pending_task(),
        board_record(gh_source()),
    );
    execute(&workspace, "operation").unwrap();
    let snapshot = workspace.snapshots.borrow();
    let task = snapshot.issues.iter().find(|i| i.id == "task").unwrap();
    assert_eq!(
        task.reference.as_ref().unwrap().repository,
        "ElseWhere/Repo"
    );
    assert_eq!(
        snapshot
            .memberships
            .iter()
            .find(|m| m.issue_id == "task")
            .unwrap()
            .remote_item_id
            .as_deref(),
        Some("ITEM1")
    );
}
#[test]
fn github_known_board_urls_match_owner_case_insensitively() {
    let source = BoardSource::Github {
        owner: "OWNER".into(),
        number: 0,
        url: String::new(),
    };
    let found =
        board_source_from_url(&source, "https://github.com/users/owner/projects/1").unwrap();
    assert!(matches!(found,BoardSource::Github{owner,number:1,..} if owner == "OWNER"));
    assert!(board_source_from_url(&source, "https://github.com/users/another/projects/1").is_err());
}
