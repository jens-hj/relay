use super::*;
use crate::Live;
use crate::model::{Effort, ModelHue, ModelIdentity};
use crate::testing::mount;
use mosaic::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

fn node(id: &str, status: AgentStatus) -> AgentNode {
    AgentNode::new(id, id, status, Harness::Codex)
}

fn assert_action_visible(mounted: &crate::testing::Mounted, label: &str, visible: bool) {
    let node = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap();
    let details = mounted.ui.inspection_details(node.id).unwrap();
    for (name, expected) in [
        ("opacity", if visible { "1.0" } else { "0.0" }),
        ("hit-testable", if visible { "true" } else { "false" }),
    ] {
        let attribute = details
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .unwrap();
        assert_eq!(attribute.value, expected, "{label}: {name}");
    }
}

#[test]
fn selected_row_actions_require_hover_or_button_focus() {
    let edited = State::new(String::new());
    let opened = State::new(false);
    let spawned = State::new(false);
    let archived = State::new(false);
    let deleted = State::new(false);
    let mounted = mount(Size::new(520.0, 300.0), || {
        let mut root = node("selected", AgentStatus::Idle).selected();
        root.can_delete = true;
        view! {
            AgentSidebar roots:(Live::fixed(vec![root]))
                on-edit:(Some(Rc::new(move |id| edited.set(id)) as OnAgent))
                on-open:(Some(Rc::new(move |_| opened.set(true)) as OnAgent))
                on-spawn:(Some(Rc::new(move |_| spawned.set(true)) as OnAgent))
                on-archive:(Some(Rc::new(move |_| archived.set(true)) as OnAgent))
                archivable:(Live::fixed(vec!["selected".into()]))
                on-delete:(Some(Rc::new(move |_| deleted.set(true)) as OnAgent))
        }
    });
    let labels = [
        "New worker for selected",
        "Edit selected",
        "Archive selected",
        "Delete selected",
    ];
    for label in labels {
        assert_action_visible(&mounted, label, false);
    }
    // Hidden overflow cannot initiate reveal, even across the entire strip.
    for label in labels {
        mounted.hover(mounted.rect(label).center());
        for other in labels {
            assert_action_visible(&mounted, other, false);
        }
    }
    mounted.click_label("selected");
    mounted.hover(Vector2::new(379.0, 299.0));
    assert_eq!(mounted.focused_label().as_deref(), Some("selected"));
    for label in labels {
        assert_action_visible(&mounted, label, false);
    }
    mounted.hover(mounted.rect("selected").center());
    for label in labels {
        assert_action_visible(&mounted, label, true);
    }
    let row = mounted.rect("selected");
    let mut edge = row.max_x();
    for label in labels {
        let action = mounted.rect(label);
        assert_eq!(action.origin.x, edge);
        assert_eq!(action.size, Size::new(row.size.height, row.size.height));
        mounted.hover(action.center());
        for other in labels {
            assert_action_visible(&mounted, other, true);
        }
        edge = action.max_x();
    }
    // Exit resets reveal; returning through outside buttons cannot rearm it.
    mounted.hover(Vector2::new(510.0, 299.0));
    for label in labels {
        mounted.hover(mounted.rect(label).center());
        for other in labels {
            assert_action_visible(&mounted, other, false);
        }
    }
    mounted.hover(row.center());
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Down(PointerButton::Primary),
        position: mounted.rect(labels[0]).center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: std::time::Duration::ZERO,
    });
    mounted.hover(Vector2::new(379.0, 299.0));
    assert_action_visible(&mounted, labels[0], true);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Up(PointerButton::Primary),
        position: Vector2::new(379.0, 299.0),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: std::time::Duration::ZERO,
    });
    mounted.hover(mounted.rect("selected").center());
    mounted.click_label(labels[0]);
    assert!(spawned.get_untracked());
    opened.set(false);
    mounted.click_label(labels[1]);
    assert_eq!(edited.get_untracked(), "selected");
    assert!(!opened.get_untracked());
    mounted.click_label(labels[2]);
    assert!(archived.get_untracked());
    // Return focus to the row; Tab must still reach the invisible buttons.
    mounted.click_label("selected");
    mounted.hover(Vector2::new(379.0, 299.0));
    for label in labels {
        mounted.key(Key::Tab);
        assert_eq!(mounted.focused_label().as_deref(), Some(label));
        assert_action_visible(&mounted, label, true);
        if label == "Edit selected" {
            edited.set(String::new());
            opened.set(false);
            mounted.key(Key::Enter);
            assert_eq!(edited.get_untracked(), "selected");
            assert!(!opened.get_untracked());
        }
    }
    mounted.key(Key::Enter);
    assert!(!deleted.get_untracked());
    assert_action_visible(&mounted, "Confirm deletion of selected", true);
    // A pending confirmation stays usable even after focus and hover leave.
    mounted.click_label("selected");
    mounted.hover(Vector2::new(379.0, 299.0));
    assert_action_visible(&mounted, "Confirm deletion of selected", true);
    mounted.click_label("Confirm deletion of selected");
    assert!(deleted.get_untracked());
}

