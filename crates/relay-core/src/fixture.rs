use crate::*;

pub fn demo_snapshot(defaults: DirectorProfile) -> Snapshot {
    let project = Project {
        id: "demo".into(),
        name: "Relay · demo".into(),
        repository: "demo/relay".into(),
        fixture: true,
        columns: [
            ("backlog", "Backlog"),
            ("in_progress", "In progress"),
            ("review", "In review"),
        ]
        .into_iter()
        .map(|(id, title)| BoardColumn {
            id: id.into(),
            title: title.into(),
        })
        .collect(),
        defaults,
        github: None,
    };
    let issues = [
        ("issue-1", 1, "Connect the project board", "backlog", "integration", "Mirror the authoritative remote board and preserve its column order and issue identity. Relay adds agent activity alongside the existing workflow.", None),
        ("issue-2", 2, "Make director profiles declarative", "in_progress", "workflow", "Define project defaults and per-director overrides. Let several directors specialize within a project and expose their effective configuration.", None),
        ("issue-3", 3, "Review conversations in context", "review", "experience", "Attach feedback to the exact message or quoted passage that prompted it. Keep the conversation and its issue links intact across sessions.", Some("Fixture result: a contextual comment prototype is ready for review.")),
        ("issue-4", 4, "Expose context and usage controls", "backlog", "efficiency", "Support compact, reset context, and archive/start-new as separate actions. Report cache state only when the connected harness provides evidence.", None),
    ].into_iter().map(|(id, number, title, column, label, body, result)| Issue {
        id: id.into(), project_id: "demo".into(), reference: IssueRef {
            provider: Provider::Github, repository: "demo/relay".into(), number,
            url: format!("https://github.com/demo/relay/issues/{number}"),
        }, title: title.into(), body: body.into(), column_id: column.into(), labels: vec![label.into()], result: result.map(String::from),
    }).collect();
    let directors = vec![
        Director {
            id: "director-main".into(),
            project_id: "demo".into(),
            name: "Project director".into(),
            overrides: ProfileOverrides::default(),
        },
        Director {
            id: "director-review".into(),
            project_id: "demo".into(),
            name: "Review director".into(),
            overrides: ProfileOverrides {
                harness: Some(Harness::ClaudeCode),
                scope: Some(DirectorScope::Issues {
                    issue_ids: vec!["issue-3".into()],
                }),
                responsibilities: Some(vec![Task::Verify, Task::Review]),
                max_workers: Some(1),
                ..Default::default()
            },
        },
    ];
    let sessions = vec![
        Session {
            id: "session-plan".into(),
            project_id: "demo".into(),
            issue_id: Some("issue-2".into()),
            director_id: "director-main".into(),
            title: "Profile design".into(),
            role: SessionRole::Director,
            fixture: true,
            worker: None,
        },
        Session {
            id: "session-worker".into(),
            project_id: "demo".into(),
            issue_id: Some("issue-2".into()),
            director_id: "director-main".into(),
            title: "Profile validation".into(),
            role: SessionRole::Worker,
            fixture: true,
            worker: None,
        },
        Session {
            id: "session-review".into(),
            project_id: "demo".into(),
            issue_id: Some("issue-3".into()),
            director_id: "director-review".into(),
            title: "Contextual review".into(),
            role: SessionRole::Director,
            fixture: true,
            worker: None,
        },
    ];
    let messages = [
        ("m1", "session-plan", "Jens", "message", "Let directors inherit project defaults, while keeping their explicit overrides intact when those defaults change."),
        ("m2", "session-plan", "Project director", "plan", "I will separate project defaults from director overrides.\n\n1. Define a typed profile for scope, responsibilities, completion, and permissions.\n2. Delegate validation to a worker.\n3. Return the effective profile and identify which values are inherited."),
        ("m3", "session-plan", "Project director", "handoff", "Profile validation is delegated to the linked worker session. Both conversations remain attached to issue #2. No agent is running in this fixture."),
        ("m4", "session-worker", "Worker", "message", "Validation checks worker limits, required permissions, and issue scope. Unknown TOML fields are rejected to catch misspelled configuration."),
        ("m5", "session-worker", "Worker", "result", "Fixture verification: inherited fields follow project defaults; explicit director fields remain unchanged. A zero worker limit represents paused delegation."),
        ("m6", "session-review", "Review director", "review", "Feedback should reference the original message, with an optional selected quote. The transcript remains immutable while comments accumulate alongside it."),
        ("m7", "session-review", "Jens", "message", "Keep review keyboard-friendly. I should be able to jump between messages and comment without losing my place."),
    ].into_iter().map(|(id, session, author, kind, body)| Message { id: id.into(), session_id: session.into(), author: author.into(), kind: kind.into(), body: body.into(), parts: vec![] }).collect();
    Snapshot {
        bindings: vec![],
        installations: vec![],
        tool_permissions: vec![],
        revision: 0,
        projects: vec![project],
        issues,
        directors,
        sessions,
        messages,
        comments: vec![],
        submissions: vec![],
    }
}
