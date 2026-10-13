//! The orchestration composites (agent tree, usage, session header,
//! context, fingerprint and the surfaces the pack does not draw), set with
//! the design boards' example data. The data lives only here.

use mosaic::prelude::*;
use relay_ui::icons::command;
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
                    col width:max-content height:{px(1100.0)}px {
                        AgentSidebar roots:(roots) on-open:(Some(open)) {
                            UsageFoot harnesses:(board_usage())
                            ConnectionStatus state:(relay_ui::surfaces::Connection::Connected)
                                latency:(Some(42u32))
                        }
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
            surfaces_section()
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

fn surfaces_section() -> Element {
    use relay_ui::surfaces::{
        Command, Connection, ConnectionInfo, HarnessAuth, IssueView, NoticeTone, Permission,
        PermissionRow,
    };
    let noop: OnPress = Rc::new(|| {});
    let theme = State::new(0usize);
    let pick: Rc<dyn Fn(usize)> = Rc::new(move |index| theme.set(index));
    let workers = State::new(4i64);
    let step: Rc<dyn Fn(i64)> = Rc::new(move |value| workers.set(value));
    let compact = State::new(true);
    let flip: Rc<dyn Fn(bool)> = Rc::new(move |on| compact.set(on));
    let model = State::new(1usize);
    let choose: Rc<dyn Fn(usize)> = Rc::new(move |index| model.set(index));
    let repository = State::new("jens-hj/relay".to_string());
    let rows = State::new(vec![
        PermissionRow {
            action: "Write to the database".into(),
            choice: Permission::Ask,
            pending: true,
            name: String::new(),
            flags: vec![],
        },
        PermissionRow {
            action: "Edit files in the worktree".into(),
            choice: Permission::Allow,
            pending: false,
            name: String::new(),
            flags: vec![],
        },
        PermissionRow {
            action: "Push to the remote".into(),
            choice: Permission::Deny,
            pending: false,
            name: String::new(),
            flags: vec![],
        },
    ]);
    let change: Rc<dyn Fn(usize, Permission)> = Rc::new(move |index, choice| {
        rows.update(|rows| rows[index].choice = choice);
    });
    let matrix = view! {
        col height:min-content {}
    };
    matrix.switch(
        move || rows.get(),
        move |parent, entries| {
            let change = change.clone();
            parent.adopt(&view! {
                PermissionMatrix entries:(entries.clone()) on-change:(change)
            });
        },
    );
    let query = State::new(String::new());
    let commands = vec![
        Command::new("new", "New agent")
            .keys("N")
            .detail("Create")
            .icon(command.create),
        Command::new("focus", "Focus branch")
            .keys("F")
            .detail("Navigate")
            .icon(command.navigate),
        Command::new("pause", "Pause session")
            .keys("P")
            .detail("Worker")
            .icon(command.worker),
        Command::new("stop", "Stop session")
            .keys("⇧S")
            .detail("Worker")
            .icon(command.worker),
        Command::new("theme", "Switch theme")
            .detail("Settings")
            .icon(command.settings),
    ];
    let run: Rc<dyn Fn(String)> = Rc::new(|_| {});
    let issues = [
        (
            "#31",
            "Agent tree, usage, header and surfaces",
            Some(AgentStatus::Working),
            vec!["ui", "relay-ui"],
            true,
        ),
        (
            "#32",
            "Tool calls and transcript",
            Some(AgentStatus::Waiting),
            vec!["ui"],
            false,
        ),
        (
            "#33",
            "Desktop wiring",
            Some(AgentStatus::Queued),
            vec![],
            false,
        ),
    ];
    let column = view! {
        col height:min-content {}
    };
    for (identifier, title, status, labels, selected) in issues {
        let issue = IssueView {
            identifier: identifier.into(),
            title: title.into(),
            status,
            labels: labels.into_iter().map(String::from).collect(),
            selected,
            ..Default::default()
        };
        column.adopt(&view! {
            IssueCard issue:(issue)
        });
    }
    let (save, retry, rebase, close) = (noop.clone(), noop.clone(), noop.clone(), noop);
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Surfaces in the same language")
            col width:{px(1172.0)}px height:min-content fill:ground.panel
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                PageHeader title:"Settings" path:(vec!["Relay".to_string()]) {
                    HeaderCell label:"Save" on-press:(save) keys:(Some("⌘S".to_string()))
                }
                Notice text:"Push rejected: the remote has new commits" tone:(NoticeTone::Failure)
                    actions:(vec![("Retry".to_string(), retry), ("Rebase".to_string(), rebase)])
                row height:min-content align:start {
                    col width:1fr height:min-content {
                        Module title:"Workspace" {
                            Field label:"Repository" value:(repository) placeholder:"owner/name"
                            row height:{px(40.0)}px
                                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                                Segments
                                    options:(vec!["Dark".to_string(), "Light".to_string(), "High contrast".to_string()])
                                    selected:(theme) on-select:(pick)
                            }
                            row height:{px(40.0)}px
                                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                                row width:{px(150.0)}px shrink:0
                                    stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                                    Stepper value:(workers) on-change:(step) min:1i64 max:12i64
                                        label:"Workers"
                                }
                                row width:{px(220.0)}px shrink:0
                                    stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                                    Select label:"Default model"
                                        options:(vec!["Sol 6.1".to_string(), "Opus 5.5".to_string(), "Sonnet 5.5".to_string()])
                                        selected:(model) on-select:(choose)
                                }
                                row width:1fr align:center
                                    pad:(horizontal:{px(14.0)}px vertical:0px) {
                                    Toggle label:"Compact rows" on:(compact) on-toggle:(flip)
                                }
                            }
                        }
                        Module title:"Permissions" caps:true {
                            {
                                parent.adopt(&matrix);
                            }
                        }
                    }
                    col width:{px(400.0)}px height:min-content
                        stroke:(width:{px(1.0)} color:ground.rule edges:left) {
                        Module title:"Harnesses" {
                            HarnessCard harness:(Harness::Codex) version:"0.42.0"
                                auth:(HarnessAuth::SignedIn)
                            HarnessCard harness:(Harness::ClaudeCode) version:"2.1.4"
                                auth:(HarnessAuth::SignedIn)
                            HarnessCard harness:(Harness::Copilot) auth:(HarnessAuth::SignedOut)
                        }
                        ConnectionStatus state:(Connection::Connected) latency:(Some(42u32))
                            info:(ConnectionInfo {
                                endpoint: "relay.local:7171".into(),
                                transport: "WebSocket".into(),
                                protocol: "5".into(),
                                revision: "cfaf756".into(),
                            })
                        ConnectionStatus state:(Connection::Connecting)
                        ConnectionStatus state:(Connection::Offline)
                    }
                }
            }
            col height:min-content gap:{px(32.0)}px {
                CommandPalette commands:(commands) query:(query) on-run:(run)
                row width:max-content height:{px(300.0)}px fill:ground.panel
                    stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                    BoardColumn name:"In progress" count:3usize {
                        {
                            parent.adopt(&column);
                        }
                    }
                    IssueInspector identifier:"#31" title:"Agent tree, usage, header and surfaces"
                        on-close:(Some(close)) {
                        col height:min-content pad:{px(18.0)}px {
                            Label text:"Body of the inspector" role:(TextRole::Label)
                                tone:(Tone::Muted)
                        }
                    }
                }
            }
        }
    }
}