#[test]
fn clipped_row_and_outside_strip_cannot_initiate_reveal() {
    let mounted = mount(Size::new(520.0, 300.0), || {
        view! {
            AgentSidebar
                roots:(Live::fixed((0..20).map(|i| node(&format!("clip-{i}"), AgentStatus::Idle)).collect()))
                on-edit:(Some(Rc::new(|_| {}) as OnAgent)) {
                el height:24px shrink:0 label:"Clipping footer" {}
            }
        }
    });
    let row = mounted.rect("clip-0");
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, -10.0),
        },
        position: row.center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: std::time::Duration::ZERO,
    });
    mounted.settle();
    let clipped = mounted.rect("clip-0");
    assert!(clipped.origin.y < row.origin.y);
    // The clipped portion is beneath the header and cannot arm reveal.
    mounted.hover(Vector2::new(clipped.center().x, clipped.origin.y + 1.0));
    assert_action_visible(&mounted, "Edit clip-0", false);
    mounted.hover(mounted.rect("Edit clip-0").center());
    assert_action_visible(&mounted, "Edit clip-0", false);
    assert_action_visible(&mounted, "Actions for clip-0", false);
    // Even hovering the visible remainder cannot let controls cover the header.
    mounted.hover(Vector2::new(clipped.center().x, clipped.max_y() - 1.0));
    assert_action_visible(&mounted, "Actions for clip-0", false);
}

#[test]
fn hovered_uninitialized_row_keeps_spawn_disabled() {
    let spawned = State::new(false);
    let mounted = mount(Size::new(380.0, 300.0), || {
        let mut root = node("uninitialized", AgentStatus::Idle).selected();
        root.can_spawn = false;
        view! {
            AgentSidebar roots:(Live::fixed(vec![root]))
                on-spawn:(Some(Rc::new(move |_| spawned.set(true)) as OnAgent))
        }
    });
    mounted.hover(mounted.rect("uninitialized").center());
    assert_action_visible(&mounted, "New worker for uninitialized", true);
    mounted.click_label("New worker for uninitialized");
    assert!(!spawned.get_untracked());
}

/// root ─ a ─ b ─ c ─ d ─ e (a chain six deep) plus a waiting leaf.
fn chain() -> Vec<AgentNode> {
    let e = node("e", AgentStatus::Done);
    let d = node("d", AgentStatus::Queued).children(vec![e]);
    let c = node("c", AgentStatus::Working).children(vec![d]);
    let b = node("b", AgentStatus::Working).children(vec![c]);
    let a = node("a", AgentStatus::Working).children(vec![b, node("w", AgentStatus::Waiting)]);
    vec![node("root", AgentStatus::Working).children(vec![a])]
}

