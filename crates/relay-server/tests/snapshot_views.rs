//! Synthetic fixtures only. No live server/database, prompts or environment reads.
#[path = "../src/snapshot_views.rs"]
mod snapshot_views;
use relay_core::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! { static ALLOCATIONS: Cell<(bool, usize)> = const { Cell::new((false, 0)) }; }
struct Counter;
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|c| {
            let (on, bytes) = c.get();
            if on {
                c.set((on, bytes + layout.size()));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|c| {
            let (on, bytes) = c.get();
            if on {
                c.set((on, bytes + size));
            }
        });
        unsafe { System.realloc(p, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn allocated<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|c| c.set((true, 0)));
    let value = f();
    let bytes = ALLOCATIONS.with(|c| {
        let (_, bytes) = c.get();
        c.set((false, 0));
        bytes
    });
    (value, bytes)
}
fn pending(s: &Submission) -> bool {
    matches!(
        s.state,
        SubmissionState::Queued
            | SubmissionState::Launching
            | SubmissionState::Running
            | SubmissionState::Paused
    )
}
fn legacy(source: &Snapshot) -> Snapshot {
    let mut copy = source.clone();
    copy.messages.clear();
    copy.comments.clear();
    copy.submissions.retain(pending);
    copy.tool_permissions
        .retain(|p| p.decision.is_none() && !p.expired);
    for s in &mut copy.sessions {
        for space in &mut s.workspaces {
            space.changes = None;
        }
        if let Some(w) = &mut s.worker {
            w.changes = None;
        }
    }
    copy
}
fn fixture() -> Snapshot {
    let mut source = demo_snapshot(AgentProfile::default());
    source.submissions.push(Submission {
        id: "current-run".into(),
        session_id: source.sessions[0].id.clone(),
        parts: vec![Part::text("preserve structured current input")],
        state: SubmissionState::Running,
        approve_implementation: true,
        error: None,
        interrupts_run: None,
        last_edit_request: Some("edit-receipt".into()),
        sender: Sender::User,
    });
    source.tool_permissions.push(ToolPermission {
        id: "pending".into(),
        session_id: source.sessions[0].id.clone(),
        run_id: "current-run".into(),
        tool: "fixture".into(),
        description: "explicit review".into(),
        decision: None,
        expired: false,
        spawn_attempt: None,
    });
    source
}
#[test]
fn metadata_preserves_contract_without_allocating_excluded_history() {
    let source = fixture();
    let (small, small_bytes) = allocated(|| snapshot_views::metadata(&source));
    let mut large = source.clone();
    for index in 0..2048 {
        large.messages.push(Message {
            id: format!("history-{index}"),
            session_id: source.sessions[0].id.clone(),
            author: "Synthetic".into(),
            sender: Sender::User,
            kind: "text".into(),
            body: "x".repeat(1024),
            parts: vec![],
            tool: None,
        });
        large.comments.push(Comment {
            id: format!("comment-{index}"),
            message_id: format!("history-{index}"),
            quote: None,
            author: "Synthetic".into(),
            body: "y".repeat(128),
            created_at: 0,
        });
        let mut done = source.submissions.last().unwrap().clone();
        done.id = format!("completed-{index}");
        done.state = SubmissionState::Completed;
        done.parts = vec![Part::text("z".repeat(1024))];
        large.submissions.push(done);
        let mut decided = source.tool_permissions[0].clone();
        decided.id = format!("decided-{index}");
        decided.description = "p".repeat(1024);
        decided.decision = Some(false);
        large.tool_permissions.push(decided);
    }
    let (actual, after_bytes) = allocated(|| snapshot_views::metadata(&large));
    let (expected, before_bytes) = allocated(|| legacy(&large));
    assert_eq!(actual, expected);
    assert_eq!(actual, small);
    assert_eq!(
        after_bytes, small_bytes,
        "excluded payloads must cause zero additional allocated bytes"
    );
    assert!(before_bytes > after_bytes + 4 * 1024 * 1024);
    // Numeric synthetic work metric, not peak live RAM or wall-clock latency.
    eprintln!(
        "synthetic_metadata allocated_before={before_bytes} allocated_after={after_bytes} excluded_messages=2048"
    );
    assert_eq!(large.messages.len(), source.messages.len() + 2048);
    assert_eq!(
        snapshot_views::execution(&large, "current-run").submissions,
        vec![source.submissions.last().unwrap().clone()]
    );
}
#[test]
fn execution_and_preparation_preserve_required_context_and_review_paths() {
    let source = fixture();
    let selected = snapshot_views::execution(&source, "current-run");
    let mut expected = source.clone();
    expected.messages.clear();
    expected.comments.clear();
    expected.tool_permissions.clear();
    expected.submissions.retain(|s| s.id == "current-run");
    assert_eq!(selected, expected);
    let prepared = snapshot_views::preparation(&source);
    assert_eq!(prepared.projects, source.projects);
    assert_eq!(prepared.boards, source.boards);
    assert_eq!(prepared.connections, source.connections);
    assert_eq!(prepared.bindings, source.bindings);
    assert_eq!(prepared.installations, source.installations);
    assert_eq!(prepared.issues, source.issues);
    assert_eq!(prepared.issue_aliases, source.issue_aliases);
    assert!(
        prepared.messages.is_empty()
            && prepared.submissions.is_empty()
            && prepared.sessions.is_empty()
    );
    let mut session = source.sessions[0].clone();
    session.workspaces = vec![SessionWorkspace {
        connection_id: "fixture".into(),
        path: "/synthetic".into(),
        repository: true,
        branch: Some("branch".into()),
        base_commit: Some("base".into()),
        changes: Some(ChangeSet {
            files: vec![],
            diff: "discarded review".repeat(100),
            truncated: false,
        }),
    }];
    let review = snapshot_views::review_workspaces(&session);
    assert_eq!(review[0].path, session.workspaces[0].path);
    assert!(review[0].changes.is_none());
    assert!(session.workspaces[0].changes.is_some());
}

// Opt-in, isolated processes for /usr/bin/time -v. These compare only projection
// transients over identical synthetic input; they do not model live Relay RSS.
fn peak_fixture(old: bool) {
    let mut source = fixture();
    for index in 0..8192 {
        source.messages.push(Message {
            id: format!("synthetic-{index}"),
            session_id: source.sessions[0].id.clone(),
            author: "Synthetic".into(),
            sender: Sender::User,
            kind: "text".into(),
            body: "x".repeat(4096),
            parts: vec![],
            tool: None,
        });
    }
    for _ in 0..50 {
        let projected = if old {
            legacy(&source)
        } else {
            snapshot_views::metadata(&source)
        };
        std::hint::black_box(projected);
    }
    eprintln!(
        "synthetic_peak_fixture history_messages=8192 message_body_bytes=4096 projections=50 legacy={old}"
    );
}
#[test]
#[ignore = "Opt-in synthetic peak RSS comparison; requires checked memory headroom"]
fn synthetic_legacy_peak() {
    peak_fixture(true);
}
#[test]
#[ignore = "Opt-in synthetic peak RSS comparison; requires checked memory headroom"]
fn synthetic_projected_peak() {
    peak_fixture(false);
}
