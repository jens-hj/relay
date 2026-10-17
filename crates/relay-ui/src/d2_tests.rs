use super::tests::{gallery, mount};
use crate::prelude::*;
use crate::tokens::{ToolKind, px};
use mosaic::prelude::*;
use std::rc::Rc;
fn noop() -> OnPress {
    Rc::new(|| {})
}
fn actions(state: State<LedgerView>) -> LedgerActions {
    LedgerActions {
        change_harness: noop(),
        change_model: noop(),
        set_effort: Rc::new(move |level| {
            state.update(|s| {
                s.effort.current = level;
            })
        }),
        toggle: Rc::new(move || state.update(|s| s.collapsed = !s.collapsed)),
    }
}
#[test]
fn all_tools_cycle_openings_and_follow_live_data() {
    for specimen in gallery::d2::specimens() {
        let value = State::new(specimen);
        let change: OnOpening = Rc::new(move |opening| value.update(|v| v.opening = opening));
        let mounted = mount(Size::new(720.0, 500.0), || {
            view! {
                col {
                    ToolCallRow data:(value) on-opening:(change)
                }
            }
        });
        for opening in [Opening::Collapsed, Opening::Summary, Opening::Expanded] {
            assert!(mounted.labels().iter().any(|l| l == opening.label()));
            let header = mounted.rect(opening.label());
            assert_eq!(header.size.height, 30.0);
            assert!(header.size.width > 600.0);
            mounted.click(Vector2::new(header.origin.x + 80.0, header.center().y));
            assert_eq!(value.get_untracked().opening, opening.next());
        }
        for x in [12.0, 350.0, 700.0] {
            let header = mounted.rect(value.get_untracked().opening.label());
            let next = value.get_untracked().opening.next();
            mounted.click(Vector2::new(header.origin.x + x, header.center().y));
            assert_eq!(value.get_untracked().opening, next);
        }
        value.update(|v| {
            v.status = AgentStatus::Failed;
            v.target = Some("changed target".into());
        });
        mounted.settle();
        assert!(mounted.labels().iter().any(|l| l == "Failed"));
    }
}
#[test]
fn missing_tool_data_has_no_fabricated_readouts() {
    let mounted = mount(Size::new(720.0, 200.0), || {
        view! {
            ToolCallRow
                data:(ToolCallView { kind:ToolKind::Run, status:AgentStatus::Done, verb:"Run".into(), target:None, opening:Opening::Expanded, detail:None })
                on-opening:(Rc::new(|_| {}) as OnOpening)
        }
    });
    let labels = mounted.labels();
    assert!(
        !labels
            .iter()
            .any(|l| l.contains("exit") || l.contains("failed") || l.contains("lines"))
    );
}
#[test]
fn failure_groups_stay_open_and_finished_groups_fold() {
    let value = State::new(ActionGroupView {
        actions: vec![CompactAction {
            status: AgentStatus::Done,
            verb: "Run".into(),
            target: Some("build".into()),
            info: None,
        }],
        expanded: false,
    });
    let toggle: OnPress = Rc::new(move || value.update(|v| v.expanded = !v.expanded));
    let mounted = mount(Size::new(720.0, 200.0), || {
        view! {
            col {
                ActionGroup data:(value) on-toggle:(toggle)
            }
        }
    });
    assert!(mounted.labels().iter().any(|l| l == "1 actions · all done"));
    assert_eq!(
        mounted.rect("1 actions · all done").size,
        Size::new(720.0, 34.0)
    );
    assert!(!mounted.labels().iter().any(|l| l == "Done"));
    value.update(|v| v.actions[0].status = AgentStatus::Failed);
    mounted.settle();
    assert!(mounted.labels().iter().any(|l| l == "Failed"));
    mounted.click(mounted.rect("1 actions").center());
    mounted.click(mounted.rect("1 actions").center());
    assert!(mounted.labels().iter().any(|l| l == "Failed"));
}
#[test]
fn framed_ledger_preserves_effort_label_height_for_both_harnesses() {
    for harness in [Harness::Codex, Harness::ClaudeCode] {
        let mut settings = gallery::d2::settings(false);
        settings.harness = harness;
        let settings = State::new(settings);
        let mounted = mount(Size::new(848.0, 600.0), || {
            view! {
                col justify:start {
                    Ledger data:(settings) actions:(actions(settings)) framed:true
                        height:(Dimension::MinContent)
                }
            }
        });
        let frame = mounted.rect("Effort selection frame");
        assert!(frame.size.height >= 20.0, "{harness:?}: {frame:?}");
        let ledger = mounted.rect("Agent ledger");
        let effort = settings.get_untracked().effort;
        let track = mounted.rect(&crate::effort::effort_label(
            effort.levels,
            effort.selected(),
            effort.tiers,
        ));
        assert_eq!(mounted.rect("Ledger border"), ledger);
        assert!(frame.max_y() <= ledger.max_y());
        assert!(
            track.origin.y >= frame.max_y() + 8.0,
            "{harness:?}: {frame:?} {track:?}"
        );
        assert!(
            track.max_y() + 9.0 <= ledger.max_y() + 0.01,
            "{harness:?}: {track:?} {ledger:?}"
        );
    }
}