#[test]
fn archive_action_is_only_available_for_eligible_sessions_and_does_not_open_them() {
    let archived = State::new(String::new());
    let opened = State::new(false);
    let eligible = State::new(vec!["idle".to_string()]);
    let on_archive: OnAgent = Rc::new(move |id| archived.set(id));
    let on_open: OnAgent = Rc::new(move |_| opened.set(true));
    let on_spawn: OnAgent = Rc::new(|_| {});
    let mounted = mount(Size::new(520.0, 500.0), || {
        view! {
            AgentSidebar
                roots:(Live::fixed(vec![node("idle", AgentStatus::Idle), node("running", AgentStatus::Working)]))
                archivable:(Live::from(eligible)) on-archive:(Some(on_archive))
                on-open:(Some(on_open)) on-spawn:(Some(on_spawn))
        }
    });
    assert!(mounted.labels().contains(&"Archive idle".to_string()));
    assert!(!mounted.labels().contains(&"Archive running".to_string()));
    let row = mounted.rect("idle");
    let archive = mounted.rect("Archive idle");
    let spawn = mounted.rect("New worker for idle");
    assert!((archive.center().y - row.center().y).abs() < 0.01);
    assert!((spawn.center().y - row.center().y).abs() < 0.01);
    assert!((spawn.origin.x - row.max_x()).abs() < 0.01);
    assert!((spawn.max_x() - archive.origin.x).abs() < 0.01);
    assert_eq!(spawn.size.width, row.size.height);
    assert_eq!(archive.size, spawn.size);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Move,
        position: mounted.rect("idle").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: std::time::Duration::ZERO,
    });
    mounted.settle();
    mounted.click_label("Archive idle");
    assert_eq!(archived.get_untracked(), "idle");
    assert!(!opened.get_untracked());
    eligible.set(Vec::new());
    mounted.settle();
    assert!(!mounted.labels().contains(&"Archive idle".to_string()));
    eligible.set(vec!["idle".into()]);
    mounted.settle();
    mounted.hover(mounted.rect("idle").center());
    archived.set(String::new());
    mounted.click_label("Archive idle");
    assert_eq!(archived.get_untracked(), "idle");
    assert!(!opened.get_untracked());
}

