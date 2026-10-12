//! The orchestration composites (agent tree, usage, session header,
//! context, fingerprint and the surfaces the pack does not draw), set with
//! the design boards' example data. The data lives only here.

use mosaic::prelude::*;
use relay_ui::prelude::*;
use relay_ui::tokens::{ground, px};
use std::rc::Rc;

fn heading(text: &str) -> Element {
    let text = text.to_uppercase();
    view! {
        col height:min-content gap:{px(10.0)}px {
            Label text:(text) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
            Hairline
        }
    }
}

fn caption(text: &str) -> Element {
    let text = text.to_uppercase();
    view! {
        Label text:(text) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
    }
}

fn sol() -> ModelIdentity {
    ModelIdentity::new("Sol", "6.1", ModelHue::Sol).vendor("GPT")
}
fn opus() -> ModelIdentity {
    ModelIdentity::new("Opus", "5.5", ModelHue::Opus).vendor("Claude")
}
fn sonnet() -> ModelIdentity {
    ModelIdentity::new("Sonnet", "5.5", ModelHue::Sonnet).vendor("Claude")
}
fn haiku() -> ModelIdentity {
    ModelIdentity::new("Haiku", "5.5", ModelHue::Haiku).vendor("Claude")
}

fn agent(
    id: &str,
    name: &str,
    status: AgentStatus,
    harness: Harness,
    model: ModelIdentity,
    level: u8,
) -> AgentNode {
    AgentNode::new(id, name, status, harness).model(model, Effort::new(5, level - 1))
}

/// The agent-tree board's 25 agents. `selected` names the open agent.
pub fn board_tree(selected: &str) -> Vec<AgentNode> {
    use AgentStatus::*;
    use Harness::*;
    let verify =
        agent("verify", "Verify rows", Working, Cursor, haiku(), 2).children(vec![
            agent("shard-a", "Shard A", Working, Cursor, haiku(), 2).children(vec![
                agent("shard-a-retry", "Shard A retry", Queued, Codex, sol(), 3).children(vec![
                    agent("diff-report", "Diff report", Working, Codex, sol(), 4),
                ]),
            ]),
            agent("shard-b", "Shard B", Done, Cursor, haiku(), 2),
            agent("shard-c", "Shard C", Working, Cursor, haiku(), 2).children(vec![agent(
                "count-check",
                "Count check",
                Queued,
                Cursor,
                haiku(),
                1,
            )]),
        ]);
    let mut roots = vec![
        agent("release", "Release 2.4", Working, ClaudeCode, opus(), 4).children(vec![
            agent("api", "API migration", Working, Codex, sol(), 5).children(vec![
                agent("schema", "Schema diff", Done, Codex, sol(), 3),
                agent(
                    "write",
                    "Write migrations",
                    Waiting,
                    ClaudeCode,
                    sonnet(),
                    3,
                ),
                agent("backfill", "Backfill script", Working, Codex, sol(), 4)
                    .children(vec![verify]),
                agent("seed", "Seed fixtures", Queued, Copilot, sonnet(), 2),
            ]),
            agent("docs", "Docs refresh", Done, Copilot, sonnet(), 2)
                .collapsed()
                .children(vec![
                    agent("docs-api", "API reference", Done, Copilot, sonnet(), 2),
                    agent("docs-guides", "Guides", Done, Copilot, sonnet(), 2),
                    agent("docs-changelog", "Changelog", Done, Copilot, haiku(), 1),
                    agent("docs-examples", "Examples", Done, Copilot, haiku(), 1),
                ]),
            agent("flaky", "Flaky tests", Working, ClaudeCode, sonnet(), 4)
                .collapsed()
                .children(vec![
                    agent("rerun", "Rerun suite", Working, ClaudeCode, sonnet(), 3),
                    agent("bisect", "Bisect timing", Working, Codex, sol(), 4),
                    agent("quarantine", "Quarantine list", Waiting, Codex, sol(), 3),
                    agent("isolate", "Isolate fixtures", Failed, Cursor, haiku(), 2),
                ]),
        ]),
        agent("perf", "Perf audit", Failed, Codex, sol(), 5),
        agent("triage", "Triage inbox", Queued, Cursor, haiku(), 1),
    ];
    fn select(nodes: &mut [AgentNode], id: &str) {
        for node in nodes {
            node.selected = node.id == id;
            select(&mut node.children, id);
        }
    }
    select(&mut roots, selected);
    roots
}

fn tree_section() -> Element {
    let roots = State::new(board_tree("backfill"));
    let open: Rc<dyn Fn(String)> = Rc::new(move |id: String| {
        roots.update(|roots| {
            fn select(nodes: &mut [AgentNode], id: &str) {
                for node in nodes {
                    node.selected = node.id == id;
                    select(&mut node.children, id);
                }
            }
            select(roots, &id);
        });
    });
    let focused = State::new(board_tree("verify"));
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Agent tree")
            row height:min-content gap:{px(48.0)}px align:start {
                col width:max-content height:min-content gap:{px(14.0)}px {
                    caption("A · Whole tree")
                    col width:max-content height:{px(862.0)}px {
                        AgentSidebar roots:(roots) on-open:(Some(open))
                    }
                }
                col width:max-content height:min-content gap:{px(14.0)}px {
                    caption("B · Focused on a deep branch")
                    col width:max-content height:{px(476.0)}px {
                        AgentSidebar roots:(focused) focus:(Some("verify".to_string()))
                    }
                }
            }
        }
    }
}

/// Every orchestration composite.
pub fn sections() -> Element {
    view! {
        col height:min-content gap:{px(28.0)}px {
            tree_section()
            usage_section()
            header_section()
        }
    }
}

const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);