#[test]
fn collapsed_ledger_expand_row_spans_model_and_effort() {
    for scale in [0.75, 1.0, 1.5, 2.0] {
        let state = State::new(gallery::d2::settings(true));
        let mounted = mount(Size::new(848.0, 360.0), || {
            install_theme(&crate::tokens::theme(
                crate::tokens::ThemeVariant::Dark,
                scale,
            ));
            view! {
                Composer settings:(state) ledger-actions:(actions(state)) on-send:(noop())
                    on-attach:(noop())
            }
        });
        let ledger = mounted.rect("Agent ledger");
        let model_label = mounted
            .labels()
            .into_iter()
            .find(|s| s.starts_with("Agent settings:"))
            .unwrap();
        let model = mounted.rect(&model_label);
        let effort = state.get_untracked().effort;
        let track = mounted.rect(&crate::effort::effort_label(
            effort.levels,
            effort.selected(),
            effort.tiers,
        ));
        let expand = mounted.rect("Expand agent settings");
        assert_eq!(expand.origin.x, ledger.origin.x);
        assert_eq!(expand.size.width, ledger.size.width);
        assert!((expand.size.height - 22.0 * scale).abs() < 0.01);
        assert!(expand.origin.y >= model.max_y() - 0.01);
        assert!(expand.origin.y >= track.max_y() - 0.01);
        assert!((expand.max_y() - ledger.max_y()).abs() < 0.01);
        let icon = mounted.rect("›");
        assert!((icon.center().x - expand.center().x).abs() < 0.01);
        assert!((icon.center().y - expand.center().y).abs() < 0.01);
        // Both the model and effort sides of the row activate the control.
        for fraction in [0.1, 0.9] {
            mounted.click(Vector2::new(
                expand.origin.x + expand.size.width * fraction,
                expand.center().y,
            ));
            assert!(!state.get_untracked().collapsed);
            state.update(|s| s.collapsed = true);
            mounted.settle();
        }
        for _ in 0..20 {
            mounted.key(Key::Tab);
            let snapshot = mounted.ui.inspection_snapshot();
            if mounted.ui.focused().is_some_and(|focused| {
                snapshot.nodes.iter().any(|n| {
                    n.id == focused.id() && n.label.as_deref() == Some("Expand agent settings")
                })
            }) {
                break;
            }
        }
        let focused = mounted.ui.focused().expect("expand button is focusable");
        assert!(
            mounted.ui.inspection_snapshot().nodes.iter().any(
                |n| n.id == focused.id() && n.label.as_deref() == Some("Expand agent settings")
            )
        );
        mounted.key(Key::Enter);
        assert!(!state.get_untracked().collapsed);
    }
}