#[test]
fn archived_branch_requires_two_delete_clicks() {
    let edited = State::new(String::new());
    let deleted = State::new(String::new());
    let callback: OnAgent = Rc::new(move |id| deleted.set(id));
    let mut archived = node("archived", AgentStatus::Done);
    archived.archived = true;
    let mounted = mount(Size::new(520.0, 500.0), || {
        let mut roots = vec![archived];
        roots.extend((0..20).map(|i| node(&format!("other-{i}"), AgentStatus::Idle)));
        roots[10] = node("lower", AgentStatus::Done);
        roots[10].archived = true;
        view! {
            AgentSidebar roots:(Live::fixed(roots)) on-delete:(Some(callback))
                on-edit:(Some(Rc::new(move |id| edited.set(id)) as OnAgent)) {
                el height:24px shrink:0 label:"Test footer" {}
            }
        }
    });
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Move,
        position: mounted.rect("archived").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: std::time::Duration::ZERO,
    });
    mounted.settle();
    mounted.click_label("Edit archived");
    assert_eq!(edited.get_untracked(), "archived");
    edited.set(String::new());
    let initial = mounted.rect("Delete archived");
    assert_action_visible(&mounted, "Actions for archived", true);
    assert_action_visible(&mounted, "Delete archived", true);
    mounted.click_label("Delete archived");
    assert!(deleted.get_untracked().is_empty());
    assert_action_visible(&mounted, "Confirm deletion of archived", true);
    let row = mounted.rect("archived");
    let button = mounted.rect("Confirm deletion of archived");
    assert_eq!(button, initial);
    assert!((button.center().y - row.center().y).abs() < 0.01);
    assert!((button.origin.x - mounted.rect("Edit archived").max_x()).abs() < 0.01);
    assert_eq!(button.size.width, row.size.height);
    assert_eq!(button.size.height, row.size.height);
    mounted.click_label("Confirm deletion of archived");
    assert_eq!(deleted.get_untracked(), "archived");
    deleted.set(String::new());
    // A confirming, focused action must not escape over the header when scrolled.
    for delta in [-10.0, -80.0] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Wheel {
                delta: Vector2::new(0.0, delta),
            },
            position: Vector2::new(row.center().x, row.center().y + 100.0),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: std::time::Duration::ZERO,
        });
        mounted.settle();
        assert!(mounted.rect("archived").origin.y < row.origin.y);
        assert_action_visible(&mounted, "Actions for archived", false);
        mounted.click_label("Edit archived");
        assert!(edited.get_untracked().is_empty());
        mounted.click_label("Confirm deletion of archived");
        assert!(deleted.get_untracked().is_empty());
    }
    let scroll_at = Vector2::new(row.center().x, row.center().y + 100.0);
    let wheel = |delta| {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Wheel {
                delta: Vector2::new(0.0, delta),
            },
            position: scroll_at,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: std::time::Duration::ZERO,
        });
        mounted.settle();
    };
    wheel(-250.0);
    mounted.hover(mounted.rect("lower").center());
    mounted.click_label("Delete lower");
    assert_action_visible(&mounted, "Confirm deletion of lower", true);
    let lower = mounted.rect("lower");
    let footer = mounted.rect("Test footer");
    // Move a confirming row partly, then fully, below the viewport.
    wheel(footer.origin.y - lower.origin.y - lower.size.height / 2.0);
    assert_action_visible(&mounted, "Actions for lower", false);
    mounted.click_label("Confirm deletion of lower");
    assert!(deleted.get_untracked().is_empty());
    wheel(50.0);
    assert_action_visible(&mounted, "Actions for lower", false);
    mounted.click_label("Confirm deletion of lower");
    assert!(deleted.get_untracked().is_empty());
}

#[test]
fn tallies_cover_folded_and_collapsed_branches() {
    let mut roots = chain();
    roots[0].expanded = false;
    let tally = StatusTally::of_forest(&roots);
    assert_eq!(
        (tally.working, tally.waiting, tally.queued, tally.done),
        (4, 1, 1, 1)
    );
    assert_eq!(tally.total(), 7);
    assert_eq!(roots[0].waiting_below(), 1);
    assert_eq!(StatusTally::of_descendants(&roots[0]).total(), 6);
}

#[test]
fn status_bar_segments_match_the_board() {
    // Flaky tests: 2 live, 1 waiting, 1 failed → 23, 11, 11 at 0, 24, 36.
    let tally = StatusTally {
        working: 2,
        waiting: 1,
        failed: 1,
        ..Default::default()
    };
    let segments = status_segments(&tally, 48.0);
    let geometry: Vec<(f32, f32)> = segments.iter().map(|s| (s.x, s.width)).collect();
    assert_eq!(geometry, vec![(0.0, 23.0), (24.0, 11.0), (36.0, 11.0)]);
    // The folded branch: 3 live, 2 queued, 1 done → 23, 15, 7.
    let folded = StatusTally {
        working: 3,
        queued: 2,
        done: 1,
        ..Default::default()
    };
    let widths: Vec<f32> = status_segments(&folded, 48.0)
        .iter()
        .map(|s| s.width)
        .collect();
    assert_eq!(widths, vec![23.0, 15.0, 7.0]);
    assert!(status_segments(&StatusTally::default(), 48.0).is_empty());
}

