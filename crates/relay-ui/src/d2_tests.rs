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
                s.effort.at_max = s.effort.has_max && level == s.effort.levels;
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
            assert_eq!(mounted.rect(opening.label()).size, Size::new(26.0, 26.0));
            mounted.click(mounted.rect(opening.label()).center());
            assert_eq!(value.get_untracked().opening, opening.next());
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
    assert_eq!(mounted.rect("Send").size.width, 44.0);
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
    mounted.click(mounted.rect("Send").center());
    mounted.click(mounted.rect("Attach file").center());
    assert_eq!(sent.get_untracked(), 1);
    assert_eq!(attached.get_untracked(), 1);
    value.set(String::new());
    mounted.settle();
    mounted.click(mounted.rect("Send").center());
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
    assert_eq!(body.origin.x, (1100.0 - 848.0) / 2.0 + 128.0 + 16.0);
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
        has_max: true,
        at_max: true,
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
    mounted.key(Key::Enter);
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
    mounted.click(mounted.rect("Send").center());
    assert_eq!(sent.get_untracked(), 0);
    present.set(true);
    mounted.settle();
    mounted.click(mounted.rect("Send").center());
    assert_eq!(sent.get_untracked(), 1);
}