#[test]
fn ledger_geometry_actions_and_editor_survive_collapse() {
    let state = State::new(gallery::d2::settings(false));
    let value = State::new("hello".to_owned());
    let sent = State::new(0usize);
    let attached = State::new(0usize);
    let send: OnPress = Rc::new(move || sent.update(|n| *n += 1));
    let attach: OnPress = Rc::new(move || attached.update(|n| *n += 1));
    let mounted = mount(Size::new(848.0, 180.0), || {
        view! {
            Composer settings:(state) ledger-actions:(actions(state)) value:(value) on-send:(send)
                on-attach:(attach)
        }
    });
    assert_eq!(
        mounted.rect("Harness: CODEX. Change harness").size.width,
        32.0
    );
    assert_eq!(mounted.rect("Collapse agent settings").size.width, 36.0);
    assert_eq!(mounted.rect("Send message").size.width, 44.0);
    let editor_id = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Message"))
        .unwrap()
        .id;
    mounted.click(mounted.rect("Effort high").center());
    assert_eq!(state.get_untracked().effort.current, 3);
    mounted.click(mounted.rect("Collapse agent settings").center());
    assert!(state.get_untracked().collapsed);
    assert!(
        mounted
            .labels()
            .iter()
            .any(|l| l == "Agent settings: CODEX, GPT Sol 6.1, effort high. Expand")
    );
    assert_eq!(
        editor_id,
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Message"))
            .unwrap()
            .id
    );
    mounted.click(mounted.rect("Send message").center());
    mounted.click(mounted.rect("Attach file").center());
    assert_eq!(sent.get_untracked(), 1);
    assert_eq!(attached.get_untracked(), 1);
    value.set(String::new());
    mounted.settle();
    mounted.click(mounted.rect("Send message").center());
    assert_eq!(sent.get_untracked(), 1);
}
#[test]
fn transcript_gutter_column_and_thinking_geometry() {
    let mounted = mount(Size::new(1100.0, 300.0), || {
        view! {
            TranscriptColumn {
                MessageRow speaker:"You" {
                    UserMessage {
                        el label:"body slot" height:{px(20.0)}px {}
                    }
                }
                MessageRow speaker:"Agent" {
                    ThinkingLine effort:5u8
                }
            }
        }
    });
    let body = mounted.rect("body slot");
    assert_eq!(body.origin.x, (1100.0 - 848.0) / 2.0 + 128.0 + 12.0);
    assert_eq!(mounted.rect("Working").size, Size::new(10.0, 10.0));
}
#[test]
fn approval_and_spawn_callbacks_are_accessible() {
    let approved = State::new(false);
    let approve: OnPress = Rc::new(move || approved.set(true));
    let vm = ApprovalView {
        requester: Some("child".into()),
        path: None,
        heading: "Apply migration".into(),
        command: Some("apply migration.sql".into()),
        why: None,
        touches: None,
        blocks: Some("backfill".into()),
    };
    let mounted = mount(Size::new(720.0, 500.0), || {
        view! {
            col {
                SpawnBlock agents:(Live::fixed(vec![])) on-open-tree:(noop())
                ApprovalBlock data:(vm) on-approve:(approve) on-reject:(noop())
                    on-review-diff:(noop())
            }
        }
    });
    for name in ["Open in tree", "Approve", "Reject", "Review diff"] {
        assert!(mounted.labels().iter().any(|l| l == name));
    }
    mounted.click(mounted.rect("Approve").center());
    assert!(approved.get_untracked());
}
#[test]
fn approval_arguments_wrap_and_keep_actions_below_all_lines() {
    let mounted = mount(Size::new(420.0, 700.0), || {
        view! {
            ApprovalBlock
                data:(ApprovalView {
                requester: Some("Worker".into()), path: None,
                heading: "Approval Requested · Write".into(),
                command: Some(format!("{{\n  \"content\": \"{}\",\n  \"file_path\": \"/workspace/approval-test.txt\"\n}}", "Long approval content ".repeat(8))),
                why: None, touches: None, blocks: None,
            })
                on-approve:(noop()) on-reject:(noop()) on-review-diff:(noop())
        }
    });
    let heading = mounted.rect("Approval Requested · Write");
    let arguments = mounted.rect("Approval arguments");
    let approve = mounted.rect("Approve");
    assert!(arguments.size.height > 4.0 * 18.0);
    assert!(arguments.origin.y >= heading.origin.y + heading.size.height);
    assert!(approve.origin.y >= arguments.origin.y + arguments.size.height);
    assert!(arguments.origin.x >= 0.0);
    assert!(arguments.origin.x + arguments.size.width <= 420.0);
}
#[test]
fn picker_reports_option_identity() {
    let selected = State::new(String::new());
    let select: Rc<dyn Fn(String)> = Rc::new(move |id| selected.set(id));
    let mounted = mount(Size::new(300.0, 180.0), || {
        view! {
            ChoiceList
                options:(vec![Choice { id:"sol".into(), label:"Sol 6.1".into(), description:Some("GPT".into()), selected:false }])
                on-select:(select)
        }
    });
    mounted.click(mounted.rect("Sol 6.1").center());
    assert_eq!(selected.get_untracked(), "sol");
}
#[test]
fn lane_normalization_and_effort_capability_are_truthful() {
    let segments = vec![
        LaneSegment {
            status: AgentStatus::Queued,
            fraction: -1.0,
        },
        LaneSegment {
            status: AgentStatus::Working,
            fraction: 3.0,
        },
        LaneSegment {
            status: AgentStatus::Waiting,
            fraction: 1.0,
        },
        LaneSegment {
            status: AgentStatus::Done,
            fraction: f32::NAN,
        },
    ];
    assert_eq!(lane_widths(&segments), vec![0.0, 0.75, 0.25, 0.0]);
    assert_eq!(lane_widths(&[]), Vec::<f32>::new());
    let ordinary = gallery::d2::effort();
    assert_eq!(ordinary.caption(), "effort · xhigh · 05/05");
    let max = Effort {
        tiers: 1,
        current: ordinary.levels,
        ..ordinary
    };
    assert_eq!(max.caption(), "effort · max · 06/06");
    assert_eq!(max.selected(), 5);
}
#[test]
fn fallback_editor_enter_sends_and_shift_enter_adds_a_line() {
    let state = State::new(gallery::d2::settings(false));
    let value = State::new("hello".to_owned());
    let sent = State::new(0usize);
    let send: OnPress = Rc::new(move || sent.update(|n| *n += 1));
    let mounted = mount(Size::new(848.0, 180.0), || {
        view! {
            Composer settings:(state) ledger-actions:(actions(state)) value:(value) on-send:(send)
                on-attach:(noop())
        }
    });
    mounted.click(mounted.rect("Message").center());
    mounted.key(Key::Enter);
    assert_eq!(sent.get_untracked(), 1);
    assert_eq!(value.get_untracked(), "hello");
    mounted.ui.dispatch_key(KeyEvent {
        kind: KeyEventKind::Down { repeat: false },
        key: Key::Enter,
        modifiers: Modifiers {
            shift: true,
            ..Default::default()
        },
    });
    mounted.settle();
    assert_eq!(sent.get_untracked(), 1);
    assert!(value.get_untracked().contains('\n'));
}
#[test]
fn injected_editor_is_preserved_and_code_footer_is_optional() {
    let state = State::new(gallery::d2::settings(false));
    let mounted = mount(Size::new(848.0, 250.0), || {
        let editor = view! {
            el label:"Injected editor" height:{px(48.0)}px {}
        };
        view! {
            col {
                Composer settings:(state) ledger-actions:(actions(state))
                    value:(State::new(String::new())) on-send:(noop()) on-attach:(noop())
                    editor:(Some(editor))
                CodeBlock
                    lines:(vec![CodeLine { number:Some(42), text:"removed".into(), change:LineChange::Removed }])
                    more:7usize
            }
        }
    });
    assert!(mounted.labels().iter().any(|l| l == "Injected editor"));
    assert!(!mounted.labels().iter().any(|l| l == "Message"));
    assert!(mounted.labels().iter().any(|l| l == "7 more lines"));
    mounted.click(mounted.rect("Collapse agent settings").center());
    assert!(mounted.labels().iter().any(|l| l == "Injected editor"));
}
#[test]
fn fallback_composer_grows_with_multiline_content() {
    let state = State::new(gallery::d2::settings(false));
    let value = State::new(String::new());
    let mounted = mount(Size::new(848.0, 600.0), || {
        view! {
            col height:min-content {
                Composer settings:(state) ledger-actions:(actions(state)) value:(value)
                    on-send:(noop()) on-attach:(noop())
            }
        }
    });
    let initial = mounted.rect("Message").size.height;
    value.set(
        (0..15)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    mounted.settle();
    assert!(mounted.rect("Message").size.height > initial + 100.0);
}
#[test]
fn opening_control_keeps_keyboard_focus_when_rebuilt() {
    let state = State::new(gallery::d2::specimens().remove(0));
    let change: OnOpening = Rc::new(move |opening| state.update(|v| v.opening = opening));
    let mounted = mount(Size::new(720.0, 500.0), || {
        view! {
            col {
                ToolCallRow data:(state) on-opening:(change)
            }
        }
    });
    mounted.click(mounted.rect(Opening::Collapsed.label()).center());
    mounted.key(Key::Enter);
    assert_eq!(state.get_untracked().opening, Opening::Expanded);
    mounted.key(Key::Space);
    assert_eq!(state.get_untracked().opening, Opening::Collapsed);
}
#[test]
fn choice_popover_closes_after_selection_and_on_escape() {
    let open = State::new(false);
    let chosen = State::new(String::new());
    let toggle: OnPress = Rc::new(move || open.set(!open.get_untracked()));
    let choose: Rc<dyn Fn(String)> = Rc::new(move |id| chosen.set(id));
    let mounted = mount(Size::new(600.0, 300.0), || {
        view! {
            col align:start {
                stack width:{px(180.0)}px height:{px(44.0)}px {
                    CellButton label:"Choose model" on-press:(toggle) {
                        Label text:"Model"
                    }
                    ChoiceList
                        options:(vec![Choice { id:"opus".into(), label:"Opus 5.5".into(), description:None, selected:false }])
                        on-select:(choose) open:(Some(open)) label:"Models"
                }
            }
        }
    });
    mounted.click(mounted.rect("Choose model").center());
    assert!(open.get_untracked());
    mounted.click(mounted.rect("Opus 5.5").center());
    assert_eq!(chosen.get_untracked(), "opus");
    assert!(!open.get_untracked());
    mounted.click(mounted.rect("Choose model").center());
    mounted.key(Key::Escape);
    assert!(!open.get_untracked());
}
#[test]
fn fallback_editor_ink_follows_every_theme() {
    use crate::tokens::{ThemeVariant, theme};
    let state = State::new(gallery::d2::settings(false));
    let mounted = mount(Size::new(848.0, 180.0), || {
        view! {
            Composer settings:(state) ledger-actions:(actions(state))
                value:(State::new("Draft".into())) on-send:(noop()) on-attach:(noop())
        }
    });
    let id = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some("Message"))
        .unwrap()
        .id;
    for variant in ThemeVariant::ALL {
        install_theme(&theme(variant, 1.0));
        mounted.settle();
        let details = mounted.ui.inspection_details(id).unwrap();
        let ink = details
            .attributes
            .iter()
            .find(|a| a.name == "text-color")
            .unwrap();
        assert!(
            ink.value
                .contains(&format!("{:?}", crate::tokens::ground.ink)),
            "{variant:?}: {}",
            ink.value
        );
    }
}
#[test]
fn send_preserves_edits_from_the_same_event_batch() {
    let state = State::new(gallery::d2::settings(false));
    let value = State::new("hello".to_owned());
    let sent = State::new(String::new());
    let send: OnPress = Rc::new(move || sent.set(value.get_untracked()));
    let mounted = mount(Size::new(848.0, 180.0), || {
        view! {
            Composer settings:(state) ledger-actions:(actions(state)) value:(value) on-send:(send)
                on-attach:(noop())
        }
    });
    mounted.click(mounted.rect("Message").center());
    for key in [Key::Character("x".into()), Key::Enter] {
        mounted.ui.dispatch_key(KeyEvent {
            kind: KeyEventKind::Down { repeat: false },
            key,
            modifiers: Modifiers::default(),
        });
    }
    mounted.settle();
    assert_eq!(sent.get_untracked(), "hellox");
    assert_eq!(value.get_untracked(), "hellox");
}
#[test]
fn unknown_tools_keep_their_recorded_verb_without_borrowing_a_tool_hue() {
    assert_eq!(
        crate::tokens::Palette::DARK.tool(ToolKind::Other),
        crate::tokens::Palette::DARK.ground.muted
    );
    let mounted = mount(Size::new(720.0, 120.0), || {
        view! {
            ToolCallRow
                data:(ToolCallView { kind:ToolKind::Other, status:AgentStatus::Done, verb:"Custom operation".into(), target:Some("recorded target".into()), opening:Opening::Summary, detail:None })
                on-opening:(Rc::new(|_| {}) as OnOpening)
        }
    });
    assert!(
        mounted
            .labels()
            .iter()
            .any(|label| label == "recorded target")
    );
    assert!(
        mounted
            .labels()
            .iter()
            .any(|label| label == Opening::Summary.label())
    );
}
#[test]
fn injected_editor_reports_content_without_a_plain_text_proxy() {
    let state = State::new(gallery::d2::settings(false));
    let present = State::new(false);
    let sent = State::new(0usize);
    let send: OnPress = Rc::new(move || sent.update(|count| *count += 1));
    let mounted = mount(Size::new(848.0, 180.0), || {
        let editor = view! {
            el label:"Rich editor" height:{px(48.0)}px {}
        };
        view! {
            Composer settings:(state) ledger-actions:(actions(state)) editor:(Some(editor))
                has-content:(Some(present.into())) on-send:(send) on-attach:(noop())
        }
    });
    mounted.click(mounted.rect("Send message").center());
    assert_eq!(sent.get_untracked(), 0);
    present.set(true);
    mounted.settle();
    mounted.click(mounted.rect("Send message").center());
    assert_eq!(sent.get_untracked(), 1);
}