#[test]
fn flattening_folds_past_four_levels_with_square_elbows() {
    let lines = flatten(&chain(), Flatten::default());
    let keys: Vec<String> = lines.iter().map(TreeLine::key).collect();
    assert_eq!(keys, ["root", "a", "b", "c", "c\u{0}folded", "w"]);
    let TreeLine::Agent(c) = &lines[3] else {
        panic!()
    };
    assert_eq!(c.guides, [Guide::Blank, Guide::Pass, Guide::Last]);
    assert!(!c.expandable && c.child_count == 0, "children fold away");
    let TreeLine::Folded(folded) = &lines[4] else {
        panic!()
    };
    assert_eq!(
        folded.guides,
        [Guide::Blank, Guide::Pass, Guide::Blank, Guide::Last]
    );
    assert_eq!(folded.levels, 2);
    assert_eq!(folded.text(), "2 more levels · 2 agents");
    let TreeLine::Agent(w) = &lines[5] else {
        panic!()
    };
    assert_eq!(w.guides, [Guide::Blank, Guide::Last]);
    assert_eq!(w.parent.as_deref(), Some("a"));

    // Unbounded when folding is off.
    let all = flatten(
        &chain(),
        Flatten {
            fold_depth: 0,
            filter: None,
        },
    );
    assert_eq!(all.len(), 7);
    assert_eq!(all[5].guides().len(), 5);
}

#[test]
fn filtering_keeps_matches_and_the_way_to_them() {
    let mut roots = chain();
    roots[0].expanded = false;
    let lines = flatten(
        &roots,
        Flatten {
            fold_depth: 4,
            filter: Some(AgentStatus::Waiting),
        },
    );
    let keys: Vec<String> = lines.iter().map(TreeLine::key).collect();
    assert_eq!(keys, ["root", "a", "w"]);
}

#[test]
fn approvals_carry_their_path_at_any_depth() {
    let deep = node("deep", AgentStatus::Waiting);
    let roots = vec![
        node("r", AgentStatus::Working)
            .children(vec![node("m", AgentStatus::Working).children(vec![deep])]),
    ];
    let found = approvals(&roots);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, ["r", "m"]);
    let branch = find_branch(&roots, "deep").unwrap();
    assert_eq!(branch.depth, 2);
    assert_eq!(levels_below(&roots[0]), 2);
}

fn board() -> Vec<AgentNode> {
    let sol = ModelIdentity::new("Sol", "6.1", ModelHue::Sol);
    vec![
        AgentNode::new(
            "release",
            "Release 2.4",
            AgentStatus::Working,
            Harness::ClaudeCode,
        )
        .model(sol.clone(), Effort::new(5, 3))
        .children(vec![
            AgentNode::new(
                "write",
                "Write migrations",
                AgentStatus::Waiting,
                Harness::ClaudeCode,
            ),
            AgentNode::new("flaky", "Flaky tests", AgentStatus::Working, Harness::Codex)
                .collapsed()
                .children(vec![
                    node("x", AgentStatus::Working),
                    node("y", AgentStatus::Failed),
                    node("q", AgentStatus::Waiting),
                ]),
        ]),
        AgentNode::new("perf", "Perf audit", AgentStatus::Failed, Harness::Codex)
            .model(sol, Effort::new(5, 4))
            .selected(),
    ]
}

