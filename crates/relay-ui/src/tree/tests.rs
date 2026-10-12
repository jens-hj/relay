use super::*;
use crate::model::{Effort, ModelHue, ModelIdentity};
use crate::testing::mount;
use mosaic::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

fn node(id: &str, status: AgentStatus) -> AgentNode {
    AgentNode::new(id, id, status, Harness::Codex)
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
    let mounted = mount(Size::new(800.0, 900.0), move || {
        let on_focus: OnFocus = Rc::new(move |id| seen.borrow_mut().push(id));
        let roots = chain();
        view! {
            row {
                AgentSidebar roots:(roots) on-focus:(Some(on_focus))
            }
        }
    });
    assert!(mounted.has("2 more levels · 2 agents"));
    mounted.click_label("Focus");
    assert_eq!(focused.borrow().as_slice(), [Some("c".to_string())]);
    assert!(mounted.has("depth 3"));
    assert!(mounted.has("root / a / b"));
    assert!(mounted.has("Back to root"));
    assert!(mounted.has("e"), "the folded levels show once focused");
    assert!(mounted.has("1 working"));
    mounted.click_label("Back to root");
    assert!(!mounted.has("depth 3"));
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