#[test]
fn spawn_rows_use_tree_anatomy_and_hide_unknown_harness() {
    let mounted = mount(Size::new(720.0, 180.0), || {
        view! {
            SpawnBlock
                agents:(Live::fixed(vec![SpawnChild {
                name:"Unknown harness".into(), status:AgentStatus::Working, harness:None,
                model:Some(ModelIdentity::new("Sol", "6.1", ModelHue::Sol)),
                effort:Some(Effort::new(5, 4)),
            }]))
                on-open-tree:(noop())
        }
    });
    assert_eq!(mounted.rect("Unknown harness").size.height, 34.0);
    // The tree action fills the header's trailing cell, flush with its edges.
    let open = mounted.rect("Open in tree");
    let row = mounted.rect("Unknown harness");
    assert_eq!(open.size.height, 36.0);
    assert_eq!(open.max_x(), row.max_x());
    assert!(open.size.width > 60.0);
    assert!(mounted.labels().iter().any(|label| label == "Sol 6.1"));
    assert!(!mounted.labels().iter().any(|label| label == "CODEX"));
}

#[test]
fn combined_gallery_keeps_both_workstreams_inside_the_capture() {
    let mounted = mount(Size::new(1440.0, 14000.0), || {
        gallery::gallery(Rc::new(|_| {}))
    });
    let labels = mounted.labels();
    assert!(labels.iter().any(|label| label == "AGENT TREE"));
    assert!(labels.iter().any(|label| label == "Open in tree"));
    let bottom = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .filter(|node| node.label.is_some())
        .map(|node| node.rect.origin.y + node.rect.size.height)
        .fold(0.0f32, f32::max);
    assert!(bottom < 13950.0, "gallery reaches {bottom}");
}
#[test]
fn ledger_without_effort_steps_names_the_selection_or_lets_the_model_fill() {
    let mut named = gallery::d2::settings(false);
    named.effort = Effort::default();
    named.effort_label = Some("high".into());
    let state = State::new(named);
    let noop: OnPress = Rc::new(|| {});
    let mounted = mount(Size::new(848.0, 180.0), || {
        view! {
            Composer settings:(state) ledger-actions:(actions(state)) on-send:(noop.clone())
                on-attach:(noop.clone())
        }
    });
    assert!(mounted.labels().iter().any(|l| l == "Selected effort"));
    assert!(!mounted.labels().iter().any(|l| l.starts_with("Effort ")));
    let model = mounted.rect("Model: GPT Sol 6.1. Change model");
    assert!((model.size.height - 40.0).abs() < 1.0, "{model:?}");
    state.update(|s| s.effort_label = None);
    mounted.settle();
    assert!(!mounted.labels().iter().any(|l| l == "Selected effort"));
    assert!(!mounted.labels().iter().any(|l| l == "EFFORT"));
    let model = mounted.rect("Model: GPT Sol 6.1. Change model");
    assert!(model.size.height > 100.0, "the model cell fills: {model:?}");
}