#[test]
fn sidebar_shows_counts_needs_you_and_rows() {
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let approved = Rc::new(RefCell::new(Vec::<String>::new()));
    let (open, approve) = (opened.clone(), approved.clone());
    let mounted = mount(Size::new(800.0, 900.0), move || {
        let open: OnAgent = Rc::new(move |id| open.borrow_mut().push(id));
        let approve: OnAgent = Rc::new(move |id| approve.borrow_mut().push(id));
        view! {
            row {
                AgentSidebar roots:(board()) on-open:(Some(open)) on-approve:(Some(approve))
            }
        }
    });
    for label in [
        "3 working",
        "2 waiting",
        "2 failed",
        "0 queued",
        "0 done",
        "New agent",
        "Release 2.4",
        "Perf audit",
        "Effort 4 of 5",
        "1 working, 1 waiting, 1 failed",
    ] {
        assert!(mounted.has(label), "{label}: {:?}", mounted.labels());
    }
    // Two approvals, one inside the collapsed manager; each button and
    // its text carry the name.
    assert_eq!(mounted.count("Approve"), 4);
    assert_eq!(mounted.count("Review diff"), 4);
    assert_eq!(mounted.count("Expand"), 1);
    assert!(
        mounted
            .rects("Release 2.4")
            .iter()
            .any(|rect| rect.size.height == 34.0),
        "rows are 34px"
    );
    let sidebar_width = mounted.rect("Agents").size.width;
    assert_eq!(sidebar_width, 380.0);
    assert_eq!(
        mounted.rect("1 working, 1 waiting, 1 failed").size,
        Size::new(48.0, 4.0)
    );

    mounted.click_label("Perf audit");
    assert_eq!(opened.borrow().as_slice(), ["perf"]);
    mounted.click(mounted.rects("Approve")[0].center());
    assert_eq!(approved.borrow().as_slice(), ["write"]);

    // Expanding the manager shows its children and folds away its bar.
    mounted.click_label("Expand");
    assert!(mounted.has("q"));
    assert!(!mounted.has("1 working, 1 waiting, 1 failed"));
    assert_eq!(mounted.count("Collapse"), 2);
}

#[test]
fn status_cells_filter_the_tree() {
    let mounted = mount(Size::new(800.0, 900.0), || {
        view! {
            row {
                AgentSidebar roots:(board())
            }
        }
    });
    mounted.click_label("2 failed");
    assert!(mounted.has("Perf audit"));
    assert!(mounted.has("y"), "matches inside collapsed branches show");
    assert!(!mounted.has("x"), "non-matching agents hide");
    mounted.click_label("2 failed");
    assert!(!mounted.has("y"));
}

#[test]
fn focus_reroots_with_a_breadcrumb_and_depth() {
    let focused = Rc::new(RefCell::new(Vec::<Option<String>>::new()));
    let seen = focused.clone();
    let mounted = mount(Size::new(240.0, 900.0), move || {
        let on_focus: OnFocus = Rc::new(move |id| seen.borrow_mut().push(id));
        let roots = chain();
        view! {
            row {
                AgentSidebar roots:(roots) width:(240.0) on-focus:(Some(on_focus))
            }
        }
    });
    assert!(mounted.has("2 more levels · 2 agents"));
    mounted.click_label("Focus");
    assert_eq!(focused.borrow().as_slice(), [Some("c".to_string())]);
    assert!(mounted.has("Depth 3"));
    assert!(mounted.has("root / a / b"));
    assert!(mounted.has("Back to Root"));
    let back = mounted.rect("Back to Root");
    let path = mounted.rect("root / a / b");
    let depth = mounted.rect("Depth 3");
    assert!(
        (back.size.width - back.size.height).abs() < 0.5,
        "back action is square: {:?}",
        back.size
    );
    assert!(
        back.size.height > 32.0,
        "back action fills the header height"
    );
    assert!(back.origin.y < path.origin.y);
    assert!(back.max_y() > depth.max_y());
    assert!(path.size.width > 0.0);
    assert!(path.max_x() <= depth.origin.x);
    assert!(depth.max_x() <= 240.0);
    assert!(mounted.has("e"), "the folded levels show once focused");
    assert!(mounted.has("1 working"));
    for _ in 0..30 {
        if mounted.focused_label().as_deref() == Some("Back to Root") {
            break;
        }
        mounted.key(Key::Tab);
    }
    assert_eq!(mounted.focused_label().as_deref(), Some("Back to Root"));
    mounted.key(Key::Enter);
    assert!(!mounted.has("Depth 3"));
    assert_eq!(focused.borrow().last(), Some(&None));
}