/// The conversation board's two harnesses.
pub fn board_usage() -> Vec<relay_ui::usage::HarnessUsage> {
    use relay_ui::usage::{HarnessUsage, ModelShare, Suggestion, UsageInsight, UsageWindow};
    let week = 7 * 24 * HOUR;
    let week_elapsed = week.mul_f32(0.57);
    vec![
        HarnessUsage {
            harness: Harness::ClaudeCode,
            session: Some(
                UsageWindow::new(62.0, 5 * HOUR)
                    .elapsed(2 * HOUR)
                    .resets("resets in 3h 00m"),
            ),
            week: Some(
                UsageWindow::new(48.0, week)
                    .elapsed(week_elapsed)
                    .resets("resets Mon 09:00"),
            ),
            insight: UsageInsight {
                model_mix: vec![
                    ModelShare {
                        name: "Opus".into(),
                        hue: ModelHue::Opus,
                        share: 68.0,
                    },
                    ModelShare {
                        name: "Sonnet".into(),
                        hue: ModelHue::Sonnet,
                        share: 32.0,
                    },
                ],
                heaviest: vec![
                    ("API migration".into(), 41.0),
                    ("Write migrations".into(), 22.0),
                    ("Flaky tests".into(), 14.0),
                ],
                stats: vec![
                    ("peak hour".into(), "14–15h".into()),
                    ("avg session".into(), "71%".into()),
                    ("safe pace".into(), "≤ 12%/h".into()),
                    ("cache hits".into(), "63%".into()),
                ],
                suggestion: Some(Suggestion {
                    subject: "Flaky tests".into(),
                    text: "Sonnet lands the session".into(),
                }),
            },
        },
        HarnessUsage {
            harness: Harness::Codex,
            session: Some(
                UsageWindow::new(84.0, 5 * HOUR)
                    .elapsed(5 * HOUR * 7 / 10)
                    .resets("resets in 1h 30m"),
            ),
            week: Some(
                UsageWindow::new(71.0, week)
                    .elapsed(week_elapsed)
                    .resets("resets Mon 09:00"),
            ),
            insight: UsageInsight::default(),
        },
    ]
}

fn usage_section() -> Element {
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Usage")
            row height:min-content gap:{px(48.0)}px align:start {
                col width:max-content height:min-content gap:{px(14.0)}px {
                    caption("Resting")
                    col width:{px(380.0)}px height:min-content
                        stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                        UsageFoot harnesses:(board_usage())
                    }
                }
                col width:max-content height:min-content gap:{px(14.0)}px {
                    caption("Claude Code opened")
                    col width:{px(380.0)}px height:min-content
                        stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                        UsageFoot harnesses:(board_usage()) open:(Some(Harness::ClaudeCode))
                    }
                }
            }
        }
    }
}

/// The conversation board's session fingerprint.
pub fn board_tools() -> Vec<ToolCount> {
    use relay_ui::tokens::ToolKind::*;
    [
        (Read, 18),
        (Search, 9),
        (Edit, 14),
        (Create, 3),
        (Run, 7),
        (Test, 4),
        (Fetch, 2),
        (Plan, 3),
        (Spawn, 4),
        (Message, 2),
        (Wait, 2),
        (Approval, 1),
        (Result, 1),
    ]
    .into_iter()
    .map(|(kind, count)| ToolCount { kind, count })
    .collect()
}

fn board_context(used: u64, compactions: u32) -> ContextView {
    use relay_ui::context::ContextShare;
    use relay_ui::tokens::ContextPart::*;
    let mut context = ContextView::new(used, 200_000, compactions);
    context.model = "Sol 6.1".into();
    context.history = "Compacted twice, last at 182k".into();
    context.parts = [
        (System, 14_000),
        (FilesRead, 38_000),
        (Conversation, 28_000),
        (SubAgentResults, 22_000),
        (Compacted, 22_000),
    ]
    .into_iter()
    .map(|(part, tokens)| ContextShare { part, tokens })
    .collect();
    context
}

fn header_section() -> Element {
    let noop: OnPress = Rc::new(|| {});
    let (below, pause, stop, compact) = (noop.clone(), noop.clone(), noop.clone(), noop);
    let gauges = view! {
        row height:min-content gap:{px(48.0)}px align:start {}
    };
    for (used, compactions) in [(68_000, 0), (168_000, 1), (192_000, 5)] {
        let context = ContextView::new(used, 200_000, compactions);
        gauges.adopt(&view! {
            ContextGauge context:(context)
        });
    }
    let panel = view! {
        col width:max-content height:min-content fill:ground.panel-raised
            stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {}
    };
    let compact_panel: OnPress = Rc::new(|| {});
    panel.adopt(&view! {
        ContextPanel context:(board_context(124_000, 2)) on-compact:(Some(compact_panel))
    });
    gauges.adopt(&panel);
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Session header, context and fingerprint")
            col width:{px(1172.0)}px height:min-content fill:ground.page
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                SessionHeader path:(vec!["Release 2.4".to_string()]) title:"API migration"
                    status:(AgentStatus::Working) effort:(Some(Effort::new(5, 4)))
                    context:(Some(board_context(124_000, 2))) on-compact:(Some(compact))
                    below:(vec![
                        AgentStatus::Done,
                        AgentStatus::Waiting,
                        AgentStatus::Working,
                        AgentStatus::Queued,
                    ])
                    on-below:(Some(below)) on-pause:(Some(pause)) on-stop:(Some(stop))
                FingerprintStrip tools:(board_tools())
            }
            caption("Context gauge · three states, and opened")
            {
                parent.adopt(&gauges);
            }
            caption("Tool usage strip")
            col width:{px(560.0)}px height:min-content {
                FingerprintStrip tools:(board_tools())
            }
        }
    }
}