#[test]
fn tool_status_and_opening_share_full_height_cells() {
    for specimen in gallery::d2::specimens().into_iter().filter(|vm| {
        matches!(
            vm.kind,
            ToolKind::Run | ToolKind::Test | ToolKind::Fetch | ToolKind::Approval
        )
    }) {
        let mounted = mount(Size::new(720.0, 300.0), || {
            view! {
                col height:min-content {
                    ToolCallRow data:(specimen.clone()) on-opening:(Rc::new(|_| {}) as OnOpening)
                }
            }
        });
        let header = mounted.rect(Opening::Collapsed.label());
        assert_eq!(header.size.height, 30.0);
        let snapshot = mounted.ui.inspection_snapshot();
        let indicator = snapshot
            .nodes
            .iter()
            .find(|n| {
                n.children.len() == 3
                    && (n.rect.size.width - 36.0).abs() < 0.1
                    && (n.rect.size.height - header.size.height).abs() < 0.1
            })
            .expect("three full-height opening rectangles");
        assert!((indicator.rect.origin.x + 36.0 - header.origin.x - header.size.width).abs() < 0.1);
        assert_eq!(indicator.rect.origin.y, header.origin.y);
        for child in &indicator.children {
            let node = snapshot.nodes.iter().find(|n| n.id == *child).unwrap();
            assert_eq!(node.rect.size, Size::new(12.0, header.size.height));
            assert_eq!(node.rect.origin.y, header.origin.y);
        }
        let label = match specimen.kind {
            ToolKind::Run => {
                if specimen.status == AgentStatus::Failed {
                    "exit 1"
                } else {
                    "exit 0"
                }
            }
            ToolKind::Test => "2 failed",
            ToolKind::Fetch => "200",
            ToolKind::Approval => "pending",
            _ => unreachable!(),
        };
        let mut node = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap();
        while node.rect.size.height < header.size.height - 0.1 {
            node = snapshot
                .nodes
                .iter()
                .find(|n| Some(n.id) == node.parent)
                .unwrap();
        }
        assert_eq!(node.rect.size.height, header.size.height);
        assert_eq!(node.rect.origin.y, header.origin.y);
        assert!((node.rect.origin.x + node.rect.size.width - indicator.rect.origin.x).abs() < 0.1);
    }
}