#[test]
fn arrow_keys_walk_and_fold_the_tree() {
    let toggles = Rc::new(RefCell::new(Vec::<(String, bool)>::new()));
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let (t, o) = (toggles.clone(), opened.clone());
    let mounted = mount(Size::new(800.0, 900.0), move || {
        let on_toggle: OnToggle = Rc::new(move |id, open| t.borrow_mut().push((id, open)));
        let on_open: OnAgent = Rc::new(move |id| o.borrow_mut().push(id));
        view! {
            row {
                AgentSidebar roots:(board()) on-toggle:(Some(on_toggle)) on-open:(Some(on_open))
            }
        }
    });
    let row = mounted
        .rects("Release 2.4")
        .into_iter()
        .find(|rect| rect.size.height == 34.0)
        .unwrap();
    mounted.click(row.center());
    assert_eq!(mounted.focused_label().as_deref(), Some("Release 2.4"));
    mounted.key(Key::ArrowDown);
    assert_eq!(mounted.focused_label().as_deref(), Some("Write migrations"));
    mounted.key(Key::ArrowDown);
    assert_eq!(mounted.focused_label().as_deref(), Some("Flaky tests"));
    mounted.key(Key::ArrowRight);
    assert_eq!(toggles.borrow().last(), Some(&("flaky".to_string(), true)));
    assert!(mounted.has("q"));
    assert_eq!(
        mounted.focused_label().as_deref(),
        Some("Flaky tests"),
        "focus survives the row rebuilding"
    );
    mounted.key(Key::ArrowRight);
    assert_eq!(mounted.focused_label().as_deref(), Some("x"));
    mounted.key(Key::ArrowLeft);
    assert_eq!(mounted.focused_label().as_deref(), Some("Flaky tests"));
    mounted.key(Key::ArrowLeft);
    assert_eq!(toggles.borrow().last(), Some(&("flaky".to_string(), false)));
    mounted.key(Key::ArrowUp);
    assert_eq!(mounted.focused_label().as_deref(), Some("Write migrations"));
    mounted.key(Key::Enter);
    assert_eq!(opened.borrow().last().map(String::as_str), Some("write"));
}

#[test]
fn each_level_indents_by_one_fourteen_pixel_elbow_column() {
    let mounted = mount(Size::new(380.0, 400.0), || {
        view! {
            AgentSidebar roots:(chain())
        }
    });
    // The status square's x for each displayed depth.
    let square_x = |name: &str| {
        let row = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| n.label.as_deref() == Some(name) && n.role == mosaic::widgets::Role::ListItem)
            .unwrap()
            .rect;
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter(|n| {
                n.role == mosaic::widgets::Role::Image && (n.rect.size.width - 10.0).abs() < 0.5
            })
            .filter(|n| (n.rect.center().y - row.center().y).abs() < 1.0)
            .map(|n| n.rect.origin.x)
            .fold(f32::MAX, f32::min)
    };
    let depths = ["root", "a", "b", "c"].map(square_x);
    for pair in depths.windows(2) {
        assert!((pair[1] - pair[0] - 14.0).abs() < 0.5, "{depths:?}");
    }
}

#[test]
fn status_words_keep_right_padding_and_trim_in_narrow_sidebars() {
    // A phone drawer narrower than the 380px design: five 48px cells.
    let mounted = mount(Size::new(240.0, 200.0), || {
        let tally = StatusTally {
            working: 10,
            waiting: 2,
            failed: 2,
            queued: 4,
            done: 7,
            idle: 0,
        };
        view! {
            col width:{240.0}px {
                StatusCounts tally:(tally)
            }
        }
    });
    for (cell, word) in [
        ("10 working", "working"),
        ("2 waiting", "waiting"),
        ("2 failed", "failed"),
        ("4 queued", "queued"),
        ("7 done", "done"),
    ] {
        let cell = mounted.rect(cell);
        let word = mounted
            .rects(word)
            .into_iter()
            .find(|rect| cell.contains(rect.center()))
            .unwrap_or_else(|| panic!("{word} inside its cell"));
        assert!(
            word.origin.x + word.size.width <= cell.origin.x + cell.size.width - 7.9,
            "{word:?} keeps the right padding of {cell:?}"
        );
    }
    let trimmed = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|node| node.role == mosaic::widgets::Role::Label)
        .filter_map(|node| node.label)
        .filter(|label| label.ends_with('…'))
        .count();
    assert!(trimmed > 0, "long words trim with an ellipsis");
}

