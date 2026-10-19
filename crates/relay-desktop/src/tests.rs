use crate::{
    model::{EditTarget, Model, Page},
    network::NetworkState,
    ui,
};
use mosaic::{core::reactive::flush, prelude::*, text::FontContext};
use relay_core::*;
use relay_ui::tokens::{ThemeVariant, theme};
use std::time::Duration;

fn windowed_history(count: usize) -> Mounted {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|snapshot| {
        snapshot.messages = (0..count)
            .map(|index| Message {
                id: format!("window-{index}"),
                session_id: "session-plan".into(),
                author: "Codex".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: format!(
                    "Entry {index}. **Markdown**.\n\n{}",
                    "A variable height paragraph.\n\n".repeat(index % 5 + 1)
                ),
                parts: vec![],
                tool: None,
            })
            .collect();
    });
    for _ in 0..3 {
        mounted.settle();
    }
    mounted
}

#[test]
fn history_window_bounds_mounted_rows_and_keeps_the_composer() {
    for count in [50, 200, 800] {
        let mounted = windowed_history(count);
        let tree = mounted.ui.inspection_snapshot();
        let sources = tree
            .nodes
            .iter()
            .filter(|node| {
                node.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Message window-"))
            })
            .count();
        eprintln!(
            "HISTORY_WINDOW rows={count} mounted={sources} nodes={}",
            tree.nodes.len()
        );
        assert!(sources < 60, "{count} rows mounted {sources}");
        assert!(
            tree.nodes.len() < 2500,
            "{count} rows mounted {} nodes",
            tree.nodes.len()
        );
        let draft = tree
            .nodes
            .iter()
            .find(|node| {
                node.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Draft text "))
            })
            .unwrap();
        let draft_id = draft.id;
        let draft_label = draft.label.clone().unwrap();
        mounted.click(&draft_label);
        mounted.key(Key::Character("draft survives".into()), false);
        mounted.model.snapshot.update(|snapshot| {
            snapshot
                .messages
                .last_mut()
                .unwrap()
                .body
                .push_str(" Streamed tail.")
        });
        mounted.settle();
        mounted.model.source_anchor.set(Some(relay_core::Anchor {
            message_id: "window-0".into(),
            start: 0,
            end: 5,
            quote: "Entry".into(),
        }));
        mounted.settle();
        assert!(
            mounted.model.source_anchor.get_untracked().is_none(),
            "source anchor was not resolved"
        );
        let source = mounted.rect("Message window-0");
        let viewport = mounted.rect("Transcript viewport");
        assert!(
            source.origin.y < viewport.origin.y + viewport.size.height
                && source.origin.y + source.size.height > viewport.origin.y
        );
        let after = mounted.ui.inspection_snapshot();
        assert_eq!(
            after
                .nodes
                .iter()
                .find(|node| node.label.as_deref() == Some(&draft_label))
                .unwrap()
                .id,
            draft_id
        );
        assert!(crate::buffer::parts(mounted.model, "session-plan").iter().any(|part| matches!(&part.kind, PartKind::Text { text } if text.contains("draft survives"))));
        assert_eq!(mounted.model.snapshot.get_untracked().messages.len(), count);
    }
}

#[test]
fn history_window_search_and_keyboard_reveal_unmounted_rows() {
    let mounted = windowed_history(800);
    mounted.model.searching.set(true);
    mounted.settle();
    mounted.focus("Find in conversation");
    mounted.key(Key::Character("Entry 317.".into()), false);
    assert!(mounted.model.source_anchor.get_untracked().is_none());
    assert_eq!(
        focused_label(&mounted).as_deref(),
        Some("Find in conversation")
    );
    assert!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|node| node.label.as_deref() == Some("Message window-317"))
    );
    mounted.focus("Message window-317");
    mounted.key(Key::Home, true);
    mounted.settle();
    let first = mounted.rect("Message window-0");
    let viewport = mounted.rect("Transcript viewport");
    assert!(
        first.origin.y + first.size.height > viewport.origin.y
            && first.origin.y < viewport.origin.y + viewport.size.height
    );
    mounted.key(Key::End, true);
    mounted.settle();
    let tree = mounted.ui.inspection_snapshot();
    assert!(
        tree.nodes
            .iter()
            .any(|node| node.label.as_deref() == Some("Message window-799"))
    );
    assert!(
        tree.nodes
            .iter()
            .filter(|node| node
                .label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message window-")))
            .count()
            < 60
    );
    let end = mounted
        .model
        .snapshot
        .with_untracked(|snapshot| snapshot.messages[317].body.len());
    mounted.model.source_anchor.set(Some(relay_core::Anchor {
        message_id: "window-317".into(),
        start: end,
        end,
        quote: String::new(),
    }));
    mounted.settle();
    let source = mounted.ui.focused().unwrap().id();
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, -1000000.0),
        },
        position: mounted.rect("Transcript viewport").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    assert_eq!(mounted.ui.focused().unwrap().id(), source);
    assert!(!has_label(&mounted, "Message window-318"));
    mounted.key(Key::ArrowRight, false);
    mounted.settle();
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .node(mounted.ui.focused().unwrap().id())
            .unwrap()
            .label
            .as_deref(),
        Some("Message window-318")
    );
}

#[test]
fn history_window_manual_scroll_keeps_draft_focus_and_reading_position() {
    let mut mounted = windowed_history(800);
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|node| node.label.filter(|label| label.starts_with("Draft text ")))
        .unwrap();
    mounted.focus(&draft);
    let focused = mounted.ui.focused().unwrap().id();
    crate::browser_text::update(&mounted.ui, focused.raw(), "kept", 4, 4, false);
    mounted.settle();
    crate::browser_text::update(&mounted.ui, focused.raw(), "kept你", 5, 5, true);
    mounted.settle();
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 2000.0),
        },
        position: mounted.rect("Transcript viewport").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    for _ in 0..3 {
        mounted.settle();
    }
    assert_eq!(mounted.ui.focused().unwrap().id(), focused);
    let viewport = mounted.rect("Transcript viewport");
    assert!(
        mounted.rect(&draft).origin.y > viewport.max_y(),
        "manual scrolling returned to the draft"
    );
    let visible = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message window-"))
                && node.rect.origin.y < viewport.max_y()
                && node.rect.max_y() > viewport.origin.y
        })
        .unwrap();
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str(" New tail.")
    });
    mounted.settle();
    assert!(
        (mounted.rect(visible.label.as_deref().unwrap()).origin.y - visible.rect.origin.y).abs()
            < 2.0
    );
    assert_eq!(mounted.ui.focused().unwrap().id(), focused);
    mounted.size.width = 950.0;
    for _ in 0..3 {
        mounted.settle();
    }
    let resized_viewport = mounted.rect("Transcript viewport");
    let anchor = mounted.rect(visible.label.as_deref().unwrap());
    assert!(
        anchor.max_y() > resized_viewport.origin.y && anchor.origin.y < resized_viewport.max_y(),
        "resize lost the reading anchor"
    );
    assert_eq!(mounted.ui.focused().unwrap().id(), focused);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "kept"
    );
    crate::browser_text::update(&mounted.ui, focused.raw(), "kept你", 5, 5, false);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "kept你"
    );
    mounted.key(Key::Character("z".into()), true);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "kept"
    );
}

#[test]
fn history_window_preserves_disclosure_state_across_disposal() {
    for tool in [false, true] {
        let mounted = windowed_history(200);
        mounted.model.snapshot.update(|snapshot| {
            snapshot.messages[0].kind = "reasoning".into();
            if tool {
                snapshot.messages[0].tool = Some(ToolCall {
                    run_id: "window-run".into(),
                    name: "exec_command".into(),
                    kind: ToolKind::Run,
                    state: ToolState::Completed,
                    input: "pwd".into(),
                    target: "pwd".into(),
                    directory: None,
                    exit_code: Some(0),
                    started_ms: None,
                    finished_ms: None,
                    detail: None,
                });
            }
        });
        mounted.model.source_anchor.set(Some(relay_core::Anchor {
            message_id: "window-0".into(),
            start: 0,
            end: 0,
            quote: String::new(),
        }));
        mounted.settle();
        let expanded_label = if tool {
            "Summary. Expand fully"
        } else {
            "Message window-0"
        };
        if tool {
            let collapsed = mounted
                .ui
                .inspection_snapshot()
                .nodes
                .into_iter()
                .find_map(|node| node.label.filter(|label| label.starts_with("Collapsed.")))
                .unwrap();
            mounted.click(&collapsed);
        }
        assert!(has_label(&mounted, expanded_label));
        // Remove focus and selection, then leave the row so it really disposes.
        let draft = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find_map(|node| node.label.filter(|label| label.starts_with("Draft text ")))
            .unwrap();
        mounted.focus(&draft);
        mounted.key(Key::End, true);
        mounted.settle();
        assert!(!has_label(&mounted, expanded_label));
        // A manual scroll mounts the first row without requesting/expanding it.
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Wheel {
                delta: Vector2::new(0.0, 1000000.0),
            },
            position: mounted.rect("Transcript viewport").center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        for _ in 0..3 {
            mounted.settle();
        }
        assert!(
            has_label(&mounted, expanded_label),
            "disclosure was lost on remount (tool={tool})"
        );
    }
}

#[test]
fn history_window_reply_source_waits_for_layout_and_preserves_selected_source() {
    let mounted = windowed_history(800);
    let anchor = relay_core::Anchor {
        message_id: "window-0".into(),
        start: 0,
        end: 5,
        quote: "Entry".into(),
    };
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![
            Part {
                id: "window-reply".into(),
                kind: PartKind::Reply {
                    anchor: anchor.clone(),
                    parts: vec![Part::text("Reply stays intact")],
                },
            },
            Part::text("After reply"),
        ],
    );
    mounted.settle();
    assert!(!has_label(&mounted, "Message window-0"));
    mounted.focus("Go to reply source");
    mounted.ui.dispatch_key(KeyEvent {
        kind: KeyEventKind::Down { repeat: false },
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    flush();
    assert_eq!(
        mounted.model.source_anchor.get_untracked(),
        Some(anchor),
        "the anchor must remain pending before layout"
    );
    mounted.settle();
    assert!(mounted.model.source_anchor.get_untracked().is_none());
    assert_eq!(focused_label(&mounted).as_deref(), Some("Message window-0"));
    let selected = mounted.ui.focused().unwrap().id();
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some("Entry"));
    mounted.key(Key::End, true);
    mounted.settle();
    assert!(has_label(&mounted, "Message window-799"));
    assert!(
        mounted.ui.inspection_snapshot().node(selected).is_some(),
        "selected source must survive leaving its window"
    );
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str(" More output.")
    });
    mounted.settle();
    mounted.focus("Message window-0");
    assert_eq!(mounted.ui.focused().unwrap().id(), selected);
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some("Entry"));
    assert!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan"))
            .contains("Reply stays intact")
    );
}

#[test]
fn history_window_tall_expanded_tool_preserves_following_and_mount_bounds() {
    let mounted = windowed_history(200);
    mounted.model.snapshot.update(|snapshot| {
        let message = &mut snapshot.messages[100];
        message.body = (0..100).map(|i| format!("Output line {i}\n")).collect();
        message.tool = Some(ToolCall {
            run_id: "tall-run".into(),
            name: "exec_command".into(),
            kind: ToolKind::Run,
            state: ToolState::Completed,
            input: "long command".into(),
            target: "long command".into(),
            directory: None,
            exit_code: Some(0),
            started_ms: None,
            finished_ms: None,
            detail: None,
        });
    });
    mounted.model.source_anchor.set(Some(relay_core::Anchor {
        message_id: "window-100".into(),
        start: 0,
        end: 0,
        quote: String::new(),
    }));
    mounted.settle();
    let collapsed = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|node| node.label.filter(|label| label.starts_with("Collapsed.")))
        .unwrap();
    mounted.click(&collapsed);
    mounted.click("Summary. Expand fully");
    let viewport = mounted.rect("Transcript viewport");
    assert!(
        mounted.rect("Tool call window-100").size.height > viewport.size.height,
        "fixture must exercise a tool taller than the viewport"
    );
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|node| node.label.filter(|label| label.starts_with("Draft text ")))
        .unwrap();
    mounted.focus(&draft);
    let editor = mounted.ui.focused().unwrap().id();
    mounted.key(Key::End, true);
    assert!(!has_label(&mounted, "Tool call window-100"));
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str("\n\nStreamed at bottom.\n\nMore output.")
    });
    mounted.settle();
    assert_eq!(mounted.ui.focused().unwrap().id(), editor);
    assert!(mounted.rect(&draft).max_y() <= mounted.rect("Transcript viewport").max_y() + 1.0);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 1500.0),
        },
        position: viewport.center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    for _ in 0..3 {
        mounted.settle();
    }
    let viewport = mounted.rect("Transcript viewport");
    let visible = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message window-"))
                && node.rect.origin.y < viewport.max_y()
                && node.rect.max_y() > viewport.origin.y
        })
        .unwrap();
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str("\n\nUnseen streaming tail.")
    });
    mounted.settle();
    assert!(
        (mounted.rect(visible.label.as_deref().unwrap()).origin.y - visible.rect.origin.y).abs()
            < 2.0
    );
    mounted.model.source_anchor.set(Some(relay_core::Anchor {
        message_id: "window-100".into(),
        start: 0,
        end: 0,
        quote: String::new(),
    }));
    mounted.settle();
    assert!(has_label(&mounted, "Expanded. Collapse"));
    assert!(mounted.rect("Tool call window-100").size.height > viewport.size.height);
    let tree = mounted.ui.inspection_snapshot();
    assert!(
        tree.nodes
            .iter()
            .filter(|node| node
                .label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message window-")))
            .count()
            < 60
    );
    assert!(tree.nodes.len() < 2500);
}

#[test]
fn history_window_changed_offscreen_height_corrects_growth_and_shrink_without_losing_anchor() {
    let mounted = windowed_history(200);
    let reveal = |id: &str, end| {
        mounted.model.source_anchor.set(Some(relay_core::Anchor {
            message_id: id.into(),
            start: 0,
            end,
            quote: String::new(),
        }));
        mounted.settle();
    };
    // Keep this source selected so its live geometry remains available while
    // neighboring rows really dispose. It is the reader's pixel anchor.
    reveal("window-101", 5);
    for tall in [true, false] {
        reveal("window-190", 0);
        assert!(!has_label(&mounted, "Message window-100"));
        let before = mounted.rect("Message window-190");
        mounted.model.snapshot.update(|snapshot| {
            let body = if tall {
                "Changed paragraph.\n\n".repeat(80)
            } else {
                "Short again.".into()
            };
            snapshot.messages[100].body = body.clone();
            snapshot.messages[100].parts = vec![Part {
                id: "changed-part".into(),
                kind: PartKind::Text { text: body },
            }];
            snapshot.comments.clear();
            if tall {
                snapshot.comments.push(Comment {
                    id: "changed-comment".into(),
                    message_id: "window-100".into(),
                    quote: None,
                    author: "Reader".into(),
                    body: "A wrapped comment. ".repeat(20),
                    created_at: 1,
                });
            }
        });
        mounted.settle();
        assert!(!has_label(&mounted, "Message window-100"));
        assert!(
            (mounted.rect("Message window-190").origin.y - before.origin.y).abs() < 1.0,
            "offscreen invalidation moved the reader before measurement"
        );
        let viewport = mounted.rect("Transcript viewport");
        let expected_y = viewport.origin.y - 8.0;
        let delta = expected_y - mounted.rect("Message window-101").origin.y;
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Wheel {
                delta: Vector2::new(0.0, delta),
            },
            position: viewport.center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        for _ in 0..3 {
            mounted.settle();
        }
        assert!(
            (mounted.rect("Message window-101").origin.y - expected_y).abs() < 2.0,
            "remount height correction moved the pixel anchor (tall={tall})"
        );
        assert!(has_label(&mounted, "Message window-100"));
    }
    reveal("window-190", 0);
    mounted
        .model
        .snapshot
        .update(|snapshot| snapshot.messages[100].body = "Grown again.\n\n".repeat(80));
    mounted
        .model
        .snapshot
        .update(|snapshot| snapshot.messages[100].parts.clear());
    reveal("window-101", 0);
    let viewport = mounted.rect("Transcript viewport");
    let target = mounted.rect("Message window-101");
    assert!(
        target.max_y() > viewport.origin.y && target.origin.y < viewport.max_y(),
        "programmatic reveal must not double-apply a height correction"
    );
}

#[test]
fn history_window_repeated_remote_reveal_resolves_without_new_layout() {
    let mounted = windowed_history(200);
    mounted.model.remote_search.set(Some("window-100".into()));
    mounted.model.focused_message.set("window-100".into());
    mounted.settle();
    assert!(mounted.model.focused_message.get_untracked().is_empty());
    assert!(has_label(&mounted, "Message window-100"));
    mounted.model.focused_message.set("window-100".into());
    flush();
    assert!(
        mounted.model.focused_message.get_untracked().is_empty(),
        "an already-mounted result must not wait for a content-size change"
    );
    let before = mounted.rect("Message window-100");
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 200.0),
        },
        position: mounted.rect("Transcript viewport").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let after = mounted.rect("Message window-100");
    assert!(after.origin.y > before.origin.y + 40.0);
    mounted
        .model
        .snapshot
        .update(|snapshot| snapshot.revision += 1);
    mounted.settle();
    assert!(
        (mounted.rect("Message window-100").origin.y - after.origin.y).abs() < 1.0,
        "retained remote result moved the reader again"
    );
}

#[test]
#[ignore = "diagnostic history scaling probe"]
fn diagnose_chat_history_scaling() {
    for count in [50, 200, 800] {
        let mounted = mount(false, 1380.0);
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|snapshot| {
            snapshot.messages.clear();
            for index in 0..count {
                snapshot.messages.push(Message {
                    id: format!("probe-{index}"), session_id: "session-plan".into(),
                    author: "Codex".into(), kind: "assistant".into(),
                    sender: Sender::User,
                    body: format!("Message {index}. **Markdown** with a [link](https://example.com).\n\nA second paragraph."),
                    parts: vec![], tool: None,
                });
            }
        });
        mounted.settle();
        let tree = mounted.ui.inspection_snapshot();
        let sources = tree
            .nodes
            .iter()
            .filter(|n| {
                n.label
                    .as_deref()
                    .is_some_and(|s| s.starts_with("Message probe-"))
            })
            .count();
        for operation in ["snapshot", "tail", "search"] {
            let start = std::time::Instant::now();
            match operation {
                "snapshot" => mounted.model.snapshot.update(|_| {}),
                "tail" => mounted
                    .model
                    .snapshot
                    .update(|s| s.messages.last_mut().unwrap().body.push_str(" Streamed.")),
                _ => {
                    mounted.model.searching.set(true);
                    mounted.model.search.set("paragraph".into());
                }
            }
            flush();
            let flush_ms = start.elapsed().as_secs_f64() * 1000.0;
            mounted.settle();
            let stats = mounted.ui.frame_stats();
            eprintln!(
                "HISTORY_PROBE rows={count} nodes={} sources={sources} op={operation} flush_ms={flush_ms:.3} total_ms={:.3} last_frame_inherit_ms={:.3} last_frame_layout_ms={:.3} last_frame_geometry_ms={:.3} last_frame_paint_ms={:.3} elements={} painted={} culled={}",
                tree.nodes.len(),
                start.elapsed().as_secs_f64() * 1000.0,
                stats.inherit,
                stats.layout,
                stats.geometry,
                stats.paint,
                stats.elements,
                stats.painted,
                stats.culled
            );
        }
    }
}

#[test]
fn transcript_keys_and_unchanged_source_survive_global_and_tail_updates() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|snapshot| {
        snapshot.messages = (0..20)
            .map(|index| Message {
                id: format!("indexed-{index}"),
                session_id: "session-plan".into(),
                author: "Codex".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: format!("Source {index}. **Text**."),
                parts: vec![],
                tool: None,
            })
            .collect();
    });
    mounted.settle();
    let field = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| node.label.as_deref() == Some("Message indexed-0"))
        .unwrap()
        .id;
    mounted.model.snapshot.update(|snapshot| {
        snapshot.revision += 1;
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str(" Streamed tail.");
    });
    mounted.settle();
    let tree = mounted.ui.inspection_snapshot();
    assert_eq!(
        tree.nodes
            .iter()
            .find(|node| node.label.as_deref() == Some("Message indexed-0"))
            .unwrap()
            .id,
        field
    );
    assert!(
        tree.nodes
            .iter()
            .any(|node| node.label.as_deref() == Some("Message indexed-19"))
    );
}

struct Mounted {
    _scope: Scope,
    ui: Ui,
    model: Model,
    commands: tokio::sync::mpsc::UnboundedReceiver<CommandEnvelope>,
    size: Size,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self._scope.dispose();
    }
}

fn mount(light: bool, width: f32) -> Mounted {
    mosaic::core::builtins::install();
    install_theme(&theme(ThemeVariant::select(light, false), 1.0));
    install_theme(&relay_ui::icons::icons());
    install_theme(&crate::icons::icons());
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    let mut fonts = FontContext::embedded_only();
    relay_ui::fonts::install(&mut fonts);
    assert!(fonts.has_family(relay_ui::fonts::MONO));
    assert!(fonts.has_family(relay_ui::fonts::SERIF));
    ui.set_fonts(fonts);
    relay_ui::attach(&ui);
    let (sender, commands) = tokio::sync::mpsc::unbounded_channel();
    let model = scope.run(|| Model::new(&ui, sender));
    model.receive(NetworkState {
        snapshot: demo_snapshot(AgentProfile::default()),
        connected: true,
        status: "Connected".into(),
        ..Default::default()
    });
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&ui::shell(model));
    });
    let mounted = Mounted {
        _scope: scope,
        ui,
        model,
        commands,
        size: Size::new(width, 900.0),
    };
    mounted.settle();
    mounted
}

#[test]
fn command_discovery_errors_are_visible_and_retryable() {
    let mounted = mount(false, 820.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.sessions.clear();
    snapshot.projects[0].fixture = false;
    let agent_config_id = snapshot.agent_configs[0].id.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_root_agent(agent_config_id);
    let id = mounted.model.session.get_untracked();
    crate::buffer::edit(mounted.model, &id, vec![Part::text("/")]);
    mounted.settle();
    assert_eq!(
        crate::command_ui::discovery_feedback(mounted.model).as_deref(),
        Some("Loading commands and skills…")
    );
    assert!(mounted.rect("Command discovery status").size.height > 0.0);
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            id.clone(),
            Err("Harness discovery timed out".into()),
        )]));
    mounted.settle();
    assert!(
        crate::command_ui::discovery_feedback(mounted.model)
            .unwrap()
            .contains("timed out")
    );
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(sender));
    mounted.click("Retry command discovery");
    assert!(
        matches!(requests.try_recv().unwrap(), crate::buffer_network::Request::Catalog {session,force:true} if session == id)
    );
    assert_eq!(plain_text(&crate::buffer::parts(mounted.model, &id)), "/");
}

#[test]
fn first_director_draft_requests_command_discovery_and_completes() {
    let mounted = mount(false, 820.0);
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(sender));
    mounted
        ._scope
        .run(|| crate::command_ui::bind_discovery(mounted.model));
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.sessions.clear();
    snapshot.projects[0].fixture = false;
    let agent_config_id = snapshot.agent_configs[0].id.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_root_agent(agent_config_id);
    mounted.settle();
    let id = mounted.model.session.get_untracked();
    assert!(id.starts_with("agent-draft-"));
    assert!(
        matches!(requests.try_recv().unwrap(), crate::buffer_network::Request::Catalog {session, force:false} if session == id)
    );
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("/com".into()), false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            id.clone(),
            Ok(HarnessCatalog {
                commands: vec![HarnessCommand {
                    name: "compact".into(),
                    description: "Compact context".into(),
                    argument_hint: String::new(),
                    dispatch: CommandDispatch::Unavailable,
                    reason: Some("Send the first message before using this command".into()),
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    assert_eq!(
        mounted.model.completion.get_untracked().unwrap().choices[0].label,
        "/compact"
    );
    mounted.key(Key::Tab, false);
    assert!(
        mounted
            .model
            .command_feedback
            .get_untracked()
            .contains("first message")
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &id)),
        "/com"
    );
}

#[test]
fn inline_suggestions_filter_browser_input_and_remain_clickable() {
    for width in [600.0, 1440.0] {
        let mounted = mount(false, width);
        mounted.model.open_session("session-plan".into());
        mounted
            .model
            .catalogs
            .set(std::collections::BTreeMap::from([(
                "session-plan".into(),
                Ok(HarnessCatalog {
                    skills: ["build", "review"]
                        .into_iter()
                        .map(|name| HarnessSkill {
                            id: name.into(),
                            name: name.into(),
                            description: "Run skill".into(),
                            argument_hint: String::new(),
                            enabled: true,
                            path: None,
                        })
                        .collect(),
                    commands: ["add-dir", "compact", "model", "resume", "status", "help"]
                        .into_iter()
                        .map(|name| HarnessCommand {
                            name: name.into(),
                            description: "Run command".into(),
                            argument_hint: String::new(),
                            dispatch: CommandDispatch::Unavailable,
                            reason: Some(
                                [
                                    "Test command with a long description",
                                    "that must stay on one line.",
                                ]
                                .join("\n")
                                .repeat(20),
                            ),
                        })
                        .collect(),
                    ..Default::default()
                }),
            )]));
        mounted.settle();
        for (prefix, query, expected) in [
            ("/", "/com", "/compact"),
            ("/", "/mod", "/model"),
            ("$", "$rev", "$review"),
            ("$", "$bui", "$build"),
        ] {
            crate::buffer::edit(mounted.model, "session-plan", vec![Part::text("")]);
            mounted.settle();
            let draft = mounted
                .ui
                .inspection_snapshot()
                .nodes
                .into_iter()
                .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
                .unwrap();
            mounted.focus(&draft);
            let id = mounted.ui.focused().unwrap().id().raw();
            for text in [prefix, query, prefix, query] {
                crate::browser_text::update(&mounted.ui, id, text, text.len(), text.len(), false);
                mounted.settle();
                let completion = mounted.model.completion.get_untracked().unwrap();
                assert_eq!(
                    completion.choices.len(),
                    if text == prefix {
                        if prefix == "/" { 4 } else { 2 }
                    } else {
                        1
                    }
                );
                let mut rendered = mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .into_iter()
                    .filter(|n| n.role == Role::Button)
                    .filter_map(|n| n.label.filter(|s| s.starts_with('/') || s.starts_with('$')))
                    .collect::<Vec<_>>();
                let mut labels = completion
                    .choices
                    .iter()
                    .map(|c| c.label.clone())
                    .collect::<Vec<_>>();
                rendered.sort();
                labels.sort();
                assert_eq!(rendered, labels, "Rendered rows must follow filtering");
                let list = mounted.rect("Inline suggestions");
                assert!((list.size.height - completion.choices.len() as f32 * 32.0).abs() < 0.1);
                let rows = completion
                    .choices
                    .iter()
                    .map(|c| mounted.rect(&c.label))
                    .collect::<Vec<_>>();
                for pair in rows.windows(2) {
                    assert!(
                        (pair[0].max_y() - pair[1].origin.y).abs() < 0.1,
                        "Suggestion rows share edges"
                    );
                }
                assert!(
                    !mounted
                        .ui
                        .inspection_snapshot()
                        .nodes
                        .iter()
                        .any(|n| n.label.as_deref() == Some("Harness commands and skills"))
                );
                let rect = mounted.rect(expected);
                let target = mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .into_iter()
                    .find(|n| n.label.as_deref() == Some(expected))
                    .unwrap();
                let tree = mounted.ui.inspection_snapshot();
                let mut hit = mounted.ui.hit_test(rect.center()).map(|e| e.id());
                while hit.is_some_and(|id| id != target.id) {
                    hit = tree
                        .nodes
                        .iter()
                        .find(|n| Some(n.id) == hit)
                        .and_then(|n| n.parent);
                }
                assert_eq!(
                    hit,
                    Some(target.id),
                    "Suggestion clipped at width {width}: {expected}"
                );
            }
            mounted.click(expected);
            assert!(mounted.model.completion.get_untracked().is_none());
            if prefix == "$" {
                assert_eq!(
                    selected_skills(&crate::buffer::parts(mounted.model, "session-plan"))[0].id,
                    expected.trim_start_matches('$')
                );
            } else {
                assert!(
                    mounted
                        .model
                        .command_feedback
                        .get_untracked()
                        .contains("Test command")
                );
            }
        }
    }
}

#[test]
fn harness_completion_inserts_durable_skills_and_opens_native_model_flow() {
    let mut mounted = mount(false, 820.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            Ok(HarnessCatalog {
                session_id: "session-plan".into(),
                harness: Harness::ClaudeCode,
                skills: vec![HarnessSkill {
                    id: "native-review".into(),
                    name: "review".into(),
                    description: "Review changes".into(),
                    argument_hint: String::new(),
                    enabled: true,
                    path: None,
                }],
                commands: vec![HarnessCommand {
                    name: "model".into(),
                    description: "Choose a model".into(),
                    argument_hint: String::new(),
                    dispatch: CommandDispatch::Flow,
                    reason: None,
                }],
                models: vec![HarnessModel {
                    id: "test-model".into(),
                    name: "Test model".into(),
                    efforts: vec!["high".into()],
                    default_effort: None,
                    fast: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    let catalog = mounted.model.catalogs.get_untracked();
    mounted.model.catalogs.set(Default::default());
    mounted.settle();
    mounted.key(Key::Character("$rev".into()), false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted.model.catalogs.set(catalog);
    mounted.settle();
    assert_eq!(
        mounted.model.completion.get_untracked().unwrap().choices[0].label,
        "$review"
    );
    let suggestion = mounted.rect("$review");
    assert!(suggestion.size.height > 0.0 && suggestion.size.width > 0.0);
    assert!(suggestion.origin.y >= 0.0 && suggestion.max_y() <= mounted.size.height);
    mounted.key(Key::Tab, false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(selected_skills(&parts)[0].id, "native-review");
    assert_eq!(plain_text(&parts), "$review ");
    mounted.key(Key::Character("λ".into()), false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$review λ"
    );
    let parts = vec![Part::text("")];
    let id = parts[0].id.clone();
    crate::buffer::edit(mounted.model, "session-plan", parts);
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.ui.set_clipboard_text("/mod");
    mounted.key(Key::Character("v".into()), true);
    assert_eq!(mounted.model.completion.get_untracked().unwrap().part, id);
    mounted.settle();
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.command_flow.get_untracked(), "model");
    mounted.click("Choose model Test model");
    mounted.click("Choose effort high");
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(envelope.command,Command::SetHarnessSelection{selection:HarnessSelection{model:Some(ref model),effort:Some(ref effort),..},..} if model=="test-model" && effort=="high")
    );
}

#[test]
fn harness_command_panel_is_attached_bounded_and_escape_restores_draft_focus() {
    for (width, height) in [(1380.0, 900.0), (820.0, 900.0), (500.0, 480.0)] {
        let mut mounted = mount(false, width);
        mounted.size.height = height;
        mounted.model.open_session("session-plan".into());
        let mut catalog = demo_catalog("session-plan", Harness::Codex);
        catalog.commands.push(HarnessCommand {
            name: "model".into(),
            description: "Choose model".into(),
            argument_hint: String::new(),
            dispatch: CommandDispatch::Flow,
            reason: None,
        });
        catalog.models = (0..24)
            .map(|i| HarnessModel {
                id: format!("model-{i}"),
                name: format!("Model {i}"),
                efforts: vec!["low".into(), "high".into()],
                default_effort: Some("low".into()),
                fast: false,
                ..Default::default()
            })
            .collect();
        mounted
            .model
            .catalogs
            .set(std::collections::BTreeMap::from([(
                "session-plan".into(),
                Ok(catalog),
            )]));
        mounted.settle();
        let before = mounted.rect("Transcript viewport");
        let draft = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
            .unwrap();
        mounted.focus(&draft);
        mounted.key(Key::Character("/model".into()), false);
        let suggestions = mounted.rect("Command suggestions panel");
        let composer = mounted.rect("Message composer");
        assert!((suggestions.max_y() - composer.origin.y).abs() < 0.1);
        assert!((suggestions.origin.x - composer.origin.x).abs() < 0.1);
        assert!((suggestions.size.width - composer.size.width).abs() < 0.1);
        mounted.key(Key::Enter, false);
        assert_eq!(mounted.model.command_flow.get_untracked(), "model");
        let panel = mounted.rect("Harness command panel");
        let composer = mounted.rect("Message composer");
        let transcript = mounted.rect("Transcript viewport");
        assert!((panel.max_y() - composer.origin.y).abs() < 0.1);
        assert!((panel.origin.x - composer.origin.x).abs() < 0.1);
        assert!((panel.size.width - composer.size.width).abs() < 0.1);
        assert!(panel.size.height <= 280.1 && panel.size.height > 0.0);
        assert!(panel.origin.x >= transcript.origin.x && panel.max_x() <= transcript.max_x());
        assert!(
            panel.origin.y >= transcript.origin.y - 0.1
                && panel.max_y() <= transcript.max_y() + 0.1,
            "{width}x{height}: transcript {transcript:?}, panel {panel:?}, composer {composer:?}"
        );
        assert_eq!(
            transcript.size.height, before.size.height,
            "the panel belongs to the shared transcript viewport"
        );
        assert!(
            composer.size.height > 0.0,
            "the inline draft stays mounted below the command panel"
        );
        assert_eq!(
            focused_label(&mounted).as_deref(),
            Some("Choose model Model 0")
        );
        mounted.key(Key::ArrowDown, false);
        assert_eq!(
            focused_label(&mounted).as_deref(),
            Some("Choose model Model 1")
        );
        mounted.key(Key::ArrowUp, false);
        assert_eq!(
            focused_label(&mounted).as_deref(),
            Some("Choose model Model 0")
        );
        // The real clip/hit chain must reject transcript painting over panel controls.
        assert_control_hit(&mounted, "Choose model Model 0");
        assert_control_hit(&mounted, "Choose model Model 23");
        mounted.key(Key::Escape, false);
        assert!(mounted.model.command_flow.get_untracked().is_empty());
        assert_eq!(focused_label(&mounted).as_deref(), Some(draft.as_str()));
        mounted.key(Key::Character("Follow up".into()), false);
        assert_eq!(
            plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
            "Follow up"
        );
        // Reopening and using the close cell follows the same focus return path.
        crate::command_ui::open(mounted.model, "model");
        mounted.settle();
        mounted.click("Close commands");
        assert!(
            mounted.model.command_flow.get_untracked().is_empty(),
            "close command click missed: {:?}",
            mounted.rect("Close commands")
        );
        assert_eq!(focused_label(&mounted).as_deref(), Some(draft.as_str()));
        crate::command_ui::open(mounted.model, "model");
        mounted.settle();
        mounted.key(Key::Enter, false);
        assert_eq!(mounted.model.command_model.get_untracked(), "model-0");
        assert_eq!(
            focused_label(&mounted).as_deref(),
            Some("Choose effort low")
        );
        mounted.key(Key::ArrowDown, false);
        mounted.key(Key::Enter, false);
        assert!(mounted.model.command_flow.get_untracked().is_empty());
        assert_eq!(focused_label(&mounted).as_deref(), Some(draft.as_str()));
        assert!(matches!(mounted.commands.try_recv().unwrap().command,
            Command::SetHarnessSelection { selection: HarnessSelection { model: Some(ref model), effort: Some(ref effort), .. }, .. }
                if model == "model-0" && effort == "high"));
        crate::command_ui::open(mounted.model, "status");
        mounted.settle();
        assert_eq!(focused_label(&mounted).as_deref(), Some("Close commands"));
        mounted.key(Key::Escape, false);
        assert!(mounted.model.command_flow.get_untracked().is_empty());
        assert_eq!(focused_label(&mounted).as_deref(), Some(draft.as_str()));
    }
}

#[test]
fn draft_reply_strip_removes_only_the_reply_and_keeps_shared_edges() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    let original = mounted.model.snapshot.get_untracked().messages;
    let source = original.iter().find(|m| m.id == "m1").unwrap();
    let other = Part::text("Unrelated draft");
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![
            Part {
                id: "reply-strip-test".into(),
                kind: PartKind::Reply {
                    anchor: relay_core::Anchor {
                        message_id: source.id.clone(),
                        start: 0,
                        end: source.body.len(),
                        quote: source.body.clone(),
                    },
                    parts: vec![Part::text("Reply draft")],
                },
            },
            other.clone(),
        ],
    );
    mounted.settle();
    let snapshot = mounted.ui.inspection_snapshot();
    // The final instance is the composer; the source transcript also mirrors it.
    let rect = |label| {
        snapshot
            .nodes
            .iter()
            .rev()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap()
            .rect
    };
    let quote = rect("Go to reply source");
    let remove = rect("Remove draft reply");
    assert!((quote.max_x() - remove.origin.x).abs() < 0.1);
    assert_eq!(quote.origin.y, remove.origin.y);
    assert_eq!(quote.size.height, remove.size.height);
    for kind in [
        PointerEventKind::Down(PointerButton::Primary),
        PointerEventKind::Up(PointerButton::Primary),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position: remove.center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
    }
    mounted.settle();
    assert_eq!(
        crate::buffer::parts(mounted.model, "session-plan"),
        vec![other]
    );
    assert_eq!(mounted.model.snapshot.get_untracked().messages, original);
}

#[test]
fn command_feedback_is_attached_and_clears_on_edit_escape_and_panel_open() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            Ok(HarnessCatalog {
                commands: vec![HarnessCommand {
                    name: "config".into(),
                    description: "Configure".into(),
                    argument_hint: String::new(),
                    dispatch: CommandDispatch::Unavailable,
                    reason: Some("Configure this feature on the server".into()),
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("/config".into()), false);
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.command_feedback.get_untracked(),
        "Configure this feature on the server"
    );
    assert!(mounted.model.notice.get_untracked().is_empty());
    let feedback = mounted.rect("Command feedback");
    let composer = mounted.rect("Message composer");
    assert!((feedback.max_y() - composer.origin.y).abs() < 0.1);
    assert_eq!(feedback.size.width, composer.size.width);
    mounted.key(Key::Character("x".into()), false);
    assert!(mounted.model.command_feedback.get_untracked().is_empty());
    mounted.model.command_feedback.set("Unknown command".into());
    mounted.settle();
    mounted.key(Key::Escape, false);
    assert!(mounted.model.command_feedback.get_untracked().is_empty());
    mounted.model.command_feedback.set("Unknown command".into());
    crate::command_ui::open(mounted.model, "model");
    assert!(mounted.model.command_feedback.get_untracked().is_empty());
    crate::command_ui::run(mounted.model, "review", "");
    mounted.settle();
    assert!(!mounted.model.command_feedback.get_untracked().is_empty());
    assert!(
        (mounted.rect("Command feedback").max_y() - mounted.rect("Message composer").origin.y)
            .abs()
            < 0.1
    );
}

#[test]
fn page_banner_escape_dismisses_settings_failure_without_changing_its_draft() {
    let mounted = mount(false, 1380.0);
    mounted.model.page.set(Page::Settings);
    mounted
        .model
        .notice
        .set("Settings could not be saved".into());
    let preferences = mounted.model.preferences.get_untracked();
    mounted.settle();
    assert!(mounted.rect("Dismiss").size.width > 0.0);
    mounted.key(Key::Escape, false);
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert_eq!(mounted.model.preferences.get_untracked(), preferences);
}

#[test]
fn request_sending_and_send_pending_clear_on_ack_and_conflicts_keep_actions() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(sender));
    let model = mounted.model;
    let mut send = |text: &str, serial| {
        model.worker_approval.set(true);
        crate::buffer::edit(model, "session-plan", vec![Part::text(text)]);
        crate::buffer::send(model);
        assert!(
            model.submission_pending(),
            "pending includes the draft-save acknowledgement stage"
        );
        let (session, request) = loop {
            match requests.try_recv().unwrap() {
                crate::buffer_network::Request::Save { session, request } => {
                    break (session, request);
                }
                crate::buffer_network::Request::Forget(_) => {}
                _ => panic!("Expected draft save"),
            }
        };
        let saved = Draft {
            session_id: session.clone(),
            revision: serial * 2 - 1,
            parts: request.parts.clone(),
        };
        crate::buffer::receive(
            model,
            crate::buffer_network::Update {
                serial,
                initialized: true,
                connected: true,
                drafts: vec![saved.clone()],
                outcomes: std::collections::BTreeMap::from([(
                    request.request_id.clone(),
                    crate::buffer_network::Outcome::Saved {
                        session,
                        request,
                        result: Ok(saved),
                    },
                )]),
                ..Default::default()
            },
        );
    };
    send("Queue this turn", 1);
    let request = mounted.commands.try_recv().unwrap();
    mounted.settle();
    assert!(mounted.model.sending() && mounted.model.submission_pending());
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert!(
        mounted
            .rect("Submission awaiting acknowledgement")
            .size
            .width
            > 0.0
    );
    assert!(
        mounted
            .rect("Connection: Connected, sending…. Show details")
            .size
            .height
            > 0.0
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((request.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    assert!(!mounted.model.sending() && !mounted.model.submission_pending());
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|label| label.starts_with("Connection:") && label.contains("sending"))
    }));
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Submission awaiting acknowledgement"))
    );
    // Another conversation must not show this draft's Send pending state.
    send("Another turn", 2);
    let request = mounted.commands.try_recv().unwrap();
    mounted.model.open_session("session-worker".into());
    assert!(mounted.model.sending() && !mounted.model.submission_pending());
    mounted.model.open_session("session-plan".into());
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            request.request_id,
            Err("Revision conflict; review latest".into()),
        )),
        outcome_serial: 2,
        outcome_conflict: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(!mounted.model.sending() && !mounted.model.submission_pending());
    for label in [
        "Review latest state",
        "Review conflict for new request",
        "Retry original request",
    ] {
        assert!(mounted.rect(label).size.width > 0.0);
    }
    mounted.key(Key::Escape, false);
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert!(mounted.model.can_retry());
    for label in ["Review conflict for new request", "Retry original request"] {
        assert!(mounted.rect(label).size.width > 0.0);
    }
    mounted.model.retry_pending();
    assert!(mounted.model.sending() && mounted.model.submission_pending());
}

#[test]
fn command_completion_escape_and_ime_keep_normal_editing() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            Ok(HarnessCatalog {
                skills: vec![HarnessSkill {
                    id: "x".into(),
                    name: "review".into(),
                    description: String::new(),
                    argument_hint: String::new(),
                    enabled: true,
                    path: None,
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("$".into()), false);
    assert!(mounted.model.completion.get_untracked().is_some());
    mounted.key(Key::Escape, false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted.key(Key::Enter, false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n"
    );
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "確定".into(),
        cursor: Some((0, 6)),
    });
    mounted.settle();
    mounted.key(Key::Enter, false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n"
    );
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n確定"
    );
    assert!(mounted.commands.is_empty());
}

#[test]
fn approval_control_scales_with_its_label_and_keeps_the_existing_approval() {
    let mounted = mount(false, 1380.0);
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.click("Open issue #2");
    let before = mounted.rect("Approve implementation for this turn");
    mounted.model.worker_approval.set(true);
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted.settle();
    let after = mounted.rect("Approve implementation for this turn");
    assert!((after.size.height / before.size.height - 1.5).abs() < 0.05);
    assert!(mounted.model.worker_approval.get_untracked());
    mounted.focus("Approve implementation for this turn");
    mounted.key(Key::Space, false);
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn display_settings_persist_validate_and_preserve_existing_file_on_failure() {
    use crate::settings::{Preferences, ThemeMode};
    let directory = std::env::temp_dir().join(format!("relay-settings-{}", uuid::Uuid::new_v4()));
    let path = directory.join("settings.toml");
    let mut preferences = Preferences::load(&path).unwrap();
    assert_eq!(preferences.mode, ThemeMode::Dark);
    assert_eq!(preferences.scale, 1.0);
    preferences.mode = ThemeMode::System;
    preferences.high_contrast = true;
    preferences.legacy.dark_neutral = true;
    preferences.scale = 1.5;
    preferences.sidebar_width = 300.0;
    preferences.save(&path).unwrap();
    assert_eq!(Preferences::load(&path).unwrap(), preferences);
    let before = std::fs::read(&path).unwrap();
    preferences.scale = f32::NAN;
    assert!(preferences.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, "scale = 8.0").unwrap();
    assert!(Preferences::load(&path).is_err());
    std::fs::write(&path, "broken syntax").unwrap();
    assert!(Preferences::load(&path).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn settings_work_disconnected_and_scale_layout_and_hit_targets_without_losing_drafts() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.receive(NetworkState::default());
    mounted.model.worker_prompt.set("Keep this draft".into());
    mounted.key(Key::Character(",".into()), true);
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
    let before = mounted.rect("Theme: Dark");
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted.settle();
    let after = mounted.rect("Theme: Dark");
    assert!((after.size.height / before.size.height - 1.5).abs() < 0.05);
    assert!((mounted.rect("Sidebar").size.width - 380.0 * 1.5).abs() < 1.0);
    let page = || mosaic::core::theme::color(relay_ui::tokens::ground.page);
    let dark = page();
    mounted.focus("Theme: Light");
    mounted.click("Theme: Light");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::Light
    );
    let light = page();
    assert_ne!(light, dark);
    mounted.focus("High contrast");
    mounted.click("High contrast");
    assert!(mounted.model.preferences.get_untracked().high_contrast);
    assert_ne!(page(), light);
    mounted.focus("Theme: Dark");
    mounted.click("Theme: Dark");
    assert_ne!(page(), dark);
    mounted.focus("Theme: System");
    mounted.click("Theme: System");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
    assert_eq!(mosaic::core::theme::scalar(relay_ui::tokens::ui_scale), 1.5);
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Keep this draft"
    );
    assert!(mounted.commands.is_empty());
}

#[test]
fn large_scale_settings_keep_controls_visible_in_a_narrow_window() {
    let mut mounted = mount(false, 820.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| {
        p.scale = 2.0;
        p.sidebar_width = 160.0;
    });
    mounted.size = Size::new(820.0, 600.0);
    mounted.key(Key::Character(",".into()), true);
    let nodes = mounted.ui.inspection_snapshot();
    let number = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("200%"))
        .unwrap();
    let details = mounted.ui.inspection_details(number.id).unwrap();
    assert_eq!(
        details
            .attributes
            .iter()
            .find(|a| a.name == "font-size")
            .unwrap()
            .value,
        "25.0"
    );
    for label in [
        "Theme: System",
        "Decrease interface scale",
        "Reset interface scale",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(rect.origin.x >= 0.0, "{label}: {rect:?}");
        assert!(
            rect.origin.x + rect.size.width <= 820.0,
            "{label}: {rect:?}"
        );
    }
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.0);
}

impl Mounted {
    fn rects(&self, label: &str) -> Vec<Rect> {
        self.ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|n| n.label.as_deref() == Some(label))
            .map(|n| n.rect)
            .collect()
    }
    fn click_nth(&self, label: &str, index: usize) {
        let position = self.rects(label)[index].center();
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            self.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        self.settle();
    }
    fn settle(&self) {
        flush();
        for _ in 0..12 {
            self.ui.tick(Duration::from_millis(16));
            flush();
            if self.ui.frame(self.size, 1.0).is_none() {
                break;
            }
        }
    }
    fn rect(&self, label: &str) -> Rect {
        self.ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing control: {label}"))
            .rect
    }
    fn click(&self, label: &str) {
        let position = self.rect(label).center();
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            self.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        self.settle();
    }
    fn focus(&self, label: &str) {
        let snapshot = self.ui.inspection_snapshot();
        let target = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing control: {label}"))
            .id;
        for _ in 0..snapshot.nodes.len() {
            self.key(Key::Tab, false);
            if self
                .ui
                .focused()
                .is_some_and(|element| element.id() == target)
            {
                self.settle();
                let rect = self.rect(label);
                assert!(
                    rect.origin.y >= 0.0
                        && rect.origin.y + rect.size.height <= self.size.height + 0.01,
                    "{label}: {rect:?}"
                );
                return;
            }
        }
        panic!("Control is not keyboard reachable: {label}");
    }
    fn key(&self, key: Key, command: bool) {
        self.ui.dispatch_key(KeyEvent {
            kind: KeyEventKind::Down { repeat: false },
            key,
            modifiers: Modifiers {
                ctrl: command && !cfg!(target_os = "macos"),
                meta: command && cfg!(target_os = "macos"),
                ..Default::default()
            },
        });
        self.settle();
    }
}

fn touch(mounted: &Mounted, kind: PointerEventKind, x: f32, y: f32) {
    mounted.ui.dispatch_pointer(PointerEvent {
        kind,
        position: Vector2::new(x, y),
        pointer_type: PointerType::Touch,
        modifiers: Modifiers::default(),
        timestamp: Duration::from_millis(100),
    });
    mounted.settle();
}

#[test]
fn phone_shell_reserves_safe_areas_without_scaling_or_squeezing_the_board() {
    for light in [false, true] {
        let mounted = mount(light, 390.0);
        mounted.ui.set_safe_area(mosaic::layout::ResolvedEdges {
            top: 59.0,
            bottom: 34.0,
            left: 0.0,
            right: 0.0,
        });
        mounted.settle();
        assert!(mounted.model.mobile_navigation.get_untracked());
        assert!(!mounted.model.sidebar_open.get_untracked());
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Sidebar"))
        );
        let content = mounted.rect("Main content");
        assert_eq!(content.origin, Vector2::new(0.0, 59.0));
        assert_eq!(content.size, Size::new(390.0, 807.0));
        assert_eq!(mounted.rect("Page header").origin.y, 59.0);
        mounted.model.browser_top_clearance.set(16.0);
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 75.0);
        mounted.model.browser_top_clearance.set(0.0);
        mounted.settle();
        let menu = mounted.rect("Open navigation");
        assert!(menu.size.width >= 44.0 && menu.size.height >= 44.0);
        mounted.click("Open navigation");
        let sidebar = mounted.rect("Sidebar");
        assert_eq!(sidebar.origin.y, 59.0);
        assert_eq!(sidebar.size.width, 342.0);
        assert_eq!(sidebar.origin.y + sidebar.size.height, 866.0);
        mounted.click("Close navigation");
        mounted.model.preferences.update(|p| p.scale = 2.0);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 59.0);
        mounted.model.browser_top_clearance.set(16.0);
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 75.0);
        mounted.ui.set_safe_area(mosaic::layout::ResolvedEdges {
            left: 24.0,
            right: 24.0,
            top: 0.0,
            bottom: 21.0,
        });
        mounted.settle();
        assert_eq!(mounted.rect("Main content").origin.x, 24.0);
        assert_eq!(mounted.rect("Main content").size.width, 342.0);
    }
}

#[test]
fn phone_navigation_tracks_edge_swipes_and_dismisses_without_activating_content() {
    let mounted = mount(false, 390.0);
    let down = PointerEventKind::Down(PointerButton::Primary);
    let up = PointerEventKind::Up(PointerButton::Primary);
    // A vertical edge scroll and a horizontal gesture in the board stay there.
    for (x, dx, dy) in [(10.0, 8.0, 80.0), (100.0, 180.0, 0.0)] {
        touch(&mounted, down, x, 150.0);
        touch(&mounted, PointerEventKind::Move, x + dx, 150.0 + dy);
        touch(&mounted, up, x + dx, 150.0 + dy);
        assert!(!mounted.model.sidebar_open.get_untracked());
        assert!(
            mounted.model.issue.get_untracked().is_none(),
            "gesture from {x}"
        );
    }
    touch(&mounted, down, 10.0, 150.0);
    touch(&mounted, PointerEventKind::Move, 90.0, 152.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    assert_eq!(mounted.rect("Sidebar").size.width, 342.0);
    assert!((mounted.rect("Sidebar").origin.x + 262.0).abs() < 1.0);
    touch(&mounted, PointerEventKind::Move, 230.0, 152.0);
    touch(&mounted, up, 230.0, 152.0);
    assert!(mounted.model.sidebar_open.get_untracked());
    assert!(mounted.commands.is_empty());
    assert!(mounted.model.issue.get_untracked().is_none());
    touch(&mounted, down, 200.0, 70.0);
    touch(&mounted, PointerEventKind::Move, 25.0, 70.0);
    touch(&mounted, up, 25.0, 70.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    // A cancelled drag leaves the initial state intact.
    touch(&mounted, down, 10.0, 150.0);
    touch(&mounted, PointerEventKind::Move, 200.0, 150.0);
    touch(&mounted, PointerEventKind::Cancel, 200.0, 150.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    mounted.key(Key::Escape, false);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    touch(&mounted, down, 370.0, 200.0);
    touch(&mounted, up, 370.0, 200.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    mounted.click("Open board for Relay · demo");
    assert!(!mounted.model.sidebar_open.get_untracked());
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
}

#[test]
fn resizing_between_phone_and_desktop_keeps_the_conversation_editor_mounted() {
    let mut mounted = mount(false, 820.0);
    mounted.click("Open issue #2");
    mounted.click("Profile design");
    let editor = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.role == Role::TextInput
                && n.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Draft text "))
        })
        .map(|n| n.id);
    mounted.size.width = 390.0;
    mounted.settle();
    assert!(mounted.model.mobile_navigation.get_untracked());
    mounted.click("Open navigation");
    mounted.click("Close navigation");
    mounted.size.width = 820.0;
    mounted.settle();
    assert!(!mounted.model.mobile_navigation.get_untracked());
    mounted.rect("Sidebar");
    let after = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.role == Role::TextInput
                && n.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Draft text "))
        })
        .map(|n| n.id);
    assert!(editor.is_some(), "Conversation editor must exist");
    assert_eq!(editor, after);
}

#[test]
fn board_issue_and_session_navigation_work_in_both_themes_and_widths() {
    for light in [true, false] {
        for width in [820.0, 1380.0] {
            let mounted = mount(light, width);
            let card = mounted.rect("Open issue #2");
            assert!(card.size.width > 100.0);
            mounted.click("Open issue #2");
            assert_eq!(
                mounted.model.issue.get_untracked().as_deref(),
                Some("issue-2")
            );
            mounted.click("Profile design");
            assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
            mounted.rect("Message m2");
            mounted.click("Session actions");
            mounted.click("Linked issue");
            assert_eq!(mounted.model.page.get_untracked(), Page::Board);
            assert_eq!(
                mounted.model.issue.get_untracked().as_deref(),
                Some("issue-2")
            );
        }
    }
}

#[test]
fn command_palette_shares_edges_scrolls_and_dismisses_without_clicking_through() {
    for (width, scale) in [(1380.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        mounted.focus("Open command palette");
        let previous = mounted.ui.focused().unwrap().id();
        mounted.key(Key::Character("k".into()), true);
        let panel = mounted.rect("Command palette panel");
        let search = mounted.rect("Command search");
        let results = mounted.rect("Command results");
        assert!((search.origin.x - panel.origin.x).abs() < 1.0);
        assert!((search.size.width - panel.size.width).abs() < 1.0);
        assert!(panel.origin.y >= 0.0 && panel.origin.y + panel.size.height <= mounted.size.height);
        assert!(
            results.size.height > 0.0
                && results.origin.y + results.size.height <= panel.origin.y + panel.size.height,
            "{results:?} {panel:?}"
        );
        for label in ["Open board", "Open sessions", "Publish Board"] {
            let action = mounted.rect(label);
            assert!((action.origin.x - panel.origin.x).abs() < 1.0);
            assert!((action.size.width - panel.size.width).abs() < 1.0);
        }
        mounted.click("Command search");
        assert!(mounted.model.palette.get_untracked());
        let page = mounted.model.page.get_untracked();
        mounted.click("Page header");
        assert!(!mounted.model.palette.get_untracked());
        assert_eq!(mounted.model.page.get_untracked(), page);
        assert_eq!(mounted.ui.focused().unwrap().id(), previous);
        mounted.key(Key::Character("k".into()), true);
        mounted.model.palette_query.set("not a command".into());
        mounted.settle();
        assert!(has_label(&mounted, "No matching commands"));
        mounted.key(Key::Enter, false);
        assert!(mounted.model.palette.get_untracked());
        mounted.key(Key::Escape, false);
        assert!(!mounted.model.palette.get_untracked());
    }
}

#[test]
fn command_palette_enter_runs_the_keyboard_selected_row() {
    let mounted = mount(false, 1380.0);
    mounted.key(Key::Character("k".into()), true);
    // The first command starts highlighted; Down moves to the second.
    mounted.key(Key::ArrowDown, false);
    mounted.key(Key::Enter, false);
    assert!(!mounted.model.palette.get_untracked());
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
}

#[test]
fn palette_and_search_open_with_keyboard_focus() {
    let mounted = mount(false, 1380.0);
    mounted.key(Key::Character("k".into()), true);
    assert!(mounted.model.palette.get_untracked());
    let focused = mounted.ui.focused().unwrap().id();
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.id == focused)
            .unwrap()
            .label
            .as_deref(),
        Some("Command search")
    );
    mounted.model.palette_query.set("sessions".into());
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    mounted.key(Key::Character("f".into()), true);
    assert!(mounted.model.searching.get_untracked());
    mounted.model.search.set("explicit overrides".into());
    mounted.settle();
    let labels: Vec<_> = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter_map(|n| n.label)
        .collect();
    assert!(labels.iter().any(|l| l == "Message m1"));
    // Find does not remove the surrounding conversation from the buffer.
    assert!(labels.iter().any(|l| l == "Message m3"));
    mounted.key(Key::Escape, false);
    assert!(!mounted.model.searching.get_untracked());
}

#[test]
fn typing_in_recorded_text_creates_an_inline_reply_and_enter_is_a_newline() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    mounted.click("Message m1");
    mounted.key(Key::Character("Feedback".into()), false);
    mounted.key(Key::Enter, false);
    mounted.key(Key::Character("Another line".into()), false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    let PartKind::Reply {
        anchor,
        parts: reply,
    } = &parts[0].kind
    else {
        panic!("No anchored reply")
    };
    assert_eq!(anchor.message_id, "m1");
    assert_eq!(plain_text(reply), "Feedback\nAnother line");
    assert_eq!(mounted.model.snapshot.get_untracked().comments.len(), 0);
    let focused = mounted.ui.focused().unwrap().id();
    assert!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.id == focused)
            .unwrap()
            .label
            .as_deref()
            .unwrap()
            .starts_with("Draft text")
    );
}

#[test]
fn long_profile_forms_scroll_without_squeezing_permission_controls() {
    let mounted = mount(false, 820.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let plan = mounted.rect("Plan permission");
    let delegate = mounted.rect("Delegate permission");
    assert!(plan.size.height >= 30.0);
    assert!(delegate.origin.y >= plan.origin.y + plan.size.height - 0.5);
    let save = mounted.rect("Save profile");
    assert!(save.origin.y + save.size.height <= mounted.size.height);
}

#[test]
fn comments_keep_drafts_after_failure_and_clear_only_on_acknowledgment() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let source = mounted
        .model
        .snapshot
        .get_untracked()
        .messages
        .iter()
        .find(|m| m.id == "m2")
        .unwrap()
        .clone();
    mounted.model.start_comment(&source);
    mounted.model.comment_quote.set("project defaults".into());
    mounted
        .model
        .comment_body
        .set("Preserve the override.".into());
    mounted.model.save_comment();
    let request = mounted.commands.try_recv().unwrap();
    let failure = NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((request.request_id.clone(), Err("Network failed".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    };
    mounted.model.receive(failure.clone());
    assert_eq!(
        mounted.model.comment_body.get_untracked(),
        "Preserve the override."
    );
    mounted.model.save_comment();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(request.request_id, retry.request_id);
    // A held previous outcome must not complete a new retry.
    mounted.model.receive(failure);
    assert!(mounted.model.busy.get_untracked());
    let mut saved = mounted.model.snapshot.get_untracked();
    saved.apply(retry.command, &retry.request_id, 123).unwrap();
    mounted.model.receive(NetworkState {
        snapshot: saved,
        connected: true,
        outcome: Some((retry.request_id, Ok(()))),
        outcome_serial: 2,
        ..Default::default()
    });
    assert!(mounted.model.comment_body.get_untracked().is_empty());
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert_eq!(mounted.model.snapshot.get_untracked().comments.len(), 1);
}

#[test]
fn profile_controls_preserve_inheritance_and_reject_invalid_imports() {
    let mut mounted = mount(true, 1380.0);
    mounted.model.open_profile(EditTarget::New);
    mounted.settle();
    mounted.focus("Increase worker limit");
    mounted.click("Increase worker limit");
    assert_eq!(mounted.model.editor_profile.get_untracked().max_workers, 5);
    assert_eq!(mounted.model.origin("max_workers"), "Agent override");
    mounted.focus("Inherit max_workers");
    mounted.click("Inherit max_workers");
    assert_eq!(mounted.model.editor_profile.get_untracked().max_workers, 4);
    mounted.model.toml.set("unknown_setting = true".into());
    mounted.model.import_toml();
    assert!(mounted.model.notice.get_untracked().contains("unknown"));
    mounted.model.editor_name.set("Security director".into());
    choose_fixture_profile_model(&mounted);
    mounted.model.save_profile();
    let request = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(request.command, &request.request_id, 123)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id.clone(), Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.editor.get_untracked(),
        EditTarget::Config(format!("agent-config-{}", request.request_id))
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().agent_configs.len(),
        3
    );
}

fn live_snapshot() -> Snapshot {
    let mut defaults = AgentProfile {
        model: "fixture-model".into(),
        ..Default::default()
    };
    defaults
        .permissions
        .insert(Task::Implement, Permission::Ask);
    let mut snapshot = demo_snapshot(defaults);
    snapshot.projects[0].fixture = false;
    snapshot.projects[0].github = Some(GitHubProject {
        owner: "team".into(),
        number: 7,
        url: "https://github.com/orgs/team/projects/7".into(),
        last_synced_at: None,
        sync_error: None,
    });
    snapshot.agent_configs[0].overrides = ProfileOverrides::default();
    snapshot
}

#[test]
fn live_worker_approval_failure_retry_and_ack_open_exact_session() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Implement the issue".into());
    mounted.settle();
    let policy = mounted.rect("Worker policy");
    for field in ["Harness", "Implement", "Scope", "Workers"] {
        let cell = mounted.rect(&format!("Worker policy {field}"));
        assert!((cell.origin.y - policy.origin.y).abs() < 1.0);
        assert!((cell.size.height - policy.size.height).abs() < 1.0);
    }
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Approve")
    );
    mounted.focus("Approve implementation for this turn");
    mounted.click("Approve implementation for this turn");
    mounted.model.run_worker(false);
    let request = mounted.commands.try_recv().unwrap();
    assert!(matches!(
        request.command,
        Command::StartSession {
            approve_implementation: true,
            ..
        }
    ));
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            request.request_id.clone(),
            Err("Revision conflict; review latest".into()),
        )),
        outcome_serial: 1,
        outcome_conflict: true,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Implement the issue"
    );
    mounted.model.review_latest();
    mounted.model.retry_pending();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(retry.request_id, request.request_id);
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((retry.request_id.clone(), Ok(()))),
        outcome_serial: 2,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.session.get_untracked(),
        format!("session-{}", retry.request_id)
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert!(mounted.model.worker_prompt.get_untracked().is_empty());
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn delayed_start_acknowledgement_opens_the_sessions_own_project() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let mut other = snapshot.projects[0].clone();
    other.id = "other-project".into();
    other.repository = "team/other".into();
    snapshot.projects.push(other);
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Implement this issue".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let request = mounted.commands.try_recv().unwrap();
    mounted.model.select_project("other-project".into());
    let mut session = snapshot.sessions[0].clone();
    session.id = format!("session-{}", request.request_id);
    snapshot.sessions.push(session.clone());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), session.project_id);
    assert_eq!(mounted.model.session.get_untracked(), session.id);
    assert_eq!(
        mounted.model.worker_director.get_untracked(),
        session.agent_config_id
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn deny_scope_harness_and_capacity_cannot_be_overridden() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Implement".into());
    mounted.model.worker_approval.set(true);
    mounted.model.snapshot.update(|s| {
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Deny);
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("denied")
    );
    mounted.model.snapshot.update(|s| {
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Allow);
        s.projects[0].defaults.scope = AgentScope::Issues {
            issue_ids: vec!["other".into()],
        };
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("scope")
    );
    mounted.model.snapshot.update(|s| {
        s.projects[0].defaults.scope = AgentScope::Project;
        s.projects[0].defaults.harness = Harness::ClaudeCode;
    });
    assert!(mounted.model.worker_gate(false).is_ok());
    mounted.model.snapshot.update(|s| {
        s.projects[0].defaults.harness = Harness::Codex;
        s.projects[0].defaults.max_workers = 0;
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("slots")
    );
}

#[test]
fn worker_running_stop_completed_review_and_continue_use_recorded_session() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread-live".into()),
        worktree: Some("/repo/worktrees/live".into()),
        branch: Some("worker/live".into()),
        base_commit: Some("abc123".into()),
        error: None,
        usage: Some(TokenUsage {
            input_tokens: 42,
            cached_input_tokens: 12,
            output_tokens: 7,
        }),
        changes: Some(ChangeSet {
            files: vec!["file.rs".into()],
            diff: "+change".into(),
            truncated: true,
        }),
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    mounted.focus("Stop");
    mounted.click("Stop");
    let stop = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(stop.command, Command::StopWorker { session_id: id, include_descendants: false } if id == session_id)
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((stop.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    mounted.click("Session actions");
    mounted.focus("Toggle change review");
    mounted.click("Toggle change review");
    mounted.rect("Session details");
    mounted.model.worker_prompt.set("Review result".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(true);
    let send = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&send.command, Command::SendWorker { session_id: id, .. } if *id == session_id)
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            send.request_id.clone(),
            Err("Continuation response lost".into()),
        )),
        outcome_serial: 2,
        outcome_ambiguous: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.worker_prompt.get_untracked(), "Review result");
    mounted.model.worker_prompt.set("Changed prompt".into());
    mounted.model.retry_pending();
    let exact = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&send).unwrap()
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((exact.request_id, Ok(()))),
        outcome_serial: 3,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Changed prompt"
    );
    mounted.model.worker_approval.set(true);
    for status in [
        WorkerStatus::Failed,
        WorkerStatus::Stopped,
        WorkerStatus::Interrupted,
    ] {
        mounted
            .model
            .snapshot
            .update(|s| s.sessions[0].worker.as_mut().unwrap().status = status);
        mounted.settle();
        assert!(mounted.model.worker_gate(true).is_ok());
        mounted.rect("Next message");
    }
}

#[test]
fn live_initial_selection_and_dynamic_columns_metadata_remain_reactive() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let mut fixture = snapshot.projects[0].clone();
    fixture.id = "fixture-project".into();
    fixture.fixture = true;
    fixture.github = None;
    snapshot.projects[0].id = "remote-project".into();
    for issue in &mut snapshot.issues {
        issue.project_id = "remote-project".into();
    }
    for director in &mut snapshot.agent_configs {
        director.project_id = "remote-project".into();
    }
    let mut historical = snapshot.projects[0].clone();
    historical.id = "historical-remote".into();
    historical.github.as_mut().unwrap().number = 6;
    snapshot.projects.push(historical);
    snapshot.projects.insert(0, fixture);
    mounted.model.project.set(String::new());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), "remote-project");
    mounted.settle();
    mounted.model.snapshot.update(|s| {
        s.projects[1].columns.reverse();
        s.projects[1].columns[0].title = "Changed status".into();
        s.projects[1].columns.push(BoardColumn {
            id: "new-column".into(),
            title: "New status".into(),
        });
        s.issues[1].title = "Updated issue".into();
        s.issues[1].body = "Updated body".into();
        s.projects[1].github.as_mut().unwrap().sync_error = Some("Sync failed".into());
    });
    mounted.settle();
    mounted.rect("Sync project");
    mounted.rect("Column new-column");
    let snapshot = mounted.model.snapshot.get_untracked();
    let columns = &snapshot.projects[1].columns;
    let first = mounted.rect(&format!("Column {}", columns[0].id));
    let second = mounted.rect(&format!("Column {}", columns[1].id));
    assert!(first.origin.x < second.origin.x);
    mounted.rect("Updated issue");
    mounted.click("Open issue #2");
    mounted.rect("Updated body");
    mounted.rect("Start worker");
}

#[test]
fn removed_board_items_keep_history_but_block_new_turns_until_restored() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let issue = snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == "issue-2")
        .unwrap();
    let previous_column = issue.column_id.clone();
    issue.column_id = "github-removed-from-board".into();
    let agent_config_id = snapshot.agent_configs[0].id.clone();
    let session = &mut snapshot.sessions[0];
    session.issue_id = Some("issue-2".into());
    session.agent_config_id = agent_config_id;
    session.fixture = false;
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Completed,
        thread_id: Some("recorded-thread".into()),
        worktree: Some("/repo/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted
        .model
        .worker_prompt
        .set("Continue implementation".into());
    mounted.model.worker_approval.set(true);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    mounted.rect("Issue removed from board");
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("no longer")
    );
    mounted.model.open_session(session_id);
    mounted.model.worker_approval.set(true);
    mounted.settle();
    mounted.rect("Session issue removed from board");
    assert!(
        mounted
            .model
            .worker_gate(true)
            .unwrap_err()
            .contains("no longer")
    );
    // Board membership changes do not stop an already active turn.
    mounted
        .model
        .snapshot
        .update(|s| s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Running);
    mounted.settle();
    mounted.focus("Stop");
    mounted.click("Stop");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StopWorker { .. }
    ));
    mounted.model.busy.set(false);
    mounted.model.snapshot.update(|s| {
        s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
        s.issues
            .iter_mut()
            .find(|i| i.id == "issue-2")
            .unwrap()
            .column_id = previous_column;
    });
    assert!(mounted.model.worker_gate(true).is_ok());
    mounted.model.issue.set(None);
    mounted.model.page.set(Page::Board);
    mounted.settle();
    mounted.rect("Open issue #2");
    mounted.model.snapshot.update(|s| {
        s.issues
            .iter_mut()
            .find(|i| i.id == "issue-2")
            .unwrap()
            .column_id = "github-removed-from-board".into()
    });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Open issue #2"))
    );
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Column github-removed-from-board"))
    );
    assert!(
        mounted
            .model
            .snapshot
            .get_untracked()
            .issues
            .iter()
            .any(|i| i.id == "issue-2")
    );
}

#[test]
fn default_director_requires_a_model_then_can_delegate_automatically() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults = AgentProfile::default();
    assert_eq!(snapshot.projects[0].defaults, AgentProfile::default());
    assert!(
        !snapshot.projects[0]
            .defaults
            .responsibilities
            .contains(&Task::Implement)
    );
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Delegate this issue".into());
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Choose a model")
    );
    mounted.model.snapshot.update(|snapshot| {
        snapshot.projects[0].defaults.model = "fixture-model".into();
    });
    assert!(mounted.model.worker_gate(false).is_ok());
    mounted.model.run_worker(false);
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StartSession {
            approve_implementation: false,
            ..
        }
    ));
}

#[test]
fn ambiguous_retry_preserves_entire_envelope_after_review_and_revision_changes() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Original prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 9;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    assert!(!mounted.model.can_rebase());
    mounted.model.review_latest();
    mounted.model.rebase_conflict();
    mounted.model.retry_pending();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&retry).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Original prompt"
    );
}

#[test]
fn known_conflict_requires_explicit_new_request_rebase_and_retains_draft() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Reviewed prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 1;
    let revision = snapshot.revision;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Revision conflict".into()))),
        outcome_serial: 1,
        outcome_conflict: true,
        ..Default::default()
    });
    mounted.model.review_latest();
    mounted.model.retry_pending();
    let exact = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((exact.request_id, Err("Revision conflict".into()))),
        outcome_serial: 2,
        outcome_conflict: true,
        ..Default::default()
    });
    mounted.settle();
    mounted.focus("Review conflict for new request");

    mounted.click("Review conflict for new request");
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Reviewed prompt"
    );
    assert!(mounted.commands.try_recv().is_err());
    mounted.model.run_worker(false);
    let rebased = mounted.commands.try_recv().unwrap();
    assert_ne!(rebased.request_id, original.request_id);
    assert_eq!(rebased.expected_revision, revision);
    assert_eq!(
        serde_json::to_value(&rebased.command).unwrap(),
        serde_json::to_value(&original.command).unwrap()
    );
}

#[test]
fn first_message_explains_an_unresolved_request_and_retains_its_draft() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Original prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((original.request_id, Err("Revision conflict".into()))),
        outcome_serial: 1,
        outcome_conflict: true,
        ..Default::default()
    });
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut config = snapshot.agent_configs[0].clone();
    config.id = "unrelated-draft".into();
    snapshot.agent_configs.push(config);
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_root_agent("unrelated-draft".into());
    let draft = mounted.model.session.get_untracked();
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
    crate::buffer::edit(mounted.model, &draft, vec![Part::text("Keep this draft")]);
    mounted.model.notice.set(String::new());
    crate::buffer::send(mounted.model);
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Resolve the previous request before sending another message"
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &draft)),
        "Keep this draft"
    );
    assert!(mounted.model.can_retry());
    assert!(mounted.commands.try_recv().is_err());
    mounted.settle();
    mounted.key(Key::Escape, false);
    assert!(mounted.rect("Review conflict for new request").size.width > 0.0);
}

#[test]
fn ambiguous_start_is_reconciled_from_snapshot_without_duplicate_launch() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Original prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    mounted.model.worker_prompt.set("Next draft".into());
    mounted.model.run_worker(false);
    assert!(mounted.commands.try_recv().is_err());
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut session = snapshot.sessions[0].clone();
    session.id = format!("session-{}", original.request_id);
    session.issue_id = Some("issue-2".into());
    snapshot.sessions.push(session);
    snapshot.revision += 1;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.session.get_untracked(),
        format!("session-{}", original.request_id)
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert_eq!(mounted.model.worker_prompt.get_untracked(), "Next draft");
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert!(!mounted.model.can_retry());
    mounted.model.retry_pending();
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn snapshot_prompt_and_comment_ids_confirm_ambiguous_writes() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.comment_target.set("m1".into());
    mounted.model.comment_body.set("Original comment".into());
    mounted.model.save_comment();
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    mounted.model.comment_body.set("Edited comment".into());
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(original.command, &original.request_id, 1)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.comment_body.get_untracked(), "Edited comment");
    assert!(!mounted.model.can_retry());
    let prompt = "Continue exact session".to_string();
    mounted.model.worker_prompt.set(prompt.clone());
    mounted.model.submit(
        Command::SendWorker {
            session_id: "session-plan".into(),
            prompt: prompt.clone(),
            approve_implementation: true,
        },
        mounted.model.snapshot.get_untracked().revision,
        crate::model::Saved::Send(prompt),
    );
    let send = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((send.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 2,
        outcome_ambiguous: true,
        ..Default::default()
    });
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.push(Message {
        id: format!("prompt-{}", send.request_id),
        session_id: "session-plan".into(),
        author: "User".into(),
        sender: Sender::User,
        kind: "prompt".into(),
        body: "Continue exact session".into(),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert!(!mounted.model.can_retry());
    assert!(mounted.model.worker_prompt.get_untracked().is_empty());
}

#[test]
fn failed_launch_without_thread_offers_new_linked_worker_instead_of_continue() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Failed,
        thread_id: None,
        worktree: Some("/repo/failed-launch".into()),
        branch: Some("failed-launch".into()),
        base_commit: None,
        error: Some("Codex could not start. Check server CLI authentication.".into()),
        usage: None,
        changes: None,
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted
        .model
        .worker_prompt
        .set("Retained recovery draft".into());
    mounted.settle();
    mounted.rect("Worker cannot continue");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Send worker prompt"))
    );
    mounted.focus("Start new worker from linked issue");
    mounted.click("Start new worker from linked issue");
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some("issue-2")
    );
    assert_eq!(
        mounted.model.worker_director.get_untracked(),
        mounted.model.snapshot.get_untracked().agent_configs[0].id
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Retained recovery draft"
    );
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Approve")
    );
    mounted.rect("Start worker");
}

#[test]
fn inline_reply_and_next_message_share_edits_images_and_undo_without_mutating_history() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let original = mounted.model.snapshot.get_untracked().messages.clone();
    mounted.click("Message m1");
    mounted.key(Key::Character("First reply λ".into()), false);
    mounted.click("Message m2");
    mounted.key(Key::Character("Second reply".into()), false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(
        parts
            .iter()
            .filter(|p| matches!(p.kind, PartKind::Reply { .. }))
            .count(),
        2
    );
    let PartKind::Reply { parts: nested, .. } = &parts[0].kind else {
        panic!()
    };
    let id = nested[0].id.clone();
    let label = format!("Draft text {id}");
    let targets = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|n| n.label.as_deref() == Some(&label))
        .map(|n| n.id)
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 2);
    for _ in 0..200 {
        mounted.key(Key::Tab, false);
        if mounted.ui.focused().is_some_and(|e| e.id() == targets[1]) {
            break;
        }
    }
    assert_eq!(mounted.ui.focused().unwrap().id(), targets[1]);
    mounted.key(Key::Character(" edited below".into()), false);
    assert!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan"))
            .contains("First reply λ edited below")
    );
    mounted.key(Key::Character("z".into()), true);
    assert!(
        !plain_text(&crate::buffer::parts(mounted.model, "session-plan")).contains("edited below")
    );
    let mut parts = crate::buffer::parts(mounted.model, "session-plan");
    let PartKind::Reply { parts: nested, .. } = &mut parts[0].kind else {
        panic!()
    };
    nested.push(Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Asset {
            asset: Asset {
                id: uuid::Uuid::new_v4().to_string(),
                name: "inline.txt".into(),
                media_type: "text/plain".into(),
                size: 1,
            },
        },
    });
    crate::buffer::edit(mounted.model, "session-plan", parts);
    mounted.settle();
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|n| n.label.as_deref() == Some("Preview inline.txt"))
            .count(),
        2
    );
    mounted.click("Remove inline file");
    assert!(assets(&crate::buffer::parts(mounted.model, "session-plan")).is_empty());
    assert_eq!(mounted.model.snapshot.get_untracked().messages, original);
}

fn buffer_worker(mounted: &Mounted) {
    let mut snapshot = live_snapshot();
    let s = &mut snapshot.sessions[0];
    s.fixture = false;
    s.role = SessionRole::Worker;
    s.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread".into()),
        worktree: Some("/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let id = s.id.clone();
    snapshot.messages.push(Message {
        id: "prompt-active-run".into(),
        session_id: id.clone(),
        author: "You".into(),
        sender: Sender::User,
        kind: "prompt".into(),
        body: "Original".into(),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.model.buffer.update(|b| {
        b.initialized = true;
        b.connected = true;
    });
    mounted.model.worker_approval.set(true);
}

#[test]
fn retry_restores_recovered_content_when_the_current_draft_is_empty() {
    use crate::buffer;
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    buffer::edit(mounted.model, "session-plan", vec![]);
    mounted.model.buffer.update(|state| {
        state.documents.get_mut("session-plan").unwrap().recovery =
            vec![vec![Part::text("Recovered")]];
    });
    mounted.settle();
    mounted.click("Retry draft save");
    assert_eq!(
        plain_text(&buffer::parts(mounted.model, "session-plan")),
        "Recovered"
    );
    assert!(mounted.model.buffer.get_untracked().documents["session-plan"].recovery_reviewed);
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Local draft restored. Ready to send."
    );
}

#[test]
fn resolved_recovery_stays_resolved_after_reloading_the_local_journal() {
    use crate::buffer;
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let directory =
        std::env::temp_dir().join(format!("relay-recovery-choice-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let settings = directory.join("settings.toml");
    let config = crate::network::Config {
        endpoint: "http://127.0.0.1:7331/".parse().unwrap(),
        token: "recovery-choice-test".into(),
    };
    buffer::load_journal(mounted.model, &settings, &config);
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Current")]);
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut("session-plan").unwrap();
        doc.remote.parts = vec![Part::text("Shared")];
        doc.recovery = vec![vec![Part::text("Recovered")]];
    });
    buffer::resolve(mounted.model, false);
    mounted.model.buffer.set(Default::default());
    buffer::load_journal(mounted.model, &settings, &config);
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents["session-plan"];
    assert_eq!(plain_text(&doc.parts), "Shared");
    assert!(doc.recovery_reviewed);
    assert!(
        doc.recovery
            .iter()
            .any(|parts| plain_text(parts) == "Current")
    );
    assert!(
        doc.recovery
            .iter()
            .any(|parts| plain_text(parts) == "Recovered")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovery_buttons_apply_choices_hide_the_panel_and_keep_alternative_drafts() {
    use crate::buffer;
    for (button, expected, notice) in [
        ("Load shared draft", "Shared", "Shared draft loaded."),
        (
            "Restore local draft",
            "Recovered",
            "Local draft restored. Ready to send.",
        ),
        ("Retry draft save", "Current", "Draft ready to send."),
    ] {
        let mounted = mount(false, 1380.0);
        buffer_worker(&mounted);
        buffer::edit(mounted.model, "session-plan", vec![Part::text("Current")]);
        mounted.model.buffer.update(|state| {
            let doc = state.documents.get_mut("session-plan").unwrap();
            doc.remote.parts = vec![Part::text("Shared")];
            doc.recovery = vec![vec![Part::text("Recovered")]];
            doc.error = "Message already launching or missing".into();
            doc.editing_queue = Some("completed-message".into());
        });
        mounted.settle();
        mounted.click(button);
        let state = mounted.model.buffer.get_untracked();
        let doc = &state.documents["session-plan"];
        assert_eq!(plain_text(&doc.parts), expected);
        assert!(doc.error.is_empty());
        assert!(doc.recovery_reviewed);
        assert!(doc.editing_queue.is_none());
        assert!(!doc.recovery.is_empty());
        assert_eq!(mounted.model.notice.get_untracked(), notice);
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(button))
        );
    }
}

#[test]
fn recovered_queue_edit_sends_a_new_turn_without_first_repeating_the_rejection() {
    use crate::buffer;
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Recovered")]);
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut("session-plan").unwrap();
        doc.remote.parts = doc.parts.clone();
        doc.editing_queue = Some("completed-message".into());
    });
    buffer::send(mounted.model);
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(request.command, Command::SubmitTurn { parts, .. } if plain_text(&parts) == "Recovered")
    );
}

#[test]
fn rejected_queue_edit_retains_draft_and_releases_pending_request() {
    use crate::{buffer, model::Saved};
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    let draft = Draft {
        session_id: "session-plan".into(),
        revision: 1,
        parts: vec![Part::text("Correction")],
    };
    buffer::edit(mounted.model, &draft.session_id, draft.parts.clone());
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut(&draft.session_id).unwrap();
        doc.remote = draft.clone();
        doc.parts.clear();
        doc.editing_queue = Some("already-launched".into());
        doc.submitting = true;
    });
    mounted.model.submit(
        Command::EditQueuedTurn {
            submission_id: "already-launched".into(),
            draft_revision: draft.revision,
            parts: draft.parts.clone(),
            approve_implementation: true,
        },
        mounted.model.snapshot.get_untracked().revision,
        Saved::Buffer(draft.clone()),
    );
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            original.request_id.clone(),
            Err("Message already launching or missing".into()),
        )),
        outcome_serial: 1,
        ..Default::default()
    });
    assert!(!mounted.model.can_retry());
    assert!(!mounted.model.busy.get_untracked());
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&draft.session_id];
    assert_eq!(doc.parts, draft.parts);
    assert!(doc.editing_queue.is_none());
    assert!(!doc.submitting);
    buffer::retry_save(mounted.model);
    buffer::send(mounted.model);
    buffer::flush(mounted.model);
    let fresh = mounted.commands.try_recv().unwrap();
    assert_ne!(fresh.request_id, original.request_id);
    assert!(matches!(fresh.command, Command::SubmitTurn { parts, .. } if parts == draft.parts));
}

#[test]
fn rapid_second_send_waits_for_ack_then_promotes_exact_queue_and_keeps_newer_draft() {
    use crate::{
        buffer,
        buffer_network::{Outcome, Request, Update},
    };
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("\n\n Correction \n\n")],
    );
    buffer::send(mounted.model);
    buffer::send(mounted.model);
    let Request::Save { session, request } = rx.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(plain_text(&request.parts), "Correction");
    let saved = Draft {
        session_id: session.clone(),
        revision: 1,
        parts: request.parts.clone(),
    };
    mounted.model.syncing.set(true);
    buffer::receive(
        mounted.model,
        Update {
            serial: 1,
            initialized: true,
            connected: true,
            drafts: vec![saved.clone()],
            outcomes: std::collections::BTreeMap::from([(
                request.request_id.clone(),
                Outcome::Saved {
                    session,
                    request,
                    result: Ok(saved),
                },
            )]),
            ..Default::default()
        },
    );
    assert!(mounted.commands.try_recv().is_err());
    let document = mounted.model.buffer.get_untracked().documents["session-plan"].clone();
    assert!(document.finalize.is_some());
    assert!(!document.submitting);
    mounted.model.syncing.set(false);
    buffer::flush(mounted.model);
    let submitted = mounted.commands.try_recv().unwrap();
    let Command::SubmitTurn { parts, .. } = &submitted.command else {
        panic!()
    };
    assert_eq!(plain_text(parts), "Correction");
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Newer next message")],
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        acknowledged_revision: snapshot.revision + 1,
        outcome_serial: 1,
        outcome: Some((submitted.request_id.clone(), Ok(()))),
        ..Default::default()
    });
    buffer::flush(mounted.model);
    assert!(mounted.commands.try_recv().is_err());
    assert_eq!(
        mounted
            .model
            .buffer
            .get_untracked()
            .promote_after_ack
            .get("session-plan"),
        Some(&submitted.request_id)
    );
    snapshot.messages.retain(|message| message.kind != "prompt");
    snapshot.revision += 1;
    snapshot.submissions.push(Submission {
        id: submitted.request_id.clone(),
        session_id: "session-plan".into(),
        parts: parts.clone(),
        state: SubmissionState::Queued,
        approve_implementation: true,
        error: None,
        interrupts_run: None,
        last_edit_request: None,
        sender: Sender::User,
    });
    mounted.model.receive(NetworkState {
        acknowledged_revision: snapshot.revision,
        snapshot,
        connected: true,
        history: HistoryState {
            active_run_ids: std::collections::BTreeMap::from([(
                "session-plan".into(),
                "active-run".into(),
            )]),
            ..Default::default()
        },
        ..Default::default()
    });
    buffer::flush(mounted.model);
    let promoted = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(promoted.command,Command::PromoteTurn{submission_id,active_run_id:Some(run)} if submission_id==submitted.request_id && run=="active-run")
    );
    assert_eq!(
        plain_text(&buffer::parts(mounted.model, "session-plan")),
        "Newer next message"
    );
}

#[test]
fn shared_draft_conflicts_retain_local_content_and_streamed_receipts_confirm_ambiguous_saves() {
    use crate::{
        buffer,
        buffer_network::{Request, Update},
    };
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Local")]);
    mounted.model.buffer.update(|s| {
        s.documents.get_mut("session-plan").unwrap().changed =
            std::time::Instant::now() - Duration::from_secs(1)
    });
    buffer::flush(mounted.model);
    let Request::Save { session, request } = rx.try_recv().unwrap() else {
        panic!()
    };
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Newer local")],
    );
    buffer::receive(
        mounted.model,
        Update {
            serial: 1,
            initialized: true,
            connected: true,
            drafts: vec![Draft {
                session_id: session.clone(),
                revision: 1,
                parts: request.parts,
            }],
            ..Default::default()
        },
    );
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert!(doc.saving.is_none());
    assert!(!doc.conflict);
    assert_eq!(plain_text(&doc.parts), "Newer local");
    buffer::receive(
        mounted.model,
        Update {
            serial: 2,
            initialized: true,
            connected: true,
            drafts: vec![Draft {
                session_id: session.clone(),
                revision: 2,
                parts: vec![Part::text("Other client")],
            }],
            ..Default::default()
        },
    );
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert!(doc.conflict);
    assert_eq!(plain_text(&doc.parts), "Newer local");
    buffer::resolve(mounted.model, false);
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert_eq!(plain_text(&doc.parts), "Other client");
    assert_eq!(plain_text(&doc.recovery[0]), "Newer local");
}

#[test]
fn draft_ime_commits_unicode_and_local_recovery_preserves_pending_file_bytes_and_ids() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "仮".into(),
        cursor: Some((0, 3)),
    });
    mounted.settle();
    assert!(crate::buffer::parts(mounted.model, "session-plan").is_empty());
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定 λ".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "確定 λ"
    );
    let directory =
        std::env::temp_dir().join(format!("relay-draft-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let config = crate::network::Config {
        endpoint: "http://127.0.0.1:7331/".parse().unwrap(),
        token: "recovery-test-token".into(),
    };
    let settings = directory.join("settings.toml");
    crate::buffer::load_journal(mounted.model, &settings, &config);
    let part = crate::buffer::insert_asset(
        mounted.model,
        "notes.txt".into(),
        "text/plain".into(),
        b"Recovered context".to_vec(),
    )
    .unwrap();
    let PartKind::Asset { asset } = &part.kind else {
        panic!()
    };
    let asset = asset.clone();
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Before"), part, Part::text("After")],
    );
    mounted.model.buffer.set(Default::default());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    crate::buffer::load_journal(mounted.model, &settings, &config);
    let crate::buffer_network::Request::Upload {
        asset: restored,
        bytes,
    } = rx.try_recv().unwrap()
    else {
        panic!()
    };
    assert_eq!(restored, asset);
    assert_eq!(bytes.as_slice(), b"Recovered context");
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Before[notes.txt]After"
    );
    assert!(mounted.commands.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn long_buffer_lines_wrap_within_the_viewport_and_leave_a_reachable_next_message() {
    let mounted = mount(false, 820.0);
    mounted
        .model
        .snapshot
        .update(|s| s.messages[0].body = "A long recorded passage with spaces. ".repeat(40));
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let rect = mounted.rect("Message m1");
    assert!(
        rect.origin.x + rect.size.width <= mounted.size.width,
        "{rect:?}"
    );
    assert!(rect.size.height > 200.0, "{rect:?}");
    let next = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&next);
}

#[test]
fn browser_keyboard_edits_preserve_the_draft_when_the_visible_viewport_shrinks() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 teh", 7, 7, false);
    mounted.settle();
    mounted.size.height = 460.0;
    mounted.settle();
    assert_eq!(mounted.ui.focused().unwrap().id(), field);
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the", 7, 7, false);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the"
    );
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你", 8, 8, true);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the"
    );
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你好", 9, 9, false);
    mounted.settle();
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你好", 9, 9, false);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the你好"
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan").len(), 1);
    assert_eq!(mounted.ui.focused().unwrap().id(), field);
    assert!(
        mounted.commands.try_recv().is_err(),
        "Typing must not submit an execution"
    );
}

#[test]
fn chat_reveals_the_caret_when_the_keyboard_resizes_without_a_new_edit() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    let text = (0..45)
        .map(|n| format!("Line {n}: a longer chat draft 🙂\n"))
        .collect::<String>();
    let end = text.encode_utf16().count();
    crate::browser_text::update(&mounted.ui, field.raw(), &text, end, end, false);
    mounted.settle();
    for height in [600.0, 460.0, 390.0, 844.0] {
        mounted.size.height = height;
        mounted.settle();
        let tree = mounted.ui.inspection_snapshot();
        let mut parent = tree.node(field).unwrap().parent;
        let viewport = loop {
            let node = tree.node(parent.unwrap()).unwrap();
            if node.role == Role::ScrollView {
                break node.rect;
            }
            parent = node.parent;
        };
        let caret = mounted.ui.ime_cursor_area().unwrap();
        assert!(
            caret.origin.y >= viewport.origin.y - 0.5,
            "caret above chat at height {height}: {caret:?} / {viewport:?}"
        );
        assert!(
            caret.origin.y + caret.size.height <= viewport.origin.y + viewport.size.height + 0.5,
            "caret below chat at height {height}: {caret:?} / {viewport:?}"
        );
        assert_eq!(mounted.ui.focused().unwrap().id(), field);
        assert_eq!(
            mounted.ui.text_input_contents().unwrap().selection,
            (text.len(), text.len())
        );
    }
    let before_scroll = mounted.ui.ime_cursor_area().unwrap();
    // Scroll the draft itself, inside the composer.
    let draft = mounted.ui.inspection_snapshot().node(field).unwrap().rect;
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 180.0),
        },
        position: Vector2::new(draft.center().x, before_scroll.origin.y - 20.0),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let after_scroll = mounted.ui.ime_cursor_area().unwrap();
    assert!(
        after_scroll.origin.y > before_scroll.origin.y + 100.0,
        "manual transcript scrolling must not be pulled back to the caret"
    );
    mounted.settle();
    assert_eq!(mounted.ui.ime_cursor_area().unwrap(), after_scroll);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        text
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn browser_visual_viewport_reflows_chat_without_resizing_the_canvas() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|node| node.label.filter(|label| label.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    let text = (0..40)
        .map(|n| format!("Keyboard and zoom, line {n}: 🙂\n"))
        .collect::<String>();
    let end = text.encode_utf16().count();
    crate::browser_text::update(&mounted.ui, field.raw(), &text, end, end, false);
    mounted.settle();
    for size in [
        Size::new(390.0, 460.0),
        Size::new(195.0, 230.0),
        Size::new(390.0, 844.0),
    ] {
        mounted.model.browser_viewport.set(Some(size));
        mounted.settle();
        let tree = mounted.ui.inspection_snapshot();
        let mut parent = tree.node(field).unwrap().parent;
        let viewport = loop {
            let node = tree.node(parent.unwrap()).unwrap();
            if node.role == Role::ScrollView {
                break node.rect;
            }
            parent = node.parent;
        };
        let caret = mounted.ui.ime_cursor_area().unwrap();
        assert!(viewport.origin.y + viewport.size.height <= size.height);
        assert!(viewport.size.width <= size.width);
        assert!(caret.origin.y >= viewport.origin.y - 0.5);
        assert!(
            caret.origin.y + caret.size.height <= viewport.origin.y + viewport.size.height + 0.5,
            "caret must fit the visible chat while the canvas stays 390×900: {caret:?} / {viewport:?}; field {:?}; composer {:?}",
            tree.node(field).unwrap().rect,
            mounted.rect("Message composer")
        );
        assert_eq!(mounted.ui.focused().unwrap().id(), field);
        assert_eq!(
            mounted.ui.text_input_contents().unwrap().selection,
            (text.len(), text.len())
        );
    }
    assert_eq!(mounted.size, Size::new(390.0, 900.0));
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        text
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn first_character_keeps_draft_focus_and_plain_paste_keeps_the_same_text_surface() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let focus = mounted.ui.focused().unwrap().id();
    for character in ["H", "e", "l", "l", "o"] {
        mounted.key(Key::Character(character.into()), false);
        assert_eq!(mounted.ui.focused().unwrap().id(), focus);
    }
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Hello"
    );
    mounted.ui.set_clipboard_text(" pasted λ");
    mounted.key(Key::Character("v".into()), true);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Hello pasted λ"
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan").len(), 1);
    assert_eq!(mounted.ui.focused().unwrap().id(), focus);
}

#[test]
fn composing_on_recorded_text_creates_one_reply_and_never_submits_an_empty_composition() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    mounted.focus("Message m1");
    let source = mounted.model.snapshot.get_untracked().messages[0]
        .body
        .clone();
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "仮".into(),
        cursor: Some((0, 3)),
    });
    mounted.settle();
    assert!(!has_content(&crate::buffer::parts(
        mounted.model,
        "session-plan"
    )));
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定".into()));
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert!(has_content(&parts));
    assert_eq!(
        parts
            .iter()
            .filter(|p| matches!(p.kind, PartKind::Reply { .. }))
            .count(),
        1
    );
    assert!(plain_text(&parts).ends_with("確定"));
    assert_eq!(
        mounted.model.snapshot.get_untracked().messages[0].body,
        source
    );
}

#[test]
fn harness_settings_refresh_and_saved_execution_override_use_server_state() {
    let mut mounted = mount(false, 1380.0);
    let (refresh, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.harness_refresh.set(Some(refresh));
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        harnesses: vec![HarnessStatus {
            harness: Harness::ClaudeCode,
            executable: "/server/claude".into(),
            version: Some("2.test".into()),
            state: "ready".into(),
            detail: "Installed and authenticated".into(),
            checked_at: 1,
        }],
        ..Default::default()
    });
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Refresh Claude Code status");
    assert!(requests.try_recv().is_ok());
    assert!(mounted.commands.try_recv().is_err());
    mounted.click("Claude Code executable and status details");
    mounted.settle();
    mounted.click("Save Claude Code executable and check status");
    let command = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(command.command, Command::ConfigureHarness {harness: Harness::ClaudeCode, executable} if executable == "/server/claude")
    );
}

#[test]
fn inline_permission_answers_exact_run_and_disappears_when_expired() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let session = mounted.model.session.get_untracked();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.retain(|m| m.session_id != session);
    snapshot.tool_permissions.push(ToolPermission {
        id: "permission-1".into(),
        session_id: session.clone(),
        run_id: "native-run-1".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    let description = mounted.rect("Approval arguments");
    let request = mounted.rect("Approval request");
    assert!(
        description.size.height > 0.0,
        "approval details must be readable"
    );
    assert!(
        description.origin.y >= request.origin.y
            && description.origin.y + description.size.height
                <= request.origin.y + request.size.height,
        "{description:?} inside {request:?}"
    );
    mounted.click_nth("Approve", 0);
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(envelope.command,Command::RespondPermission{permission_id, run_id, allow:true} if permission_id == "permission-1" && run_id == "native-run-1")
    );
    snapshot.revision += 1;
    snapshot.tool_permissions[0].expired = true;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((envelope.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Approve"))
    );
}

fn local_project_snapshot() -> Snapshot {
    let mut snapshot = live_snapshot();
    snapshot.projects[0].github = None;
    snapshot.projects[0].root = Some("/server/projects/relay".into());
    snapshot.projects[0].defaults = AgentProfile {
        model: "fixture-model".into(),
        ..Default::default()
    };
    for issue in &mut snapshot.issues {
        issue.reference = None;
    }
    snapshot.migrate_projects();
    snapshot
}

fn type_in(mounted: &Mounted, label: &str, text: &str) {
    mounted.focus(label);
    mounted.key(Key::Character("a".into()), true);
    mounted.ui.dispatch_ime(ImeEvent::Commit(text.into()));
    mounted.settle();
}

#[test]
fn new_project_preserves_full_setup_draft_and_appends_on_acknowledgement() {
    let mut mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    type_in(&mounted, "Project name", "Second project");
    type_in(&mounted, "Project root on server", "/srv/second");
    mounted.click("New connection");
    type_in(
        &mounted,
        "Repository URL",
        "git@example.com:team/second.git",
    );
    mounted.click("Settings");
    open_project_menu(&mounted);
    mounted.click("New Project");
    assert_eq!(
        mounted.model.project_draft.get_untracked().name,
        "Second project"
    );
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connection_form
            .address,
        "git@example.com:team/second.git"
    );
    mounted.focus("Add connection");
    mounted.click("Add connection");
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .len(),
        1
    );
    mounted.focus("Create project");
    mounted.click("Create project");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&request.command,Command::CreateProject{name,root,connections} if name=="Second project" && root=="/srv/second" && matches!(&connections[..],[ConnectionInput::Repository{remote}] if remote=="git@example.com:team/second.git"))
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut project = snapshot.projects[0].clone();
    project.id = format!("project-{}", request.request_id);
    project.name = "Second project".into();
    project.fixture = false;
    snapshot.projects.push(project.clone());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), project.id);
    assert_eq!(mounted.model.snapshot.get_untracked().projects.len(), 2);
    assert!(mounted.model.project_draft.get_untracked().name.is_empty());
    assert!(mounted.model.notice.get_untracked().is_empty());
}

/// Types into an input the way a person does: click it, then type keys.
fn click_and_type(mounted: &Mounted, label: &str, text: &str) {
    mounted.click(label);
    mounted.key(Key::Character(text.into()), false);
}

fn add_initial_repository(mounted: &Mounted, remote: &str) {
    mounted.click("New connection");
    click_and_type(mounted, "Repository URL", remote);
    mounted.click("Add connection");
}

fn assert_creates_playground(mounted: &mut Mounted) {
    mounted.click("Create project");
    assert_eq!(mounted.model.notice.get_untracked(), "");
    let request = mounted
        .commands
        .try_recv()
        .expect("CreateProject submitted");
    assert!(
        matches!(&request.command, Command::CreateProject { name, root, connections }
            if name == "Playground"
                && root == "/home/gmk/repos/kevork/playground"
                && matches!(&connections[..], [ConnectionInput::Repository { remote }]
                    if remote == "git@github.com:KevorkC/Up.git")),
        "{:?}",
        request.command
    );
}

#[test]
fn new_project_creates_when_the_connection_is_added_after_typing() {
    let mut mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    click_and_type(&mounted, "Project name", " Playground ");
    click_and_type(
        &mounted,
        "Project root on server",
        "/home/gmk/repos/kevork/playground ",
    );
    add_initial_repository(&mounted, "git@github.com:KevorkC/Up.git");
    assert_creates_playground(&mut mounted);
}

#[test]
fn new_project_creates_when_the_connection_is_added_before_typing() {
    let mut mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    add_initial_repository(&mounted, "git@github.com:KevorkC/Up.git");
    click_and_type(&mounted, "Project name", "Playground");
    click_and_type(
        &mounted,
        "Project root on server",
        "/home/gmk/repos/kevork/playground",
    );
    assert_creates_playground(&mut mounted);
}

#[test]
fn new_project_notice_names_only_the_failing_field() {
    let mut mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    click_and_type(
        &mounted,
        "Project root on server",
        "/home/gmk/repos/kevork/playground",
    );
    mounted.click("Create project");
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Enter a project name."
    );
    click_and_type(&mounted, "Project name", "Playground");
    type_in(
        &mounted,
        "Project root on server",
        "repos/playground",
    );
    mounted.click("Create project");
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Enter an absolute root on the server."
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn server_roots_are_checked_as_unix_paths_on_every_client_platform() {
    // The browser client's std rejects every path as relative, so the check
    // must not depend on the client's `Path::is_absolute` (#157).
    assert!(crate::projects::server_absolute(
        "/home/gmk/repos/kevork/playground"
    ));
    assert!(!crate::projects::server_absolute("repos/playground"));
    assert!(!crate::projects::server_absolute(r"C:\projects\playground"));
    assert!(!crate::projects::server_absolute(""));
}

fn project_draft(name: &str, root: &str) -> crate::projects::ProjectDraft {
    crate::projects::ProjectDraft {
        name: name.into(),
        root: root.into(),
        ..Default::default()
    }
}

#[test]
fn an_empty_root_is_the_servers_automatic_folder() {
    use crate::projects::{root_hint, setup_problem};
    let base = "/srv/relay/projects";
    assert_eq!(setup_problem(&project_draft("Mosaic", ""), base), None);
    assert_eq!(setup_problem(&project_draft("Mosaic", "  "), base), None);
    assert_eq!(
        setup_problem(&project_draft("Mosaic", "/srv/custom"), ""),
        None
    );
    assert_eq!(
        setup_problem(&project_draft("Mosaic", ""), ""),
        Some("Enter an absolute root on the server.")
    );
    assert_eq!(
        setup_problem(&project_draft("", ""), base),
        Some("Enter a project name.")
    );
    assert_eq!(
        setup_problem(&project_draft("!!!", ""), base),
        Some("Use letters or numbers in the project name, or enter a root on the server.")
    );
    assert_eq!(
        setup_problem(&project_draft("!!!", "/srv/custom"), base),
        None
    );
    assert_eq!(
        setup_problem(&project_draft("Mosaic", "projects/mosaic"), base),
        Some("Enter an absolute root on the server.")
    );
    assert_eq!(root_hint(base, "Mosaic"), "/srv/relay/projects/mosaic");
    assert_eq!(root_hint(base, "My  App"), "/srv/relay/projects/my-app");
    assert_eq!(root_hint(base, ""), "/srv/relay/projects/project-name");
    assert_eq!(root_hint("", "Mosaic"), "Absolute path on the server");
}

#[test]
fn new_project_sends_an_empty_root_when_the_automatic_folder_is_kept() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.server_build.set(Some(BuildInfo {
        projects_root: "/srv/relay/projects".into(),
        ..Default::default()
    }));
    open_project_menu(&mounted);
    mounted.click("New Project");
    click_and_type(&mounted, "Project name", "Mosaic");
    mounted.click("Create project");
    assert_eq!(mounted.model.notice.get_untracked(), "");
    let request = mounted
        .commands
        .try_recv()
        .expect("CreateProject submitted");
    assert!(
        matches!(&request.command, Command::CreateProject { name, root, connections }
            if name == "Mosaic" && root.is_empty() && connections.is_empty()),
        "{:?}",
        request.command
    );
}

#[test]
fn new_project_keeps_a_typed_root_over_the_automatic_folder() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.server_build.set(Some(BuildInfo {
        projects_root: "/srv/relay/projects".into(),
        ..Default::default()
    }));
    open_project_menu(&mounted);
    mounted.click("New Project");
    click_and_type(&mounted, "Project name", "Mosaic");
    click_and_type(&mounted, "Project root on server", "/srv/existing/mosaic");
    mounted.click("Create project");
    let request = mounted
        .commands
        .try_recv()
        .expect("CreateProject submitted");
    assert!(
        matches!(&request.command, Command::CreateProject { root, .. }
            if root == "/srv/existing/mosaic"),
        "{:?}",
        request.command
    );
}

#[test]
fn new_project_without_a_reported_server_folder_asks_for_a_root() {
    let mut mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    click_and_type(&mounted, "Project name", "Mosaic");
    mounted.click("Create project");
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Enter an absolute root on the server."
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn connection_forms_describe_each_kind_or_name_the_problem() {
    use crate::projects::{ConnectionDraft, form_connection};
    let form = |kind, address: &str, host: &str, number: &str, gitlab, group| ConnectionDraft {
        kind,
        address: address.into(),
        host: host.into(),
        number: number.into(),
        group,
        url: String::new(),
        gitlab,
    };
    assert_eq!(
        form_connection(&form(0, " git@example.com:team/a.git ", "", "", false, false)),
        Ok(ConnectionInput::Repository {
            remote: "git@example.com:team/a.git".into()
        })
    );
    assert!(form_connection(&form(0, " ", "", "", false, false)).is_err());
    assert_eq!(
        form_connection(&form(1, "/srv/data", "", "", false, false)),
        Ok(ConnectionInput::Directory {
            path: "/srv/data".into()
        })
    );
    assert!(form_connection(&form(1, "data", "", "", false, false)).is_err());
    assert_eq!(
        form_connection(&form(3, "team", "gitlab.com", " 12 ", false, true)),
        Ok(ConnectionInput::Board {
            source: BoardSource::Github {
                owner: "team".into(),
                number: 12,
                url: String::new()
            }
        })
    );
    assert_eq!(
        form_connection(&form(3, "team/group", " gitlab.example ", "7", true, true)),
        Ok(ConnectionInput::Board {
            source: BoardSource::Gitlab {
                host: "gitlab.example".into(),
                path: "team/group".into(),
                group: true,
                number: 7,
                url: String::new()
            }
        })
    );
    for number in ["", "0", "-1", "x", "1.5"] {
        assert_eq!(
            form_connection(&form(3, "team", "", number, false, false)),
            Err("Enter the number of an existing board.")
        );
    }
    assert_eq!(
        form_connection(&form(3, "", "", "1", false, false)),
        Err("Enter the GitHub user or organization.")
    );
    assert_eq!(
        form_connection(&form(3, "", "gitlab.com", "1", true, false)),
        Err("Enter the GitLab project or group path.")
    );
    assert_eq!(
        form_connection(&form(3, "team/app", " ", "1", true, false)),
        Err("Enter the GitLab host.")
    );
    // A board URL is resolved by the server, never built from the form.
    assert!(form_connection(&form(2, "team", "gitlab.com", "1", false, false)).is_err());
}

#[test]
fn several_connections_of_different_kinds_can_be_added_before_creating() {
    let mounted = mount(false, 1380.0);
    open_project_menu(&mounted);
    mounted.click("New Project");
    mounted.click("New connection");
    // An incomplete form adds nothing.
    mounted.click("Add connection");
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
    click_and_type(&mounted, "Repository URL", "owner/one");
    mounted.click("Add connection");
    // The form stays open and empty for the next repository.
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connection_form
            .address
            .is_empty()
    );
    click_and_type(&mounted, "Repository URL", "owner/two");
    mounted.click("Add connection");
    mounted.click("Connection type: Manual board");
    mounted.click("Board provider: GitLab");
    click_and_type(&mounted, "GitLab project or group path", "team/group");
    mounted.click("GitLab board scope: Group board");
    click_and_type(&mounted, "Existing board number", "7");
    mounted.click("Add connection");
    let draft = mounted.model.project_draft.get_untracked();
    assert_eq!(
        draft.connections,
        vec![
            ConnectionInput::Repository {
                remote: "owner/one".into()
            },
            ConnectionInput::Repository {
                remote: "owner/two".into()
            },
            ConnectionInput::Board {
                source: BoardSource::Gitlab {
                    host: "gitlab.com".into(),
                    path: "team/group".into(),
                    group: true,
                    number: 7,
                    url: String::new()
                }
            },
        ]
    );
    assert_eq!(draft.connection_form.kind, 3);
}

fn gitlab_choice(name: &str, number: u64) -> BoardChoice {
    BoardChoice {
        name: name.into(),
        source: BoardSource::Gitlab {
            host: "gitlab.com".into(),
            group: true,
            path: "unincorporated/mosaic".into(),
            number,
            url: String::new(),
        },
    }
}

const PASTED_BOARD: &str = "https://gitlab.com/groups/unincorporated/mosaic/-/boards#/";

/// A New Project form on the Board URL tab with a lookup channel, and the
/// request the server would receive after the address is submitted.
fn submit_pasted_board(
    mounted: &Mounted,
) -> crate::project_network::LookupRequest {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.lookup_requests.set(Some(sender));
    open_project_menu(mounted);
    mounted.click("New Project");
    mounted.click("New connection");
    mounted.click("Connection type: Board URL");
    click_and_type(mounted, "Board URL", PASTED_BOARD);
    mounted.click("Add board");
    receiver.try_recv().expect("board lookup requested")
}

fn answer_lookup(
    mounted: &Mounted,
    request: crate::project_network::LookupRequest,
    result: Result<Vec<BoardChoice>, String>,
) {
    mounted.model.lookup.set(crate::project_network::LookupUpdate {
        request: Some(request),
        result: Some(result),
    });
    mounted.settle();
}

#[test]
fn a_pasted_board_address_with_one_board_is_added_without_a_number() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    assert_eq!(request.url, PASTED_BOARD);
    // Nothing is added until the server answers.
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
    answer_lookup(&mounted, request, Ok(vec![gitlab_choice("Roadmap", 7)]));
    let draft = mounted.model.project_draft.get_untracked();
    assert_eq!(
        draft.connections,
        vec![ConnectionInput::Board {
            source: gitlab_choice("Roadmap", 7).source
        }]
    );
    assert!(draft.connection_form.url.is_empty());
}

#[test]
fn a_pasted_board_address_with_several_boards_offers_a_picker_by_name() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    answer_lookup(
        &mounted,
        request,
        Ok(vec![
            gitlab_choice("Roadmap", 7),
            gitlab_choice("Sprint", 9),
        ]),
    );
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
    mounted.click("Add board Sprint");
    let draft = mounted.model.project_draft.get_untracked();
    assert_eq!(
        draft.connections,
        vec![ConnectionInput::Board {
            source: gitlab_choice("Sprint", 9).source
        }]
    );
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Add board Roadmap"))
    }));
}

#[test]
fn a_pasted_board_address_that_finds_nothing_adds_nothing_and_says_why() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    answer_lookup(
        &mounted,
        request,
        Err("No boards found in unincorporated/mosaic. Create a board in GitLab, then paste its address.".into()),
    );
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
    // The address stays for correction and the form can look again.
    assert_eq!(
        mounted.model.project_draft.get_untracked().connection_form.url,
        PASTED_BOARD
    );
    mounted.click("Add board");
}

#[test]
fn an_empty_board_address_is_not_sent_to_the_server() {
    let mounted = mount(false, 1380.0);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.lookup_requests.set(Some(sender));
    open_project_menu(&mounted);
    mounted.click("New Project");
    mounted.click("New connection");
    mounted.click("Connection type: Board URL");
    mounted.click("Add board");
    assert!(receiver.try_recv().is_err());
}

#[test]
fn a_board_answer_for_another_request_is_ignored() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    let mut other = request.clone();
    other.id = "another-lookup".into();
    answer_lookup(&mounted, other, Ok(vec![gitlab_choice("Roadmap", 7)]));
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
    answer_lookup(&mounted, request, Ok(vec![gitlab_choice("Roadmap", 7)]));
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .len(),
        1
    );
}

fn offers_board(mounted: &Mounted, title: &str) -> bool {
    let label = format!("Add board {title}");
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .any(|n| n.label.as_deref() == Some(label.as_str()))
}

#[test]
fn editing_the_address_discards_an_answer_for_the_old_one() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    click_and_type(&mounted, "Board URL", "x");
    let edited = mounted
        .model
        .project_draft
        .get_untracked()
        .connection_form
        .url;
    assert_ne!(edited, PASTED_BOARD);
    answer_lookup(&mounted, request, Ok(vec![gitlab_choice("Roadmap", 7)]));
    let draft = mounted.model.project_draft.get_untracked();
    // Nothing is added for the old address and the new one is not erased.
    assert!(draft.connections.is_empty());
    assert_eq!(draft.connection_form.url, edited);
}

#[test]
fn editing_the_address_withdraws_the_boards_found_for_the_old_one() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    answer_lookup(
        &mounted,
        request,
        Ok(vec![
            gitlab_choice("Roadmap", 7),
            gitlab_choice("Sprint", 9),
        ]),
    );
    assert!(offers_board(&mounted, "Sprint"));
    click_and_type(&mounted, "Board URL", "x");
    mounted.settle();
    assert!(!offers_board(&mounted, "Sprint"));
    assert!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .is_empty()
    );
}

#[test]
fn a_board_that_cannot_be_queued_stays_offered_with_its_address() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.lookup_requests.set(Some(sender));
    mounted.click("Connection type: Board URL");
    click_and_type(&mounted, "Board URL", PASTED_BOARD);
    mounted.click("Add board");
    let request = receiver.try_recv().expect("board lookup requested");
    // Another save is running when the answer arrives.
    mounted.model.busy.set(true);
    mounted.settle();
    answer_lookup(&mounted, request, Ok(vec![gitlab_choice("Roadmap", 7)]));
    assert!(mounted.commands.try_recv().is_err());
    assert!(offers_board(&mounted, "Roadmap"));
    // Once the save finishes, the offered board is queued as a command.
    mounted.model.busy.set(false);
    mounted.settle();
    mounted.click("Add board Roadmap");
    let sent = mounted.commands.try_recv().expect("AddConnection queued");
    assert!(matches!(
        &sent.command,
        Command::AddConnection {
            connection: ConnectionInput::Board { source },
            ..
        } if *source == gitlab_choice("Roadmap", 7).source
    ));
    assert!(!offers_board(&mounted, "Roadmap"));
}

#[test]
fn the_same_board_is_one_connection_however_its_address_was_entered() {
    use crate::projects::same_connection;
    let board = |url: &str, number: u64| ConnectionInput::Board {
        source: BoardSource::Gitlab {
            host: "gitlab.com".into(),
            group: true,
            path: "unincorporated/mosaic".into(),
            number,
            url: url.into(),
        },
    };
    let numbered = board(
        "https://gitlab.com/groups/unincorporated/mosaic/-/boards/7",
        7,
    );
    // A numbered address returns the canonical URL; a listing or manual entry
    // leaves it empty. Both are the board the server refuses to connect twice.
    assert!(same_connection(&numbered, &board("", 7)));
    assert!(!same_connection(&numbered, &board("", 8)));
    let github = |owner: &str| ConnectionInput::Board {
        source: BoardSource::Github {
            owner: owner.into(),
            number: 3,
            url: String::new(),
        },
    };
    assert!(same_connection(&github("Team"), &github("team")));
    assert!(!same_connection(&github("team"), &numbered));
    let repository = |remote: &str| ConnectionInput::Repository {
        remote: remote.into(),
    };
    assert!(same_connection(&repository("a/b"), &repository("a/b")));
    assert!(!same_connection(&repository("a/b"), &repository("a/c")));
}

#[test]
fn a_board_found_twice_is_added_to_the_draft_once() {
    let mounted = mount(false, 1380.0);
    let request = submit_pasted_board(&mounted);
    let mut numbered = gitlab_choice("Roadmap", 7);
    if let BoardSource::Gitlab { url, .. } = &mut numbered.source {
        *url = "https://gitlab.com/groups/unincorporated/mosaic/-/boards/7".into();
    }
    answer_lookup(&mounted, request, Ok(vec![numbered]));
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .len(),
        1
    );
    // The same board again, as a listing would return it.
    click_and_type(&mounted, "Board URL", PASTED_BOARD);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.lookup_requests.set(Some(sender));
    mounted.click("Add board");
    let request = receiver.try_recv().expect("second lookup requested");
    answer_lookup(&mounted, request, Ok(vec![gitlab_choice("Roadmap", 7)]));
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .len(),
        1
    );
}

#[test]
fn board_choices_with_the_same_name_show_their_numbers() {
    use crate::projects::choice_title;
    let boards = vec![
        gitlab_choice("Board", 1),
        gitlab_choice("Board", 2),
        gitlab_choice("Other", 3),
    ];
    assert_eq!(choice_title(&boards, 0), "Board (1)");
    assert_eq!(choice_title(&boards, 1), "Board (2)");
    assert_eq!(choice_title(&boards, 2), "Other");
}

#[test]
fn board_memberships_selection_and_project_switching_preserve_running_sessions() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let mut second = snapshot.boards[0].clone();
    second.id = "second-board".into();
    second.name = "Second board".into();
    second.columns = local_columns();
    snapshot.boards.push(second);
    snapshot.memberships.push(BoardMembership {
        board_id: "second-board".into(),
        issue_id: "issue-2".into(),
        column_ids: vec!["done".into()],
        remote_item_id: None,
    });
    let mut other = snapshot.projects[0].clone();
    other.id = "other-project".into();
    other.name = "Other".into();
    snapshot.projects.push(other);
    let original_projects = snapshot.projects.clone();
    let original_sessions = snapshot.sessions.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    let group = mounted.rect("Board selector");
    let choice = mounted.rect("Select board Second board");
    assert!(choice.origin.y > group.origin.y);
    assert!(choice.size.height >= 30.0);
    assert!(choice.origin.y + choice.size.height <= group.origin.y + group.size.height + 1.0);
    let task = mounted
        .model
        .snapshot
        .get_untracked()
        .issues
        .iter()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .clone();
    assert!(mounted.model.task_in_column(&task, &task.column_id));
    mounted.click("Select board Second board");
    assert!(mounted.model.task_in_column(&task, "done"));
    assert!(!mounted.model.task_in_column(&task, &task.column_id));
    mounted.model.select_project("other-project".into());
    mounted.model.select_project("demo".into());
    assert_eq!(mounted.model.selected_board().unwrap().id, "second-board");
    assert_eq!(
        mounted.model.snapshot.get_untracked().projects,
        original_projects
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions,
        original_sessions
    );
}

#[test]
fn local_tasks_render_without_remote_reference_and_move_on_selected_board() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    let task = mounted
        .model
        .snapshot
        .get_untracked()
        .issues
        .iter()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .clone();
    let label = format!("Open local task {}", task.title);
    mounted.click(&label);
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref().is_some_and(|s| s.contains("#0")))
    );
    mounted.focus("Edit task");
    mounted.click("Edit task");
    mounted.focus("Move task to In review");
    mounted.click("Move task to In review");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::MoveTask{board_id,issue_id,column_id} if board_id=="board-demo" && issue_id=="issue-2" && column_id=="review")
    );
}

#[test]
fn initial_workspace_choice_is_explicit_and_followup_keeps_recorded_resources() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.projects[0].defaults.model = "fixture-model".into();
    for (id, name) in [("repo", "Repository"), ("directory", "Directory")] {
        snapshot.connections.push(ProjectConnection {
            id: id.into(),
            project_id: "demo".into(),
            name: name.into(),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Directory {
                path: format!("/server/{id}"),
            },
        });
    }
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    // Director and resource choices are disclosed from the worker setup head.
    mounted.click("Choose director and resources");
    mounted.focus("Choose session resources");
    mounted.click("Choose session resources");
    mounted.focus("Workspace resource Directory");
    mounted.click("Workspace resource Directory");
    assert_eq!(
        mounted.model.workspace_selection.get_untracked(),
        Some(vec!["repo".into()])
    );
    mounted.model.worker_prompt.set("Run local task".into());
    mounted.model.run_worker(false);
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::StartSession{issue_id:Some(issue_id),connection_ids:Some(ids),role:SessionRole::Worker,..} if issue_id=="issue-2" && ids==vec!["repo"])
    );
}

#[test]
fn reconciliation_has_no_retry_and_tracks_live_operation_state() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.operations.push(ProjectOperation {
        id: "publish-1".into(),
        project_id: "demo".into(),
        kind: OperationKind::Sync {
            board_id: "board-demo".into(),
        },
        state: OperationState::NeedsReconciliation,
        error: Some("Unknown provider write outcome".into()),
        results: [("pending".into(), "membership/issue-2".into())]
            .into_iter()
            .collect(),
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Retry operation"))
    );
    mounted.rect("This recovery step does not support URL lookup. Ask the server administrator to inspect the provider result before continuing.");
    assert!(mounted.commands.try_recv().is_err());
    mounted
        .model
        .snapshot
        .update(|s| s.operations[0].state = OperationState::Running);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Use this result and continue"))
    );
}

#[test]
fn appearance_radios_arrow_keys_and_scale_stepper_keep_accessible_semantics() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.click("Settings");
    mounted.focus("Theme: Dark");
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Theme: Dark"))
            .unwrap()
            .role,
        Role::Radio
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::Light
    );
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .node(mounted.ui.focused().unwrap().id())
            .unwrap()
            .label
            .as_deref(),
        Some("Theme: Light")
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
    assert!(!has_label(&mounted, "Reset interface scale"));
    mounted.focus("Interface scale percent");
    mounted.key(Key::ArrowUp, false);
    assert!((mounted.model.preferences.get_untracked().scale - 1.1).abs() < 0.001);
    mounted.focus("Reset interface scale");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.0);
    mounted.settle();
    assert!(!has_label(&mounted, "Reset interface scale"));
}

#[test]
fn first_director_prompt_uses_planning_session_without_issue_or_implementation_approval() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted
        .model
        .snapshot
        .update(|s| s.sessions.retain(|s| s.role != SessionRole::Root));
    mounted
        .model
        .worker_prompt
        .set("Unrelated worker draft".into());
    mounted.model.open_root_agent("director-main".into());
    mounted.settle();
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
    let draft_label = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
            .unwrap()
    };
    type_in(
        &mounted,
        &draft_label(&mounted),
        "\n\n Plan the project \n\n",
    );
    mounted.model.open_root_agent("director-review".into());
    mounted.settle();
    type_in(&mounted, &draft_label(&mounted), "Review plan");
    mounted.model.open_root_agent("director-main".into());
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(
            mounted.model,
            "agent-draft-director-main"
        )),
        "\n\n Plan the project \n\n"
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Unrelated worker draft"
    );
    mounted.click("Send message");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::StartRootAgent{agent_config_id,prompt,parts,issue_id:None,approve_implementation:false} if agent_config_id=="director-main" && prompt=="Plan the project" && plain_text(&parts)==prompt)
    );
}

#[test]
fn nonempty_columns_cannot_be_deleted_without_explicit_task_moves() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    mounted.click("Board actions");
    mounted.click("Manage columns");
    mounted.focus("Column title");
    mounted.click("Delete empty column");
    assert!(mounted.commands.is_empty());
    assert_eq!(mounted.model.selected_board().unwrap().columns.len(), 3);
}

#[test]
fn new_project_form_remains_keyboard_reachable_at_two_hundred_percent() {
    let mut mounted = mount(false, 820.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| {
        p.scale = 2.0;
        p.sidebar_width = 160.0;
    });
    mounted.size = Size::new(820.0, 600.0);
    mounted.model.page.set(Page::NewProject);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Repository URL"))
    );
    mounted.focus("New connection");
    mounted.click("New connection");
    for label in [
        "Project name",
        "Project root on server",
        "Repository URL",
        "Add connection",
        "Create project",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= 820.01,
            "{label}: {rect:?}"
        );
    }
}

#[test]
fn nix_commands_setting_sends_the_chosen_mode_for_the_project() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    let project = mounted.model.project.get_untracked();
    for option in ["Off", "Ask", "Allow"] {
        mounted.rect(&format!("Nix commands: {option}"));
    }
    mounted.click("Nix commands: Allow");
    let command = mounted.commands.try_recv().unwrap().command;
    assert!(
        matches!(command, Command::SetNixCommands { ref project_id, mode: NixCommands::Allow } if *project_id == project),
        "{command:?}"
    );
}

#[test]
fn claude_window_start_is_off_until_chosen_and_sends_the_setting() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    assert!(!mounted.model.snapshot.get_untracked().claude_window_start.enabled);
    for option in ["Off", "On"] {
        mounted.rect(&format!("Claude window start: {option}"));
    }
    mounted.click("Claude window start: On");
    let command = mounted.commands.try_recv().unwrap().command;
    assert!(
        matches!(command, Command::SetClaudeWindowStart { enabled: true }),
        "{command:?}"
    );
}

#[test]
fn connection_states_update_and_retry_and_remove_use_stable_connection_ids() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot
        .connections
        .retain(|c| !matches!(c.kind, ConnectionKind::Board { .. }));
    snapshot.connections.push(ProjectConnection {
        id: "repo-1".into(),
        project_id: "demo".into(),
        name: "Repository".into(),
        enabled: true,
        state: ConnectionState::Failed,
        error: Some("Clone failed".into()),
        kind: ConnectionKind::Repository {
            remote: "git@example.com:team/repo.git".into(),
            checkout: None,
            owned: true,
        },
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    let row = mounted.rect("Connection row repo-1");
    let remove = mounted.rect("Remove connection");
    assert_eq!(remove.origin.y, row.origin.y);
    assert_eq!(remove.size.height, row.size.height);
    assert_eq!(
        remove.origin.x + remove.size.width,
        row.origin.x + row.size.width
    );
    mounted.click("Retry connection");
    let retry = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(retry.command,Command::RetryConnection{connection_id} if connection_id=="repo-1")
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((retry.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted
        .model
        .snapshot
        .update(|s| s.connections[0].state = ConnectionState::Ready);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Retry connection"))
    );
    mounted.click("Remove connection");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::RemoveConnection{connection_id} if connection_id=="repo-1")
    );
    mounted.model.snapshot.update(|s| s.connections.clear());
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Remove connection"))
    );
}

#[test]
fn connection_edit_submits_only_a_name_and_cancel_keeps_resources() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot
        .connections
        .retain(|c| !matches!(c.kind, ConnectionKind::Board { .. }));
    snapshot.connections.push(ProjectConnection {
        id: "workspace".into(),
        project_id: snapshot.projects[0].id.clone(),
        name: "Workspace".into(),
        enabled: true,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Directory {
            path: "/srv/relay/workspace".into(),
        },
    });
    let original = snapshot.connections[0].clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    let edit = format!("Edit connection {}", original.name);
    mounted.click(&edit);
    type_in(&mounted, "Connection name", "Source workspace");
    mounted.click("Cancel connection edit");
    assert!(mounted.commands.is_empty());
    assert_eq!(
        mounted.model.snapshot.get_untracked().connections[0],
        original
    );
    mounted.click(&edit);
    type_in(&mounted, "Connection name", "Source workspace");
    mounted.click("Save connection name");
    assert!(matches!(mounted.commands.try_recv().unwrap().command,
        Command::UpdateConnection { connection_id, name } if connection_id == original.id && name == "Source workspace"));
    assert_eq!(
        mounted.model.snapshot.get_untracked().connections[0],
        original,
        "the client waits for the authoritative server snapshot"
    );
}

#[test]
fn publish_maps_columns_and_task_repositories_without_pretending_board_is_remote() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.connections.push(ProjectConnection {
        id: "repository-1".into(),
        project_id: "demo".into(),
        name: "Repository".into(),
        enabled: true,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Repository {
            remote: "git@example.com:team/repo.git".into(),
            checkout: Some("/srv/repo".into()),
            owned: true,
        },
    });
    let tasks = snapshot.issues.clone();
    let columns = snapshot.boards[0].columns.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination title", "Published work");
    let source = BoardSource::Github {
        owner: "team".into(),
        number: 0,
        url: String::new(),
    };
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source,
                name: "New board defaults".into(),
                columns: vec![BoardColumn {
                    id: "provider-default-done".into(),
                    title: "Done".into(),
                }],
            })),
        });
    mounted.settle();
    for column in columns {
        let label = format!("Map {} to Done", column.title);
        mounted.focus(&label);
        mounted.click(&label);
    }
    for task in tasks {
        let label = format!("Issue repository Repository for {}", task.title);
        mounted.focus(&label);
        mounted.click(&label);
    }
    mounted.focus("Confirm board publication");
    mounted.click("Confirm board publication");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(request.command,Command::PublishBoard{board_id,target,columns,tasks} if board_id=="board-demo" && matches!(&target.source,BoardSource::Github{owner,number:0,..} if owner=="team") && target.name=="Published work" && columns.len()==3 && tasks.len()==4 && tasks.iter().all(|t|t.repository_connection_id=="repository-1"))
    );
    assert_eq!(
        mounted.model.selected_board().unwrap().source,
        BoardSource::Local
    );
}

#[test]
fn multi_repository_review_and_followup_keep_recorded_workspace_selection() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    mounted.model.snapshot.update(|s| {
        let session = &mut s.sessions[0];
        session.connection_ids = vec!["repo-one".into(), "repo-two".into()];
        session.worker.as_mut().unwrap().status = WorkerStatus::Completed;
        session.workspaces = vec![SessionWorkspace {
            connection_id: "repo-two".into(),
            path: "/srv/repo-two/worktree".into(),
            repository: true,
            branch: Some("worker/two".into()),
            base_commit: Some("base-two".into()),
            changes: Some(ChangeSet {
                files: vec!["two.rs".into()],
                diff: "second repository diff".into(),
                truncated: false,
            }),
        }];
    });
    mounted.settle();
    mounted.click("Session actions");
    mounted.click("Toggle change review");
    assert!(has_label(&mounted, "repo-two"));
    assert!(mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|text| text.contains("two.rs") && text.contains("second repository diff"))
    }));
    assert!(has_label(&mounted, "worker/two"));
    mounted
        .model
        .workspace_selection
        .set(Some(vec!["unrelated".into()]));
    mounted
        .model
        .worker_prompt
        .set("Continue recorded resources".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(true);
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::SendWorker{session_id,..} if session_id=="session-plan")
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions[0].connection_ids,
        vec!["repo-one", "repo-two"]
    );
}

#[test]
fn destination_discovery_uses_server_metadata_and_ignores_a_different_destination() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.discovery_requests.set(Some(sender));
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination number (0 creates new)", "7");
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    let source = requests.try_recv().unwrap();
    assert!(matches!(&source,BoardSource::Github{owner,number:7,..} if owner=="team"));
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source: source.clone(),
                name: "Remote destination".into(),
                columns: vec![BoardColumn {
                    id: "real-status-id".into(),
                    title: "Ready".into(),
                }],
            })),
        });
    mounted.settle();
    mounted.focus("Map Backlog to Ready");
    mounted.click("Map Backlog to Ready");
    type_in(&mounted, "Destination owner or path", "other-team");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Map Backlog to Ready"))
    );
    assert_eq!(
        mounted.model.selected_board().unwrap().source,
        BoardSource::Local
    );
}

#[test]
fn new_destination_defaults_come_from_server_and_publish_draft_survives_navigation_errors() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.discovery_requests.set(Some(sender));
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination title", "New destination");
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    let source = requests.try_recv().unwrap();
    assert!(matches!(&source, BoardSource::Github { number: 0, .. }));
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Err("Destination access denied".into())),
        });
    mounted.settle();
    mounted.rect("Destination access denied");
    mounted.click("Settings");
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let draft = mounted
        .model
        .publish_drafts
        .get_untracked()
        .get("board-demo")
        .unwrap()
        .clone();
    assert_eq!(draft.path, "team");
    assert_eq!(draft.title, "New destination");
    mounted.rect("Destination access denied");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Enter status IDs"))
    );
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source,
                name: "Default statuses".into(),
                columns: vec![BoardColumn {
                    id: "opaque-provider-id".into(),
                    title: "Ready".into(),
                }],
            })),
        });
    mounted.settle();
    mounted.focus("Map Backlog to Ready");
    mounted.click("Map Backlog to Ready");
    mounted.click("Settings");
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    assert_eq!(
        mounted
            .model
            .publish_drafts
            .get_untracked()
            .get("board-demo")
            .unwrap()
            .mappings
            .get("backlog")
            .unwrap(),
        "opaque-provider-id"
    );
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|label| label.contains("opaque-provider-id"))
    }));
    drop(requests);
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    mounted.rect("Cannot read destination statuses. Reconnect and try again.");
}

#[test]
fn board_reconciliation_accepts_a_url_and_builds_the_typed_result_for_the_pending_step() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.operations.push(ProjectOperation {
        id: "publish-url".into(),
        project_id: "demo".into(),
        kind: OperationKind::Publish {
            board_id: "board-demo".into(),
            target: PublishTarget {
                source: BoardSource::Github {
                    owner: "team".into(),
                    number: 0,
                    url: String::new(),
                },
                name: "Work".into(),
            },
            columns: vec![],
            tasks: vec![],
        },
        state: OperationState::NeedsReconciliation,
        error: Some("Connection lost after board creation".into()),
        results: [("pending".into(), "board".into())].into_iter().collect(),
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Provider operation key"))
    );
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.recovery_requests.set(Some(sender));
    type_in(
        &mounted,
        "Created board or issue URL",
        "https://github.com/orgs/team/projects/9",
    );
    mounted.focus("Check result");
    mounted.click("Check result");
    let request = requests.try_recv().unwrap();
    let result = ReconciliationResult {
        key: "board".into(),
        result: serde_json::to_string(&BoardSource::Github {
            owner: "team".into(),
            number: 9,
            url: request.input.url.clone(),
        })
        .unwrap(),
        description: "Found board Work in team".into(),
    };
    type_in(
        &mounted,
        "Created board or issue URL",
        "https://github.com/orgs/team/projects/10",
    );
    mounted
        .model
        .recovery
        .set(crate::project_network::RecoveryUpdate {
            request: Some(request.clone()),
            result: Some(Ok(result.clone())),
        });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Use this result and continue"))
    );
    type_in(&mounted, "Created board or issue URL", &request.input.url);
    mounted.focus("Check result");
    mounted.click("Check result");
    let request = requests.try_recv().unwrap();
    mounted
        .model
        .recovery
        .set(crate::project_network::RecoveryUpdate {
            request: Some(request),
            result: Some(Ok(result)),
        });
    mounted.settle();
    mounted.rect("Found board Work in team");
    mounted.focus("Use this result and continue");
    mounted.click("Use this result and continue");
    let Command::ReconcileOperation { key, result, .. } =
        mounted.commands.try_recv().unwrap().command
    else {
        panic!("Expected reconciliation");
    };
    assert_eq!(key, "board");
    assert!(
        matches!(serde_json::from_str::<BoardSource>(&result).unwrap(),BoardSource::Github{owner,number:9,url} if owner=="team" && url=="https://github.com/orgs/team/projects/9")
    );
}

#[test]
fn disabled_board_connections_hide_selectors_and_block_new_work_without_losing_history() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.boards[0].name = "Disabled historical board".into();
    let board = snapshot.boards[0].clone();
    snapshot.connections.retain(
        |c| !matches!(&c.kind, ConnectionKind::Board { board_id } if board_id == &board.id),
    );
    snapshot.connections.push(ProjectConnection {
        id: "disabled-board".into(),
        project_id: "demo".into(),
        name: "Disabled board".into(),
        enabled: false,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Board {
            board_id: board.id.clone(),
        },
    });
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), board.id.clone());
    });
    mounted.model.snapshot.set(snapshot.clone());
    mounted.settle();
    assert!(mounted.model.selected_board().is_none());
    assert!(mounted.model.board_columns().is_empty());
    assert!(
        !mounted
            .model
            .task_in_column(&snapshot.issues[0], &snapshot.issues[0].column_id)
    );
    assert!(!snapshot.visible_task(&snapshot.issues[0].id));
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some(&board.name))
    );
    mounted.model.action(Command::CreateTask {
        board_id: board.id.clone(),
        title: "Blocked".into(),
        body: String::new(),
        repository_connection_id: None,
    });
    mounted.model.sync_project();
    assert!(mounted.commands.try_recv().is_err());
    assert_eq!(
        mounted.model.snapshot.get_untracked().memberships,
        snapshot.memberships
    );
    snapshot.connections.clear();
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    assert_eq!(mounted.model.selected_board().unwrap().id, board.id);
}

#[test]
fn imported_task_recovery_marker_is_hidden_and_preserved_when_editing() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let marker = "<!-- relay-operation:publish-1:task:issue-2 -->";
    snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .body = format!("Ordinary task description\n\n{marker}");
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    mounted.rect("Ordinary task description");
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_ref()
            .is_some_and(|l| l.contains("relay-operation:"))
    }));
    mounted.focus("Edit task");
    mounted.key(Key::Enter, false);
    type_in(&mounted, "Task body", "Updated task description");
    mounted.focus("Save task");
    mounted.click("Save task");
    let Command::UpdateTask { body, .. } = mounted.commands.try_recv().unwrap().command else {
        panic!("Expected edit")
    };
    assert_eq!(body, format!("Updated task description\n\n{marker}"));
    assert_eq!(
        crate::projects::task_body(&body),
        "Updated task description"
    );
    assert_eq!(
        crate::projects::task_body("User <!-- ordinary comment --> text"),
        "User <!-- ordinary comment --> text"
    );
    assert_eq!(
        crate::projects::task_body("<!-- relay-operation:unfinished"),
        "<!-- relay-operation:unfinished"
    );
    assert_eq!(crate::projects::preserve_task_markers(&body, &body), body);
}

#[test]
fn director_execution_does_not_consume_worker_capacity_or_block_director_continuation() {
    let mounted = mount(false, 1380.0);
    mounted.model.worker_prompt.set("Continue planning".into());
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults.max_workers = 1;
    snapshot.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Allow);
    let agent_config_id = snapshot.agent_configs[0].id.clone();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.agent_config_id = agent_config_id.clone();
    session.issue_id = Some("issue-2".into());
    session.role = SessionRole::Root;
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread".into()),
        worktree: Some("/server/work".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let director_session = session.id.clone();
    mounted.model.snapshot.set(snapshot.clone());
    mounted.model.worker_director.set(agent_config_id);
    mounted.model.issue.set(Some("issue-2".into()));
    assert_eq!(mounted.model.worker_profile(false).unwrap().1, 0);
    assert!(mounted.model.worker_gate(false).is_ok());
    let mut worker = snapshot.sessions[0].clone();
    worker.id = "active-worker".into();
    worker.role = SessionRole::Worker;
    snapshot.sessions.push(worker);
    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session(director_session);
    assert_eq!(mounted.model.worker_profile(true).unwrap().1, 1);
    assert!(mounted.model.worker_gate(true).is_ok());
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("slots")
    );
}

#[test]
fn session_and_issue_navigation_choose_matching_active_boards_and_keep_current_matches() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let original = snapshot.boards[0].clone();
    let mut second = original.clone();
    second.id = "second-board".into();
    second.name = "Second board".into();
    snapshot.boards.push(second.clone());
    snapshot.memberships.retain(|m| m.issue_id != "issue-2");
    snapshot.memberships.push(BoardMembership {
        board_id: second.id.clone(),
        issue_id: "issue-2".into(),
        column_ids: vec![second.columns[0].id.clone()],
        remote_item_id: None,
    });
    snapshot.sessions[0].issue_id = Some("issue-2".into());
    let session_id = snapshot.sessions[0].id.clone();
    mounted.model.snapshot.set(snapshot.clone());
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_session(session_id);
    assert_eq!(mounted.model.selected_board().unwrap().id, second.id);
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_worker_issue();
    assert_eq!(mounted.model.selected_board().unwrap().id, second.id);
    snapshot.memberships.push(BoardMembership {
        board_id: original.id.clone(),
        issue_id: "issue-2".into(),
        column_ids: vec![original.columns[0].id.clone()],
        remote_item_id: None,
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_worker_issue();
    assert_eq!(mounted.model.selected_board().unwrap().id, original.id);
}

#[test]
fn harness_cards_keep_summary_and_actions_compact_and_responsive() {
    for width in [1380.0, 820.0] {
        let mounted = mount(false, width);
        mounted.model.receive(NetworkState {
            snapshot: live_snapshot(),
            connected: true,
            harnesses: vec![HarnessStatus {
                harness: Harness::ClaudeCode,
                executable: "/server/claude".into(),
                version: Some("2.test".into()),
                state: "ready".into(),
                detail: "Lengthy private diagnostic".into(),
                checked_at: 1,
            }],
            ..Default::default()
        });
        mounted.model.page.set(Page::Settings);
        if width < 1000.0 {
            mounted.model.preferences.update(|p| p.scale = 2.0);
            mounted
                ._scope
                .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        }
        mounted.settle();
        let refresh = mounted.rect("Refresh Claude Code status");
        let details = mounted.rect("Claude Code executable and status details");
        assert!((refresh.origin.y - details.origin.y).abs() < 1.0);
        assert!(details.origin.x + details.size.width <= mounted.size.width);
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Lengthy private diagnostic"))
        );
    }
}

#[test]
fn published_aliases_preserve_selected_board_scope_and_linked_sessions() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let canonical = snapshot.issues[0].id.clone();
    let mut historical = snapshot.issues[0].clone();
    historical.id = "imported-history".into();
    snapshot.issues.push(historical);
    snapshot
        .issue_aliases
        .insert("imported-history".into(), canonical.clone());
    snapshot.sessions[0].issue_id = Some("imported-history".into());
    let director = snapshot.agent_configs[0].id.clone();
    snapshot.agent_configs[0].overrides.scope = Some(AgentScope::Issues {
        issue_ids: vec!["imported-history".into()],
    });
    let mut old_board = snapshot.boards[0].clone();
    let current_board = old_board.id.clone();
    old_board.id = "existing-destination".into();
    snapshot.boards.push(old_board);
    snapshot
        .board_aliases
        .insert("existing-destination".into(), current_board.clone());
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert(
            snapshot.projects[0].id.clone(),
            "existing-destination".into(),
        );
    });
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some(canonical.clone()));
    mounted.model.worker_director.set(director);
    mounted.model.worker_prompt.set("Continue this task".into());
    mounted.settle();
    assert_eq!(mounted.model.selected_board().unwrap().id, current_board);
    assert!(mounted.model.worker_gate(false).is_ok());
    assert!(
        mounted
            .model
            .sessions_for_task(&canonical)
            .iter()
            .any(|s| s.id == snapshot.sessions[0].id)
    );
    mounted.model.open_session(snapshot.sessions[0].id.clone());
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some(canonical.as_str())
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions[0]
            .issue_id
            .as_deref(),
        Some("imported-history")
    );
}

#[test]
fn every_theme_keeps_desktop_text_and_controls_readable() {
    use relay_ui::tokens::{ThemeVariant, contrast};
    for variant in ThemeVariant::ALL {
        let palette = variant.palette();
        let (g, s) = (palette.ground, palette.status);
        let name = variant.name();
        // Hover and pressed are washes; text sits on them over a panel.
        let over = |wash: mosaic::prelude::Color| {
            let [r, gr, b, a] = wash.to_srgb8();
            let [ur, ug, ub, _] = g.panel.to_srgb8();
            let mix = |top: u8, under: u8| {
                (f32::from(top) * f32::from(a) / 255.0
                    + f32::from(under) * (1.0 - f32::from(a) / 255.0))
                    .round() as u8
            };
            mosaic::prelude::Color::from_srgb8(mix(r, ur), mix(gr, ug), mix(b, ub), 255)
        };
        // Desktop controls set text on their hover and pressed fills, and
        // signal words on fields and selected rows.
        for background in [
            g.page,
            g.panel,
            g.panel_raised,
            g.code,
            g.selected,
            over(g.hover),
            over(g.pressed),
        ] {
            for (role, foreground) in [("ink", g.ink), ("muted", g.muted)] {
                let ratio = contrast(foreground, background);
                assert!(
                    ratio >= 4.5,
                    "{name}: {role} on {background:?} is {ratio:.2}"
                );
            }
        }
        for background in [g.page, g.panel, g.code, g.selected] {
            for (role, foreground) in [
                ("failed", s.failed),
                ("needs you", s.needs_you),
                ("live", s.live),
            ] {
                let ratio = contrast(foreground, background);
                assert!(
                    ratio >= 4.5,
                    "{name}: {role} on {background:?} is {ratio:.2}"
                );
            }
            let ring = contrast(g.ink, background);
            assert!(ring >= 3.0, "{name}: focus ring is {ring:.2}");
        }
        let primary = contrast(g.inverse, s.needs_you);
        assert!(
            primary >= 4.5,
            "{name}: primary action text is {primary:.2}"
        );
        assert!(
            contrast(g.selected, g.panel) > 1.0,
            "{name}: selection must show"
        );
    }
}

fn settings_directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("relay-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn high_contrast_settings_are_additive_and_omitted_while_off() {
    use crate::settings::{Preferences, ThemeMode};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(
        &path,
        "mode = \"light\"\nlight_warm = true\ndark_neutral = true\nscale = 1.25\nsidebar_width = 300.0\n\n[selected_boards]\ndemo = \"board-1\"\n",
    )
    .unwrap();
    let mut preferences = Preferences::load(&path).unwrap();
    assert_eq!(preferences.mode, ThemeMode::Light);
    assert!(preferences.legacy.light_warm && preferences.legacy.dark_neutral);
    assert!(!preferences.high_contrast);
    assert_eq!(preferences.scale, 1.25);
    assert_eq!(preferences.sidebar_width, 300.0);
    assert_eq!(preferences.selected_boards["demo"], "board-1");
    preferences.save(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(!written.contains("high_contrast"), "{written}");
    // Older clients still find the families they chose.
    assert!(written.contains("light_palette = \"warm\""), "{written}");
    assert!(written.contains("dark_palette = \"neutral\""), "{written}");
    preferences.high_contrast = true;
    preferences.save(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("light_palette = \"high_contrast\""),
        "{written}"
    );
    assert!(
        written.contains("dark_palette = \"high_contrast\""),
        "{written}"
    );
    let reloaded = Preferences::load(&path).unwrap();
    assert!(reloaded.high_contrast && reloaded.legacy.dark_neutral && reloaded.legacy.light_warm);
    assert_eq!(reloaded, preferences);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn obsolete_palette_settings_load_without_suspending_saves() {
    use crate::settings::{Persistence, open};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    // Files written by the palette-family versions, versioned and not.
    for source in [
        "version = 2\nmode = \"dark\"\nlight_palette = \"paper\"\ndark_palette = \"high_contrast\"\ndark_neutral = true\n",
        "version = 2\nlight_palette = \"warm\"\ndark_palette = \"slate\"\n",
        "mode = \"system\"\nlight_high_contrast = true\ndark_neutral = true\n",
    ] {
        std::fs::write(&path, source).unwrap();
        let (preferences, persistence) = open(&path);
        assert_eq!(persistence, Persistence::Enabled, "{source}");
        preferences.save(&path).unwrap();
        assert_eq!(open(&path), (preferences, Persistence::Enabled), "{source}");
    }
    let (first, _) = {
        std::fs::write(
            &path,
            "version = 2\ndark_palette = \"high_contrast\"\ndark_neutral = true\n",
        )
        .unwrap();
        open(&path)
    };
    assert!(first.high_contrast && first.legacy.dark_neutral);
    // Unknown fields still suspend saving rather than being dropped.
    std::fs::write(&path, "version = 2\naccent = \"pink\"\n").unwrap();
    assert!(matches!(open(&path).1, Persistence::Suspended { .. }));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn backups_never_replace_existing_files() {
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(&path, b"newer = true\n").unwrap();
    let taken = directory.join("settings.toml.unreadable-20000229T000000Z");
    std::fs::write(&taken, b"older backup").unwrap();
    let backup =
        crate::settings::write_backup(&path, b"newer = true\n", "20000229T000000Z").unwrap();
    assert_ne!(backup, taken);
    assert_eq!(std::fs::read(&taken).unwrap(), b"older backup");
    assert_eq!(std::fs::read(&backup).unwrap(), b"newer = true\n");
    assert_eq!(crate::settings::utc_stamp(0), "19700101T000000Z");
    assert_eq!(crate::settings::utc_stamp(951_782_400), "20000229T000000Z");
    assert_eq!(crate::settings::utc_stamp(951_868_799), "20000229T235959Z");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unreadable_settings_are_never_overwritten_until_explicitly_recovered() {
    use crate::settings::{Persistence, Store};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let original = b"mode = \"dark\"\nfuture_setting = 3\n".to_vec();
    std::fs::write(&path, &original).unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    assert!(matches!(persistence, Persistence::Suspended { .. }));

    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted
        .model
        .preferences
        .update(|p| p.sidebar_width = 300.0);
    mounted.settle();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    mounted.key(Key::Character(",".into()), true);
    mounted.rect("Display settings not saved");

    // Retrying a still-invalid file changes nothing.
    mounted.click("Retry reading settings file");
    assert!(matches!(
        mounted.model.settings_store.get_untracked().persistence,
        Persistence::Suspended { .. }
    ));
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.5);
    assert_eq!(std::fs::read(&path).unwrap(), original);

    mounted.click("Back up file and save current settings");
    let store = mounted.model.settings_store.get_untracked();
    assert_eq!(store.persistence, Persistence::Enabled);
    let backup = store.backup.unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let saved = crate::settings::Preferences::load(&path).unwrap();
    assert_eq!(saved.scale, 1.5);
    assert_eq!(saved.sidebar_width, 300.0);

    // Persistence resumes once recovered.
    mounted.model.preferences.update(|p| p.scale = 1.25);
    mounted.settle();
    assert_eq!(
        crate::settings::Preferences::load(&path).unwrap().scale,
        1.25
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_settings_backup_keeps_everything_and_stays_suspended() {
    use crate::settings::{Persistence, Store};
    // A directory where the settings file should be cannot be read for a
    // backup on any platform or user, so recovery must fail without writing.
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep.txt"), b"untouched").unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    assert!(matches!(persistence, Persistence::Suspended { .. }));
    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    crate::settings::recover_by_backup(mounted.model);
    let store = mounted.model.settings_store.get_untracked();
    assert!(matches!(store.persistence, Persistence::Suspended { .. }));
    assert!(store.backup.is_none());
    assert!(path.is_dir());
    assert_eq!(std::fs::read(path.join("keep.txt")).unwrap(), b"untouched");
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn backup_that_cannot_be_created_keeps_the_original_file() {
    use crate::settings::{Persistence, Store};
    use std::os::unix::fs::PermissionsExt;
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let original = b"future_setting = 3\n".to_vec();
    std::fs::write(&path, &original).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Privileged users can still write; there is nothing to verify then.
    let denied = std::fs::write(directory.join("probe"), b"").is_err();
    if denied {
        let (preferences, persistence) = crate::settings::open(&path);
        let mounted = mount(false, 1380.0);
        mounted.model.preferences.set(preferences);
        mounted.model.settings_store.set(Store {
            path: Some(path.clone()),
            persistence,
            backup: None,
        });
        crate::settings::recover_by_backup(mounted.model);
        assert!(matches!(
            mounted.model.settings_store.get_untracked().persistence,
            Persistence::Suspended { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn retry_reading_adopts_a_repaired_settings_file() {
    use crate::settings::{Persistence, Store};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(&path, "broken syntax").unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    std::fs::write(&path, "scale = 1.5\n").unwrap();
    crate::settings::retry_reading(mounted.model);
    assert_eq!(
        mounted.model.settings_store.get_untracked().persistence,
        Persistence::Enabled
    );
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.5);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn usage_split_separates_cached_input_and_survives_bad_reports() {
    use crate::ui::UsageSplit;
    let usage = |input, cached, output| TokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
    };
    let split = UsageSplit::new(&usage(48_213, 31_004, 2_910));
    assert_eq!(
        (split.uncached, split.cached, split.output),
        (17_209, 31_004, 2_910)
    );
    assert!(!split.clamped);
    let [uncached, cached, output] = split.fractions().unwrap();
    assert!((uncached + cached + output - 1.0).abs() < 1e-5);
    assert!(cached > uncached && uncached > output);

    assert_eq!(UsageSplit::new(&usage(0, 0, 0)).fractions(), None);

    let inconsistent = UsageSplit::new(&usage(100, 250, 10));
    assert!(inconsistent.clamped);
    assert_eq!((inconsistent.uncached, inconsistent.cached), (0, 100));

    let huge = UsageSplit::new(&usage(u64::MAX, u64::MAX / 2, u64::MAX));
    for fraction in huge.fractions().unwrap() {
        assert!((0.0..=1.0).contains(&fraction));
    }
    assert_eq!(crate::ui::grouped(1_234_567), "1,234,567");
    assert_eq!(crate::ui::grouped(999), "999");
}

#[test]
fn issue_status_prefers_waiting_and_active_work_then_the_latest_outcome() {
    use crate::adapt::issue_status;
    use relay_ui::prelude::AgentStatus;
    let mut snapshot = demo_snapshot(AgentProfile::default());
    let template = snapshot.sessions[1].clone();
    let session = |id: &str, status: WorkerStatus| {
        let mut session = template.clone();
        session.id = id.into();
        session.fixture = false;
        session.worker = Some(worker_run(status));
        session
    };
    assert_eq!(issue_status(&snapshot, &[]), None);
    let older_failure = session("a", WorkerStatus::Failed);
    let newer_success = session("b", WorkerStatus::Completed);
    assert_eq!(
        issue_status(&snapshot, &[older_failure.clone(), newer_success.clone()]),
        Some(AgentStatus::Done)
    );
    assert_eq!(
        issue_status(&snapshot, &[newer_success.clone(), older_failure.clone()]),
        Some(AgentStatus::Failed)
    );
    let queued = session("c", WorkerStatus::Queued);
    let running = session("d", WorkerStatus::Running);
    assert_eq!(
        issue_status(
            &snapshot,
            &[running.clone(), queued.clone(), newer_success.clone()]
        ),
        Some(AgentStatus::Working)
    );
    assert_eq!(
        issue_status(&snapshot, &[queued.clone(), newer_success.clone()]),
        Some(AgentStatus::Queued)
    );
    snapshot.tool_permissions.push(ToolPermission {
        id: "p".into(),
        session_id: "c".into(),
        run_id: "r".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    assert_eq!(
        issue_status(&snapshot, &[running, queued, newer_success]),
        Some(AgentStatus::Waiting)
    );
}

fn worker_run(status: WorkerStatus) -> WorkerRun {
    WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status,
        thread_id: Some("thread".into()),
        worktree: Some("/repo/worktrees/issue".into()),
        branch: Some("worker/issue".into()),
        base_commit: Some("abc123".into()),
        error: None,
        usage: None,
        changes: None,
    }
}

#[test]
fn session_header_usage_and_execution_mode_use_measured_state() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.usage = Some(TokenUsage {
        input_tokens: 48_213,
        cached_input_tokens: 31_004,
        output_tokens: 2_910,
    });
    session.worker = Some(run);
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let status = |mounted: &Mounted| header_status(mounted);
    assert_eq!(status(&mounted), "Working");
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    mounted.rect("Latest turn usage");
    mounted.rect("Usage meter");

    mounted.focus("Worker approval: Ask");
    mounted.key(Key::Enter, false);
    let command = mounted.commands.try_recv().unwrap().command;
    assert!(matches!(
        command,
        Command::SetWorkerExecution { session_id: ref id, execution: Some(ExecutionSettings { approval: ApprovalMode::Ask }) } if *id == session_id
    ));

    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Queued;
    snapshot.sessions[0].worker.as_mut().unwrap().usage = None;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(status(&mounted), "Queued");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Latest turn usage"))
    );
}

#[test]
fn permission_rows_set_exact_values_for_every_action_by_click_and_keyboard() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let permission = |task: Task| {
        mounted
            .model
            .editor_profile
            .get_untracked()
            .permissions
            .get(&task)
            .copied()
    };
    mounted.focus("Merge permission: Allow");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Allow));
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Ask));
    mounted.key(Key::Home, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Deny));
    mounted.focus("Deploy permission: Ask");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Deploy), Some(Permission::Ask));
    mounted.focus("Implement permission: Deny");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Implement), Some(Permission::Deny));

    let responsible = mounted
        .model
        .editor_profile
        .get_untracked()
        .responsibilities
        .contains(&Task::Merge);
    mounted.focus("Merge responsibility");
    mounted.key(Key::Enter, false);
    assert_ne!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .responsibilities
            .contains(&Task::Merge),
        responsible
    );
}

// Regression for https://github.com/jens-hj/relay/issues/1: accessibility
// bindings must die with the profile controls they update.
#[test]
fn director_options_survive_reopening_after_profile_controls_are_removed() {
    let mounted = mount(false, 1380.0);
    for _ in 0..3 {
        mounted
            .model
            .open_profile(EditTarget::Config("director-main".into()));
        mounted.settle();
        mounted
            .model
            .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
        mounted
            .model
            .modify_profile("responsibilities", |p| p.responsibilities.clear());
        mounted
            .model
            .modify_profile("completion", |p| p.completion.clear());
        mounted.model.modify_profile("scope", |p| {
            p.scope = AgentScope::Issues {
                issue_ids: vec!["issue-1".into()],
            }
        });
        mounted.settle();
        mounted.model.open_root_agent("director-main".into());
        mounted.settle();
        // Reloading defaults changes every toggle after the old page is gone.
        mounted
            .model
            .open_profile(EditTarget::Config("director-review".into()));
        mounted.settle();
        assert_eq!(
            mounted.model.editor.get_untracked(),
            EditTarget::Config("director-review".into())
        );
        mounted.model.open_root_agent("director-main".into());
        mounted.settle();
    }
}

fn has_label(mounted: &Mounted, label: &str) -> bool {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .any(|n| n.label.as_deref() == Some(label))
}

#[test]
fn usage_readout_follows_new_reports_for_the_same_session() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.sessions[0].fixture = false;
    snapshot.sessions[0].issue_id = Some("issue-2".into());
    snapshot.sessions[0].agent_config_id = snapshot.agent_configs[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.usage = Some(TokenUsage {
        input_tokens: 1_000,
        cached_input_tokens: 400,
        output_tokens: 50,
    });
    snapshot.sessions[0].worker = Some(run);
    let session_id = snapshot.sessions[0].id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    assert!(has_label(&mounted, "1,000"));
    assert!(has_label(&mounted, "400"));

    snapshot.sessions[0].worker.as_mut().unwrap().usage = Some(TokenUsage {
        input_tokens: 48_213,
        cached_input_tokens: 31_004,
        output_tokens: 2_910,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "48,213"));
    assert!(has_label(&mounted, "31,004"));
    assert!(has_label(&mounted, "2,910"));
    assert!(!has_label(&mounted, "1,000"));
    mounted.rect("Usage meter");
}

#[test]
fn provenance_names_connections_and_omits_unrecorded_values() {
    let mut snapshot = live_snapshot();
    let mut session = snapshot.sessions[0].clone();
    session.workspaces = vec![SessionWorkspace {
        connection_id: "workspace-project-1".into(),
        path: "/srv/project/workspace".into(),
        repository: false,
        branch: None,
        base_commit: None,
        changes: None,
    }];
    let mut run = worker_run(WorkerStatus::Completed);
    run.branch = None;
    run.base_commit = None;
    session.worker = Some(run);
    snapshot.sessions[0] = session.clone();
    let groups = crate::conversation::provenance(&snapshot, &session, false);
    assert_eq!(groups[0].title, "Project workspace");
    assert_eq!(
        groups[0].rows,
        vec![
            ("Kind", "Directory".to_string()),
            ("Path", "/srv/project/workspace".to_string())
        ]
    );
    let worker = &groups[1];
    assert_eq!(worker.title, "Worker Run");
    assert!(
        worker
            .rows
            .iter()
            .all(|(key, _)| *key != "Branch" && *key != "Base")
    );
    assert!(worker.changes.is_none());
    // Groups keep their identity when recorded values change, so the details
    // pane updates them in place.
    let mut moved = session.clone();
    moved.workspaces[0].path = "/srv/project/other".into();
    moved.worker.as_mut().unwrap().worktree = Some("/srv/worktrees/next".into());
    let keys = |groups: &[crate::conversation::ProvenanceGroup]| {
        groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>()
    };
    let changed = crate::conversation::provenance(&snapshot, &moved, false);
    assert_eq!(keys(&groups), ["workspace-project-1", "worker"]);
    assert_eq!(keys(&changed), keys(&groups));
    assert_ne!(changed, groups);
}

#[test]
fn card_preview_strips_markers_and_collapses_whitespace() {
    let marker = "<!-- relay-operation:publish-1:task:issue-2 -->";
    assert_eq!(
        crate::ui::body_preview(&format!("Short   body\n\nwith lines\n\n{marker}")),
        "Short body with lines"
    );
    assert_eq!(crate::ui::body_preview(marker), "");
}

#[test]
fn board_chrome_aligns_with_the_sidebar_and_frames_cards() {
    let mounted = mount(false, 1600.0);
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    let bottom = |r: Rect| r.origin.y + r.size.height;
    let header = mounted.rect("Page header");
    let brand = mounted.rect("Sidebar header");
    assert!(near(header.size.height, 60.0), "{header:?}");
    assert!(near(bottom(header), bottom(brand)), "{header:?} {brand:?}");
    // Column heads start on the header rule, with no bar in between.
    let columns: Vec<Rect> = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .filter(|n| n.label.as_deref().is_some_and(|l| l.starts_with("Column ")))
        .map(|n| n.rect)
        .collect();
    assert!(!columns.is_empty());
    for column in &columns {
        assert!(near(column.origin.y, bottom(header)), "{column:?}");
        assert!(near(column.size.height, 40.0), "{column:?}");
    }
    // The board summary and the sidebar connection footer share a rule.
    let summary = mounted.rect("Board summary");
    let connection = mounted.rect("Server connection details");
    assert!(
        near(summary.origin.y, connection.origin.y),
        "{summary:?} {connection:?}"
    );
    assert!(near(summary.size.height, 32.0));
    // Card previews are clipped to three laid-out lines.
    for node in mounted.ui.inspection_snapshot().nodes {
        if node.label.as_deref() == Some("Task preview") {
            assert!(node.rect.size.height <= 51.5, "{:?}", node.rect);
        }
    }
    // The inspector opens on its identifier header, which holds Close.
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    let issue = mounted.rect("Issue header");
    let close = mounted.rect("Close issue details");
    assert!(issue.origin.y < 1.0, "{issue:?}");
    assert!(issue.size.height >= 91.5, "{issue:?}");
    assert!(close.origin.y >= issue.origin.y && bottom(close) <= bottom(issue));
    assert!((close.origin.y - issue.origin.y).abs() < 1.0);
    assert!((close.size.height - issue.size.height).abs() < 1.0);
}

#[test]
fn wide_profiles_show_the_action_matrix_beside_the_controls() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let matrix = mounted.rect("Action matrix");
    let harness = mounted.rect("Harness choices");
    assert!(matrix.origin.y + mounted.rect("Deploy permission").size.height < mounted.size.height);
    assert!(harness.origin.x > matrix.origin.x + matrix.size.width - 1.0);
    let codex = mounted.rect("Agent harness: Codex");
    let claude = mounted.rect("Agent harness: Claude Code");
    assert!(
        (codex.size.width + claude.size.width - harness.size.width).abs() <= 1.0,
        "harness={harness:?}, codex={codex:?}, claude={claude:?}"
    );
    assert!(
        (claude.origin.x + claude.size.width - harness.origin.x - harness.size.width).abs() <= 1.0
    );
    assert!(mounted.rect("Deploy permission").origin.y < mounted.size.height);

    let narrow = mount(false, 820.0);
    narrow.model.open_profile(EditTarget::Defaults);
    narrow.settle();
    let matrix = narrow.rect("Action matrix");
    let harness = narrow.rect("Harness choices");
    assert!(harness.origin.y > matrix.origin.y + matrix.size.height - 1.0);
    // Same column: the selector sits inside its module's 12px padding.
    assert!(harness.origin.x >= matrix.origin.x && harness.origin.x <= matrix.origin.x + 13.0);
}

#[test]
fn inverse_controls_show_hover_and_press_in_every_palette() {
    for variant in ThemeVariant::ALL {
        let light = matches!(variant, ThemeVariant::Light | ThemeVariant::LightContrast);
        let mounted = mount(light, 1380.0);
        install_theme(&theme(variant, 1.0));
        mounted
            .model
            .open_profile(EditTarget::Config("director-main".into()));
        mounted.settle();
        let fill = |label: &str| {
            let id = mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(label))
                .unwrap()
                .id;
            mounted
                .ui
                .inspection_details(id)
                .unwrap()
                .attributes
                .into_iter()
                .find(|a| a.name == "fill")
                .map(|a| a.value)
        };
        let pointer = |kind: PointerEventKind, position: Vector2| {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
            mounted.settle();
        };
        // Real pointer events must change the fill and restore it on leaving.
        let hold = |target: &str, filled: &str| {
            let rest = fill(filled);
            let center = mounted.rect(target).center();
            pointer(PointerEventKind::Move, center);
            let hovered = fill(filled);
            assert_ne!(hovered, rest, "{filled} while hovered");
            pointer(PointerEventKind::Down(PointerButton::Primary), center);
            assert_ne!(fill(filled), rest, "{filled} while pressed");
            assert_ne!(fill(filled), hovered, "{filled} press differs from hover");
            let away = Vector2::new(700.0, 880.0);
            pointer(PointerEventKind::Move, away);
            pointer(PointerEventKind::Up(PointerButton::Primary), away);
            assert_eq!(fill(filled), rest, "{filled} after leaving");
            rest
        };
        hold("Save profile", "Save profile");
        hold("Agent harness: Codex", "Agent harness: Codex");
        open_project_menu(&mounted);
        hold("Select project Relay · demo", "Select project Relay · demo");
        open_project_menu(&mounted);
        mounted.click("New Project");
        hold("Create project", "Create project");
        mounted.click("New connection");
    }
}

#[test]
fn fixture_projects_read_as_fixtures_and_local_projects_keep_their_status() {
    let mounted = mount(false, 1380.0);
    let card_states = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|n| {
                n.label.as_deref().is_some_and(|l| {
                    l.starts_with("Open issue #") || l.starts_with("Open local task")
                })
            })
            .map(|n| n.description.clone().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(crate::ui::source_label(mounted.model), "Fixture");
    let fixture_cards = card_states(&mounted);
    assert!(!fixture_cards.is_empty());
    assert!(
        fixture_cards.iter().all(|d| d.starts_with("Fixture ·")),
        "{fixture_cards:?}"
    );
    // A fixture project cannot run, so its summary shows only the task count.
    assert!(has_label(&mounted, "Tasks"));
    assert!(!has_label(&mounted, "Running"));

    let mut snapshot = demo_snapshot(AgentProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.sessions.clear();
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(crate::ui::source_label(mounted.model), "Local board");
    let local_cards = card_states(&mounted);
    assert!(!local_cards.is_empty());
    assert!(
        local_cards
            .iter()
            .all(|d| d.starts_with("Ready to scope ·")),
        "{local_cards:?}"
    );
    for cell in ["Tasks", "Running", "Waiting", "Project Workers"] {
        assert!(has_label(&mounted, cell), "{cell}");
    }
}

#[test]
fn board_actions_menu_closes_on_escape_and_outside_clicks_and_reopens_at_once() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(AgentProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    // Closing waits out the tooltip's hide delay.
    let closed = |mounted: &Mounted| {
        mounted.ui.tick(Duration::from_millis(250));
        mounted.settle();
        !has_label(mounted, "Manage columns")
    };
    mounted.settle();
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    // Escape from inside the menu closes it and returns focus to the trigger.
    mounted.focus("Manage columns");
    mounted.key(Key::Escape, false);
    assert!(!has_label(&mounted, "Manage columns"));
    let focused = mounted.ui.inspection_snapshot();
    let trigger = focused
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Board actions"))
        .unwrap();
    assert_eq!(mounted.ui.focused().map(|e| e.id()), Some(trigger.id));
    // One click reopens it.
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    // A click elsewhere closes it; a click on the trigger toggles it.
    mounted.click("Page header");
    assert!(closed(&mounted), "outside click");
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    mounted.click("Board actions");
    assert!(closed(&mounted), "trigger toggle");
    // Pressing another control closes the menu and still activates it.
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    mounted.click("New Task");
    assert!(closed(&mounted), "another control");
    assert!(has_label(&mounted, "New task title"));
    // Menu items act before the menu closes.
    mounted.click("Board actions");
    mounted.click("Operation History");
    assert!(closed(&mounted));
    assert!(has_label(&mounted, "Completed operations"));
}

#[test]
fn profile_header_counts_real_overrides_and_keeps_its_geometry() {
    let mounted = mount(false, 1380.0);
    mounted
        .model
        .open_profile(EditTarget::Config("director-main".into()));
    mounted.settle();
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    assert!(near(mounted.rect("Profile header").origin.y, 0.0));
    assert!(near(mounted.rect("Profile header").size.height, 74.0));
    assert!(near(mounted.rect("Inheritance").size.height, 54.0));
    assert!(near(mounted.rect("Save bar").size.height, 50.0));
    assert!(has_label(&mounted, "Overrides"));
    let before = crate::ui::overridden_fields(&mounted.model.editor_overrides.get_untracked());
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    mounted.settle();
    let after = crate::ui::overridden_fields(&mounted.model.editor_overrides.get_untracked());
    assert_eq!(
        after.len(),
        before.len() + usize::from(!before.contains(&"Harness"))
    );
    assert!(after.contains(&"Harness"));
    // Project defaults have no overrides to count.
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    assert!(!has_label(&mounted, "Overrides"));
    assert!(has_label(&mounted, "Directors"));
}

#[test]
fn profile_field_headers_contain_their_text_and_controls() {
    for (width, scale) in [(1380.0, 1.0), (1180.0, 1.0), (820.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        mounted
            .model
            .open_profile(EditTarget::Config("director-main".into()));
        for overrides in [false, true] {
            if overrides {
                mounted.model.modify_profile("harness", |_| {});
                mounted.model.modify_profile("execution", |_| {});
                mounted.model.modify_profile("scope", |_| {});
                mounted.model.modify_profile("max_workers", |_| {});
            }
            mounted.settle();
            let snapshot = mounted.ui.inspection_snapshot();
            if let Some(divider) = snapshot
                .nodes
                .iter()
                .find(|n| n.label.as_deref() == Some("Profile column divider"))
            {
                let workers = mounted.rect("Concurrent Workers field header");
                assert!(divider.rect.size.width > 0.0);
                assert!(
                    divider.rect.origin.y + divider.rect.size.height
                        >= workers.origin.y + workers.size.height + relay_ui::tokens::px(40.0)
                            - 1.0,
                    "column divider must extend through the worker count row: {:?}",
                    divider.rect
                );
            }
            for title in [
                "Agent Harness",
                "Execution Approval",
                "Scope",
                "Concurrent Workers",
            ] {
                let label = format!("{title} field header");
                let head = snapshot
                    .nodes
                    .iter()
                    .find(|n| n.label.as_deref() == Some(&label))
                    .unwrap();
                for node in &snapshot.nodes {
                    let mut parent = node.parent;
                    while let Some(id) = parent {
                        if id == head.id {
                            let rect = node.rect;
                            if rect.size.width > 0.0 && rect.size.height > 0.0 {
                                assert!(
                                    rect.origin.x >= head.rect.origin.x - 1.0
                                        && rect.origin.x + rect.size.width
                                            <= head.rect.origin.x + head.rect.size.width + 1.0
                                        && rect.origin.y >= head.rect.origin.y - 1.0
                                        && rect.origin.y + rect.size.height
                                            <= head.rect.origin.y + head.rect.size.height + 1.0,
                                    "{label} at {width}px, scale {scale}, overrides {overrides}: {:?} {rect:?} outside {:?}",
                                    node.label,
                                    head.rect
                                );
                            }
                            break;
                        }
                        parent = snapshot.nodes.iter().find(|n| n.id == id).unwrap().parent;
                    }
                }
            }
        }
    }
}

#[test]
fn live_readout_values_keep_their_container_typography_after_updates() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults.max_workers = 4;
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    // The text leaf showing `value`, with its resolved type attributes.
    let typography = |value: &str| {
        let snapshot = mounted.ui.inspection_snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|n| {
                n.element_name.as_deref() == Some("text") && n.label.as_deref() == Some(value)
            })
            .unwrap_or_else(|| panic!("Missing text: {value}"));
        let details = mounted.ui.inspection_details(node.id).unwrap();
        ["font-size", "font-family", "text-color", "font-weight"].map(|name| {
            details
                .attributes
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.value.clone())
                .unwrap_or_default()
        })
    };
    let before = typography("0 / 4 active");
    assert_eq!(before[0], "12.5");
    assert_eq!(before[1], "Monospace");
    snapshot.revision += 1;
    snapshot.projects[0].defaults.max_workers = 5;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(typography("0 / 5 active"), before);
}

#[test]
fn profile_fields_remain_reachable_at_double_scale_without_rebuilding_the_draft() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    mounted
        .model
        .open_profile(EditTarget::Config("director-main".into()));
    mounted.settle();
    type_in(
        &mounted,
        "Custom name (optional)",
        "Retained director draft",
    );
    mounted.focus("Deploy permission: Allow");
    let control = mounted.rect("Deploy permission: Allow");
    assert!(
        control.origin.x >= 0.0
            && control.origin.x + control.size.width <= mounted.size.width + 1.0,
        "{control:?}"
    );
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().permissions[&Task::Deploy],
        Permission::Allow
    );
    mounted.focus("Increase worker limit");
    mounted.key(Key::Enter, false);
    assert!(
        mounted
            .model
            .editor_overrides
            .get_untracked()
            .max_workers
            .is_some()
    );
    for label in [
        "Increase worker limit",
        "Inherit max_workers",
        "Scope: Whole project",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= mounted.size.width + 1.0,
            "{label}: {rect:?}"
        );
    }
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    type_in(&mounted, "Profile TOML", "retained TOML draft");
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.toml.get_untracked(), "retained TOML draft");
    assert_eq!(
        mounted.model.editor_name.get_untracked(),
        "Retained director draft"
    );
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.toml.get_untracked(),
        mounted.model.editor_profile.get_untracked().to_toml()
    );
    assert_ne!(mounted.model.toml.get_untracked(), "retained TOML draft");
}

#[test]
fn profile_scope_reacts_to_publication_and_harness_choice_reports_actual_availability() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let issue_id = snapshot.issues[0].id.clone();
    snapshot.issues[0].reference = None;
    snapshot.issues[0].title = "Local task title".into();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted
        .model
        .open_profile(EditTarget::Config("director-main".into()));
    mounted.settle();
    assert!(has_label(&mounted, "Scope Local task title"));
    type_in(&mounted, "Custom name (optional)", "Unsubmitted edit");
    let task = snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == issue_id)
        .unwrap();
    task.title = "Published task title".into();
    task.reference = Some(IssueRef {
        provider: Provider::Github,
        repository: "team/repo".into(),
        number: 456,
        url: "https://github.com/team/repo/issues/456".into(),
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.harnesses.set(vec![HarnessStatus {
        harness: Harness::ClaudeCode,
        executable: "claude".into(),
        version: None,
        state: "missing".into(),
        detail: "Not installed on this server".into(),
        checked_at: 1,
    }]);
    mounted.settle();
    assert!(!has_label(&mounted, "Scope Local task title"));
    assert!(has_label(&mounted, "Scope #456"));
    let chooser = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some("Agent harness: Claude Code"))
        .unwrap();
    assert_eq!(chooser.description.as_deref(), Some("Not installed"));
    mounted.click("Agent harness: Claude Code");
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
    assert_eq!(
        mounted.model.editor_overrides.get_untracked().harness,
        Some(Harness::ClaudeCode)
    );
    assert_eq!(
        mounted.model.editor_name.get_untracked(),
        "Unsubmitted edit"
    );
    mounted.focus("Agent harness: Claude Code");
    mounted.key(Key::Home, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::Codex
    );
    assert_eq!(
        mounted.ui.focused().unwrap().id(),
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| n.label.as_deref() == Some("Agent harness: Codex"))
            .unwrap()
            .id
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::Codex
    );
    mounted.key(Key::End, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
}

#[test]
fn new_task_draft_survives_closing_and_reopening_the_form() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(AgentProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    mounted.click("New Task");
    type_in(&mounted, "New task title", "Kept title");
    type_in(&mounted, "New task body", "Kept body");
    mounted.click("New Task");
    assert!(!has_label(&mounted, "New task title"));
    mounted.click("New Task");
    mounted.click("Create task");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&request.command, Command::CreateTask { title, body, .. } if title == "Kept title" && body == "Kept body"),
        "{:?}",
        request.command
    );
}

#[test]
fn wide_sessions_turn_narrow_at_double_scale_without_rebuilding_the_draft() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    session.worker = Some(worker_run(WorkerStatus::Running));
    let session_id = session.id.clone();
    snapshot.tool_permissions.push(ToolPermission {
        id: "p".into(),
        session_id: session_id.clone(),
        run_id: "r".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("kept draft".into()));
    mounted.settle();
    let focused = mounted.ui.focused().map(|e| e.id());
    // Doubling the scale switches the conversation to its narrow layout.
    mounted.model.preferences.update(|p| p.scale = 2.0);
    mounted.settle();
    assert_eq!(
        mounted.ui.focused().map(|e| e.id()),
        focused,
        "draft surface kept"
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &session_id)),
        "kept draft"
    );
    for label in ["Approve", "Reject", "Send message", "Session actions"] {
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= 1381.0,
            "{label}: {rect:?}"
        );
    }
    // Stop moves into the session actions when the header is narrow.
    assert!(!has_label(&mounted, "Stop"));
    mounted.click("Session actions");
    assert!(has_label(&mounted, "Stop worker"));
}

/// Pinned Mosaic `text_dyn_styled` binds a live text leaf's content and style
/// in separate effects. Typography from leaf attributes, leaf classes and
/// styled parent containers stays the same at mount, after content updates
/// and after a theme switch.
#[test]
fn live_text_typography_survives_content_and_theme_updates() {
    use relay_ui::prelude::{Label, LabelProps, TextRole};
    use relay_ui::tokens::ground;
    mosaic::core::builtins::install();
    install_theme(&theme(ThemeVariant::Dark, 1.0));
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    let mut fonts = FontContext::embedded_only();
    relay_ui::fonts::install(&mut fonts);
    ui.set_fonts(fonts);
    let value = State::new("one".to_string());
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&view! {
            col font-size:20px {
                row font-size:13px {
                    text {format!("container {}", value.get())}
                }
                text font-size:13px {format!("leaf {}", value.get())}
                Label text:(Derived::new(move || format!("class {}", value.get())))
                    role:(TextRole::Label)
                row font-size:24px {
                    Label text:(Derived::new(move || format!("parent class {}", value.get())))
                        role:(TextRole::Label)
                }
                text font-color:ground.muted {format!("leaf color {}", value.get())}
                row font-color:ground.muted {
                    text {format!("container color {}", value.get())}
                }
            }
        });
    });
    let attribute = |label: &str, name: &str| {
        flush();
        ui.frame(Size::new(400.0, 300.0), 1.0);
        let snapshot = ui.inspection_snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing text: {label}"));
        ui.inspection_details(node.id)
            .unwrap()
            .attributes
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.value)
            .unwrap_or_default()
    };
    let size = |label: &str| attribute(label, "font-size");
    let colors = |n: &str| {
        [
            attribute(&format!("leaf color {n}"), "text-color"),
            attribute(&format!("container color {n}"), "text-color"),
        ]
    };
    let read = |n: &str| {
        [
            size(&format!("container {n}")),
            size(&format!("leaf {n}")),
            size(&format!("class {n}")),
            size(&format!("parent class {n}")),
        ]
    };
    let initial = read("one");
    let initial_colors = colors("one");
    value.set("two".into());
    let updated = read("two");
    let updated_colors = colors("two");
    install_theme(&theme(ThemeVariant::Light, 1.0));
    let themed = read("two");
    let themed_colors = colors("two");
    assert_eq!(initial, ["13.0", "13.0", "11.5", "11.5"].map(String::from));
    assert_eq!(updated, initial, "content update");
    assert_eq!(themed, initial, "theme switch");
    assert_eq!(updated_colors, initial_colors, "content update");
    assert_eq!(themed_colors, initial_colors, "theme switch");
}

/// A focused control must also survive its local scroll clip, not just have a
/// rectangle inside the window. Hit testing uses Mosaic's real clip chain.
fn assert_control_hit(mounted: &Mounted, label: &str) {
    mounted.focus(label);
    let snapshot = mounted.ui.inspection_snapshot();
    let target = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some(label))
        .unwrap();
    let rect = target.rect;
    assert!(
        rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= mounted.size.width + 1.0,
        "{label}: {rect:?}"
    );
    let mut hit = mounted.ui.hit_test(rect.center()).map(|e| e.id());
    let first = hit
        .and_then(|id| snapshot.nodes.iter().find(|n| n.id == id))
        .map(|n| (n.label.clone(), n.rect));
    while let Some(id) = hit {
        if id == target.id {
            return;
        }
        hit = snapshot
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.parent);
    }
    panic!("Focused control clipped or obscured: {label}: {rect:?}, hit {first:?}");
}

#[test]
fn bounded_session_disclosures_keep_controls_reachable_and_draft_mounted() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    let mut worker = worker_run(WorkerStatus::Running);
    worker.error = Some("A long recorded diagnostic. ".repeat(80));
    session.worker = Some(worker);
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Draft text"))
        })
        .unwrap();
    mounted.focus(draft.label.as_deref().unwrap());
    mounted.ui.dispatch_ime(ImeEvent::Commit(
        "Draft retained through disclosures".into(),
    ));
    mounted.settle();
    mounted.model.buffer.update(|b| {
        b.approval_needed = true;
        let doc = b.documents.get_mut(&session_id).unwrap();
        doc.error = "Draft save failed with a long diagnostic. ".repeat(80);
        doc.recovery.push(doc.parts.clone());
    });
    mounted.settle();
    mounted.click("Session actions");
    assert_control_hit(&mounted, "Session usage and provenance");
    mounted.key(Key::Enter, false);
    for label in [
        "Stop worker",
        "Inherit worker execution settings",
        "Load shared draft",
        "Restore local draft",
        "Retry draft save",
        "Approve this turn and send",
        "Send message",
    ] {
        assert_control_hit(&mounted, label);
    }
    assert!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.id == draft.id),
        "editor remains mounted"
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &session_id)),
        "Draft retained through disclosures"
    );
}

#[test]
fn bounded_worker_setup_scrolls_long_choices_without_losing_prompt() {
    for (width, scale) in [(1600.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        let mut snapshot = local_project_snapshot();
        for i in 0..24 {
            let mut director = snapshot.agent_configs[0].clone();
            director.id = format!("director-extra-{i}");
            director.name = format!("Agent {i}");
            snapshot.agent_configs.push(director);
            snapshot.connections.push(ProjectConnection {
                id: format!("directory-{i}"),
                project_id: "demo".into(),
                name: format!("Directory {i}"),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Directory {
                    path: format!("/server/{i}"),
                },
            });
        }
        mounted.model.snapshot.set(snapshot);
        mounted.model.issue.set(Some("issue-2".into()));
        mounted.settle();
        mounted
            .model
            .worker_prompt
            .set("Retained worker prompt".into());
        assert_control_hit(&mounted, "Choose director and resources");
        mounted.key(Key::Enter, false);
        assert_control_hit(&mounted, "Choose session resources");
        mounted.key(Key::Enter, false);
        for label in [
            "Workspace resource Directory 23",
            "Worker prompt",
            "Start worker",
        ] {
            assert_control_hit(&mounted, label);
        }
        assert_eq!(
            mounted.model.worker_prompt.get_untracked(),
            "Retained worker prompt"
        );
    }
}

#[test]
fn bounded_operation_history_exposes_recorded_results_and_keeps_close_reachable() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    let mut snapshot = local_project_snapshot();
    for i in 0..50 {
        snapshot.operations.push(ProjectOperation {
            id: format!("operation-{i}"),
            project_id: "demo".into(),
            kind: OperationKind::CreateTask {
                board_id: "board-demo".into(),
                issue_id: "issue-2".into(),
            },
            state: OperationState::Completed,
            error: None,
            results: [(
                "issue/issue-2".into(),
                serde_json::to_string(&IssueRef {
                    provider: Provider::Github,
                    repository: "team/repo".into(),
                    number: 456,
                    url: "https://github.com/team/repo/issues/456".into(),
                })
                .unwrap(),
            )]
            .into_iter()
            .collect(),
        });
    }
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    mounted.click("Board actions");
    assert_control_hit(&mounted, "Operation History");
    mounted.key(Key::Enter, false);
    assert_control_hit(&mounted, "Operation details operation-49");
    mounted.key(Key::Enter, false);
    assert!(has_label(
        &mounted,
        "https://github.com/team/repo/issues/456"
    ));
    assert!(has_label(&mounted, "Operation: operation-49"));
    assert_control_hit(&mounted, "Close operation history");
    mounted.key(Key::Enter, false);
    assert!(!has_label(&mounted, "Operation details operation-49"));
    assert_control_hit(&mounted, "New Task");
}

#[test]
fn page_header_eyebrow_follows_navigation_between_shared_header_pages() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(AgentProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    // The eyebrow is the first text in the page header.
    let eyebrow = |mounted: &Mounted| {
        let snapshot = mounted.ui.inspection_snapshot();
        let header = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Page header"))
            .unwrap()
            .rect;
        snapshot
            .nodes
            .iter()
            .filter(|n| {
                n.element_name.as_deref() == Some("text") && header.contains(n.rect.center())
            })
            .min_by(|a, b| a.rect.origin.y.total_cmp(&b.rect.origin.y))
            .and_then(|n| n.label.clone())
            .unwrap_or_default()
    };
    for (page, expected) in [
        (Page::Settings, "Relay"),
        (Page::Connections, "Project Connections"),
        (Page::Publish, "Publish Board"),
        (Page::NewProject, "Relay"),
    ] {
        mounted.model.page.set(page);
        mounted.settle();
        assert_eq!(eyebrow(&mounted), expected, "{page:?}");
    }
}

#[test]
fn conversation_tracks_constrain_recorded_text_and_draft_to_the_reading_width() {
    let mounted = mount(false, 1600.0);
    let mut snapshot = live_snapshot();
    let session_id = snapshot.sessions[0].id.clone();
    snapshot.messages.push(Message {
        id: "reading-width".into(),
        session_id: session_id.clone(),
        author: "Codex".into(),
        sender: Sender::User,
        kind: "agent_message".into(),
        body: "A recorded paragraph should wrap inside the reading column. ".repeat(10),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    let text = mounted.rect("Message reading-width");
    let column = mounted.rect("Transcript");
    // 848px reading column less the 128px speaker gutter.
    assert!(column.size.width <= 848.0 + 1.0, "{column:?}");
    assert!(text.size.width <= 720.0 + 1.0, "{text:?}");
    assert!(text.size.height > 40.0, "long text must wrap: {text:?}");
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Draft text"))
        })
        .unwrap();
    // The composer spans the same reading column, its draft right of the
    // ledger.
    assert!(
        draft.rect.origin.x >= column.origin.x
            && draft.rect.origin.x + draft.rect.size.width
                <= column.origin.x + column.size.width + 1.0,
        "draft {:?} within the reading column {column:?}",
        draft.rect
    );
    assert!(
        (draft.rect.origin.x - text.origin.x - 16.0).abs() < 1.0,
        "draft text must align with the message body padding: {:?} {text:?}",
        draft.rect
    );
    mounted.click("Collapse agent settings");
    let folded = mounted.rect("Next message");
    assert!((folded.origin.x - draft.rect.origin.x).abs() < 1.0);
    for action in ["Send message", "Attach file"] {
        let button = mounted.rect(action);
        assert!(button.origin.x >= folded.origin.x + folded.size.width);
    }
}

#[test]
fn clicking_below_the_draft_focuses_its_end_across_the_buffer_width() {
    for width in [820.0, 1600.0] {
        let mounted = mount(false, width);
        buffer_worker(&mounted);
        mounted.settle();
        let draft = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| {
                n.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("Draft text"))
            })
            .unwrap();
        let label = draft.label.unwrap();
        // The empty draft is reachable without hitting its text-height field.
        // Across the composer's message area, left of the draft to its end.
        for x in [
            draft.rect.origin.x - 5.0,
            draft.rect.origin.x + draft.rect.size.width - 10.0,
        ] {
            mounted.focus("Session actions");
            let rect = mounted.rect(&label);
            let position = Vector2::new(x, rect.origin.y + rect.size.height + 16.0);
            for kind in [
                PointerEventKind::Down(PointerButton::Primary),
                PointerEventKind::Up(PointerButton::Primary),
            ] {
                mounted.ui.dispatch_pointer(PointerEvent {
                    kind,
                    position,
                    pointer_type: PointerType::Mouse,
                    modifiers: Modifiers::default(),
                    timestamp: Duration::ZERO,
                });
            }
            mounted.settle();
            assert_eq!(mounted.ui.focused().unwrap().id(), draft.id);
        }
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit("First line\nLast line".into()));
        mounted.settle();
        mounted.focus(&label);
        mounted.key(Key::Home, true);
        mounted.focus("Session actions");
        // The draft is scrollable now; reveal its blank space before clicking it.
        let rect = mounted.rect(&label);
        let viewport = mounted.rect("Transcript viewport");
        if rect.max_y() + 20.0 > viewport.max_y() {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind: PointerEventKind::Wheel {
                    delta: Vector2::new(0.0, -(rect.max_y() + 40.0 - viewport.max_y())),
                },
                position: viewport.center(),
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
            mounted.settle();
        }
        let rect = mounted.rect(&label);
        let position = Vector2::new(
            rect.origin.x + rect.size.width - 10.0,
            rect.origin.y + rect.size.height + 16.0,
        );
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        mounted.settle();
        assert_eq!(
            mounted.ui.focused().map(|field| field.id()),
            Some(draft.id),
            "blank draft click at {position:?}, field {:?}, viewport {:?}",
            mounted.rect(&label),
            mounted.rect("Transcript viewport")
        );
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit(" appended".into()));
        mounted.settle();
        assert_eq!(
            plain_text(&crate::buffer::parts(
                mounted.model,
                &mounted.model.session.get_untracked()
            )),
            "First line\nLast line appended"
        );
    }
}

#[test]
fn display_settings_write_sparse_overrides_and_reset_independently() {
    use crate::settings::{Preferences, Setting, ThemeMode};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let mut p = Preferences::default();
    p.save(&path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().trim(),
        "version = 2"
    );
    p.mode = ThemeMode::Light;
    p.high_contrast = true;
    p.scale = 1.6;
    p.sidebar_width = 310.0;
    p.selected_boards.insert("project".into(), "board".into());
    p.save(&path).unwrap();
    assert_eq!(Preferences::load(&path).unwrap(), p);
    for setting in [Setting::Mode, Setting::Contrast, Setting::Scale] {
        assert!(p.overridden(setting));
        p.reset(setting);
        assert!(!p.overridden(setting));
    }
    p.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for key in ["mode", "light_palette", "dark_palette", "scale"] {
        assert!(!text.contains(&format!("{key} =")), "{text}");
    }
    let loaded = Preferences::load(&path).unwrap();
    assert_eq!(loaded.sidebar_width, 310.0);
    assert_eq!(loaded.selected_boards["project"], "board");
    std::fs::write(&path, "version = 99\nscale = 1.2\n").unwrap();
    assert!(Preferences::load(&path).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "version = 99\nscale = 1.2\n"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn direct_project_connections_and_header_actions_use_full_cells() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    mounted.click("Toggle project Relay · demo");
    mounted.click("Project Connections for Relay · demo");
    assert_eq!(mounted.model.page.get_untracked(), Page::Connections);
    mounted.model.page.set(Page::Board);
    mounted.settle();
    let new = mounted.rect("New Task");
    let more = mounted.rect("Board actions");
    assert_eq!(new.origin.y, more.origin.y);
    assert_eq!(new.size.height, more.size.height);
    assert!(more.size.height > 34.0);
    open_project_menu(&mounted);
    let tree = mounted.rect("Project Connections for Relay · demo");
    assert_eq!(tree.size.width, tree.size.height);
    let nodes = mounted.ui.inspection_snapshot();
    let button = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Project Connections for Relay · demo"))
        .unwrap();
    let icon = nodes
        .nodes
        .iter()
        .find(|n| n.parent == Some(button.id))
        .unwrap();
    assert!((icon.rect.center().x - tree.center().x).abs() < 0.1);
    assert!((icon.rect.center().y - tree.center().y).abs() < 0.1);
    // A card with linked sessions counts them in its foot, inside the card.
    let card = nodes
        .nodes
        .iter()
        .find(|n| {
            n.description.as_deref().is_some_and(|d| {
                d.ends_with("linked sessions") && !d.ends_with(" 0 linked sessions")
            })
        })
        .unwrap();
    let count = nodes
        .nodes
        .iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|l| l.ends_with(" session") || l.ends_with(" sessions"))
        })
        .unwrap();
    assert!(
        card.rect.contains(count.rect.center()),
        "{card:?} {count:?}"
    );
    mounted
        .model
        .snapshot
        .set(demo_snapshot(AgentProfile::default()));
    mounted.settle();
    mounted.size = Size::new(820.0, 900.0);
    mounted.settle();
    mounted.size = Size::new(1600.0, 900.0);
    mounted.settle();
    assert_eq!(
        mounted.rect("Source").origin.x + mounted.rect("Source").size.width,
        mounted.rect("Board actions").origin.x
    );
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    let nodes = mounted.ui.inspection_snapshot();
    let new = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("New Task"))
        .unwrap();
    let details = mounted.ui.inspection_details(new.id).unwrap();
    let expected = mosaic::render::PaintSpec::solid(mosaic::core::theme::color(
        relay_ui::tokens::ground.selected,
    ));
    assert_eq!(
        details
            .attributes
            .iter()
            .find(|a| a.name == "fill")
            .unwrap()
            .value,
        format!("{expected:?}")
    );
}

#[test]
fn agent_sent_prompts_name_their_sender_by_tree_relationship_and_link_to_it() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for session in &mut snapshot.sessions {
        match session.id.as_str() {
            "session-plan" => session.title = "hi".into(),
            "session-worker" => {
                session.title = "Markdown crate".into();
                session.parent_session_id = Some("session-plan".into());
            }
            "session-review" => {
                session.title = "Disk usage".into();
                session.parent_session_id = Some("session-plan".into());
            }
            _ => {}
        }
    }
    let prompt = |id: &str, session: &str, sender: Sender| Message {
        id: id.into(),
        session_id: session.into(),
        author: "You".into(),
        sender,
        kind: "prompt".into(),
        body: format!("Body {id}"),
        parts: vec![],
        tool: None,
    };
    let agent = |id: &str| Sender::Agent {
        session_id: id.into(),
    };
    snapshot.messages = vec![
        prompt("from-user", "session-worker", Sender::User),
        prompt("from-parent", "session-worker", agent("session-plan")),
        prompt("from-sibling", "session-worker", agent("session-review")),
        prompt("from-deleted", "session-worker", agent("session-deleted")),
        prompt("from-child", "session-plan", agent("session-worker")),
    ];
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-worker".into());
    mounted.settle();
    for label in ["You", "Parent · hi", "Sibling · Disk usage", "Agent"] {
        assert!(has_label(&mounted, label), "{label}");
    }
    // The linked name keeps the plain label's place and size in the gutter.
    let text = |label: &str| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .rfind(|n| n.label.as_deref() == Some(label))
            .unwrap()
            .rect
    };
    let (user, parent) = (text("You"), text("Parent · hi"));
    assert_eq!(user.origin.x, parent.origin.x);
    assert!(parent.size.width > 0.0);
    assert_eq!(user.size.height, parent.size.height);
    mounted.click("Parent · hi");
    mounted.settle();
    assert_eq!(mounted.model.session.get_untracked(), "session-plan");
    assert!(has_label(&mounted, "Child · Markdown crate"));
    mounted.click("Child · Markdown crate");
    mounted.settle();
    assert_eq!(mounted.model.session.get_untracked(), "session-worker");
    // A deleted sender has no session to open.
    mounted.click("Agent");
    mounted.settle();
    assert_eq!(mounted.model.session.get_untracked(), "session-worker");
}

#[test]
fn relay_notice_prompts_keep_the_relay_label() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let worker = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == "session-worker")
        .unwrap();
    worker.parent_session_id = Some("session-plan".into());
    worker.title = "Markdown crate".into();
    let notice = |sender: Sender| Message {
        id: "prompt-relay-notice-1".into(),
        session_id: "session-plan".into(),
        author: "Relay".into(),
        sender,
        kind: "prompt".into(),
        body: "Markdown crate ended without a result".into(),
        parts: vec![],
        tool: None,
    };
    for sender in [
        Sender::User,
        Sender::Agent {
            session_id: "session-worker".into(),
        },
    ] {
        snapshot.messages = vec![notice(sender)];
        mounted.model.snapshot.set(snapshot.clone());
        mounted.model.open_session("session-plan".into());
        mounted.settle();
        assert!(has_label(&mounted, "Relay"));
        assert!(!has_label(&mounted, "You"));
        assert!(!has_label(&mounted, "Child · Markdown crate"));
    }
}

#[test]
fn transcript_sender_and_content_share_the_first_glyph_baseline() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.retain(|m| m.session_id == "session-plan");
    snapshot.messages.truncate(1);
    snapshot.messages[0].kind = "message".into();
    snapshot.messages[0].author = "Agent".into();
    snapshot.messages[0].body = "Agent output".into();
    snapshot.messages[0].parts.clear();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let body = mounted.rect("Message m1");
    let glyphs: Vec<_> = mounted
        .ui
        .scene()
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            mosaic::render::PaintCmd::Glyphs(run) => run.glyphs.first().map(|g| (g.dest, g.key)),
            _ => None,
        })
        .filter(|(r, _)| {
            r.origin.y >= body.origin.y - 10.0
                && r.origin.y < body.origin.y + body.size.height
                && r.origin.x > mounted.rect("Sidebar").max_x()
        })
        .collect();
    // The speaker in the gutter and the first line of the message sit on
    // one line, though the speaker is set smaller.
    assert_eq!(glyphs.len(), 2, "{glyphs:?}");
    assert!(
        (glyphs[0].0.origin.y - glyphs[1].0.origin.y).abs() < 6.0,
        "{glyphs:?}"
    );
    mounted.model.snapshot.update(|s| {
        s.messages[0].author = "Personal name".into();
        s.messages[0].kind = "prompt".into();
        s.messages[0].body = "You asked for this".into();
    });
    mounted.settle();
    assert!(has_label(&mounted, "You"));
    assert!(!has_label(&mounted, "Personal name"));
    assert_eq!(
        mounted.model.snapshot.get_untracked().messages[0].author,
        "Personal name"
    );
}

#[test]
fn removed_settings_selectors_do_not_react_to_later_preferences() {
    let mounted = mount(false, 1380.0);
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Theme: Light");
    mounted.model.page.set(Page::Board);
    mounted.settle();
    mounted.model.preferences.update(|p| {
        p.mode = crate::settings::ThemeMode::Dark;
        p.scale = 1.3;
    });
    mounted.settle();
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Theme: System");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
}

#[test]
fn first_director_message_uses_inline_assets_shortcuts_and_retains_edits_on_acknowledgement() {
    let mut mounted = mount(false, 600.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.snapshot.update(|s| s.sessions.clear());
    mounted.model.open_root_agent("director-main".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    type_in(&mounted, &label, "Before λ");
    mounted.key(Key::Enter, false);
    mounted.ui.set_clipboard_text("After");
    mounted.key(Key::Character("v".into()), true);
    let session = mounted.model.session.get_untracked();
    let mut parts = crate::buffer::parts(mounted.model, &session);
    assert_eq!(plain_text(&parts), "Before λ\nAfter");
    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(requests));
    mounted.model.buffer.update(|s| {
        s.initialized = true;
        s.connected = true;
    });
    crate::buffer::flush(mounted.model);
    assert!(received.try_recv().is_err());
    mounted.model.buffer_requests.set(None);
    let asset = Asset {
        id: uuid::Uuid::new_v4().to_string(),
        name: "first.png".into(),
        media_type: "image/png".into(),
        size: 1,
    };
    parts.push(Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Asset {
            asset: asset.clone(),
        },
    });
    crate::buffer::edit(mounted.model, &session, parts.clone());
    mounted.model.buffer.update(|s| {
        s.uploads.insert(asset.id.clone(), asset.clone());
    });
    mounted.settle();
    mounted.rect("Preview first.png");
    mounted.click("Send message");
    assert!(mounted.commands.try_recv().is_err());
    mounted.model.buffer.update(|s| {
        s.uploads.clear();
    });
    mounted.settle();
    mounted.focus(&label);
    mounted.key(Key::Enter, true);
    let command = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(command.command, Command::StartRootAgent { parts: ref sent, approve_implementation: false, .. } if sent == &parts)
    );
    // Typing while the request is in flight must survive switching to the real session.
    let mut later = parts.clone();
    later.push(Part::text("more"));
    crate::buffer::edit(mounted.model, &session, later.clone());
    crate::buffer::acknowledge_first(
        mounted.model,
        &Draft {
            session_id: session.clone(),
            revision: 0,
            parts,
        },
        "session-created",
    );
    assert_eq!(
        crate::buffer::parts(mounted.model, "session-created"),
        later
    );
    assert!(crate::buffer::parts(mounted.model, &session).is_empty());
}

#[test]
fn platform_chrome_keeps_native_macos_buttons_and_client_controls_elsewhere() {
    let config = crate::window_chrome::window_config();
    assert_eq!(config.size, Size::new(1380.0, 900.0));
    #[cfg(target_os = "macos")]
    {
        assert_eq!(config.decorations, WindowDecorations::Native);
        assert!(config.macos.transparent_titlebar);
        assert!(config.macos.title_hidden);
        assert!(!config.macos.hidden_titlebar);
        assert!(config.macos.full_size_content_view);
        assert!(config.macos.traffic_lights_visible);
        assert_eq!(config.macos.traffic_light_inset, Some(28.0));
    }
    #[cfg(not(target_os = "macos"))]
    assert_eq!(config.decorations, WindowDecorations::Client);
}

#[test]
fn merged_headers_keep_window_controls_at_the_right_across_pages_and_scales() {
    for (light, width, scale) in [
        (false, 1380.0, 1.0),
        (true, 820.0, 1.0),
        (false, 760.0, 2.0),
    ] {
        let mounted = mount(light, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        for page in [
            Page::Board,
            Page::Sessions,
            Page::Profiles,
            Page::Settings,
            Page::NewProject,
            Page::Connections,
            Page::Publish,
            Page::AgentStart,
        ] {
            mounted.model.page.set(page);
            mounted.model.notice.set("A connection notice".into());
            mounted.settle();
            #[cfg(not(target_os = "macos"))]
            {
                for label in [
                    "Minimize window",
                    "Maximize or restore window",
                    "Close window",
                ] {
                    assert_control_hit(&mounted, label);
                    assert!(
                        mounted.rect(label).origin.y.abs() < 1.0,
                        "{page:?}: {label}"
                    );
                }
                let close = mounted.rect("Close window");
                assert!(
                    (close.origin.x + close.size.width - width).abs() < 1.0,
                    "{page:?}: {close:?}"
                );
                assert_eq!(
                    mounted
                        .ui
                        .inspection_snapshot()
                        .nodes
                        .iter()
                        .filter(|n| n.label.as_deref() == Some("Close window"))
                        .count(),
                    1
                );
            }
            #[cfg(target_os = "macos")]
            {
                assert!(!has_label(&mounted, "Close window"));
                assert_eq!(mounted.rect("Sidebar header").size.height, 56.0);
            }
        }
        mounted.model.page.set(Page::Board);
        mounted.model.issue.set(Some("issue-1".into()));
        mounted.settle();
        #[cfg(not(target_os = "macos"))]
        {
            assert_control_hit(&mounted, "Close window");
            let close = mounted.rect("Close window");
            assert!(close.origin.y.abs() < 1.0);
            assert!((close.origin.x + close.size.width - width).abs() < 1.0);
            assert_eq!(
                mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .iter()
                    .filter(|n| n.label.as_deref() == Some("Close window"))
                    .count(),
                1
            );
        }
        mounted.model.receive(NetworkState::default());
        mounted.model.snapshot.set(Snapshot::default());
        mounted.settle();
        #[cfg(not(target_os = "macos"))]
        assert_control_hit(&mounted, "Close window");
    }
}

#[test]
fn header_drag_targets_include_text_and_gaps_but_exclude_interactive_descendants() {
    let mounted = mount(false, 1380.0);
    let draggable = |header: &str, position: Vector2| {
        let nodes = mounted.ui.inspection_snapshot().nodes;
        let header = nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(header))
            .unwrap()
            .id;
        let target = mounted.ui.hit_test(position).unwrap().id();
        crate::window_chrome::is_drag_target(&nodes, target, header)
    };
    // The sidebar head is filled by its cells; page and session headers drag.
    assert!(!draggable(
        "Sidebar header",
        mounted.rect("Toggle project Relay · demo").center()
    ));
    let header = mounted.rect("Page header");
    for offset in [10.0, 100.0, 240.0] {
        assert!(draggable(
            "Page header",
            header.origin + Vector2::new(offset, 20.0)
        ));
    }
    assert!(!draggable(
        "Page header",
        mounted.rect("Open command palette").center()
    ));
    assert!(!draggable(
        "Page header",
        mounted.rect("Board actions").center()
    ));
    #[cfg(not(target_os = "macos"))]
    assert!(!draggable(
        "Page header",
        mounted.rect("Close window").center()
    ));
    mounted.click("Open command palette");
    assert!(mounted.model.palette.get_untracked());
    mounted.model.palette.set(false);
    mounted
        .model
        .open_profile(EditTarget::Config("director-main".into()));
    mounted.settle();
    assert!(!draggable(
        "Profile header",
        mounted.rect("Custom name (optional)").center()
    ));
    mounted.click("Custom name (optional)");
    mounted.key(Key::Character("X".into()), false);
    assert!(mounted.model.editor_name.get_untracked().contains('X'));
}

#[test]
fn client_resize_targets_cover_edges_without_covering_header_controls() {
    if cfg!(target_os = "macos") {
        return;
    }
    let mounted = mount(false, 1380.0);
    for (label, point) in [
        ("Resize window North", Vector2::new(500.0, 2.0)),
        ("Resize window East", Vector2::new(1378.0, 400.0)),
        ("Resize window South", Vector2::new(500.0, 898.0)),
        ("Resize window West", Vector2::new(2.0, 400.0)),
        ("Resize window NorthWest", Vector2::new(2.0, 2.0)),
        ("Resize window SouthEast", Vector2::new(1378.0, 898.0)),
    ] {
        let target = mounted.ui.hit_test(point).unwrap().id();
        assert_eq!(
            mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .find(|n| n.id == target)
                .unwrap()
                .label
                .as_deref(),
            Some(label)
        );
    }
    assert_control_hit(&mounted, "Close window");
}

#[test]
fn native_gesture_handoff_releases_capture_when_the_os_consumes_mouse_up() {
    let mounted = mount(false, 1380.0);
    let header = mounted.rect("Page header");
    let positions = std::iter::once(header.origin + Vector2::new(100.0, 20.0)).chain(
        if cfg!(target_os = "macos") {
            vec![]
        } else {
            vec![Vector2::new(1378.0, 400.0), Vector2::new(1378.0, 898.0)]
        },
    );
    for (index, position) in positions.enumerate() {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Down(PointerButton::Primary),
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::from_secs(index as u64 + 1),
        });
        // No Up: the window manager owns that release. Flush the handoff
        // before the next pointer press, as the native runtime does.
        mounted.settle();
        assert!(mounted.model.window_gesture.get_untracked().is_none());
        mounted.click("Open command palette");
        assert!(
            mounted.model.palette.get_untracked(),
            "capture retained at {position:?}"
        );
        mounted.model.palette.set(false);
        mounted.settle();
    }
}

#[test]
fn streamed_messages_keep_the_bottom_visible_and_preserve_manual_reading_position() {
    let mounted = mount(false, 1100.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|s| {
        s.messages.push(Message {
            id: "stream-follow".into(),
            session_id: "session-plan".into(),
            author: "Agent".into(),
            sender: Sender::User,
            kind: "assistant".into(),
            body: "line\n".repeat(80),
            parts: vec![],
            tool: None,
        })
    });
    mounted.settle();
    let next = mounted.rect("Next message");
    assert!(next.origin.y + next.size.height < mounted.size.height);
    mounted.model.snapshot.update(|s| {
        s.messages
            .iter_mut()
            .find(|m| m.id == "stream-follow")
            .unwrap()
            .body
            .push_str(&"new line\n".repeat(40))
    });
    mounted.settle();
    let following = mounted.rect("Next message");
    assert!((following.origin.y - next.origin.y).abs() < 2.0);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 400.0),
        },
        position: Vector2::new(800.0, 400.0),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let reading = mounted.rect("Message stream-follow");
    mounted.model.snapshot.update(|s| {
        s.messages
            .iter_mut()
            .find(|m| m.id == "stream-follow")
            .unwrap()
            .body
            .push_str(&"more line\n".repeat(40))
    });
    mounted.settle();
    assert!((mounted.rect("Message stream-follow").origin.y - reading.origin.y).abs() < 2.0);
}

#[test]
#[ignore = "manual native GPU performance probe"]
fn native_fixture_gpu_cost() {
    use mosaic::render::Renderer;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("Relay fixture GPU: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    for [width, height] in [[3771u32, 2141u32], [1123, 848]] {
        let scale = 1.25;
        let mut mounted = mount(false, width as f32 / scale);
        mounted.size = Size::new(width as f32 / scale, height as f32 / scale);
        flush();
        for _ in 0..12 {
            mounted.ui.tick(Duration::from_millis(16));
            flush();
            mounted.ui.frame(mounted.size, scale);
        }
        let scene = mounted.ui.scene().clone();
        let mut renderer = mosaic::render_wgpu::WgpuRenderer::from_device(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8UnormSrgb,
            Size::new(width as f32, height as f32),
        );
        renderer.set_scale_factor(f64::from(scale));
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        // Ablations retain structural commands; isolation bounds may shrink,
        // so report raster work and do not interpret deltas as additive costs.
        let mut no_glyphs = scene.clone();
        no_glyphs
            .cmds
            .retain(|cmd| !matches!(cmd, mosaic::render::PaintCmd::Glyphs(_)));
        let mut no_shapes = scene.clone();
        no_shapes
            .cmds
            .retain(|cmd| !matches!(cmd, mosaic::render::PaintCmd::Shape(_)));
        for (workload, scene) in [
            ("clear", mosaic::render::Scene::new(scene.clear)),
            ("solid", {
                let mut solid = mosaic::render::Scene::new(scene.clear);
                solid.shape(
                    mosaic::render::Shape::new(Rect::from_xywh(40.0, 40.0, 256.0, 256.0))
                        .fill(Color::WHITE),
                );
                solid
            }),
            ("fixture", scene.clone()),
            ("fixture-no-glyphs", no_glyphs),
            ("fixture-no-shapes", no_shapes),
        ] {
            if std::env::var("RELAY_GPU_PROBE_WORKLOAD")
                .is_ok_and(|selection| selection != workload)
            {
                continue;
            }
            for depth in [1, 3] {
                let mut pending = std::collections::VecDeque::new();
                let mut durations = vec![];
                // Warm up pipelines and fonts before measuring throughput.
                for _ in 0..64 {
                    queue.submit([renderer.encode_into(
                        &scene,
                        &view,
                        mosaic::render_wgpu::TargetLoad::Clear(scene.clear),
                    )]);
                    renderer.map_timings();
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                }
                let started = std::time::Instant::now();
                for _ in 0..192 {
                    let commands = renderer.encode_into(
                        &scene,
                        &view,
                        mosaic::render_wgpu::TargetLoad::Clear(scene.clear),
                    );
                    pending.push_back(queue.submit([commands]));
                    renderer.map_timings();
                    if pending.len() >= depth {
                        device
                            .poll(wgpu::PollType::Wait {
                                submission_index: pending.pop_front(),
                                timeout: None,
                            })
                            .unwrap();
                    }
                    if depth == 1
                        && let Some(gpu) = renderer.stats().gpu
                    {
                        durations.push(f64::from(gpu.total));
                    }
                }
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let seconds = started.elapsed().as_secs_f64();
                durations.sort_by(f64::total_cmp);
                let percentile = |percent: usize| {
                    durations
                        .get((durations.len().saturating_sub(1) * percent) / 100)
                        .copied()
                };
                eprintln!(
                    "Relay GPU probe: workload={workload} physical={width}x{height} logical={:?} scale={scale} depth={depth} fps={:.1} median_gpu={:?}ms p95_gpu={:?}ms stats={:?}",
                    mounted.size,
                    192.0 / seconds,
                    percentile(50),
                    percentile(95),
                    renderer.stats()
                );
                if workload == "fixture" {
                    assert_eq!(
                        renderer.stats().render_passes,
                        3,
                        "masked fixture should share one isolation target"
                    );
                }
            }
        }
    }
}

#[test]
fn small_acknowledgement_preserves_newer_edits_until_state_catches_up() {
    let mut mounted = mount(false, 1380.0);
    let initial = mounted.model.snapshot.get_untracked();
    mounted.model.comment_body.set("submitted".into());
    mounted.model.submit(
        Command::AddComment {
            message_id: "m2".into(),
            quote: None,
            author: "Reviewer".into(),
            body: "submitted".into(),
        },
        initial.revision,
        crate::model::Saved::Comment("submitted".into()),
    );
    let request = mounted.commands.try_recv().unwrap();
    mounted.model.comment_body.set("newer edit".into());
    mounted.model.receive(NetworkState {
        snapshot: initial.clone(),
        connected: true,
        acknowledged_revision: initial.revision + 1,
        outcome_serial: 1,
        outcome: Some((request.request_id, Ok(()))),
        ..Default::default()
    });
    assert!(!mounted.model.busy.get_untracked());
    assert!(mounted.model.syncing.get_untracked());
    assert_eq!(mounted.model.comment_body.get_untracked(), "newer edit");
    mounted.model.submit(
        Command::SyncProject {
            project_id: "demo".into(),
        },
        initial.revision,
        crate::model::Saved::Action,
    );
    assert!(mounted.commands.try_recv().is_err());
    let mut updated = initial;
    updated.revision += 1;
    mounted.model.receive(NetworkState {
        acknowledged_revision: updated.revision,
        snapshot: updated,
        connected: true,
        ..Default::default()
    });
    assert!(!mounted.model.syncing.get_untracked());
}

#[test]
fn late_history_response_cannot_replace_a_new_session_and_reconnect_releases_paging() {
    let mounted = mount(false, 1380.0);
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.transport_requests.set(Some(sender));
    mounted.model.subscribe_history("session-plan".into());
    assert!(matches!(
        requests.try_recv().unwrap(),
        StreamRequest::Subscribe { generation: 1, .. }
    ));
    mounted.model.subscribe_history("session-review".into());
    let previous = mounted.model.snapshot.get_untracked();
    let mut stale = previous.clone();
    stale.messages.clear();
    mounted.model.receive(NetworkState {
        snapshot: stale,
        connected: true,
        history: HistoryState {
            session_id: "session-plan".into(),
            generation: 1,
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(
        mounted.model.snapshot.get_untracked().messages,
        previous.messages
    );
    mounted.model.history_loading.set(true);
    mounted.model.receive(NetworkState {
        snapshot: previous,
        connected: false,
        ..Default::default()
    });
    assert!(!mounted.model.history_loading.get_untracked());
}

#[test]
fn scrolling_loads_one_older_page_and_preserves_the_visible_message() {
    for tools in [false, true] {
        let mounted = mount(false, 1380.0);
        mounted.model.open_session("session-plan".into());
        let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
        mounted.model.transport_requests.set(Some(sender));
        mounted.model.subscribe_history("session-plan".into());
        requests.try_recv().unwrap();
        let mut snapshot = mounted.model.snapshot.get_untracked();
        snapshot.messages = (50..100)
            .map(|i| Message {
                id: format!("paged-{i}"),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: format!("Message {i}\n{}", "line\n".repeat(5)),
                parts: vec![],
                tool: tools.then(|| ToolCall {
                    run_id: "run".into(),
                    name: "exec_command".into(),
                    kind: ToolKind::Run,
                    state: ToolState::Completed,
                    input: "pwd".into(),
                    target: "pwd".into(),
                    directory: None,
                    exit_code: Some(0),
                    started_ms: None,
                    finished_ms: None,
                    detail: None,
                }),
            })
            .collect();
        let history = HistoryState {
            session_id: "session-plan".into(),
            generation: 1,
            oldest_message_id: Some("paged-50".into()),
            newest_message_id: Some("paged-99".into()),
            has_more: true,
            ..Default::default()
        };
        mounted.model.receive(NetworkState {
            snapshot: snapshot.clone(),
            connected: true,
            history: history.clone(),
            ..Default::default()
        });
        mounted.settle();
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Wheel {
                delta: Vector2::new(0.0, 100000.0),
            },
            position: Vector2::new(800.0, 400.0),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        let request = requests.try_recv().unwrap();
        assert!(matches!(request,StreamRequest::Older{before,..} if before=="paged-50"));
        assert!(requests.try_recv().is_err());
        let label = if tools {
            "Tool call paged-50"
        } else {
            "Message paged-50"
        };
        let before = mounted.rect(label);
        let mut older = (0..50)
            .map(|i| Message {
                id: format!("paged-{i}"),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: format!("Earlier {i}\n{}", "line\n".repeat(5)),
                parts: vec![],
                tool: tools.then(|| ToolCall {
                    run_id: "run".into(),
                    name: "exec_command".into(),
                    kind: ToolKind::Run,
                    state: ToolState::Completed,
                    input: "pwd".into(),
                    target: "pwd".into(),
                    directory: None,
                    exit_code: Some(0),
                    started_ms: None,
                    finished_ms: None,
                    detail: None,
                }),
            })
            .collect::<Vec<_>>();
        older.extend(snapshot.messages);
        snapshot.messages = older;
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            history: HistoryState {
                oldest_message_id: Some("paged-0".into()),
                has_more: false,
                ..history
            },
            ..Default::default()
        });
        mounted.settle();
        let after = mounted.rect(label);
        assert!(
            (after.origin.y - before.origin.y).abs() < 2.0,
            "history insertion moved the reading position by {} (tools={tools})",
            after.origin.y - before.origin.y
        );
    }
}

#[test]
fn retained_search_result_does_not_repeatedly_move_the_reader() {
    let mounted = mount(false, 1380.0);
    mounted.model.search.set("needle".into());
    let snapshot = mounted.model.snapshot.get_untracked();
    let update = NetworkState {
        snapshot,
        connected: true,
        search_serial: 1,
        search_result: Some((
            "session-plan".into(),
            0,
            "needle".into(),
            Some("m2".into()),
            "needle".into(),
        )),
        ..Default::default()
    };
    mounted.model.receive(update.clone());
    assert_eq!(mounted.model.focused_message.get_untracked(), "m2");
    mounted.model.focused_message.set(String::new());
    mounted.model.receive(update);
    assert!(mounted.model.focused_message.get_untracked().is_empty());
}

/// The demo workspace running its example orchestration.
fn orchestration() -> Snapshot {
    let mut snapshot = demo_snapshot(AgentProfile::default());
    demo_orchestration(&mut snapshot);
    snapshot
}

fn mount_orchestration(width: f32) -> Mounted {
    let mounted = mount(false, width);
    mounted.model.receive(NetworkState {
        snapshot: orchestration(),
        connected: true,
        status: "Connected".into(),
        ..Default::default()
    });
    mounted.settle();
    mounted
}

#[test]
fn spawn_receipt_and_harness_event_render_one_child_and_keep_the_fallback() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.open_session("session-plan".into());
    let mut native = mounted
        .model
        .snapshot
        .get_untracked()
        .messages
        .into_iter()
        .find(|m| m.id == "t-spawn")
        .unwrap();
    native.id = "native-spawn".into();
    let mut receipt = native.clone();
    receipt.id = "agent-tool-receipt".into();
    receipt.author = "Relay".into();
    receipt.kind = "tool".into();
    mounted
        .model
        .snapshot
        .update(|s| s.messages = vec![receipt.clone()]);
    mounted.settle();
    assert_eq!(mounted.rects("Spawned 1 agents").len(), 1);
    mounted
        .model
        .snapshot
        .update(|s| s.messages.push(native.clone()));
    mounted.settle();
    assert_eq!(mounted.rects("Spawned 1 agents").len(), 1);
    assert!(has_label(&mounted, "Tool call native-spawn"));
    assert!(!has_label(&mounted, "Tool call agent-tool-receipt"));
    assert_eq!(mounted.model.snapshot.get_untracked().messages.len(), 2);
    mounted
        .model
        .snapshot
        .update(|s| s.messages = vec![native.clone(), receipt.clone()]);
    mounted.settle();
    assert_eq!(mounted.rects("Spawned 1 agents").len(), 1);
    mounted
        .model
        .snapshot
        .update(|s| s.messages = vec![receipt]);
    mounted.settle();
    assert_eq!(mounted.rects("Spawned 1 agents").len(), 1);
    assert!(has_label(&mounted, "Tool call agent-tool-receipt"));
}

fn focused_label(mounted: &Mounted) -> Option<String> {
    let focused = mounted.ui.focused()?.id();
    mounted
        .ui
        .inspection_snapshot()
        .node(focused)?
        .label
        .clone()
}

#[test]
fn sidebar_keeps_project_navigation_in_ruled_cells_above_the_agent_tree() {
    let mounted = mount(false, 1380.0);
    let sidebar = mounted.rect("Sidebar");
    let head = mounted.rect("Sidebar header");
    assert!((sidebar.size.width - 380.0).abs() < 1.0);
    assert!((head.size.height - 60.0).abs() < 1.0);
    // Head cells fill the head and sit above the agents section.
    for cell in [
        "Toggle project Relay · demo",
        "Open board for Relay · demo",
        "Settings",
    ] {
        let rect = mounted.rect(cell);
        assert!((rect.size.height - head.size.height).abs() < 1.0, "{cell}");
        assert!(rect.origin.y + rect.size.height <= mounted.rect("Agents").origin.y + 1.0);
    }
    open_project_menu(&mounted);
    assert!(!has_label(&mounted, "Open director Project director"));
    assert!(!has_label(&mounted, "Profile for Project director"));
    mounted.click("Select project Relay · demo");
    assert!(!has_label(&mounted, "Projects"));
    open_project_menu(&mounted);
    assert!(!has_label(&mounted, "Create director"));
    mounted.click("Toggle project Relay · demo");
    mounted.click("New agent");
    assert_eq!(mounted.model.editor.get_untracked(), EditTarget::New);
    mounted.click("Toggle project Relay · demo");
    mounted.click("Project defaults");
    assert_eq!(mounted.model.editor.get_untracked(), EditTarget::Defaults);
    mounted.click("Toggle project Relay · demo");
    open_project_menu(&mounted);
    mounted.click("New Project");
    assert_eq!(mounted.model.page.get_untracked(), Page::NewProject);
    mounted.click("Open board for Relay · demo");
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
    mounted.click("Settings");
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
    // No control nests inside another one.
    let snapshot = mounted.ui.inspection_snapshot();
    let sidebar = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Sidebar"))
        .unwrap()
        .id;
    for node in snapshot.nodes.iter().filter(|n| n.role == Role::Button) {
        let mut parent = node.parent;
        let mut button_ancestor = false;
        while let Some(id) = parent {
            if id == sidebar {
                assert!(!button_ancestor, "Nested sidebar control: {:?}", node.label);
                break;
            }
            let ancestor = snapshot.nodes.iter().find(|n| n.id == id).unwrap();
            button_ancestor |= ancestor.role == Role::Button;
            parent = ancestor.parent;
        }
    }
}

#[test]
fn agent_tree_follows_parent_links_scoped_to_the_selected_project() {
    let mounted = mount_orchestration(1380.0);
    let plan = tree_row(&mounted, "Profile design");
    let migration = tree_row(&mounted, "Schema migration");
    let write = tree_row(&mounted, "Write migrations");
    assert!((plan.size.height - 34.0).abs() < 1.0);
    assert!(plan.origin.y < migration.origin.y && migration.origin.y < write.origin.y);
    // Each level adds one 14px elbow column: compare the status squares.
    let indent = |row: &str| {
        let rect = tree_row(&mounted, row);
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter(|n| n.role == Role::Image && (n.rect.size.width - 10.0).abs() < 0.5)
            .filter(|n| (n.rect.center().y - rect.center().y).abs() < 1.0)
            .map(|n| n.rect.origin.x)
            .fold(f32::MAX, f32::min)
    };
    assert!((indent("Write migrations") - indent("Schema migration") - 14.0).abs() < 1.0);
    // Another project's agents stay out of this project's tree.
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut other = snapshot.projects[0].clone();
    other.id = "other".into();
    other.name = "Other project".into();
    snapshot.projects.push(other);
    let mut session = snapshot.sessions[0].clone();
    session.id = "other-session".into();
    session.project_id = "other".into();
    session.title = "Other agent".into();
    session.parent_session_id = None;
    snapshot.sessions.push(session);
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    assert!(!has_row(&mounted, "Other agent"));
    mounted.click("Toggle project Relay · demo");
    mounted.click("Select project Other project");
    assert_eq!(mounted.model.project.get_untracked(), "other");
    assert!(has_row(&mounted, "Other agent"));
    assert!(!has_row(&mounted, "Schema migration"));
    mounted.click("Other agent");
    assert_eq!(mounted.model.session.get_untracked(), "other-session");
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
}

#[test]
fn deep_branches_fold_and_focus_re_roots_the_tree_with_a_breadcrumb() {
    let mounted = mount_orchestration(1380.0);
    assert!(has_label(&mounted, "2 more levels · 2 agents"));
    assert!(!has_row(&mounted, "Index check"));
    mounted.click("Focus");
    assert_eq!(
        mounted.model.agent_focus.get_untracked().as_deref(),
        Some("session-backfill")
    );
    assert!(has_label(&mounted, "Depth 3"));
    assert!(has_label(&mounted, "Verify backfill"));
    assert!(has_row(&mounted, "Index check"));
    assert!(!has_row(&mounted, "Migration tests"));
    mounted.click("Back to Root");
    assert_eq!(mounted.model.agent_focus.get_untracked(), None);
    assert!(has_row(&mounted, "Migration tests"));
    // Collapsing a manager hides its branch and keeps what waits below it.
    focus_row(&mounted, "Profile design");
    mounted.key(Key::ArrowLeft, false);
    assert!(
        mounted
            .model
            .collapsed_agents
            .get_untracked()
            .contains("session-plan")
    );
    assert!(!has_row(&mounted, "Schema migration"));
    assert!(has_label(&mounted, "1 waiting"));
    mounted.key(Key::ArrowRight, false);
    assert!(has_row(&mounted, "Schema migration"));
    assert_eq!(focused_label(&mounted).as_deref(), Some("Profile design"));
}

#[test]
fn needs_you_projects_a_deep_permission_and_one_answer_resolves_it_everywhere() {
    let mut mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.projects[0].fixture = false;
    for session in &mut snapshot.sessions {
        session.fixture = false;
    }
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    assert!(has_label(&mounted, "Needs You"));
    // The manager two levels up shows the request in its transcript.
    assert!(has_label(&mounted, "Approval request"));
    assert!(has_label(&mounted, "Approval Requested · Bash"));
    assert!(mounted.rects("Approve").len() >= 2);
    mounted.click_nth("Approve", 0);
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(matches!(
        envelope.command,
        Command::RespondPermission { ref permission_id, allow: true, .. } if permission_id == "permission-migrate"
    ));
    // The server records one decision; every projection clears.
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.tool_permissions[0].decision = Some(true);
    snapshot.revision += 1;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((envelope.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    assert!(!has_label(&mounted, "Needs You"));
    assert!(!has_label(&mounted, "Approval request"));
    assert!(!has_label(&mounted, "Approve"));
    let _ = &mut mounted;
}

#[test]
fn connection_status_reports_state_latency_endpoint_protocol_and_builds() {
    let mounted = mount(false, 1380.0);
    mounted
        .model
        .server_endpoint
        .set("https://relay.example:7440/".into());
    mounted.model.server_build.set(Some(BuildInfo {
        version: "0.1.0".into(),
        revision: "1234567890123456789012345678901234567890".into(),
        ..Default::default()
    }));
    mounted.model.round_trip_ms.set(Some(42));
    mounted.settle();
    let foot = mounted.rect("Connection: Connected, 42 ms. Show details");
    let sidebar = mounted.rect("Sidebar");
    assert!(foot.origin.y + foot.size.height <= sidebar.origin.y + sidebar.size.height + 1.0);
    let details = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some("Server connection details"))
        .unwrap();
    assert!(details.description.unwrap().contains("relay.example:7440"));
    mounted.click("Connection: Connected, 42 ms. Show details");
    for text in [
        "https://relay.example:7440/",
        "WebSocket · open",
        &format!("v{PROTOCOL_VERSION}"),
        "42 ms",
    ] {
        assert!(has_label(&mounted, text), "Missing {text}");
    }
    assert!(has_label(&mounted, "1234567"));
    let details = mounted.rect("Connection details");
    assert!(
        details.origin.x >= sidebar.origin.x
            && details.origin.x + details.size.width <= sidebar.origin.x + sidebar.size.width + 0.5
    );
    mounted.click("Copy endpoint");
    assert_eq!(
        mounted.ui.clipboard_text().as_deref(),
        Some("https://relay.example:7440/")
    );
    mounted.click("Connection: Connected, 42 ms. Hide details");
    assert!(!has_label(&mounted, "Connection details"));
    mounted.model.receive(NetworkState {
        connected: false,
        status: "Reconnecting in 2s".into(),
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "Offline"));
    assert_eq!(mounted.model.round_trip_ms.get_untracked(), None);
}

#[test]
fn ledger_effort_steps_send_the_session_harness_selection() {
    let mut mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for session in &mut snapshot.sessions {
        session.fixture = false;
    }
    snapshot.projects[0].fixture = false;
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".to_string(),
            Ok(HarnessCatalog {
                session_id: "session-plan".into(),
                harness: Harness::ClaudeCode,
                models: vec![HarnessModel {
                    id: "claude-opus-4-5".into(),
                    name: "Opus 4.5".into(),
                    efforts: vec!["low".into(), "medium".into(), "high".into(), "max".into()],
                    default_effort: Some("medium".into()),
                    fast: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    assert!(has_label(&mounted, "Model: CLAUDE Opus 4.5. Change model"));
    assert!(has_label(&mounted, "Effort high"));
    mounted.click("Effort low");
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(matches!(
        envelope.command,
        Command::SetHarnessSelection { ref session_id, selection: HarnessSelection { model: Some(ref model), effort: Some(ref effort), .. } }
            if session_id == "session-plan" && model == "claude-opus-4-5" && effort == "low"
    ));
    let _ = &mut mounted;
}

#[test]
fn consecutive_tool_calls_are_compact_while_prose_keeps_its_spacing() {
    for scale in [1.0, 2.0] {
        let mounted = mount(false, 1380.0);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|snapshot| {
            snapshot.messages.retain(|m| m.session_id != "session-plan");
            for (id, kind, body) in [
                ("dense-1", "command_execution", "echo first\nExit: 0\nfirst"),
                (
                    "dense-2",
                    "command_execution",
                    "echo second\nExit: 0\nsecond",
                ),
                (
                    "dense-prose",
                    "assistant",
                    "Some explanation between the tool calls.",
                ),
                ("dense-3", "command_execution", "echo third\nExit: 0\nthird"),
                (
                    "dense-4",
                    "command_execution",
                    "echo fourth\nExit: 0\nfourth",
                ),
            ] {
                snapshot.messages.push(Message {
                    id: id.into(),
                    session_id: "session-plan".into(),
                    author: "Codex".into(),
                    sender: Sender::User,
                    kind: kind.into(),
                    body: body.into(),
                    parts: vec![],
                    tool: None,
                });
            }
        });
        mounted.settle();
        let first = mounted.rect("Tool call dense-1");
        let second = mounted.rect("Tool call dense-2");
        let third = mounted.rect("Tool call dense-3");
        let fourth = mounted.rect("Tool call dense-4");
        assert!(
            (first.size.height - 30.0 * scale).abs() < 1.0,
            "scale {scale}: first {first:?}, second {second:?}"
        );
        assert!((second.origin.y - first.max_y() - 6.0 * scale).abs() < 1.0);
        assert!((fourth.origin.y - third.max_y() - 6.0 * scale).abs() < 1.0);
        assert!(third.origin.y - second.max_y() >= 56.0 * scale);
        // Paging in older history must update the existing first row's spacing.
        mounted.model.snapshot.update(|snapshot| {
            let mut older = snapshot
                .messages
                .iter()
                .find(|m| m.id == "dense-1")
                .unwrap()
                .clone();
            older.id = "dense-older".into();
            snapshot.messages.insert(0, older);
        });
        mounted.settle();
        let older = mounted.rect("Tool call dense-older");
        let first = mounted.rect("Tool call dense-1");
        assert!((first.origin.y - older.max_y() - 6.0 * scale).abs() < 1.0);
    }
}

#[test]
fn fingerprint_shows_whole_chat_counts_with_one_page_loaded_and_follows_new_calls() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.open_session("session-plan".into());
    // As a connected client does; this harness has no transport to subscribe it.
    mounted.model.subscribe_history("session-plan".into());
    mounted.settle();
    // The loaded page holds one Read call; the server counts 99 older ones.
    let mut whole = relay_core::tool_counts(
        &mounted.model.snapshot.get_untracked().messages,
        "session-plan",
    );
    let loaded: u32 = whole.iter().map(|c| c.count).sum();
    whole
        .iter_mut()
        .find(|c| c.kind == ToolKind::Read)
        .unwrap()
        .count += 99;
    let receive = |snapshot: Snapshot, tool_counts: Vec<relay_core::ToolCount>| {
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            status: "Connected".into(),
            history: HistoryState {
                session_id: "session-plan".into(),
                generation: mounted.model.history_generation.get_untracked(),
                tool_counts: Some(tool_counts),
                ..Default::default()
            },
            ..Default::default()
        });
    };
    receive(mounted.model.snapshot.get_untracked(), whole.clone());
    mounted.settle();
    let total = loaded + 99;
    let read = |count: u32, total: u32| {
        format!(
            "Read: {count} calls, {} percent",
            relay_ui::fingerprint::share(count, total)
        )
    };
    assert!(has_label(&mounted, &read(100, total)));
    // A new call arrives with the server's updated counts in the same update;
    // the bar shows it at once and counts it once.
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut call = snapshot
        .messages
        .iter()
        .find(|m| {
            m.session_id == "session-plan"
                && m.tool.as_ref().is_some_and(|t| t.kind == ToolKind::Read)
        })
        .unwrap()
        .clone();
    call.id = "read-new".into();
    snapshot.messages.push(call);
    whole
        .iter_mut()
        .find(|c| c.kind == ToolKind::Read)
        .unwrap()
        .count += 1;
    receive(snapshot, whole);
    mounted.settle();
    assert!(has_label(&mounted, &read(101, total + 1)));
    assert!(!has_label(&mounted, &read(102, total + 2)));
}

#[test]
fn tool_calls_open_in_three_steps_and_the_fingerprint_counts_them() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    // One fingerprint segment per tool kind the director used.
    for kind in [
        "Read", "Edit", "Create", "Search", "Run", "Test", "Fetch", "Plan", "Message", "Wait",
        "Result", "Other",
    ] {
        assert!(
            has_label(&mounted, &format!("{kind}: 1 call, 8 percent")),
            "fingerprint {kind}"
        );
    }
    let opening = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter_map(|n| n.label)
            .filter(|l| {
                l.starts_with("Collapsed.")
                    || l.starts_with("Summary.")
                    || l.starts_with("Expanded.")
            })
            .collect::<Vec<_>>()
    };
    let before = opening(&mounted);
    // The failed test run opens to its summary; the rest start collapsed.
    assert_eq!(
        before.iter().filter(|l| l.starts_with("Summary.")).count(),
        1
    );
    let collapsed = before
        .iter()
        .filter(|l| l.starts_with("Collapsed."))
        .count();
    let last = mounted.rects("Collapsed. Open summary").len() - 1;
    mounted.click_nth("Collapsed. Open summary", last);
    assert_eq!(
        opening(&mounted)
            .iter()
            .filter(|l| l.starts_with("Collapsed."))
            .count(),
        collapsed - 1
    );
    let last = mounted.rects("Summary. Expand fully").len() - 1;
    mounted.click_nth("Summary. Expand fully", last);
    assert_eq!(
        opening(&mounted)
            .iter()
            .filter(|l| l.starts_with("Expanded."))
            .count(),
        1
    );
    mounted.click("Expanded. Collapse");
    assert_eq!(opening(&mounted), before);
    // The spawn is the child's tree row with a way into the tree.
    assert!(has_label(&mounted, "Spawned 1 agents"));
    mounted.focus("Open in tree");
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.agent_focus.get_untracked().as_deref(),
        Some("session-plan")
    );
}

#[test]
fn stop_all_is_a_separate_session_action_that_stops_descendants() {
    let mut mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for session in &mut snapshot.sessions {
        session.fixture = false;
    }
    snapshot.projects[0].fixture = false;
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    // The header stop only stops this session's turn.
    mounted.click("Stop");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StopWorker { ref session_id, include_descendants: false } if session_id == "session-plan"
    ));
    assert!(mounted.commands.try_recv().is_err());
    mounted.model.busy.set(false);
    mounted.click("Session actions");
    mounted.settle();
    assert!(has_label(&mounted, "Stop this agent and all its subagents"));
    mounted.click("Stop this agent and all its subagents");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StopWorker { ref session_id, include_descendants: true } if session_id == "session-plan"
    ));
    // Without active descendants the action is not offered.
    mounted.model.busy.set(false);
    mounted.model.snapshot.update(|s| {
        let below = s
            .descendants("session-plan")
            .iter()
            .map(|d| d.id.clone())
            .collect::<Vec<_>>();
        for session in &mut s.sessions {
            if below.contains(&session.id)
                && let Some(worker) = session.worker.as_mut()
            {
                worker.status = WorkerStatus::Completed;
            }
        }
    });
    mounted.settle();
    assert!(!has_label(
        &mounted,
        "Stop this agent and all its subagents"
    ));
}

#[test]
fn session_header_shows_context_controls_and_agents_below() {
    let mut mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for session in &mut snapshot.sessions {
        session.fixture = false;
    }
    snapshot.projects[0].fixture = false;
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let header = mounted.rect("Session header");
    assert!((header.size.height - 88.0).abs() < 1.0);
    assert!(has_label(
        &mounted,
        "Context 62 percent used, 124k of 200k, compacted 2 times"
    ));
    assert!(mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Working. 7 agents below"))
    }));
    mounted.click("Stop");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StopWorker { ref session_id, include_descendants: false } if session_id == "session-plan"
    ));
    // A session without measured context shows no gauge.
    mounted.model.open_session("session-worker".into());
    mounted.settle();
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Context "))
    }));
    assert!(!has_label(&mounted, "Stop"));
    let _ = &mut mounted;
}

fn tree_row(mounted: &Mounted, name: &str) -> Rect {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some(name) && n.role == Role::ListItem)
        .unwrap_or_else(|| panic!("Missing tree row: {name}"))
        .rect
}

fn focus_row(mounted: &Mounted, name: &str) {
    for _ in 0..mounted.ui.inspection_snapshot().nodes.len() {
        mounted.key(Key::Tab, false);
        let focused = mounted.ui.focused().map(|f| f.id());
        let snapshot = mounted.ui.inspection_snapshot();
        if focused
            .and_then(|id| snapshot.node(id))
            .is_some_and(|n| n.label.as_deref() == Some(name) && n.role == Role::ListItem)
        {
            mounted.settle();
            return;
        }
    }
    panic!("Tree row is not keyboard reachable: {name}");
}

fn has_row(mounted: &Mounted, name: &str) -> bool {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .any(|n| n.label.as_deref() == Some(name) && n.role == Role::ListItem)
}
#[test]
fn session_header_shows_only_recorded_values() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.agent_config_id = snapshot.agent_configs[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.thread_id = None;
    run.worktree = Some("/srv/relay/worktrees/issue-2-a41c".into());
    session.worker = Some(run);
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    let header = mounted.rect("Session header");
    assert!((header.size.height - 88.0).abs() <= 1.0, "{header:?}");
    // Unmeasured context has no gauge; an unreported model reads as the
    // harness default rather than a guess.
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Context "))
    }));
    assert!(has_label(&mounted, "Model: Default. Change model"));
    // Recorded provenance stays one disclosure away.
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    assert!(has_label(&mounted, "Worktree"));
    assert!(!has_label(&mounted, "Thread"));
    // Send stays to the right of the inline draft.
    let draft = mounted.rect("Next message");
    let send = mounted.rect("Send message");
    assert!(send.origin.x >= draft.max_x());
    assert!(has_label(&mounted, "Stop"));
}

#[test]
fn session_model_and_context_follow_reported_updates() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.worker = Some(worker_run(WorkerStatus::Running));
    let id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.settle();
    assert!(has_label(&mounted, "Model: Default. Change model"));
    let worker = snapshot.sessions[0].worker.as_mut().unwrap();
    worker.model = Some("claude-sonnet-4-5".into());
    worker.context_tokens = Some(12345);
    worker.context_window = Some(200000);
    snapshot.sessions[0].compactions = vec![Compaction { at_tokens: None }];
    snapshot.revision += 1;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(
        &mounted,
        "Model: CLAUDE Sonnet 4.5. Change model"
    ));
    assert!(has_label(
        &mounted,
        "Context 6 percent used, 12k of 200k, compacted 1 times"
    ));
}

#[test]
fn stop_control_fills_the_session_header_cell() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|session| session.id == "session-worker")
            .unwrap()
            .worker = Some(worker_run(WorkerStatus::Running));
    });
    mounted.model.open_session("session-worker".into());
    mounted.settle();
    let header = mounted.rect("Session header");
    let stop = mounted.rect("Stop");
    assert!((stop.origin.y - header.origin.y).abs() < 0.1);
    assert!((stop.size.height - (header.size.height - 1.0)).abs() < 1.1);
    assert!(
        (stop.origin.x + stop.size.width - mounted.rect("Session actions").origin.x).abs() < 0.1
    );
}

/// The status word in the session header's control block.
fn header_status(mounted: &Mounted) -> String {
    let header = mounted.rect("Session header");
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|n| header.contains(n.rect.center()))
        .filter_map(|n| n.label)
        .find(|l| ["Working", "Waiting", "Failed", "Queued", "Done"].contains(&l.as_str()))
        .unwrap_or_default()
}

#[test]
fn sidebar_usage_includes_connected_harnesses_before_first_run() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.usage_limits.clear();
    snapshot.usage_ledger.clear();
    let statuses = [Harness::Codex, Harness::ClaudeCode]
        .map(|harness| HarnessStatus {
            harness,
            executable: String::new(),
            version: None,
            state: "ready".into(),
            detail: String::new(),
            checked_at: 1,
        })
        .to_vec();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        harnesses: statuses.clone(),
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "Codex. Show details"));
    assert!(has_label(&mounted, "Claude Code. Show details"));
    assert!(has_label(&mounted, "No usage data"));
    let usage = crate::adapt::harness_usage(&snapshot, &statuses, 1_000_000);
    assert_eq!(usage.len(), 2);
    assert!(
        usage
            .iter()
            .all(|u| u.session.is_none() && u.week.is_none() && u.insight.stats.is_empty())
    );
    let mut statuses = statuses;
    statuses[1].state = "signed_out".into();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        harnesses: statuses,
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "Codex. Show details"));
    assert!(!has_label(&mounted, "Claude Code. Show details"));
}

#[test]
fn session_model_control_preserves_gpt_variant() {
    let mounted = mount(false, 2400.0);
    let mut snapshot = live_snapshot();
    let mut run = worker_run(WorkerStatus::Completed);
    run.model = Some("gpt-6-luna".into());
    snapshot.sessions[0].worker = Some(run);
    let id = snapshot.sessions[0].id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.settle();
    assert!(has_label(&mounted, "Model: GPT Luna 6. Change model"));
}

#[test]
fn reopened_session_retains_model_and_measured_usage_without_claiming_live_cache() {
    let mounted = mount(false, 2400.0);
    let mut snapshot = live_snapshot();
    let mut run = worker_run(WorkerStatus::Completed);
    run.model = Some("recorded-model".into());
    run.context_window = Some(200_000);
    run.last_usage = Some(TokenUsage {
        input_tokens: 42_000,
        cached_input_tokens: 40_000,
        output_tokens: 1000,
    });
    snapshot.sessions[0].worker = Some(run);
    let snapshot: Snapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    let id = snapshot.sessions[0].id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.settle();
    // An unknown model keeps its own name in the neutral hue.
    assert!(has_label(&mounted, "Model: recorded-model. Change model"));
    // A window without measured occupancy draws no gauge.
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Context "))
    }));
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    assert!(has_label(&mounted, "42,000"));
    assert!(mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Cache status is unknown"))
    }));
}

/// Opens the sidebar's project menu unless it is open.
fn open_project_menu(mounted: &Mounted) {
    if !has_label(mounted, "Projects") {
        let toggle = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter_map(|n| n.label)
            .find(|l| l.starts_with("Toggle project "))
            .unwrap();
        mounted.click(&toggle);
    }
}
fn tool_row_description(mounted: &Mounted, id: &str) -> String {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some(&format!("Tool call {id}")))
        .and_then(|n| n.description)
        .unwrap_or_default()
}

#[test]
fn tool_activity_updates_in_place_and_opens_failures() {
    for light in [false, true] {
        let mounted = mount(light, 820.0);
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|s| {
            s.messages.retain(|m| m.session_id != "session-plan");
            s.messages.push(Message {
                id: "terminal-call".into(),
                session_id: "session-plan".into(),
                author: "Codex".into(),
                sender: Sender::User,
                kind: "tool_call".into(),
                body: "first output\n".into(),
                parts: vec![],
                tool: Some(ToolCall {
                    run_id: "turn".into(),
                    name: "commandExecution".into(),
                    kind: ToolKind::Run,
                    state: ToolState::Running,
                    input: "printf hello".into(),
                    target: "printf hello".into(),
                    directory: Some("/workspace".into()),
                    exit_code: None,
                    started_ms: None,
                    finished_ms: None,
                    detail: None,
                }),
            });
        });
        mounted.settle();
        assert_eq!(
            tool_row_description(&mounted, "terminal-call"),
            "Run · Working"
        );
        mounted.model.snapshot.update(|s| {
            s.tool_permissions.push(ToolPermission {
                id: "tool-approval".into(),
                session_id: "session-plan".into(),
                run_id: "turn".into(),
                tool: "commandExecution".into(),
                description: "printf hello".into(),
                decision: None,
                expired: false,
            })
        });
        mounted.settle();
        assert_eq!(
            tool_row_description(&mounted, "terminal-call"),
            "Run · Waiting for approval"
        );
        mounted
            .model
            .snapshot
            .update(|s| s.tool_permissions.clear());
        mounted.settle();
        let row = mounted.rect("Tool call terminal-call");
        assert!(row.size.width > 200.0);
        assert!(row.origin.x + row.size.width <= mounted.size.width);
        assert!(!has_label(&mounted, "first output"));
        mounted.click("Collapsed. Open summary");
        assert!(has_label(&mounted, "Summary. Expand fully"));
        mounted.click("Summary. Expand fully");
        mounted.click("Expanded. Collapse");
        mounted.model.snapshot.update(|s| {
            let m = s
                .messages
                .iter_mut()
                .find(|m| m.id == "terminal-call")
                .unwrap();
            m.body.push_str("failed output\n");
            let t = m.tool.as_mut().unwrap();
            t.state = ToolState::Failed;
            t.exit_code = Some(1);
        });
        mounted.settle();
        assert_eq!(
            tool_row_description(&mounted, "terminal-call"),
            "Run · Failed"
        );
        assert!(has_label(&mounted, "exit 1"));
    }
}

#[test]
fn tool_kinds_keep_their_verbs_and_unknown_outcomes_stay_unclaimed() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|s| {
        s.messages.retain(|m| m.session_id != "session-plan");
        for (i, (kind, state)) in [
            (ToolKind::Edit, ToolState::Completed),
            (ToolKind::Read, ToolState::Completed),
            (ToolKind::Search, ToolState::Completed),
            (ToolKind::Fetch, ToolState::Completed),
            (ToolKind::Spawn, ToolState::Completed),
            (ToolKind::Other, ToolState::Unknown),
        ]
        .into_iter()
        .enumerate()
        {
            s.messages.push(Message {
                id: format!("tool-{i}"),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "tool_call".into(),
                body: "result".into(),
                parts: vec![],
                tool: Some(ToolCall {
                    run_id: "run".into(),
                    name: "tool".into(),
                    kind,
                    state,
                    input: String::new(),
                    target: "src/main.rs".into(),
                    directory: None,
                    exit_code: None,
                    started_ms: None,
                    finished_ms: None,
                    detail: None,
                }),
            });
        }
    });
    mounted.settle();
    for (i, expected) in [
        "Edit · Done",
        "Read · Done",
        "Search · Done",
        "Fetch · Done",
        "Spawn · Done",
        "tool · Queued",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            tool_row_description(&mounted, &format!("tool-{i}")),
            expected
        );
    }
}

#[test]
fn one_catalog_per_project_harness_lets_every_row_show_effort() {
    let mut mounted = mount_orchestration(1380.0);
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(sender));
    mounted
        ._scope
        .run(|| crate::command_ui::bind_discovery(mounted.model));
    mounted.settle();
    let mut asked = Vec::new();
    while let Ok(request) = requests.try_recv() {
        if let crate::buffer_network::Request::Catalog { session, .. } = request {
            asked.push(session);
        }
    }
    asked.sort();
    asked.dedup();
    // The first Codex and the first Claude Code session of the project.
    assert_eq!(asked, ["session-plan", "session-worker"]);
    // With both catalogs loaded, rows of every model show their effort.
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([
            (
                "session-plan".to_string(),
                Ok(demo_catalog("session-plan", Harness::ClaudeCode)),
            ),
            (
                "session-worker".to_string(),
                Ok(demo_catalog("session-worker", Harness::Codex)),
            ),
        ]));
    mounted.settle();
    // Opus at high (of four steps and max), Sonnet at medium, Codex at medium.
    for track in ["Effort 3 of 4", "Effort 2 of 3"] {
        assert!(has_label(&mounted, track), "{track}");
    }
    let _ = &mut mounted;
}

#[test]
fn the_ledger_names_a_selected_effort_without_its_steps() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    // No catalog yet: the selection is named, with no step track.
    assert!(has_label(&mounted, "Selected effort"));
    assert!(has_label(&mounted, "high"));
    assert!(!has_label(&mounted, "Effort max"));
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".to_string(),
            Ok(demo_catalog("session-plan", Harness::ClaudeCode)),
        )]));
    mounted.settle();
    assert!(!has_label(&mounted, "Selected effort"));
    assert!(has_label(&mounted, "Effort max"));
}

#[test]
fn the_header_pause_cell_and_p_hold_and_resume_the_queue() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let session = mounted.model.session.get_untracked();
    let submission = |state: SubmissionState| Submission {
        id: "queued-turn".into(),
        session_id: session.clone(),
        parts: vec![Part::text("Next")],
        state,
        approve_implementation: false,
        error: None,
        interrupts_run: None,
        last_edit_request: None,
        sender: Sender::User,
    };
    mounted
        .model
        .snapshot
        .update(|s| s.submissions.push(submission(SubmissionState::Queued)));
    mounted.settle();
    mounted.click("Pause");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::PauseQueue { ref session_id } if *session_id == session
    ));
    mounted.model.busy.set(false);
    mounted.model.snapshot.update(|s| {
        s.submissions[0].state = SubmissionState::Paused;
        s.revision += 1;
    });
    mounted.settle();
    assert!(!has_label(&mounted, "Pause"));
    mounted.click("Resume");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::ResumeQueue { ref session_id } if *session_id == session
    ));
    mounted.model.busy.set(false);
    // P toggles outside text fields, and types into the draft inside one.
    mounted.focus("Session actions");
    mounted.key(Key::Character("p".into()), false);
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::ResumeQueue { .. }
    ));
    mounted.model.busy.set(false);
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("p".into()), false);
    assert!(mounted.commands.try_recv().is_err());
    // Without queued or held turns there is no pause cell.
    mounted.model.snapshot.update(|s| s.submissions.clear());
    mounted.settle();
    assert!(!has_label(&mounted, "Pause") && !has_label(&mounted, "Resume"));
    let _ = &mut mounted;
}

#[test]
fn window_controls_are_one_ruled_group_apart_from_page_controls() {
    #[cfg(not(target_os = "macos"))]
    {
        let mounted = mount(false, 1380.0);
        mounted.model.page.set(Page::Board);
        mounted.model.issue.set(Some("issue-2".into()));
        mounted.settle();
        let group = mounted.rect("Window controls");
        let close_issue = mounted.rect("Close issue details");
        // The group sits at the window's top-right corner, and the
        // inspector's own close cell ends before it starts.
        assert!((group.origin.x + group.size.width - 1380.0).abs() < 1.0);
        assert!(group.origin.y.abs() < 1.0);
        assert!(close_issue.origin.x + close_issue.size.width <= group.origin.x + 0.5);
        for page in [Page::Sessions, Page::Settings, Page::Profiles] {
            mounted.model.issue.set(None);
            mounted.model.page.set(page);
            mounted.settle();
            assert_eq!(mounted.rect("Window controls"), group, "{page:?}");
        }
    }
}

#[test]
fn director_presets_enable_delegation_even_under_a_worker_parent() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.snapshot.update(|s| {
        let base = s.projects[0].defaults.clone();
        let mut profile = AgentProfile::worker(&base);
        profile.scope = AgentScope::Issues {
            issue_ids: vec!["issue-2".into()],
        };
        profile.execution.approval = ApprovalMode::Ask;
        s.sessions
            .iter_mut()
            .find(|s| s.id == "session-plan")
            .unwrap()
            .profile = Some(profile);
    });
    mounted.model.open_new_worker("session-plan".into());
    assert!(!mounted.model.editor_profile.get_untracked().can_delegate());
    mounted.model.select_agent_template("director".into());
    let profile = mounted.model.editor_profile.get_untracked();
    assert!(profile.can_delegate());
    assert_eq!(profile.execution.approval, ApprovalMode::Ask);
    assert_eq!(
        profile.scope,
        AgentScope::Issues {
            issue_ids: vec!["issue-2".into()]
        }
    );
}

#[test]
fn scoped_root_agents_preserve_their_task_and_request_first_turn_approval() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.snapshot.update(|s| {
        s.sessions.clear();
        s.agent_configs[0].overrides.scope = Some(AgentScope::Issues {
            issue_ids: vec!["issue-2".into()],
        });
        s.agent_configs[0].overrides.permissions = Some({
            let mut permissions = s.projects[0].defaults.permissions.clone();
            permissions.insert(Task::Implement, Permission::Ask);
            permissions
        });
    });
    mounted.model.open_root_agent("director-main".into());
    mounted.settle();
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some("issue-2")
    );
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    type_in(&mounted, &draft, "Review the scoped task");
    crate::buffer::send(mounted.model);
    mounted.settle();
    assert!(mounted.commands.try_recv().is_err());
    assert!(mounted.model.buffer.get_untracked().approval_needed);
    mounted.click("Approve this turn and send");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command, Command::StartRootAgent { issue_id: Some(issue), approve_implementation: true, .. } if issue == "issue-2")
    );
}

#[test]
fn hovering_a_selected_agent_reveals_child_creation_without_reopening_the_parent() {
    let mounted = mount_orchestration(1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Move,
        position: tree_row(&mounted, "Profile design").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    mounted.click("New worker for Profile design");
    assert_eq!(mounted.model.page.get_untracked(), Page::Profiles);
    assert_eq!(
        mounted.model.editor_parent.get_untracked().as_deref(),
        Some("session-plan")
    );
}

#[test]
fn saved_agent_profiles_wait_for_metadata_and_retain_edits_made_during_save() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::New);
    mounted.model.select_agent_template("worker".into());
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    mounted.model.editor_template_name.set("Reviewer".into());
    mounted.model.save_agent_template();
    let envelope = mounted.commands.try_recv().unwrap();
    mounted
        .model
        .modify_profile("execution", |p| p.execution.approval = ApprovalMode::Ask);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        outcome: Some((envelope.request_id.clone(), Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert!(mounted.model.busy.get_untracked());
    assert_eq!(mounted.model.editor_template.get_untracked(), "worker");
    snapshot
        .apply(envelope.command, &envelope.request_id, 0)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(!mounted.model.busy.get_untracked());
    assert_eq!(
        mounted.model.editor_template.get_untracked(),
        format!("profile-{}", envelope.request_id)
    );
    assert_eq!(
        mounted.model.editor_base.get_untracked().execution.approval,
        ApprovalMode::Automatic
    );
    assert_eq!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .execution
            .approval,
        ApprovalMode::Ask
    );
    assert!(
        mounted
            .model
            .editor_overrides
            .get_untracked()
            .execution
            .is_some()
    );
}

fn choose_fixture_profile_model(mounted: &Mounted) {
    mounted.model.modify_profile("model", |profile| {
        profile.model = "fixture-model".into();
    });
    mounted.model.editor_override_accepted.set(true);
    mounted.settle();
}

#[test]
fn new_root_agents_keep_the_selected_profile_and_open_after_metadata_arrives() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::New);
    mounted.model.select_agent_template("worker".into());
    mounted.model.editor_name.set("Independent worker".into());
    choose_fixture_profile_model(&mounted);
    mounted.model.save_profile();
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&envelope.command, Command::CreateAgentConfig { template, .. } if template == "worker")
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        outcome: Some((envelope.request_id.clone(), Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(mounted.model.page.get_untracked(), Page::Profiles);
    assert!(mounted.model.busy.get_untracked());
    snapshot
        .apply(envelope.command, &envelope.request_id, 0)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
    assert!(!mounted.model.busy.get_untracked());
    mounted.model.page.set(Page::Board);
    mounted.settle();
    assert!(has_row(&mounted, "Independent worker"));
    mounted.click("Independent worker");
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
}

#[test]
fn waiting_agent_is_idle_and_can_compose_its_first_turn_without_a_thread() {
    let mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for session in &mut snapshot.sessions {
        if let Some(worker) = &mut session.worker {
            worker.status = WorkerStatus::Completed;
        }
    }
    let session = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == "session-plan")
        .unwrap();
    session.awaiting_first_message = true;
    session.automatic_name = true;
    session.fixture = false;
    session.conversations.clear();
    let worker = session.worker.as_mut().unwrap();
    worker.status = WorkerStatus::Completed;
    worker.thread_id = None;
    worker.worktree = Some("/tmp/relay-waiting-agent-test".into());
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    assert_eq!(crate::buffer::execution_problem(mounted.model), None);
    let snapshot = mounted.model.snapshot.get_untracked();
    assert_eq!(
        crate::adapt::session_status(
            &snapshot,
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == "session-plan")
                .unwrap()
        ),
        relay_ui::prelude::AgentStatus::Idle
    );
    assert!(!has_label(&mounted, "No resumable thread was recorded"));
}

#[test]
fn unnamed_root_agent_opens_a_chat_without_a_task_or_custom_name() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::New);
    mounted.settle();
    choose_fixture_profile_model(&mounted);
    mounted.click("Create agent");
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&envelope.command, Command::CreateAgentConfig { name, .. } if name.is_empty())
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(envelope.command, &envelope.request_id, 0)
        .unwrap();
    let config = snapshot.agent_configs.last().unwrap();
    assert!(config.automatic_name);
    assert_eq!(config.name, "New agent");
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((envelope.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
    assert!(mounted.model.worker_prompt.get_untracked().is_empty());
    assert!(
        !mounted
            .model
            .snapshot
            .get_untracked()
            .messages
            .iter()
            .any(|m| m.session_id == mounted.model.session.get_untracked())
    );
}

#[test]
fn agent_configuration_requires_and_persists_an_explicit_harness_model() {
    let mut mounted = mount_orchestration(1380.0);
    mounted.click("New agent");
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    assert!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .model
            .is_empty()
    );
    mounted.model.save_profile();
    assert!(mounted.commands.try_recv().is_err());
    let choice = HarnessModel {
        id: "claude-haiku-test".into(),
        name: "Haiku Test".into(),
        efforts: vec!["medium".into()],
        default_effort: Some("medium".into()),
        fast: false,
        ..Default::default()
    };
    mounted.model.catalogs.update(|catalogs| {
        catalogs.insert(
            format!(
                "profile:{}:{:?}",
                mounted.model.project.get_untracked(),
                Harness::ClaudeCode
            ),
            Ok(HarnessCatalog {
                harness: Harness::ClaudeCode,
                models: vec![choice],
                ..Default::default()
            }),
        );
    });
    mounted.settle();
    mounted.click("Select profile model");
    mounted.click("Use profile model claude-haiku-test");
    assert_eq!(
        mounted.model.editor_profile.get_untracked().model,
        "claude-haiku-test"
    );
    mounted.click("Override for this agent");
    mounted.model.save_profile();
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&envelope.command, Command::CreateAgentConfig { overrides, .. } if overrides.model.as_deref() == Some("claude-haiku-test"))
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(envelope.command, &envelope.request_id, 1)
        .unwrap();
    let config = snapshot.agent_configs.last().unwrap();
    assert_eq!(
        snapshot.effective_profile(config).unwrap().model.as_str(),
        "claude-haiku-test"
    );
    let config_id = config.id.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_root_agent(config_id);
    assert_eq!(
        mounted.model.harness_selection().model.as_deref(),
        Some("claude-haiku-test")
    );
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::Codex);
    assert!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .model
            .is_empty()
    );
}

#[test]
fn worker_model_editor_covers_both_modes_as_one_override() {
    let mut mounted = mount(false, 1380.0);
    mounted.size = Size::new(1380.0, 2600.0);
    let project = mounted.model.project.get_untracked();
    let model = |id: &str, efforts: &[&str]| HarnessModel {
        id: id.into(),
        name: id.into(),
        efforts: efforts.iter().map(|e| e.to_string()).collect(),
        default_effort: efforts.first().map(|e| e.to_string()),
        fast: false,
        ..Default::default()
    };
    mounted.model.catalogs.update(|catalogs| {
        catalogs.insert(
            format!("profile:{project}:{:?}", Harness::Codex),
            Ok(HarnessCatalog {
                harness: Harness::Codex,
                models: vec![model("gpt-light", &["low", "medium"])],
                ..Default::default()
            }),
        );
        catalogs.insert(
            format!("profile:{project}:{:?}", Harness::ClaudeCode),
            Ok(HarnessCatalog {
                harness: Harness::ClaudeCode,
                models: vec![model("claude-heavy", &["low", "high", "max"])],
                ..Default::default()
            }),
        );
    });
    mounted
        .model
        .open_profile(EditTarget::Config("director-main".into()));
    mounted.settle();
    let policy = |m: &Mounted| m.model.editor_profile.get_untracked().worker_models;
    assert_eq!(policy(&mounted), WorkerModelPolicy::default());
    assert!(has_label(&mounted, "Worker Model field header"));
    assert!(has_label(&mounted, "Worker Model Mode: Delegator Chooses"));
    assert!(has_label(&mounted, "Allow worker model Codex gpt-light"));

    // Delegator Chooses: allowlist across harnesses, cap and considerations.
    mounted.click("Allow worker model Claude Code claude-heavy");
    mounted.click("Maximum Worker Effort: High");
    mounted.click("Consider available usage");
    mounted.click("Consider model capability");
    assert_eq!(
        policy(&mounted),
        WorkerModelPolicy::DelegatorChooses {
            allowed_models: vec![ModelChoice {
                harness: Harness::ClaudeCode,
                model: "claude-heavy".into(),
            }],
            max_effort: Some("high".into()),
            model_capability: false,
            available_usage: true,
        }
    );
    mounted.click("Consider model capability");
    mounted.click("Allow worker model Claude Code claude-heavy");
    mounted.click("Maximum Worker Effort: No Cap");
    assert!(matches!(
        policy(&mounted),
        WorkerModelPolicy::DelegatorChooses { allowed_models, max_effort: None, model_capability: true, available_usage: true } if allowed_models.is_empty()
    ));
    assert_eq!(
        mounted.model.editor_overrides.get_untracked().worker_models,
        Some(policy(&mounted))
    );
    assert!(
        crate::ui::overridden_fields(&mounted.model.editor_overrides.get_untracked())
            .contains(&"Worker Model")
    );

    // Fixed: one harness, model and effort; a harness change clears the model.
    mounted.click("Worker Model Mode: Fixed");
    let profile = mounted.model.editor_profile.get_untracked();
    assert_eq!(
        policy(&mounted),
        WorkerModelPolicy::Fixed {
            harness: profile.harness,
            model: profile.model.clone(),
            effort: None,
        }
    );
    mounted.click("Fixed Worker Harness: Claude Code");
    assert!(matches!(
        policy(&mounted),
        WorkerModelPolicy::Fixed { harness: Harness::ClaudeCode, ref model, effort: None } if model.is_empty()
    ));
    assert!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .validate()
            .is_err()
    );
    mounted.click("Select fixed worker model");
    mounted.click("Use worker model claude-heavy");
    mounted.click("Fixed Worker Effort: Max");
    let fixed = WorkerModelPolicy::Fixed {
        harness: Harness::ClaudeCode,
        model: "claude-heavy".into(),
        effort: Some("max".into()),
    };
    assert_eq!(policy(&mounted), fixed);
    assert!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .validate()
            .is_ok()
    );
    mounted.model.save_profile();
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&envelope.command, Command::UpdateAgentConfig { overrides, .. } if overrides.worker_models.as_ref() == Some(&fixed))
    );

    // Inherit restores the project default.
    mounted.click("Inherit worker_models");
    assert_eq!(policy(&mounted), WorkerModelPolicy::default());
    assert!(
        mounted
            .model
            .editor_overrides
            .get_untracked()
            .worker_models
            .is_none()
    );
    assert!(has_label(&mounted, "Worker Model Mode: Delegator Chooses"));
}

#[test]
fn agent_creation_uses_director_and_worker_profiles_and_preserves_parent_links() {
    let mut mounted = mount_orchestration(1380.0);
    mounted.click("New agent");
    assert!(has_label(&mounted, "New Agent"));
    assert_eq!(mounted.model.editor_template.get_untracked(), "director");
    assert!(mounted.model.editor_profile.get_untracked().can_delegate());
    assert!(mounted.model.editor_parent.get_untracked().is_none());
    mounted.model.editor_name.set("Coordinator".into());
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    mounted.settle();
    assert!(has_label(&mounted, "Override for this agent"));
    assert!(has_label(&mounted, "Save as new profile"));
    mounted.click("Discard profile changes");
    assert_eq!(
        mounted.model.editor_profile.get_untracked(),
        mounted.model.editor_base.get_untracked()
    );
    assert!(!has_label(&mounted, "Discard profile changes"));
    mounted.focus("New worker for Profile design");
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.editor_parent.get_untracked().as_deref(),
        Some("session-plan")
    );
    assert_eq!(mounted.model.editor_template.get_untracked(), "worker");
    assert!(!mounted.model.editor_profile.get_untracked().can_delegate());
    assert!(!has_label(&mounted, "Agent task"));
    assert!(mounted.model.editor_name.get_untracked().is_empty());
    mounted.settle();
    choose_fixture_profile_model(&mounted);
    mounted.click("Create agent");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command, Command::StartChildAgent { parent_session_id, profile, name, prompt, approve_implementation } if parent_session_id == "session-plan" && !profile.can_delegate() && name.is_empty() && prompt.is_empty() && !approve_implementation)
    );
}

#[test]
fn agent_profile_selector_and_named_profile_save_keep_instance_changes() {
    let mut mounted = mount(false, 1380.0);
    mounted.click("New agent");
    mounted.click("Select agent profile");
    mounted.click("Use Worker profile");
    assert_eq!(mounted.model.editor_template.get_untracked(), "worker");
    mounted.model.editor_name.set("Reviewer".into());
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    mounted.settle();
    mounted.click("Override for this agent");
    assert!(mounted.model.editor_override_accepted.get_untracked());
    mounted.click("Save as new profile");
    mounted
        .model
        .editor_template_name
        .set("Claude reviewer".into());
    mounted.settle();
    mounted.click("Save new agent profile");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command, Command::SaveAgentTemplate { name, profile, .. } if name == "Claude reviewer" && profile.harness == Harness::ClaudeCode && !profile.can_delegate())
    );
}

#[test]
fn project_menu_summarizes_running_agents_and_updates_without_reopening() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.update(|snapshot| {
        snapshot.tool_permissions.clear();
        snapshot.submissions.clear();
        snapshot.sessions.truncate(2);
        for session in &mut snapshot.sessions {
            session.worker = Some(WorkerRun {
                model: None,
                context_tokens: None,
                context_window: None,
                last_usage: None,
                harness: Harness::Codex,
                execution: None,
                status: WorkerStatus::Running,
                thread_id: None,
                worktree: None,
                branch: None,
                base_commit: None,
                error: None,
                usage: None,
                changes: None,
            });
        }
        let mut other = snapshot.projects[0].clone();
        other.id = "other".into();
        other.name = "Other project".into();
        snapshot.projects.push(other);
        let mut session = snapshot.sessions[0].clone();
        session.id = "other-session".into();
        session.project_id = "other".into();
        session.title = "Other agent".into();
        snapshot.sessions.push(session);
    });
    mounted.settle();
    open_project_menu(&mounted);
    let images = |name: &str| {
        let rect = mounted.rect(&format!("Project row {name}"));
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter(|n| n.role == Role::Image && rect.contains(n.rect.center()))
            .filter_map(|n| n.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(images("Relay · demo"), vec!["2 working"]);
    assert_eq!(images("Other project"), vec!["1 working"]);
    assert!(has_label(&mounted, "3 agents"));
    assert!(!has_row(&mounted, "Other agent"));
    assert!(!has_label(&mounted, "Director row Project director"));
    mounted.model.snapshot.update(|snapshot| {
        snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
    });
    mounted.settle();
    assert_eq!(images("Relay · demo"), vec!["1 working"]);
    mounted.model.snapshot.update(|snapshot| {
        snapshot.sessions[1].worker.as_mut().unwrap().status = WorkerStatus::Queued;
    });
    mounted.settle();
    assert!(images("Relay · demo").is_empty());
    assert_eq!(images("Other project"), vec!["1 working"]);
    mounted.click("Select project Other project");
    assert!(!has_label(&mounted, "Projects"));
    assert!(has_row(&mounted, "Other agent"));
    assert!(!has_row(&mounted, "Profile design"));
}

#[test]
fn project_sidebar_summaries_keep_the_selected_agent_tree_indented() {
    let mounted = mount(false, 1380.0);
    mounted.click("Toggle project Relay · demo");
    let project = mounted.rect("Select project Relay · demo");
    assert!(!has_label(&mounted, "Director row Project director"));
    assert!(!has_label(&mounted, "Open director Project director"));
    // The 60px agents header starts right under the project menu; its New
    // agent cell is centred in it.
    let new_project = mounted.rect("New Project");
    let header_top = new_project.origin.y + new_project.size.height;
    assert!((mounted.rect("New agent").center().y - (header_top + 30.0)).abs() < 1.0);
    let root = tree_row(&mounted, "Profile design");
    // The tree keeps its normal 12px row padding plus the project's elbow.
    assert!(root.origin.x >= project.origin.x);
    let snapshot = mounted.ui.inspection_snapshot();
    let title = snapshot
        .nodes
        .iter()
        .find(|n| {
            n.label.as_deref() == Some("Profile design")
                && n.role == Role::Label
                && root.contains(n.rect.center())
        })
        .unwrap();
    let indented_x = title.rect.origin.x;
    assert!(indented_x >= project.origin.x + 14.0);
    mounted.model.agent_focus.set(Some("session-plan".into()));
    mounted.settle();
    let root = tree_row(&mounted, "Profile design");
    let snapshot = mounted.ui.inspection_snapshot();
    let focused_title = snapshot
        .nodes
        .iter()
        .find(|n| {
            n.label.as_deref() == Some("Profile design")
                && n.role == Role::Label
                && root.contains(n.rect.center())
        })
        .unwrap();
    assert!((indented_x - focused_title.rect.origin.x - 14.0).abs() < 0.1);
}

#[test]
fn connection_panel_is_embedded_and_shrinks_the_tree() {
    let mounted = mount(false, 1380.0);
    mounted.model.round_trip_ms.set(Some(57));
    mounted.settle();
    let viewport = |mounted: &Mounted| {
        let snapshot = mounted.ui.inspection_snapshot();
        let mut node = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Agent tree"))
            .unwrap();
        while node.role != Role::ScrollView {
            node = snapshot
                .nodes
                .iter()
                .find(|n| Some(n.id) == node.parent)
                .unwrap();
        }
        node.rect
    };
    let before = viewport(&mounted);
    mounted.click("Connection: Connected, 57 ms. Show details");
    let sidebar = mounted.rect("Sidebar");
    let panel = mounted.rect("Connection details");
    let after = viewport(&mounted);
    assert!(after.size.height < before.size.height - panel.size.height + 1.0);
    assert!(
        panel.origin.x >= sidebar.origin.x
            && panel.origin.x + panel.size.width <= sidebar.origin.x + sidebar.size.width + 0.5
    );
    assert!(
        panel.origin.y >= sidebar.origin.y
            && panel.origin.y + panel.size.height <= sidebar.origin.y + sidebar.size.height
    );
    mounted
        .model
        .connection_telemetry
        .set((vec![57, 91], Some(1_000), Some(u64::MAX)));
    mounted.model.connected.set(false);
    mounted
        .model
        .status
        .set("Connection lost · retrying".into());
    mounted.settle();
    assert!(has_label(&mounted, "Connection details"));
    assert!(has_label(&mounted, "WebSocket · closed"));
    assert!(has_label(&mounted, "last 91 ms"));
    assert!(!has_label(&mounted, "Connected"));
    mounted
        .model
        .connection_telemetry
        .update(|health| health.2 = None);
    mounted.model.status.set("Offline".into());
    mounted.settle();
    assert!(has_label(&mounted, "Offline"));
    assert!(has_label(&mounted, "Connection details"));
}

#[test]
fn relay_context_messages_use_the_relay_mark_not_a_harness_glyph() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    // Harness glyphs are images named after their harness; count those in
    // the reading column (right of the sidebar).
    let glyphs = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter(|n| {
                n.rect.origin.x > 400.0
                    && matches!(n.label.as_deref(), Some("Codex" | "Claude Code"))
            })
            .count()
    };
    let before = glyphs(&mounted);
    mounted.model.snapshot.update(|snapshot| {
        snapshot.messages.push(Message {
            id: "context-relay".into(),
            session_id: "session-plan".into(),
            author: "Relay".into(),
            sender: Sender::User,
            kind: "issue-context".into(),
            body: "{\"issue\":\"#2\"}".into(),
            parts: vec![],
            tool: None,
        });
    });
    mounted.settle();
    assert!(
        has_label(&mounted, "Relay"),
        "Relay speaks for its own context"
    );
    assert!(has_label(&mounted, "Context Sent to Agent"));
    assert!(!has_label(&mounted, "Source context"));
    assert_eq!(glyphs(&mounted), before, "no harness glyph for Relay");
}

#[test]
fn effort_selection_feedback_tracks_the_command_response_and_chat() {
    for (accepted, lagging) in [(false, false), (true, false), (true, true)] {
        let mut mounted = mount(false, 1380.0);
        let id = "session-plan";
        mounted.model.open_session(id.into());
        mounted.model.snapshot.update(|snapshot| {
            let session = snapshot.sessions.iter_mut().find(|s| s.id == id).unwrap();
            session.selection.model = Some("gpt-6.1-sol".into());
            session.selection.effort = Some("medium".into());
        });
        mounted
            .model
            .catalogs
            .set(std::collections::BTreeMap::from([(
                id.into(),
                Ok(HarnessCatalog {
                    models: vec![HarnessModel {
                        id: "gpt-6.1-sol".into(),
                        name: "Sol".into(),
                        efforts: vec!["low".into(), "medium".into(), "high".into()],
                        default_effort: Some("medium".into()),
                        fast: false,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            )]));
        crate::transcript::choose_effort(mounted.model, 2);
        let request = mounted.commands.try_recv().unwrap();
        assert_eq!(
            crate::transcript::ledger_view(mounted.model, true).pending_effort,
            Some(2)
        );
        assert_eq!(mounted.model.pending_effort("session-worker"), None);
        let mut snapshot = mounted.model.snapshot.get_untracked();
        if accepted && !lagging {
            snapshot
                .sessions
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .selection
                .effort = Some("high".into());
        }
        let acknowledged_revision = snapshot.revision + u64::from(lagging);
        mounted.model.receive(NetworkState {
            acknowledged_revision,
            snapshot,
            connected: true,
            outcome: Some((
                request.request_id,
                if accepted {
                    Ok(())
                } else {
                    Err("Rejected".into())
                },
            )),
            outcome_serial: 1,
            ..Default::default()
        });
        let ledger = crate::transcript::ledger_view(mounted.model, true);
        assert_eq!(ledger.pending_effort, None);
        assert_eq!(ledger.effort.current, if accepted { 2 } else { 1 });
        if lagging {
            let mut snapshot = mounted.model.snapshot.get_untracked();
            snapshot.revision = acknowledged_revision;
            snapshot
                .sessions
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .selection
                .effort = Some("high".into());
            mounted.model.receive(NetworkState {
                snapshot,
                acknowledged_revision,
                connected: true,
                ..Default::default()
            });
            assert_eq!(mounted.model.acknowledged_effort(id), None);
            assert_eq!(
                crate::transcript::ledger_view(mounted.model, true)
                    .effort
                    .current,
                2
            );
        }
    }
}

#[test]
fn deleted_branch_clears_local_drafts_and_leaves_unrelated_drafts_intact() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let branch = snapshot.session_branch_ids("session-plan");
    let other = snapshot
        .sessions
        .iter()
        .find(|s| !branch.contains(&s.id))
        .unwrap()
        .id
        .clone();
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("remove draft")],
    );
    crate::buffer::edit(mounted.model, &other, vec![Part::text("keep draft")]);
    for session in &mut snapshot.sessions {
        if let Some(worker) = &mut session.worker {
            worker.status = WorkerStatus::Completed;
        }
    }
    snapshot
        .apply(
            Command::SetSessionArchived {
                session_id: "session-plan".into(),
                archived: true,
            },
            "archive",
            1,
        )
        .unwrap();
    snapshot
        .apply(
            Command::DeleteSessionBranch {
                session_id: "session-plan".into(),
            },
            "delete",
            2,
        )
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(
        !mounted
            .model
            .buffer
            .get_untracked()
            .documents
            .contains_key("session-plan")
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &other)),
        "keep draft"
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
}

#[test]
fn archived_chat_is_hidden_by_default_readable_when_shown_and_cannot_send() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.update(|snapshot| {
        let session = snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "session-plan")
            .unwrap();
        session.archived = true;
        session.title = "Archived transcript test".into();
    });
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let tree_contains = |mounted: &Mounted| {
        mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
            n.label.as_deref() == Some("Archived transcript test") && n.role == Role::ListItem
        })
    };
    assert!(!tree_contains(&mounted));
    mounted.click("Show archived chats");
    assert!(tree_contains(&mounted));
    assert!(has_label(
        &mounted,
        "This chat has been archived. Unarchive it to continue."
    ));
    let archived = mounted.rect("Archived chat message");
    let transcript = mounted.rect("Transcript");
    assert!(archived.origin.y >= transcript.origin.y);
    assert!(
        archived.origin.y + archived.size.height
            <= transcript.origin.y + transcript.size.height + 1.0
    );
    assert!(archived.origin.y + archived.size.height <= mounted.rect("Draft boundary").origin.y);
    crate::buffer::send(mounted.model);
    assert!(mounted.commands.try_recv().is_err());
    assert!(mounted.model.notice.get_untracked().contains("Unarchive"));
    let row = mounted.rect("Archived transcript test");
    let delete = mounted.rect("Delete Archived transcript test");
    let edit = mounted.rect("Edit Archived transcript test");
    assert_eq!(edit.origin.x, row.max_x());
    assert_eq!(edit.origin.x, mounted.rect("Sidebar").max_x());
    assert_eq!(delete.origin.x, edit.max_x());
    assert_eq!(delete.size.width, row.size.height);
    assert_eq!(delete.size.height, row.size.height);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Move,
        position: row.center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    // Exercise the mounted desktop's Sidebar clip and scroll clip, with an
    // opaque ordinary sibling painted later at the overflow coordinates.
    let sibling_clicks = State::new(0);
    let sibling_fill = Color::new(1.0, 0.0, 1.0, 1.0);
    let sibling = mounted
        .ui
        .root()
        .child(Style::default().size(delete.size.width, delete.size.height));
    sibling.translate(delete.origin);
    sibling.visual(mosaic::widgets::Visual::new().fill(sibling_fill));
    sibling.on_pointer(move |event, _| {
        if matches!(event.kind, PointerEventKind::Click(PointerButton::Primary)) {
            sibling_clicks.update(|count| *count += 1);
        }
    });
    mounted.settle();
    assert_eq!(sibling.layout_rect(), delete);
    let scene = mounted.ui.scene();
    let mut clips = Vec::new();
    let mut sibling_paint = None;
    let mut action_paint = None;
    for (index, command) in scene.cmds.iter().enumerate() {
        match command {
            mosaic::render::PaintCmd::PushClip(clip) => clips.push(*clip),
            mosaic::render::PaintCmd::PopClip => {
                clips.pop();
            }
            mosaic::render::PaintCmd::Shape(shape) if shape.rect == delete => {
                if shape.fill.as_solid() == Some(sibling_fill) {
                    sibling_paint = Some(index);
                } else if shape.fill.as_solid().is_some() {
                    assert!(
                        clips.iter().all(|clip| clip.contains(delete.center())),
                        "Desktop ancestors must not clip the overflowing action"
                    );
                    action_paint = Some(index);
                }
            }
            _ => {}
        }
    }
    assert!(
        action_paint.unwrap() > sibling_paint.unwrap(),
        "Session actions must paint above later ordinary desktop siblings"
    );
    drop(scene);
    mounted.click("Delete Archived transcript test");
    assert!(mounted.commands.try_recv().is_err());
    assert_eq!(
        mounted.rect("Confirm deletion of Archived transcript test"),
        delete
    );
    mounted.click("Confirm deletion of Archived transcript test");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::DeleteSessionBranch { session_id } if session_id == "session-plan"
    ));
    assert_eq!(sibling_clicks.get_untracked(), 0);
    sibling.remove();
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "session-plan")
            .unwrap()
            .archived = false
    });
    mounted.settle();
    assert!(!has_label(
        &mounted,
        "This chat has been archived. Unarchive it to continue."
    ));
}

#[test]
fn draft_shares_the_transcript_viewport_and_has_a_full_width_boundary() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let viewport = mounted.rect("Transcript viewport");
    let boundary = mounted.rect("Draft boundary");
    assert_eq!(boundary.size.width, viewport.size.width);
    assert_eq!(boundary.origin.x, viewport.origin.x);
    let editor = mounted.rect("Next message");
    assert!(mounted.rect("Send message").origin.x >= editor.origin.x + editor.size.width);
    let ledger_height = mounted.rect("Agent ledger").size.height;
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Draft line\n".repeat(40))],
    );
    mounted.settle();
    assert!(
        (mounted.rect("Agent ledger").size.height - ledger_height).abs() < 1.0,
        "ledger grew from {ledger_height} to {:?}",
        mounted.rect("Agent ledger")
    );
    assert!(mounted.rect("Next message").size.height > ledger_height);
    let controller = mounted.model.ui.get_untracked().inspection_snapshot();
    assert!(
        !controller
            .nodes
            .iter()
            .any(|node| node.label.as_deref() == Some("Composer dock"))
    );
}

#[test]
fn prelaunch_model_and_effort_render_before_any_message_is_sent() {
    let mounted = mount(false, 1380.0);
    let config = mounted.model.snapshot.get_untracked().agent_configs[0]
        .id
        .clone();
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .retain(|s| s.agent_config_id != config || s.role != SessionRole::Root);
        let config = snapshot
            .agent_configs
            .iter_mut()
            .find(|c| c.id == config)
            .unwrap();
        config.overrides.harness = Some(Harness::ClaudeCode);
        config.draft_selection = HarnessSelection {
            model: Some("claude-haiku-5-5".into()),
            effort: Some("low".into()),
            ..Default::default()
        };
    });
    mounted.model.open_root_agent(config.clone());
    let draft = format!("agent-draft-{config}");
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            draft.clone(),
            Ok(HarnessCatalog {
                harness: Harness::ClaudeCode,
                models: vec![HarnessModel {
                    id: "claude-haiku-5-5".into(),
                    name: "Haiku".into(),
                    efforts: vec!["low".into(), "medium".into(), "high".into()],
                    default_effort: Some("low".into()),
                    fast: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let ledger = crate::transcript::ledger_view(mounted.model, true);
    assert_eq!(ledger.model.short_name(), "Haiku 5.5");
    assert_eq!(ledger.effort.name(), "low");
    assert!(has_label(&mounted, "Model: CLAUDE Haiku 5.5. Change model"));
    assert!(mounted.commands.is_empty());
    assert!(
        !mounted
            .model
            .snapshot
            .get_untracked()
            .sessions
            .iter()
            .any(|s| s.id == draft)
    );
}

#[test]
fn unused_chat_deletion_requires_confirmation_and_recovers_selection() {
    for profile_open in [false, true] {
        let mut mounted = mount(false, 1380.0);
        let mut snapshot = mounted.model.snapshot.get_untracked();
        let mut unused = snapshot.agent_configs[0].clone();
        unused.id = "unused".into();
        unused.name = "New director".into();
        snapshot.agent_configs.push(unused);
        snapshot.sessions.retain(|s| s.id == "session-plan");
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            ..Default::default()
        });
        mounted.model.open_root_agent("unused".into());
        let draft_id = mounted.model.session.get_untracked();
        crate::buffer::edit(mounted.model, &draft_id, vec![Part::text("Draft prompt")]);
        crate::buffer::edit(
            mounted.model,
            "session-plan",
            vec![Part::text("Keep draft")],
        );
        if profile_open {
            mounted
                .model
                .open_profile(EditTarget::Config("unused".into()));
        }
        mounted.settle();
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect("New director").center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        // Unstarted chats offer the same confirmed deletion control as archived chats.
        let row = mounted.rect("New director");
        let delete = mounted.rect("Delete New director");
        assert!(delete.origin.x >= row.max_x());
        assert!(delete.origin.x >= mounted.rect("Sidebar").max_x());
        assert_eq!(delete.size.width, row.size.height);
        assert_eq!(delete.size.height, row.size.height);
        mounted.click("Delete New director");
        assert!(mounted.commands.try_recv().is_err());
        assert_eq!(mounted.rect("Confirm deletion of New director"), delete);
        mounted.click("Confirm deletion of New director");
        let request = mounted.commands.try_recv().unwrap();
        assert!(
            matches!(&request.command, Command::RemoveAgentConfig { agent_config_id } if agent_config_id == "unused")
        );
        mounted.model.receive(NetworkState {
            snapshot: mounted.model.snapshot.get_untracked(),
            connected: true,
            outcome: Some((request.request_id.clone(), Err("Revision changed".into()))),
            outcome_serial: 1,
            ..Default::default()
        });
        mounted.settle();
        assert_eq!(
            plain_text(&crate::buffer::parts(mounted.model, &draft_id)),
            "Draft prompt"
        );
        mounted.click("Confirm deletion of New director");
        let retry = mounted.commands.try_recv().unwrap();
        let mut snapshot = mounted.model.snapshot.get_untracked();
        snapshot.apply(retry.command, &retry.request_id, 1).unwrap();
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            outcome: Some((retry.request_id, Ok(()))),
            outcome_serial: 2,
            ..Default::default()
        });
        mounted.settle();
        assert_eq!(mounted.model.page.get_untracked(), Page::Board);
        assert!(
            !mounted
                .model
                .buffer
                .get_untracked()
                .documents
                .contains_key(&draft_id)
        );
        assert_eq!(
            plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
            "Keep draft"
        );
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Delete New director"))
        );
    }
}

#[test]
fn incoming_messages_keep_the_focused_input_and_send_control_visible() {
    for (width, height) in [(820.0, 900.0), (1380.0, 640.0)] {
        let mut mounted = mount(false, width);
        mounted.size.height = height;
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|snapshot| {
            snapshot.messages.push(Message {
                id: "focused-stream".into(),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: "line\n".repeat(80),
                parts: vec![],
                tool: None,
            });
        });
        mounted.settle();
        mounted.click("Next message");
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit("Draft stays here".into()));
        mounted.settle();
        let input = mounted.rect("Next message");
        let send = mounted.rect("Send message");
        for append in [false, true, false] {
            mounted.model.snapshot.update(|snapshot| {
                if append {
                    snapshot.messages.push(Message {
                        id: "focused-new-message".into(),
                        session_id: "session-plan".into(),
                        author: "Agent".into(),
                        sender: Sender::User,
                        kind: "assistant".into(),
                        body: "new message\n".repeat(30),
                        parts: vec![],
                        tool: None,
                    });
                } else {
                    snapshot
                        .messages
                        .iter_mut()
                        .find(|m| m.id == "focused-stream")
                        .unwrap()
                        .body
                        .push_str(&"streamed line\n".repeat(40));
                }
            });
            mounted.settle();
            let current = mounted.rect("Next message");
            assert!(
                (current.origin.y - input.origin.y).abs() < 2.0,
                "input shifted at width {width}: {input:?} -> {current:?}"
            );
            let current_send = mounted.rect("Send message");
            assert!((current_send.origin.y - send.origin.y).abs() < 2.0);
            assert!(current_send.max_y() <= mounted.size.height);
            assert!(current.max_y() <= mounted.size.height);
            assert_eq!(
                plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
                "Draft stays here"
            );
        }
    }
}

fn draft_labels(mounted: &Mounted) -> Vec<String> {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter_map(|node| node.label.filter(|label| label.starts_with("Draft text")))
        .collect()
}

#[test]
fn pasted_image_splits_text_at_the_cursor_and_supports_typing_and_arrows_on_both_sides() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = draft_labels(&mounted).remove(0);
    type_in(&mounted, &label, "Before λAfter");
    mounted.ui.dispatch_ime(ImeEvent::SetSelection {
        start: "Before λ".len(),
        end: "Before λ".len(),
    });
    let target = crate::conversation::attachment_target(mounted.model).unwrap();
    let image = Part {
        id: "inline-image".into(),
        kind: PartKind::Asset {
            asset: Asset {
                id: "image-asset".into(),
                name: "image.png".into(),
                media_type: "image/png".into(),
                size: 1,
            },
        },
    };
    target.insert(mounted.model, vec![image.clone()]).unwrap();
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(parts.len(), 3);
    assert_eq!(plain_text(&parts[..1]), "Before λ");
    assert_eq!(parts[1], image);
    assert_eq!(plain_text(&parts[2..]), "After");
    assert_eq!(mounted.ui.text_input_contents().unwrap().selection, (0, 0));
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("Continued ".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")[2..]),
        "Continued After"
    );
    mounted.key(Key::Home, false);
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(mounted.ui.text_input_contents().unwrap().text, "Before λ");
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.ui.text_input_contents().unwrap().text,
        "Continued After"
    );
    mounted.key(Key::Character("z".into()), true);
    mounted.key(Key::Character("z".into()), true);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Before λAfter"
    );
    assert!(
        !crate::buffer::parts(mounted.model, "session-plan")
            .iter()
            .any(|p| matches!(p.kind, PartKind::Asset { .. }))
    );
    mounted.key(Key::Character("y".into()), true);
    assert_eq!(
        crate::buffer::parts(mounted.model, "session-plan")[1],
        image
    );
}

#[test]
fn drafts_ending_in_an_image_have_a_cursor_position_after_the_image() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![
            Part::text("Before"),
            Part {
                id: "last-image".into(),
                kind: PartKind::Asset {
                    asset: Asset {
                        id: "last-asset".into(),
                        name: "last.png".into(),
                        media_type: "image/png".into(),
                        size: 1,
                    },
                },
            },
        ],
    );
    mounted.settle();
    let labels = draft_labels(&mounted);
    assert_eq!(labels.len(), 2);
    mounted.focus(&labels[0]);
    mounted.key(Key::End, false);
    mounted.key(Key::ArrowRight, false);
    assert_eq!(mounted.ui.text_input_contents().unwrap().text, "");
    mounted.ui.dispatch_ime(ImeEvent::Commit("After".into()));
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(parts.len(), 3);
    assert!(matches!(&parts[1].kind, PartKind::Asset { .. }));
    assert_eq!(plain_text(&parts[2..]), "After");
}

#[test]
fn asynchronous_attachment_keeps_its_captured_selection_and_rejects_changed_drafts() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = draft_labels(&mounted).remove(0);
    type_in(&mounted, &label, "Before selected After");
    mounted
        .ui
        .dispatch_ime(ImeEvent::SetSelection { start: 7, end: 15 });
    let target = crate::conversation::attachment_target(mounted.model).unwrap();
    mounted
        .ui
        .dispatch_ime(ImeEvent::SetSelection { start: 0, end: 0 });
    target
        .insert(mounted.model, vec![Part::text("inserted")])
        .unwrap();
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Before inserted After"
    );
    let pending = crate::conversation::attachment_target(mounted.model).unwrap();
    mounted.key(Key::Character("a".into()), true);
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("changed anchor".into()));
    mounted.settle();
    let before = crate::buffer::parts(mounted.model, "session-plan");
    assert!(
        pending
            .insert(mounted.model, vec![Part::text("stale")])
            .is_err()
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), before);
    let pending = crate::conversation::attachment_target(mounted.model).unwrap();
    mounted.model.open_session("session-review".into());
    mounted.settle();
    assert!(
        pending
            .insert(mounted.model, vec![Part::text("wrong chat")])
            .is_err()
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), before);
}

#[test]
fn typing_during_image_read_is_retained_after_the_image() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = draft_labels(&mounted).remove(0);
    type_in(&mounted, &label, "Before ");
    let target = crate::conversation::attachment_target(mounted.model).unwrap();
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("after image".into()));
    mounted.settle();
    let image = Part {
        id: "pending-image".into(),
        kind: PartKind::Asset {
            asset: Asset {
                id: "pending-asset".into(),
                name: "pending.png".into(),
                media_type: "image/png".into(),
                size: 1,
            },
        },
    };
    target.insert(mounted.model, vec![image.clone()]).unwrap();
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(plain_text(&parts[..1]), "Before ");
    assert_eq!(parts[1], image);
    assert_eq!(plain_text(&parts[2..]), "after image");
    mounted.ui.dispatch_ime(ImeEvent::Commit(" more".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")[2..]),
        "after image more"
    );
}

#[test]
fn queued_gutter_actions_fit_short_messages_and_work_with_keyboard_at_each_scale() {
    for scale in [1.0, 1.5, 2.0] {
        let mut mounted = mount(false, 1800.0);
        mounted.model.preferences.update(|p| p.scale = scale);
        install_theme(&theme(ThemeVariant::Dark, scale));
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|snapshot| {
            snapshot.messages.retain(|m| m.session_id != "session-plan");
            snapshot.submissions.push(Submission {
                id: "compact-queue".into(),
                session_id: "session-plan".into(),
                parts: vec![Part::text("Short message")],
                state: SubmissionState::Queued,
                approve_implementation: false,
                error: None,
                interrupts_run: None,
                last_edit_request: None,
                sender: Sender::User,
            });
        });
        mounted.settle();
        let labels = [
            "Cancel queued message",
            "Edit queued message",
            "Send queued message now",
        ];
        let queued = mounted.rect("Queued message");
        let text = mounted.rect("Short message");
        let number = mounted.rect("01");
        assert!(
            (queued.size.height - 80.0 * scale).abs() < 0.1,
            "{queued:?}"
        );
        let tree = mounted.ui.inspection_snapshot();
        let number_node = tree
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("01"))
            .unwrap();
        let number_cell = tree.node(number_node.parent.unwrap()).unwrap().rect;
        assert!((number.center().y - number_cell.center().y).abs() < 0.1);
        assert!((number.center().y - (queued.origin.y + 10.0 * scale)).abs() < 0.1);
        let number_glyphs: Vec<_> = mounted
            .ui
            .scene()
            .cmds
            .iter()
            .filter_map(|command| match command {
                mosaic::render::PaintCmd::Glyphs(run) => Some(&run.glyphs),
                _ => None,
            })
            .flatten()
            .filter(|glyph| number_cell.contains(glyph.dest.center()))
            .map(|glyph| glyph.dest)
            .collect();
        assert_eq!(number_glyphs.len(), 2, "expected both painted digits");
        for glyph in number_glyphs {
            assert!(
                (glyph.center().y - number_cell.center().y).abs() < 0.1,
                "scale={scale}: number glyph={glyph:?}, cell={number_cell:?}"
            );
        }
        // Inspect the rendered SVG leaf, not just its full-height wrapper or hit target.
        for label in labels {
            let button = tree
                .nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(label))
                .unwrap();
            let mut descendants = button.children.clone();
            let mut glyphs = Vec::new();
            while let Some(id) = descendants.pop() {
                let node = tree.node(id).unwrap();
                descendants.extend(&node.children);
                if node.role == Role::Image {
                    glyphs.push(node.rect);
                }
            }
            assert_eq!(glyphs.len(), 1, "{label}: expected one SVG glyph");
            let glyph = glyphs[0];
            assert!((glyph.size.height - 12.0 * scale).abs() < 0.1);
            assert!((glyph.center().x - button.rect.center().x).abs() < 0.1);
            assert!(
                (glyph.center().y - button.rect.center().y).abs() < 0.1,
                "scale={scale}, {label}: glyph={glyph:?}, button={:?}",
                button.rect
            );
        }
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect(labels[0]).center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        mounted.ui.tick(Duration::from_millis(600));
        mounted.settle();
        assert!(
            mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|node| node.role == Role::Tooltip && node.label.as_deref() == Some(labels[0]))
        );
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: Vector2::ZERO,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        let mut bottom = queued.origin.y + 20.0 * scale;
        for label in labels {
            let button = mounted.rect(label);
            assert!(
                (button.origin.y - bottom).abs() < 0.1,
                "{label}: {button:?}"
            );
            assert!((button.origin.x - queued.origin.x).abs() < 0.1);
            assert!((button.size.width - 34.0 * scale).abs() < 0.1);
            assert!(button.max_x() < text.origin.x, "{label} stays in gutter");
            assert!(button.origin.x >= queued.origin.x && button.max_y() <= queued.max_y() + 0.1);
            assert!(
                (button.size.height - 20.0 * scale).abs() < 0.1,
                "scale={scale}, button={button:?}"
            );
            assert!((button.center().x - number.center().x).abs() < 0.1);
            bottom = button.max_y();
            mounted.focus(label);
            assert_eq!(focused_label(&mounted).as_deref(), Some(label));
            assert!(
                mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .iter()
                    .any(|node| node.role == Role::Tooltip && node.label.as_deref() == Some(label))
            );
            assert_control_hit(&mounted, label);
        }
        // Tall content stretches only Send Now to the message bottom.
        mounted.model.snapshot.update(|snapshot| {
            snapshot.submissions.last_mut().unwrap().parts =
                vec![Part::text("Long queued content. ".repeat(30))];
        });
        mounted.settle();
        let tall = mounted.rect("Queued message");
        assert!(tall.size.height > queued.size.height);
        let content = mounted.rect(&"Long queued content. ".repeat(30));
        assert!(
            tall.size.height <= content.size.height + 80.0 * scale,
            "message must hug its content: message={tall:?}, content={content:?}"
        );
        for label in &labels[..2] {
            let button = mounted.rect(label);
            assert!((button.size.height - 20.0 * scale).abs() < 0.1);
            assert!((button.size.width - 34.0 * scale).abs() < 0.1);
        }
        let send = mounted.rect(labels[2]);
        assert!((send.origin.y - (tall.origin.y + 60.0 * scale)).abs() < 0.1);
        assert!((send.max_y() - tall.max_y()).abs() < 0.1);
        assert!(send.size.height >= 20.0 * scale);
        assert!((send.size.width - 34.0 * scale).abs() < 0.1);
        let tree = mounted.ui.inspection_snapshot();
        let button = tree
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(labels[2]))
            .unwrap();
        let mut descendants = button.children.clone();
        let mut icons = Vec::new();
        while let Some(id) = descendants.pop() {
            let node = tree.node(id).unwrap();
            descendants.extend(&node.children);
            if node.role == Role::Image {
                icons.push(node.rect);
            }
        }
        assert_eq!(icons.len(), 1);
        assert!((icons[0].center().y - send.center().y).abs() < 0.1);
        assert!((icons[0].center().x - send.center().x).abs() < 0.1);
        assert!((icons[0].size.height - 12.0 * scale).abs() < 0.1);
        // Click the expanded lower region, away from the original 20px cell.
        let position = Vector2::new(send.max_x() - 2.0 * scale, send.max_y() - 2.0 * scale);
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        mounted.settle();
        assert!(matches!(mounted.commands.try_recv().unwrap().command,
            Command::PromoteTurn { submission_id, .. } if submission_id == "compact-queue"));
        mounted.model.busy.set(false);
        mounted.model.snapshot.update(|snapshot| {
            snapshot.submissions.last_mut().unwrap().parts = vec![Part::text("Short message")];
        });
        mounted.settle();
        // Focus traverses the three controls in their visual order.
        mounted.focus(labels[0]);
        mounted.key(Key::Tab, false);
        assert_eq!(focused_label(&mounted).as_deref(), Some(labels[1]));
        mounted.key(Key::Tab, false);
        assert_eq!(focused_label(&mounted).as_deref(), Some(labels[2]));
        mounted.key(Key::Enter, false);
        assert!(matches!(mounted.commands.try_recv().unwrap().command,
            Command::PromoteTurn { submission_id, .. } if submission_id == "compact-queue"));
        mounted.model.busy.set(false);
        mounted.focus(labels[0]);
        mounted.key(Key::Enter, false);
        assert!(matches!(mounted.commands.try_recv().unwrap().command,
            Command::CancelTurn { submission_id } if submission_id == "compact-queue"));
        mounted.model.busy.set(false);
        mounted.focus(labels[1]);
        mounted.key(Key::Enter, false);
        assert_eq!(
            plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
            "Short message"
        );
    }
}

#[test]
fn queued_and_paused_messages_render_images_between_their_text_and_preserve_edits() {
    for state in [SubmissionState::Queued, SubmissionState::Paused] {
        let mounted = mount(false, 1800.0);
        let scale = if state == SubmissionState::Paused {
            1.5
        } else {
            1.0
        };
        mounted.model.preferences.update(|p| p.scale = scale);
        install_theme(&theme(ThemeVariant::Dark, scale));
        mounted.model.open_session("session-plan".into());
        mounted
            .model
            .snapshot
            .update(|snapshot| snapshot.messages.retain(|m| m.session_id != "session-plan"));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([120, 170, 200, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let picture = crate::buffer::insert_asset(
            mounted.model,
            "queued.png".into(),
            "image/png".into(),
            bytes.into_inner(),
        )
        .unwrap();
        let parts = vec![
            Part::text("Text before the queued image"),
            picture,
            Part::text("Text after the queued image"),
            crate::buffer::insert_asset(
                mounted.model,
                "queued.txt".into(),
                "text/plain".into(),
                b"Attachment contents".to_vec(),
            )
            .unwrap(),
            Part {
                id: "queued-long-reply".into(),
                kind: PartKind::Reply {
                    anchor: relay_core::Anchor {
                        message_id: "source".into(),
                        start: 0,
                        end: 5,
                        quote: "Quote".into(),
                    },
                    parts: vec![Part::text(
                        "A long queued reply with enough words to wrap across several lines. "
                            .repeat(8),
                    )],
                },
            },
        ];
        mounted.model.snapshot.update(|snapshot| {
            snapshot.submissions.push(Submission {
                id: "queued-picture".into(),
                session_id: "session-plan".into(),
                parts: parts.clone(),
                state,
                approve_implementation: false,
                error: None,
                interrupts_run: None,
                last_edit_request: None,
                sender: Sender::User,
            })
        });
        mounted.settle();
        let queued = mounted.rect("Queued message");
        let before = mounted.rect("Text before the queued image");
        let after = mounted.rect("Text after the queued image");
        let tree = mounted.ui.inspection_snapshot();
        let attachment = tree
            .nodes
            .iter()
            .find(|node| node.label.as_deref() == Some("Attachment queued.png"))
            .unwrap()
            .id;
        let images: Vec<_> = tree
            .nodes
            .iter()
            .filter(|node| {
                if node.role != Role::Image {
                    return false;
                }
                let mut parent = node.parent;
                while let Some(id) = parent {
                    if id == attachment {
                        return true;
                    }
                    parent = tree
                        .nodes
                        .iter()
                        .find(|node| node.id == id)
                        .and_then(|node| node.parent);
                }
                false
            })
            .collect();
        assert_eq!(images.len(), 1);
        assert!(before.max_y() <= images[0].rect.origin.y);
        assert!(images[0].rect.max_y() <= after.origin.y);
        assert!(images[0].rect.size.height > 0.0);
        let file = mounted.rect("Attachment queued.txt");
        let reply = mounted.rect("↩ Quote");
        assert!(after.max_y() <= file.origin.y && file.max_y() <= reply.origin.y);
        let last_text = mounted.rect(
            &"A long queued reply with enough words to wrap across several lines. ".repeat(8),
        );
        assert!(last_text.max_y() <= queued.max_y());
        // No footer is reserved beneath long content: only the content inset remains.
        assert!(
            queued.max_y() - last_text.max_y() <= 20.0 * scale,
            "scale={scale}, queued={queued:?}, last={last_text:?}"
        );
        assert!(!has_label(&mounted, "Remove inline file"));
        for action in [
            "Cancel queued message",
            "Edit queued message",
            "Send queued message now",
        ] {
            let button = mounted.rect(action);
            assert!(
                button.max_x() < before.origin.x,
                "{action} stays in number gutter"
            );
            assert!(button.origin.y >= queued.origin.y && button.max_y() <= queued.max_y());
            assert_eq!(
                mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.role == Role::Image && button.contains(node.rect.center())
                    })
                    .count(),
                1,
                "{action} should have an icon"
            );
        }
        // Same part ID with changed text must refresh the queue preview.
        mounted.model.snapshot.update(|snapshot| {
            if let PartKind::Text { text } =
                &mut snapshot.submissions.last_mut().unwrap().parts[0].kind
            {
                *text = "Updated queued text".into();
            }
        });
        mounted.settle();
        assert!(has_label(&mounted, "Updated queued text"));
        assert!(!has_label(&mounted, "Text before the queued image"));
        let updated = mounted
            .model
            .snapshot
            .get_untracked()
            .submissions
            .last()
            .unwrap()
            .parts
            .clone();
        assert_control_hit(&mounted, "Edit queued message");
        mounted.click("Edit queued message");
        assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), updated);
    }
}

#[test]
fn queued_reply_images_fetch_missing_bytes_and_appear_when_received() {
    use crate::buffer_network::Request;
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .snapshot
        .update(|snapshot| snapshot.messages.retain(|m| m.session_id != "session-plan"));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba([120, 170, 200, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let bytes = bytes.into_inner();
    let asset = Asset {
        id: "remote-queued-image".into(),
        name: "remote.png".into(),
        media_type: "image/png".into(),
        size: bytes.len() as u64,
    };
    mounted.model.snapshot.update(|snapshot| {
        snapshot.submissions.push(Submission {
            id: "queued-reply".into(),
            session_id: "session-plan".into(),
            parts: vec![Part {
                id: "reply-part".into(),
                kind: PartKind::Reply {
                    anchor: relay_core::Anchor {
                        message_id: "source".into(),
                        start: 0,
                        end: 5,
                        quote: "Quote".into(),
                    },
                    parts: vec![
                        Part::text("Reply before image"),
                        Part {
                            id: "reply-image".into(),
                            kind: PartKind::Asset {
                                asset: asset.clone(),
                            },
                        },
                        Part::text("Reply after image"),
                    ],
                },
            }],
            state: SubmissionState::Paused,
            approve_implementation: false,
            error: Some("Queue paused".into()),
            interrupts_run: None,
            last_edit_request: None,
            sender: Sender::User,
        })
    });
    mounted.settle();
    assert!(matches!(rx.try_recv().unwrap(), Request::Fetch(id) if id == asset.id));
    assert!(rx.try_recv().is_err());
    assert!(has_label(&mounted, "Preview remote.png"));
    assert!(has_label(&mounted, "Queue paused"));
    mounted.model.buffer.update(|buffer| {
        buffer
            .blobs
            .insert(asset.id.clone(), std::sync::Arc::new(bytes));
    });
    mounted.settle();
    let attachment = mounted.rect("Attachment remote.png");
    let after = mounted.rect("Reply after image");
    let image = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| {
            node.role == Role::Image
                && attachment.contains(node.rect.center())
                && node.rect.max_y() <= after.origin.y
        })
        .unwrap();
    assert!(mounted.rect("Reply before image").max_y() <= image.rect.origin.y);
    assert!(image.rect.max_y() <= mounted.rect("Reply after image").origin.y);
    assert!(rx.try_recv().is_err());
}

#[test]
fn pasted_text_file_has_an_inline_preview_and_allows_typing_after_it() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = draft_labels(&mounted).remove(0);
    type_in(&mounted, &label, "Before file");
    let target = crate::conversation::attachment_target(mounted.model).unwrap();
    let file = crate::buffer::insert_asset(
        mounted.model,
        "test-attachment.txt".into(),
        "text/plain".into(),
        b"Hi".to_vec(),
    )
    .unwrap();
    target.insert(mounted.model, vec![file.clone()]).unwrap();
    mounted.settle();
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("After file".into()));
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(plain_text(&parts[..1]), "Before file");
    assert_eq!(parts[1], file);
    assert_eq!(plain_text(&parts[2..]), "After file");
    mounted.key(Key::Home, false);
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(
        mounted.ui.text_input_contents().unwrap().text,
        "Before file"
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(mounted.ui.text_input_contents().unwrap().text, "After file");
    mounted.click("Preview test-attachment.txt");
    let frame = mounted.rect("Attachment test-attachment.txt");
    assert!(frame.contains(mounted.rect("Hi").center()));
    assert!(frame.contains(mounted.rect("Remove inline file").center()));
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), parts);
}

fn deployed_build(digit: &str) -> BuildInfo {
    BuildInfo {
        version: BuildInfo::current().version,
        revision: digit.repeat(40),
        ..Default::default()
    }
}

#[test]
fn deployment_notifications_compare_loaded_build_and_dismiss_each_revision_once() {
    let mounted = mount(false, 1380.0);
    assert!(mounted.model.available_update().is_none());
    let mut loaded = BuildInfo::current();
    loaded.revision = loaded
        .revision
        .trim_end_matches("-local")
        .trim_end_matches("-dirty")
        .into();
    mounted.model.server_build.set(Some(loaded.clone()));
    assert!(mounted.model.available_update().is_none());
    for unknown in ["", "unknown", "development"] {
        mounted.model.server_build.set(Some(BuildInfo {
            version: loaded.version.clone(),
            revision: unknown.into(),
            ..Default::default()
        }));
        assert!(mounted.model.available_update().is_none());
    }
    let first = deployed_build("a");
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        server_build: Some(first.clone()),
        ..Default::default()
    });
    assert_eq!(mounted.model.available_update(), Some(first.clone()));
    // A temporary failed version poll or reconnect must not remove the notice.
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: false,
        ..Default::default()
    });
    assert_eq!(mounted.model.available_update(), Some(first.clone()));
    mounted.model.dismissed_update.set(Some(first.clone()));
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        server_build: Some(first),
        ..Default::default()
    });
    assert!(mounted.model.available_update().is_none());
    let second = deployed_build("b");
    mounted.model.server_build.set(Some(second.clone()));
    assert_eq!(mounted.model.available_update(), Some(second));
    mounted.model.server_build.set(Some(loaded));
    assert!(mounted.model.available_update().is_none());
}

#[test]
fn deployment_toast_preserves_draft_focus_and_composer_position_on_every_page() {
    for width in [390.0, 1380.0] {
        let mounted = mount(false, width);
        mounted.model.open_session("session-plan".into());
        mounted.settle();
        let label = draft_labels(&mounted).remove(0);
        type_in(&mounted, &label, "Keep my unfinished message");
        let before = mounted.rect("Next message");
        let focused = mounted.ui.focused().unwrap().id();
        let parts = crate::buffer::parts(mounted.model, "session-plan");
        mounted.model.server_build.set(Some(deployed_build("a")));
        mounted.settle();
        assert!(has_label(&mounted, "New Version Available"));
        assert_eq!(mounted.ui.focused().unwrap().id(), focused);
        assert_eq!(mounted.rect("Next message"), before);
        assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), parts);
        mounted.click(&label);
        assert_eq!(mounted.ui.focused().unwrap().id(), focused);
        mounted.click("Dismiss update notification");
        assert!(!has_label(&mounted, "Update notification"));
        mounted.model.server_build.set(Some(deployed_build("a")));
        mounted.settle();
        assert!(!has_label(&mounted, "Update notification"));
        mounted.model.page.set(Page::Settings);
        mounted.model.server_build.set(Some(deployed_build("b")));
        mounted.settle();
        assert!(has_label(&mounted, "Update notification"));
        assert_eq!(crate::buffer::parts(mounted.model, "session-plan"), parts);
    }
}

#[test]
fn browser_deployment_toast_refresh_is_explicit_and_controls_fit_a_short_scaled_viewport() {
    use crate::ui::{UpdateToast, UpdateToastProps};
    let mut mounted = mount(false, 390.0);
    mounted.size.height = 260.0;
    mounted.model.preferences.update(|p| p.scale = 2.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Unsaved draft")],
    );
    let refreshed = std::rc::Rc::new(std::cell::Cell::new(false));
    let requested = refreshed.clone();
    mounted._scope.run(|| {
        let _ambient = mounted.ui.enter();
        mounted.ui.mount(&view! {
            UpdateToast model:(mounted.model) viewport-height:(State::new(260.0)) browser:true
                on-refresh:(Some(std::rc::Rc::new(move || requested.set(true)) as relay_ui::prelude::OnPress))
        });
    });
    mounted.model.server_build.set(Some(deployed_build("a")));
    mounted.settle();
    assert!(!refreshed.get());
    for label in ["Refresh Relay", "Dismiss update notification"] {
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.max_x() <= mounted.size.width,
            "{label}: {rect:?}"
        );
        assert!(
            rect.origin.y >= 0.0 && rect.max_y() <= mounted.size.height,
            "{label}: {rect:?}"
        );
    }
    mounted.click("Refresh Relay");
    assert!(refreshed.get());
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Unsaved draft"
    );
    mounted.click("Dismiss update notification");
    assert!(!has_label(&mounted, "Update notification"));
}

#[test]
fn markdown_transcript_copies_source_and_anchors_replies_across_inline_attachments() {
    let mounted = mount(false, 1380.0);
    let before =
        "# Heading\n\n[issue #124](https://github.com/jens-hj/relay/issues/124) and `λ code`";
    let after = "**After** the file\n\n```rust\nlet answer = 42;\n```";
    let parts = vec![
        Part::text(before),
        Part {
            id: "markdown-asset-part".into(),
            kind: PartKind::Asset {
                asset: Asset {
                    id: "markdown-asset".into(),
                    name: "notes.txt".into(),
                    media_type: "text/plain".into(),
                    size: 0,
                },
            },
        },
        Part::text(after),
    ];
    let mut original = plain_text(&parts);
    mounted.model.snapshot.update(|s| {
        let message = s.messages.iter_mut().find(|m| m.id == "m1").unwrap();
        message.body = original.clone();
        message.parts = parts;
    });
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let fields = mounted.rects("Message m1");
    assert_eq!(fields.len(), 2);
    let file = mounted.rect("Attachment notes.txt");
    assert!(fields[0].origin.y + fields[0].size.height <= file.origin.y + 0.5);
    assert!(file.origin.y + file.size.height <= fields[1].origin.y + 0.5);
    // Grow existing parts with unchanged IDs, shifting the later source range.
    let after = format!("{after}\n\n[Streamed](https://example.com)");
    mounted.model.snapshot.update(|s| {
        let message = s.messages.iter_mut().find(|m| m.id == "m1").unwrap();
        if let PartKind::Text { text } = &mut message.parts[0].kind {
            text.push_str("\nEarlier part grew.");
        }
        if let PartKind::Text { text } = &mut message.parts[2].kind {
            *text = after.clone();
        }
        message.body = plain_text(&message.parts);
        original = message.body.clone();
    });
    mounted.settle();
    let fields = mounted.rects("Message m1");
    // Focus the second source surface without activating a link, then select/copy.
    let position = fields[1].origin + Vector2::new(2.0, 2.0);
    for kind in [
        PointerEventKind::Down(PointerButton::Primary),
        PointerEventKind::Up(PointerButton::Primary),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
    }
    mounted.settle();
    mounted.key(Key::Character("a".into()), true);
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some(after.as_str()));
    // Reverse-drag within the rendered fence: the quote must index the original
    // body after the preceding Markdown and file, without either fence marker.
    let field = mounted.rects("Message m1")[1];
    let mut document = crate::markdown::document(&after);
    let answer = after.find("answer").unwrap();
    let start = document
        .caret(
            &mut mounted.ui.fonts().borrow_mut(),
            field.size.width,
            answer,
        )
        .center()
        + field.origin;
    let end = document
        .caret(
            &mut mounted.ui.fonts().borrow_mut(),
            field.size.width,
            answer + 6,
        )
        .center()
        + field.origin;
    for (kind, position) in [
        (PointerEventKind::Down(PointerButton::Primary), end),
        (PointerEventKind::Move, start),
        (PointerEventKind::Up(PointerButton::Primary), start),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::from_secs(1),
        });
    }
    mounted.settle();
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some("answer"));
    mounted.key(Key::Character("Reply".into()), false);
    let draft = crate::buffer::parts(mounted.model, "session-plan");
    let PartKind::Reply { anchor, parts } = &draft[0].kind else {
        panic!("missing reply")
    };
    assert_eq!(anchor.message_id, "m1");
    assert_eq!(anchor.quote, "answer");
    assert_eq!(anchor.start, original.find(&after).unwrap() + answer);
    assert_eq!(anchor.end, anchor.start + 6);
    assert_eq!(plain_text(parts), "Reply");
    mounted.click_nth("Go to reply source", 0);
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some("answer"));
    assert_eq!(
        mounted
            .model
            .snapshot
            .get_untracked()
            .messages
            .iter()
            .find(|m| m.id == "m1")
            .unwrap()
            .body,
        original
    );
}

#[test]
fn markdown_transcript_tables_fit_the_message_and_copy_their_source() {
    let mounted = mount(false, 1380.0);
    let columns = (0..12)
        .map(|i| format!("/nix/store/column-{i}"))
        .collect::<Vec<_>>();
    let body = format!(
        "| Worktree | Size | Status |\n|---|---:|:---:|\n| relay-1 | 1.2 GB | clean |\n\n| {} |\n|{}|\n| {} |",
        columns.join(" | "),
        ["---"; 12].join("|"),
        columns.join(" | ")
    );
    mounted.model.snapshot.update(|s| {
        let message = s.messages.iter_mut().find(|m| m.id == "m1").unwrap();
        message.body = body.clone();
        message.parts = vec![];
    });
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let field = mounted.rects("Message m1")[0];
    let mut document = crate::markdown::document(&body);
    assert!(!document.display_text().contains('|'));
    let size = document.size(&mut mounted.ui.fonts().borrow_mut(), Some(field.size.width));
    assert!(size.width <= field.size.width + 0.5);
    assert!(size.height <= field.size.height + 0.5);
    // Horizontal wheel input over the wide table scrolls it in place.
    let wide = body.find("/nix/store/column-0").unwrap();
    let caret = document.caret(&mut mounted.ui.fonts().borrow_mut(), field.size.width, wide);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(-60.0, 0.0),
        },
        position: field.origin + caret.center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let position = field.origin + Vector2::new(2.0, 2.0);
    for kind in [
        PointerEventKind::Down(PointerButton::Primary),
        PointerEventKind::Up(PointerButton::Primary),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::from_secs(1),
        });
    }
    mounted.settle();
    mounted.key(Key::Character("a".into()), true);
    mounted.key(Key::Character("c".into()), true);
    assert_eq!(mounted.ui.clipboard_text().as_deref(), Some(body.as_str()));
}

#[test]
fn sidebar_edit_targets_the_clicked_session_or_config_without_selecting_it() {
    for kind in ["root", "worker", "archived", "draft"] {
        let mut mounted = mount_orchestration(1380.0);
        let mut snapshot = mounted.model.snapshot.get_untracked();
        let mut config = snapshot.agent_configs[0].clone();
        config.id = "edit-target".into();
        config.name = if kind == "worker" {
            "Worker config"
        } else {
            "Edit target"
        }
        .into();
        config.automatic_name = false;
        snapshot.agent_configs.push(config);
        if kind != "draft" {
            let mut session = snapshot.sessions[0].clone();
            session.id = "edit-session".into();
            session.agent_config_id = "edit-target".into();
            session.title = "Edit target".into();
            session.role = if kind == "worker" {
                SessionRole::Worker
            } else {
                SessionRole::Root
            };
            session.parent_session_id = (kind == "worker").then(|| "session-plan".into());
            session.archived = kind == "archived";
            snapshot.sessions.push(session);
        }
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            ..Default::default()
        });
        mounted.model.show_archived.set(true);
        mounted.model.open_session("session-plan".into());
        mounted.settle();
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: tree_row(&mounted, "Edit target").center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        mounted.click("Edit Edit target");
        assert_eq!(
            mounted.model.editor.get_untracked(),
            EditTarget::Config("edit-target".into()),
            "{kind}"
        );
        assert_eq!(
            mounted.model.session.get_untracked(),
            "session-plan",
            "{kind}"
        );
        assert_eq!(mounted.model.page.get_untracked(), Page::Profiles);
        mounted.model.editor_name.set("Updated target".into());
        mounted.model.save_profile();
        assert!(matches!(mounted.commands.try_recv().unwrap().command,
            Command::UpdateAgentConfig { agent_config_id, name, .. }
                if agent_config_id == "edit-target" && name == "Updated target"));
    }
}

#[test]
fn session_header_groups_nested_descendants_and_updates_live() {
    let mounted = mount_orchestration(1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let template = snapshot
        .sessions
        .iter()
        .find(|s| s.id == "session-plan")
        .unwrap()
        .clone();
    snapshot.sessions.clear();
    snapshot.tool_permissions.clear();
    snapshot.submissions.clear();
    for (id, parent, state) in [
        ("summary-root", None, WorkerStatus::Running),
        (
            "summary-child",
            Some("summary-root"),
            WorkerStatus::Completed,
        ),
        (
            "summary-grandchild",
            Some("summary-child"),
            WorkerStatus::Completed,
        ),
        ("summary-queued", Some("summary-root"), WorkerStatus::Queued),
        ("summary-other", None, WorkerStatus::Failed),
    ] {
        let mut session = template.clone();
        session.id = id.into();
        session.title = id.into();
        session.fixture = false;
        session.awaiting_first_message = false;
        session.parent_session_id = parent.map(Into::into);
        session.worker.as_mut().unwrap().status = state;
        snapshot.sessions.push(session);
    }
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("summary-root".into());
    mounted.settle();
    assert!(has_label(
        &mounted,
        "Working. 3 agents below: 1 queued, 2 done. Show in tree"
    ));
    assert!(has_label(&mounted, "Done: 2 agents below"));
    assert!(has_label(&mounted, "Queued: 1 agent below"));
    assert!(!has_label(&mounted, "Failed: 1 agent below"));
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "summary-grandchild")
            .unwrap()
            .worker
            .as_mut()
            .unwrap()
            .status = WorkerStatus::Running;
    });
    mounted.settle();
    assert!(has_label(
        &mounted,
        "Working. 3 agents below: 1 working, 1 queued, 1 done. Show in tree"
    ));
    assert!(has_label(&mounted, "Working: 1 agent below"));
    assert!(has_label(&mounted, "Done: 1 agent below"));
    assert!(!has_label(&mounted, "Done: 2 agents below"));
    mounted.model.snapshot.update(|snapshot| {
        snapshot.sessions.retain(|s| s.id != "summary-queued");
    });
    mounted.settle();
    assert!(!has_label(&mounted, "Queued: 1 agent below"));
    mounted.click("Working. 2 agents below: 1 working, 1 done. Show in tree");
    assert_eq!(
        mounted.model.agent_focus.get_untracked().as_deref(),
        Some("summary-root")
    );
}

#[test]
fn progress_peek_uses_only_selected_active_run_metadata_without_loaded_tail() {
    let mut snapshot = live_snapshot();
    let session = snapshot.sessions[0].id.clone();
    snapshot.sessions[0].worker = Some(worker_run(WorkerStatus::Running));
    snapshot.messages.clear(); // Active prompt and summary are outside the loaded page.
    let mut history = HistoryState {
        session_id: session.clone(),
        progress_summary: Some(Box::new(ProgressSummary {
            run_id: "active".into(),
            text: "Checking adapter λ".into(),
        })),
        ..Default::default()
    };
    let read = |snapshot: &Snapshot, history: &HistoryState, session: &str, run: Option<&str>| {
        crate::transcript::progress_summary(snapshot, history, session, run)
    };
    assert_eq!(
        read(&snapshot, &history, &session, Some("active")),
        "Checking adapter λ"
    );
    snapshot.messages.push(Message {
        id: "prompt-queued".into(),
        session_id: session.clone(),
        author: "You".into(),
        sender: Sender::User,
        kind: "prompt".into(),
        body: "Next turn".into(),
        parts: vec![],
        tool: None,
    });
    assert_eq!(
        read(&snapshot, &history, &session, Some("active")),
        "Checking adapter λ"
    );
    assert!(read(&snapshot, &history, &session, Some("next-run")).is_empty());
    assert!(read(&snapshot, &history, &session, None).is_empty());
    assert!(read(&snapshot, &history, "other-session", Some("active")).is_empty());
    for status in [
        WorkerStatus::Completed,
        WorkerStatus::Failed,
        WorkerStatus::Interrupted,
        WorkerStatus::Stopped,
        WorkerStatus::Queued,
    ] {
        snapshot.sessions[0].worker.as_mut().unwrap().status = status;
        assert!(read(&snapshot, &history, &session, Some("active")).is_empty());
    }
    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Running;
    history.progress_summary.as_mut().unwrap().text = " \n".into();
    assert!(read(&snapshot, &history, &session, Some("active")).is_empty());
}

#[test]
fn progress_peek_updates_preserve_windowed_history_and_draft_focus() {
    let mounted = windowed_history(200);
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "session-plan")
            .unwrap()
            .worker = Some(worker_run(WorkerStatus::Running));
    });
    mounted
        .model
        .active_run_ids
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            "live-run".into(),
        )]));
    mounted.model.history.update(|history| {
        history.session_id = "session-plan".into();
        history.progress_summary = Some(Box::new(ProgressSummary {
            run_id: "live-run".into(),
            text: "Checking stream boundaries".into(),
        }));
    });
    mounted.settle();
    assert!(has_label(&mounted, "Peek"));
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| label.starts_with("Draft text "))
        })
        .unwrap();
    let label = draft.label.unwrap();
    mounted.click(&label);
    mounted.key(Key::Character("Keep this draft".into()), false);
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "確定".into(),
        cursor: Some((0, 6)),
    });
    for _ in 0..3 {
        mounted.model.history.update(|history| {
            history
                .progress_summary
                .as_mut()
                .unwrap()
                .text
                .push_str(" λ")
        });
        mounted.settle();
    }
    assert_eq!(focused_label(&mounted).as_deref(), Some(label.as_str()));
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定".into()));
    mounted.settle();
    assert!(plain_text(&crate::buffer::parts(mounted.model, "session-plan")).contains("確定"));
    mounted.model.source_anchor.set(Some(relay_core::Anchor {
        message_id: "window-30".into(),
        start: 0,
        end: 0,
        quote: String::new(),
    }));
    mounted.settle();
    let reading_y = mounted.rect("Message window-30").origin.y;
    let reading_focus = focused_label(&mounted);
    mounted.model.history.update(|history| {
        history
            .progress_summary
            .as_mut()
            .unwrap()
            .text
            .push_str(" More progress")
    });
    mounted.settle();
    assert_eq!(focused_label(&mounted), reading_focus);
    assert!((mounted.rect("Message window-30").origin.y - reading_y).abs() < 1.0);
    // Keep the older-page reading anchor and the disclosure in the same
    // viewport. Focusing a control outside the viewport intentionally reveals it.
    mounted.model.source_anchor.set(Some(relay_core::Anchor {
        message_id: "window-198".into(),
        start: 0,
        end: 0,
        quote: String::new(),
    }));
    mounted.settle();
    mounted.focus("Peek");
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 1.0),
        },
        position: mounted.rect("Transcript viewport").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let peek_reading_y = mounted.rect("Message window-198").origin.y;
    mounted.key(Key::Enter, false);
    mounted.settle();
    assert!(has_label(&mounted, "Latest Update"));
    assert!((mounted.rect("Message window-198").origin.y - peek_reading_y).abs() < 1.0);
    mounted.key(Key::Space, false);
    mounted.settle();
    assert!(!has_label(&mounted, "Latest Update"));
    assert!((mounted.rect("Message window-198").origin.y - peek_reading_y).abs() < 1.0);
    mounted.click("Peek");
    assert!(has_label(&mounted, "Latest Update"));
    mounted.model.history.update(|history| {
        history
            .progress_summary
            .as_mut()
            .unwrap()
            .text
            .push_str(" while open")
    });
    mounted.settle();
    assert!((mounted.rect("Message window-198").origin.y - peek_reading_y).abs() < 1.0);
    mounted.click("Hide");
    assert!(!has_label(&mounted, "Latest Update"));
    assert!((mounted.rect("Message window-198").origin.y - peek_reading_y).abs() < 1.0);
    let tree = mounted.ui.inspection_snapshot();
    assert!(tree.nodes.iter().any(|node| node.id == draft.id));
    assert!(
        tree.nodes
            .iter()
            .filter(|node| node
                .label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message window-")))
            .count()
            < 60
    );
    assert!(crate::buffer::parts(mounted.model, "session-plan").iter().any(
        |part| matches!(&part.kind, PartKind::Text {text} if text.contains("Keep this draft"))
    ));
}

fn scroll_history(session: &str, count: usize) -> Vec<Message> {
    (0..count)
        .map(|index| Message {
            id: format!("{session}-{index}"),
            session_id: session.into(),
            sender: Sender::User,
            author: "Codex".into(),
            kind: "assistant".into(),
            body: format!(
                "Entry {index}.\n\n{}",
                "A variable height paragraph.\n\n".repeat(index % 5 + 1)
            ),
            parts: vec![],
            tool: None,
        })
        .collect()
}

fn wheel(mounted: &Mounted, delta: f32) {
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, delta),
        },
        position: mounted.rect("Transcript viewport").center(),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    for _ in 0..3 {
        mounted.settle();
    }
}

fn first_visible_message(mounted: &Mounted) -> (String, f32) {
    let viewport = mounted.rect("Transcript viewport");
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| label.starts_with("Message session-"))
                && node.rect.origin.y >= viewport.origin.y
                && node.rect.origin.y < viewport.max_y()
        })
        .min_by(|a, b| a.rect.origin.y.total_cmp(&b.rect.origin.y))
        .map(|node| (node.label.unwrap(), node.rect.origin.y))
        .expect("a visible message")
}

#[test]
fn transcript_at_bottom_stays_pinned_after_scrolling_back_while_content_grows() {
    let mounted = mount(false, 1380.0);
    mounted
        .model
        .snapshot
        .update(|s| s.messages.extend(scroll_history("session-plan", 120)));
    mounted.model.open_session("session-plan".into());
    for _ in 0..3 {
        mounted.settle();
    }
    let boundary = mounted.rect("Draft boundary");
    let viewport = mounted.rect("Transcript viewport");
    assert!(boundary.max_y() <= viewport.max_y() + 1.0);
    // Reading earlier history and returning to the end must resume following.
    wheel(&mounted, 1500.0);
    assert!(mounted.rect("Draft boundary").origin.y > viewport.max_y());
    wheel(&mounted, -100_000.0);
    assert!((mounted.rect("Draft boundary").origin.y - boundary.origin.y).abs() < 2.0);
    for index in 120..124 {
        mounted.model.snapshot.update(|s| {
            s.messages.extend(
                scroll_history("session-plan", index + 1)
                    .into_iter()
                    .skip(index),
            )
        });
        mounted.settle();
        assert!(
            (mounted.rect("Draft boundary").origin.y - boundary.origin.y).abs() < 2.0,
            "appending message {index} moved the composer"
        );
    }
    mounted.model.snapshot.update(|s| {
        s.messages
            .last_mut()
            .unwrap()
            .body
            .push_str(&"Streamed line.\n\n".repeat(30))
    });
    mounted.settle();
    assert!(
        (mounted.rect("Draft boundary").origin.y - boundary.origin.y).abs() < 2.0,
        "streaming growth moved the composer"
    );
    assert!(has_label(&mounted, "Message session-plan-123"));
}

#[test]
fn transcript_scrolled_up_keeps_its_reading_position_while_content_grows() {
    let mounted = mount(false, 1380.0);
    mounted
        .model
        .snapshot
        .update(|s| s.messages.extend(scroll_history("session-plan", 120)));
    mounted.model.open_session("session-plan".into());
    for _ in 0..3 {
        mounted.settle();
    }
    wheel(&mounted, 2000.0);
    let (label, y) = first_visible_message(&mounted);
    mounted.model.snapshot.update(|s| {
        s.messages
            .extend(scroll_history("session-plan", 123).into_iter().skip(120));
        s.messages
            .last_mut()
            .unwrap()
            .body
            .push_str(&"Streamed line.\n\n".repeat(30));
    });
    mounted.settle();
    assert!(
        (mounted.rect(&label).origin.y - y).abs() < 2.0,
        "appending moved the reading position"
    );
}

#[test]
fn opening_a_session_lands_at_its_latest_messages() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.update(|s| {
        s.messages.extend(scroll_history("session-plan", 120));
        s.messages.extend(scroll_history("session-worker", 120));
    });
    mounted.model.open_session("session-plan".into());
    for _ in 0..3 {
        mounted.settle();
    }
    let viewport = mounted.rect("Transcript viewport");
    assert!(mounted.rect("Draft boundary").max_y() <= viewport.max_y() + 1.0);
    // Reading earlier history in one session must not carry into the next.
    wheel(&mounted, 3000.0);
    mounted.model.open_session("session-worker".into());
    for _ in 0..3 {
        mounted.settle();
    }
    let viewport = mounted.rect("Transcript viewport");
    let boundary = mounted.rect("Draft boundary");
    assert!(
        boundary.origin.y >= viewport.origin.y && boundary.max_y() <= viewport.max_y() + 1.0,
        "opened session is not at its end: boundary {boundary:?} viewport {viewport:?}"
    );
    assert!(has_label(&mounted, "Message session-worker-119"));
}

#[test]
fn composer_keeps_keyboard_focus_after_sending() {
    use crate::buffer_network::{Outcome, Request, Update};
    let draft_label = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
            .unwrap()
    };
    let focused_label = |mounted: &Mounted| {
        let id = mounted.ui.focused()?.id();
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| n.id == id)?
            .label
    };
    // A restored draft keeps its own part id, so sending remounts its field.
    for case in 0..8 {
        let (running, button, restored) = (case & 1 == 1, case & 2 == 2, case & 4 == 4);
        let case = format!("running {running}, send button {button}, restored {restored}");
        let mut mounted = mount(false, 1380.0);
        buffer_worker(&mounted);
        if !running {
            mounted.model.snapshot.update(|s| {
                s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
            });
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        mounted.model.buffer_requests.set(Some(tx));
        mounted.settle();
        if restored {
            crate::buffer::edit(
                mounted.model,
                "session-plan",
                vec![Part::text("First message")],
            );
            mounted.settle();
            mounted.focus(&draft_label(&mounted));
        } else {
            type_in(&mounted, &draft_label(&mounted), "First message");
        }
        if button {
            mounted.click("Send message");
        } else {
            mounted.key(Key::Enter, true);
        }
        assert!(
            mounted.model.buffer.get_untracked().documents["session-plan"]
                .finalize
                .is_some(),
            "{case}: the message is submitted"
        );
        assert!(
            focused_label(&mounted).is_some_and(|l| l.starts_with("Draft text")),
            "{case}: focus returns to the composer, found {:?}",
            focused_label(&mounted)
        );
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit("Next message".into()));
        mounted.settle();
        assert_eq!(
            plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
            "Next message",
            "{case}: typing continues in the composer"
        );
        // Focus the user moves elsewhere is kept when the send completes.
        mounted.focus("Session actions");
        let Request::Save { session, request } = rx.try_recv().unwrap() else {
            panic!("{case}: expected draft save")
        };
        let saved = Draft {
            session_id: session.clone(),
            revision: 1,
            parts: request.parts.clone(),
        };
        crate::buffer::receive(
            mounted.model,
            Update {
                serial: 1,
                initialized: true,
                connected: true,
                drafts: vec![saved.clone()],
                outcomes: std::collections::BTreeMap::from([(
                    request.request_id.clone(),
                    Outcome::Saved {
                        session,
                        request,
                        result: Ok(saved),
                    },
                )]),
                ..Default::default()
            },
        );
        mounted.settle();
        if let Ok(submitted) = mounted.commands.try_recv() {
            let snapshot = mounted.model.snapshot.get_untracked();
            mounted.model.receive(NetworkState {
                snapshot: snapshot.clone(),
                connected: true,
                acknowledged_revision: snapshot.revision + 1,
                outcome_serial: 1,
                outcome: Some((submitted.request_id, Ok(()))),
                ..Default::default()
            });
            mounted.settle();
        }
        assert_eq!(
            focused_label(&mounted).as_deref(),
            Some("Session actions"),
            "{case}: completion must not steal focus"
        );
    }
}