#[test]
fn opened_tool_body_does_not_cycle_the_header() {
    let state = State::new(
        gallery::d2::specimens()
            .into_iter()
            .find(|vm| vm.kind == ToolKind::Run)
            .unwrap(),
    );
    state.update(|vm| vm.opening = Opening::Summary);
    let change: OnOpening = Rc::new(move |opening| state.update(|vm| vm.opening = opening));
    let mounted = mount(Size::new(720.0, 500.0), || {
        view! {
            col height:min-content {
                ToolCallRow data:(state) on-opening:(change)
            }
        }
    });
    let header = mounted.rect(Opening::Summary.label());
    mounted.click(header.origin + Vector2::new(90.0, 65.0));
    assert_eq!(state.get_untracked().opening, Opening::Summary);
}
#[test]
fn tool_rows_hover_in_their_own_tool_colour() {
    let specimens = gallery::d2::specimens();
    let read = specimens
        .iter()
        .find(|s| s.kind == ToolKind::Read)
        .unwrap()
        .clone();
    let edit = specimens
        .iter()
        .find(|s| s.kind == ToolKind::Edit)
        .unwrap()
        .clone();
    let hover_fill = |view: crate::prelude::ToolCallView| {
        let mounted = crate::testing::mount(Size::new(720.0, 120.0), move || {
            view! {
                col {
                    ToolCallRow data:(view) on-opening:(Rc::new(|_| {}) as OnOpening)
                }
            }
        });
        let node = |m: &crate::testing::Mounted| {
            m.ui.inspection_snapshot()
                .nodes
                .into_iter()
                .find(|n| n.label.as_deref() == Some(Opening::Collapsed.label()))
                .unwrap()
        };
        let fill = |m: &crate::testing::Mounted| {
            let id = node(m).id;
            m.ui.inspection_details(id)
                .unwrap()
                .attributes
                .into_iter()
                .find(|a| a.name == "fill")
                .map(|a| a.value)
        };
        let rest = fill(&mounted);
        mounted.hover(node(&mounted).rect.center());
        mounted.settle();
        (rest, fill(&mounted))
    };
    let (read_rest, read_hover) = hover_fill(read);
    let (_, edit_hover) = hover_fill(edit);
    assert_ne!(read_rest, read_hover, "hover changes the fill");
    assert_ne!(read_hover, edit_hover, "each tool hovers in its own colour");
}

#[test]
fn thinking_mark_follows_model_family_changes() {
    let hue = State::new(ModelHue::Sol);
    let mounted = mount(Size::new(720.0, 80.0), || {
        view! {
            ThinkingLine hue:(hue) effort:3u8
        }
    });
    for family in [
        ModelHue::Sol,
        ModelHue::Astra,
        ModelHue::Luna,
        ModelHue::Terra,
        ModelHue::Haiku,
        ModelHue::Neutral,
    ] {
        hue.set(family);
        mounted.settle();
        let mark = mounted.rect("Working");
        let scene = mounted.ui.scene();
        let tint = scene
            .cmds
            .iter()
            .find_map(|cmd| match cmd {
                mosaic::render::PaintCmd::Shape(shape)
                    if shape.rect.size == Size::new(8.0, 8.0)
                        && shape.rect.origin == mark.origin + Vector2::new(1.0, 1.0) =>
                {
                    shape.fill.as_solid()
                }
                _ => None,
            })
            .expect("painted working square");
        let expected = mosaic::core::theme::color(family.token());
        assert_eq!(
            (tint.r, tint.g, tint.b),
            (expected.r, expected.g, expected.b)
        );
        assert!(tint.a > 0.0);
        assert_eq!(mark.size, Size::new(10.0, 10.0));
    }
}

#[test]
fn ledger_effort_click_is_grey_until_confirmed_and_rejection_restores_selection() {
    for collapsed in [false, true] {
        let state = State::new(gallery::d2::settings(collapsed));
        let confirmed = state.get_untracked().effort;
        let step = 3;
        let mut callbacks = actions(state);
        callbacks.set_effort =
            Rc::new(move |step| state.update(|vm| vm.pending_effort = Some(step)));
        let mounted = mount(Size::new(212.0, 220.0), || {
            view! {
                Ledger data:(state) actions:(callbacks)
            }
        });
        let assert_track = |selected, token| {
            let label = crate::effort::effort_label(confirmed.levels, selected, confirmed.tiers);
            let rect = mounted.rect(&label);
            let tint = mosaic::core::theme::color(token);
            let scene = mounted.ui.scene();
            assert!(
                scene.cmds.iter().any(|cmd| match cmd {
                    mosaic::render::PaintCmd::Shape(shape) => {
                        let centre = shape.rect.center();
                        centre.x >= rect.origin.x
                            && centre.x <= rect.max_x()
                            && centre.y >= rect.origin.y
                            && centre.y <= rect.max_y()
                            && shape.fill.as_solid().is_some_and(|fill| fill == tint)
                    }
                    _ => false,
                }),
                "selected effort should paint in the expected colour"
            );
        };
        mounted.click(
            mounted
                .rect(&format!("Effort {}", confirmed.step_name(step)))
                .center(),
        );
        assert_eq!(state.get_untracked().pending_effort, Some(step));
        assert_track(step, crate::tokens::ground.muted);
        state.update(|vm| {
            vm.effort.current = step;
            vm.pending_effort = None;
        });
        mounted.settle();
        assert_track(step, state.get_untracked().model.hue.token());
        mounted.click(
            mounted
                .rect(&format!("Effort {}", confirmed.step_name(0)))
                .center(),
        );
        assert_track(0, crate::tokens::ground.muted);
        state.update(|vm| vm.pending_effort = None);
        mounted.settle();
        assert_track(step, state.get_untracked().model.hue.token());
    }
}