#[test]
fn status_words_show_in_full_at_the_design_width() {
    let mounted = mount(Size::new(380.0, 200.0), || {
        let tally = StatusTally {
            working: 10,
            waiting: 2,
            failed: 2,
            queued: 4,
            done: 7,
            idle: 0,
        };
        view! {
            col width:{380.0}px {
                StatusCounts tally:(tally)
            }
        }
    });
    let shown: Vec<String> = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|node| node.role == mosaic::widgets::Role::Label)
        .filter_map(|node| node.label)
        .collect();
    for word in ["working", "waiting", "failed", "queued", "done"] {
        assert!(
            shown.iter().any(|label| label == word),
            "{word} in {shown:?}"
        );
    }
    assert!(!shown.iter().any(|label| label.ends_with('…')), "{shown:?}");
}

#[test]
fn spawn_overlay_reserves_no_space_and_archived_rows_remain_readable() {
    let root = node("overlay", AgentStatus::Done).model(
        ModelIdentity::new("Sol", "6.1", ModelHue::Sol),
        Effort::new(5, 2).with_tiers(2, 2),
    );
    let TreeLine::Agent(line) = flatten(&[root], Flatten::default()).remove(0) else {
        panic!("agent row");
    };
    let opened = State::new(false);
    let on_open: OnAgent = Rc::new(move |_| opened.set(true));
    let mounted = mount(Size::new(500.0, 150.0), || {
        let mut archived = line.clone();
        archived.id = "archived".into();
        archived.name = "archived".into();
        archived.archived = true;
        view! {
            col {
                AgentTreeRow line:(line.clone())
                AgentTreeRow line:(line.clone()) on-spawn:(Some(Rc::new(|_| {}) as OnAgent))
                AgentTreeRow line:(archived) on-open:(Some(on_open))
                    on-spawn:(Some(Rc::new(|_| {}) as OnAgent))
            }
        }
    });
    let before = mounted.rects("Effort 3 of 5");
    assert_eq!(before.len(), 3);
    for (index, rect) in before.iter().enumerate() {
        assert_eq!(rect.size, Size::new(3.0, 21.0));
        let row_top = index as f32 * 34.0;
        assert!(rect.origin.y >= row_top);
        assert!(rect.origin.y + rect.size.height <= row_top + 34.0);
    }
    assert!(
        before
            .iter()
            .all(|rect| rect.origin.x == before[0].origin.x)
    );
    mounted.hover(mounted.rect("New worker for overlay").center());
    assert_eq!(mounted.rects("Effort 3 of 5"), before);
    assert!(mounted.rects("New worker for archived").is_empty());
    mounted.click_label("archived");
    assert!(opened.get_untracked());
}

#[test]
fn elbow_columns_span_the_full_row_and_follow_each_indentation_level() {
    let mounted = mount(Size::new(200.0, 34.0), || {
        view! {
            Elbows guides:(vec![Guide::Pass, Guide::Branch, Guide::Last])
        }
    });
    let scene = mounted.ui.scene();
    let guides: Vec<_> = scene
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            mosaic::render::PaintCmd::Shape(shape)
                if shape.fill.as_solid()
                    == Some(mosaic::core::theme::color(crate::tokens::ground.hairline)) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(guides.len(), 5);
    assert_eq!(guides[0], Rect::from_xywh(6.0, 0.0, 1.0, 34.0));
    assert_eq!(guides[1], Rect::from_xywh(20.0, 0.0, 1.0, 34.0));
    assert_eq!(guides[2], Rect::from_xywh(20.0, 16.5, 8.0, 1.0));
    assert_eq!(guides[3], Rect::from_xywh(34.0, 0.0, 1.0, 17.5));
    assert_eq!(guides[4], Rect::from_xywh(34.0, 16.5, 8.0, 1.0));
}