#[test]
fn inline_composer_puts_send_on_the_right_and_grows_with_the_draft() {
    let mut settings = gallery::d2::settings(true);
    settings.pending_effort = None;
    let value = State::new("Draft".to_owned());
    let settings = State::new(settings);
    let mounted = mount(Size::new(848.0, 1200.0), || {
        view! {
            Composer inline:true settings:(settings) ledger-actions:(actions(settings))
                value:(value) on-send:(noop()) on-attach:(noop()) hint:"Ctrl/Cmd+Enter sends"
        }
    });
    let send = mounted.rect("Send message");
    let attach = mounted.rect("Attach file");
    let editor = mounted.rect("Message");
    assert!(send.origin.x >= editor.max_x());
    assert_eq!(send.origin.x, attach.origin.x);
    assert!(send.origin.y < attach.origin.y);
    let actions = mounted.rect("Composer actions border");
    assert_eq!(actions.origin, send.origin);
    assert_eq!(actions.size.width, send.size.width);
    assert_eq!(actions.max_y(), attach.max_y());
    assert!(mounted.rect("Expand agent settings").max_x() <= editor.origin.x);
    value.set(
        (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    mounted.settle();
    assert!(mounted.rect("Message").size.height > 500.0);
    assert_eq!(mounted.rect("Composer actions border").size, actions.size);
}

#[test]
fn inline_composer_expand_row_geometry_and_painted_chevron() {
    for scale in [0.75, 1.0, 1.5, 2.0] {
        for initial_stacked in [false, true] {
            for height in [40.0, 240.0, 600.0] {
                let settings = State::new(gallery::d2::settings(true));
                let stacked = State::new(initial_stacked);
                let mounted = mount(Size::new(1200.0, 1600.0), || {
                    install_theme(&crate::tokens::theme(
                        crate::tokens::ThemeVariant::Dark,
                        scale,
                    ));
                    let editor = view! {
                        col height:{px(height)}px label:"Injected draft" {
                            Label text:"Draft content"
                        }
                    };
                    view! {
                        col height:min-content {
                            Composer inline:true stacked:(stacked) gutter-width:(Live::fixed(212.0))
                                settings:(settings) ledger-actions:(actions(settings))
                                editor:(Some(editor)) on-send:(noop()) on-attach:(noop())
                        }
                    }
                });
                for is_stacked in [initial_stacked, !initial_stacked] {
                    stacked.set(is_stacked);
                    mounted.settle();
                    let ledger = mounted.rect("Agent ledger");
                    let expand = mounted.rect("Expand agent settings");
                    let model_label = mounted
                        .labels()
                        .into_iter()
                        .find(|s| s.starts_with("Agent settings:"))
                        .unwrap();
                    let model = mounted.rect(&model_label);
                    let effort = settings.get_untracked().effort;
                    let track = mounted.rect(&crate::effort::effort_label(
                        effort.levels,
                        effort.selected(),
                        effort.tiers,
                    ));
                    eprintln!(
                        "INLINE scale={scale} stacked={is_stacked} editor={height} ledger={ledger:?} expand={expand:?} model={model:?} track={track:?}"
                    );
                    assert!((expand.origin.x - ledger.origin.x).abs() < 0.01);
                    assert!((expand.size.width - ledger.size.width).abs() < 0.01);
                    assert!((expand.size.height - 22.0 * scale).abs() < 0.01);
                    assert!(
                        expand.origin.y >= model.max_y() - 0.01
                            && expand.origin.y >= track.max_y() - 0.01
                    );
                    assert!((expand.max_y() - ledger.max_y()).abs() < 0.01);
                    let scene = mounted.ui.scene();
                    let (shape, points) = scene
                        .cmds
                        .iter()
                        .find_map(|cmd| {
                            if let mosaic::render::PaintCmd::Shape(shape) = cmd
                                && expand.contains(shape.rect.center())
                                && let mosaic::render::Geometry::Path {
                                    points,
                                    closed: false,
                                } = &shape.geometry
                                && points.len() == 3
                            {
                                Some((shape, points))
                            } else {
                                None
                            }
                        })
                        .expect("painted chevron path inside expand row");
                    let stroke = &shape.strokes[0];
                    assert_eq!(stroke.cap, LineCap::Round);
                    assert_eq!(stroke.join, LineJoin::Round);
                    assert_eq!(stroke.offset, 0.0);
                    assert_eq!(shape.rotation, 0.0);
                    let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min)
                        - stroke.width / 2.0;
                    let max_x = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max)
                        + stroke.width / 2.0;
                    let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min)
                        - stroke.width / 2.0;
                    let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max)
                        + stroke.width / 2.0;
                    let painted_center = shape.rect.center()
                        + Vector2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                    eprintln!(
                        "PAINTED center={painted_center:?} control={:?}",
                        expand.center()
                    );
                    assert!(
                        (painted_center.x - expand.center().x).abs() < 0.01
                            && (painted_center.y - expand.center().y).abs() < 0.01,
                        "painted chevron is off center"
                    );
                    assert!(
                        shape.rect.center().x + min_x >= expand.origin.x
                            && shape.rect.center().x + max_x <= expand.max_x()
                    );
                    assert!(
                        shape.rect.center().y + min_y >= expand.origin.y
                            && shape.rect.center().y + max_y <= expand.max_y()
                    );
                    drop(scene);
                    let editor_id = mounted
                        .ui
                        .inspection_snapshot()
                        .nodes
                        .iter()
                        .find(|n| n.label.as_deref() == Some("Injected draft"))
                        .unwrap()
                        .id;
                    for x in [expand.origin.x + scale, expand.max_x() - scale] {
                        mounted.click(Vector2::new(x, expand.center().y));
                        assert!(!settings.get_untracked().collapsed);
                        assert!(
                            (mounted.rect("Agent ledger").size.width - 212.0 * scale).abs() < 0.01
                        );
                        settings.update(|s| s.collapsed = true);
                        mounted.settle();
                    }
                    for _ in 0..20 {
                        mounted.key(Key::Tab);
                        if mounted.ui.focused().is_some_and(|f| {
                            mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
                                n.id == f.id()
                                    && n.label.as_deref() == Some("Expand agent settings")
                            })
                        }) {
                            break;
                        }
                    }
                    assert!(mounted.ui.focused().is_some_and(|f| {
                        mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
                            n.id == f.id() && n.label.as_deref() == Some("Expand agent settings")
                        })
                    }));
                    mounted.key(Key::Enter);
                    assert!(!settings.get_untracked().collapsed);
                    assert!(
                        mounted.ui.inspection_snapshot().node(editor_id).is_some(),
                        "editor survives the ledger toggle"
                    );
                    settings.update(|s| s.collapsed = true);
                    mounted.settle();
                }
            }
        }
    }
}

#[test]
fn working_peek_streams_and_discloses_public_summary() {
    let summary = State::new(String::new());
    let run_key = State::new("session/run-one".to_owned());
    let mounted = mount(Size::new(720.0, 300.0), || {
        view! {
            ThinkingLine summary:(summary) run-key:(run_key)
        }
    });
    assert!(mounted.labels().iter().any(|s| s == "No update yet"));
    assert!(!mounted.labels().iter().any(|s| s == "Peek"));
    summary.set("Checking the provider adapter".into());
    mounted.settle();
    let peek = mounted.rect("Peek");
    assert!(peek.size.width > 0.0);
    mounted.click(peek.center());
    assert!(mounted.labels().iter().any(|s| s == "Latest Update"));
    summary.set("Checking the provider adapter and stream λ".into());
    run_key.set("session/run-one".into());
    mounted.settle();
    assert!(mounted.labels().iter().any(|s| s.contains("stream λ")));
    assert!(mounted.labels().iter().any(|s| s == "Latest Update"));
    mounted.click(mounted.rect("Hide").center());
    assert!(!mounted.labels().iter().any(|s| s == "Latest Update"));
    mounted.click(mounted.rect("Peek").center());
    run_key.set("session/run-two".into());
    mounted.settle();
    assert!(!mounted.labels().iter().any(|s| s == "Latest Update"));
    summary.set(String::new());
    mounted.settle();
    assert!(mounted.labels().iter().any(|s| s == "No update yet"));
}

#[test]
fn working_peek_bounds_long_unicode_and_keeps_keyboard_controls_inside_narrow_scaled_views() {
    for (text, expected) in [
        ("λ".repeat(65536), 240),
        ("🐱".repeat(20000), 240),
        (" \n\t".into(), 0),
    ] {
        let (excerpt, truncated) = crate::transcript::update_excerpt(&text, 240);
        assert_eq!(excerpt.chars().count(), expected);
        assert_eq!(truncated, expected > 0);
    }
    assert_eq!(
        crate::transcript::update_excerpt("  α  \n β ", 240),
        ("α β".into(), false)
    );
    for (width, scale) in [(360.0, 1.0), (360.0, 2.0), (720.0, 2.0)] {
        let summary = State::new("🐱".repeat(20000));
        let identity = State::new("first/run".to_owned());
        let mounted = mount(Size::new(width, 500.0), || {
            install_theme(&crate::tokens::theme(
                crate::tokens::ThemeVariant::Dark,
                scale,
            ));
            view! {
                ThinkingLine summary:(summary) run-key:(identity)
            }
        });
        let button = mounted.rect("Peek");
        assert!(
            button.origin.x >= 0.0 && button.max_x() <= width + 1.0,
            "{button:?}"
        );
        assert!(
            mounted
                .labels()
                .iter()
                .all(|label| label.chars().count() <= 241)
        );
        mounted.key(Key::Tab);
        mounted.key(Key::Enter);
        assert!(
            mounted
                .labels()
                .iter()
                .any(|label| label == "Latest Update")
        );
        mounted.key(Key::Space);
        assert!(
            !mounted
                .labels()
                .iter()
                .any(|label| label == "Latest Update")
        );
        mounted.key(Key::Enter);
        identity.set("second/run".into()); // Both sessions remain Working.
        mounted.settle();
        assert!(
            !mounted
                .labels()
                .iter()
                .any(|label| label == "Latest Update")
        );
        summary.set(String::new()); // Remove the focused disclosure control safely.
        mounted.settle();
        assert!(
            !mounted
                .labels()
                .iter()
                .any(|label| label == "Peek" || label == "Hide")
        );
        assert!(
            mounted
                .labels()
                .iter()
                .any(|label| label == "No update yet")
        );
    }
}
