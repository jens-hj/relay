//! The conversation's relay-ui parts: the session header, tool calls, spawn
//! and approval blocks, queued turns, and the ledger's settings.

use crate::conversation::{InlineAsset, InlineAssetProps};
use crate::ui::press;
use crate::ui::{NavigationButton, NavigationButtonProps};
use crate::window_chrome::{WindowControlsSpace, WindowControlsSpaceProps};
use crate::{adapt, buffer, icons::*, model::Model};
use mosaic::prelude::*;
use relay_core::*;
use relay_ui::prelude::{
    AgentStatus, ApprovalBlock, ApprovalBlockProps, LedgerView, MessageRow, MessageRowProps,
    ModelHue, ModelIdentity, OnPress, Opening, SessionHeader, SessionHeaderProps, SpawnBlock,
    SpawnBlockProps, SpawnChild, ToolCallRow, ToolCallRowProps, ToolCallView, ToolDetail,
};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Label, LabelProps, Rule, TextRole, Tone, Tooltip, TooltipProps,
};
use relay_ui::tokens::{ToolKind as UiKind, ground, px};
use std::rc::Rc;

/// Use page-independent metadata, scoped to the selected session's active run.
pub(crate) fn progress_summary(
    snapshot: &Snapshot,
    history: &HistoryState,
    session: &str,
    active_run: Option<&str>,
) -> String {
    if history.session_id != session
        || !snapshot.sessions.iter().any(|s| {
            s.id == session
                && s.worker
                    .as_ref()
                    .is_some_and(|w| w.status == WorkerStatus::Running)
        })
    {
        return String::new();
    }
    history
        .progress_summary
        .as_ref()
        .filter(|summary| {
            Some(summary.run_id.as_str()) == active_run && !summary.text.trim().is_empty()
        })
        .map(|summary| summary.text.clone())
        .unwrap_or_default()
}

/// What the session header shows; rebuilt only when it changes.
#[derive(Clone, Debug, PartialEq)]
struct HeaderView {
    model_hue: Option<relay_ui::model::ModelHue>,
    path: Vec<String>,
    title: String,
    status: AgentStatus,
    effort: Option<relay_ui::prelude::Effort>,
    context: Option<relay_ui::prelude::ContextView>,
    below: Vec<AgentStatus>,
    stoppable: bool,
    /// Turns wait to follow the current one.
    queued: bool,
    /// Turns are held until resumed.
    paused: bool,
}

fn header_view(model: Model) -> HeaderView {
    let snapshot = model.snapshot.get();
    let id = model.session.get();
    let Some(session) = snapshot.sessions.iter().find(|s| s.id == id) else {
        let draft = model.page.get() == crate::model::Page::AgentStart;
        return HeaderView {
            model_hue: None,
            path: Vec::new(),
            title: if draft {
                "Agent conversation".into()
            } else {
                "Agent".into()
            },
            status: AgentStatus::Queued,
            effort: None,
            context: None,
            below: Vec::new(),
            stoppable: false,
            queued: false,
            paused: false,
        };
    };
    HeaderView {
        model_hue: adapt::session_model(session, &model.catalogs.get())
            .0
            .map(|model| model.hue),
        path: adapt::ancestor_path(&snapshot, &id),
        title: session.title.clone(),
        status: adapt::session_status(&snapshot, session),
        effort: adapt::session_model(session, &model.catalogs.get()).1,
        context: adapt::context_view(session),
        below: adapt::subtree_statuses(&snapshot, &id),
        stoppable: session
            .worker
            .as_ref()
            .is_some_and(|w| matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)),
        queued: snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == id && s.state == SubmissionState::Queued),
        paused: snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == id && s.state == SubmissionState::Paused),
    }
}

/// The 88px session header row: navigation on phones, the session header
/// with its context gauge and controls, session actions and the window
/// controls.
#[component]
pub(crate) fn SessionHead(model: Model, menu: State<bool>, compact: Derived<bool>) -> Element {
    // Narrow headers keep Stop in the session actions instead.
    let data = Derived::new(move || {
        let mut view = header_view(model);
        view.stoppable &= !compact.get();
        view
    });
    let host = view! {
        row width:1fr min-width:0px {}
    };
    host.switch(
        move || data.get(),
        move |parent, view| {
            let stop: Option<OnPress> = view
                .stoppable
                .then(|| Rc::new(move || model.stop_worker()) as OnPress);
            let below: Option<OnPress> = (!view.below.is_empty()).then(|| {
                Rc::new(move || {
                    let id = model.session.get_untracked();
                    model.collapsed_agents.update(|ids| {
                        ids.remove(&id);
                    });
                    model.agent_focus.set(Some(id));
                }) as OnPress
            });
            let pause: Option<OnPress> = (view.queued || view.paused)
                .then(|| Rc::new(move || model.toggle_queue()) as OnPress);
            let compact: Option<OnPress> = view
                .context
                .is_some()
                .then(|| Rc::new(move || crate::command_ui::run(model, "compact", "")) as OnPress);
            parent.adopt(&view! {
                col width:1fr min-width:0px {
                    SessionHeader path:(view.path.clone()) title:(view.title.clone())
                        model-hue:(view.model_hue) status:(view.status) effort:(view.effort)
                        context:(view.context.clone()) on-compact:(compact)
                        below:(view.below.clone()) on-below:(below) on-pause:(pause)
                        paused:(view.paused) on-stop:(stop)
                }
            });
        },
    );
    view! {
        row height:{px(88.0)}px shrink:0 label:"Session header"
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
            @pointer:{move |event, ctx| crate::window_chrome::drag_header(model, event, ctx)} {
            NavigationButton model:(model)
            { parent.adopt(&host); }
            row width:{px(44.0)}px shrink:0 {
                CellButton label:"Session actions" rule:(Rule::Left) horizontal-inset:(0.0)
                    selected:(menu) on-press:(press(move || menu.set(!menu.get_untracked()))) {
                    icon size:{px(16.0)}px more-icon
                }
            }
            WindowControlsSpace
        }
    }
}

/// A tool call row, or the spawned agent's tree row for a spawn whose
/// child session is known. Openings belong to the row; failures open to
/// their summary until changed.
#[component]
pub(crate) fn ToolMessage(model: Model, message: Derived<Option<Message>>) -> Element {
    let chosen = State::new(None::<Opening>);
    let controller = model.conversation_controller.get_untracked();
    let id = message.get_untracked().map(|m| m.id).unwrap_or_default();
    if let Some(controller) = &controller {
        chosen.set(controller.borrow().tool_openings.get(&id).copied());
    }
    Effect::new(move || {
        let opening = chosen.get();
        if let Some(controller) = &controller
            && let Some(opening) = opening
        {
            let mut state = controller.borrow_mut();
            if state.tool_openings.insert(id.clone(), opening) != Some(opening) {
                state.invalidate_history_height(&id);
            }
        }
    });
    let data = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let message = message.get()?;
        let failed = adapt::tool_status(&snapshot, &message) == AgentStatus::Failed;
        let opening = chosen.get().unwrap_or(if failed {
            Opening::Summary
        } else {
            Opening::Collapsed
        });
        adapt::tool_view(&snapshot, &message, opening, &model.catalogs.get())
    });
    let child = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let message = message.get()?;
        let tool = message.tool.as_ref()?;
        let Some(ToolDetailRef::Spawn(id)) = spawn_child(tool) else {
            return None;
        };
        let session = snapshot.sessions.iter().find(|s| s.id == id)?;
        let (model_identity, effort) = adapt::session_model(session, &model.catalogs.get());
        Some(SpawnChild {
            name: session.title.clone(),
            status: adapt::session_status(&snapshot, session),
            harness: session.worker.as_ref().map(|w| adapt::harness(w.harness)),
            model: model_identity,
            effort,
        })
    });
    let opening: Rc<dyn Fn(Opening)> = Rc::new(move |next| chosen.set(Some(next)));
    let open_tree: OnPress = Rc::new(move || {
        if let Some(parent) = message.get_untracked().map(|m| m.session_id) {
            model.collapsed_agents.update(|ids| {
                ids.remove(&parent);
            });
            model.agent_focus.set(Some(parent));
        }
    });
    let id = message.get_untracked().map(|m| m.id).unwrap_or_default();
    let host = view! {
        col height:min-content min-width:0px label:(format!("Tool call {id}"))
            description:{data.get().map(|d| format!("{} · {}", d.verb, d.status.label())).unwrap_or_default()} {}
    };
    host.switch(
        move || (child.get(), data.get().is_some()),
        move |parent, (child, known)| {
            if let Some(child) = child {
                parent.adopt(&view! {
                    SpawnBlock agents:(relay_ui::Live::fixed(vec![child.clone()]))
                        on-open-tree:(open_tree.clone())
                });
            } else if *known {
                let data = Derived::new(move || data.get().unwrap_or_else(placeholder));
                parent.adopt(&view! {
                    ToolCallRow data:(relay_ui::Live::from(data)) on-opening:(opening.clone())
                });
            }
        },
    );
    host
}

enum ToolDetailRef {
    Spawn(String),
}

fn spawn_child(tool: &ToolCall) -> Option<ToolDetailRef> {
    match &tool.detail {
        Some(relay_core::ToolDetail::Spawn {
            child_session_id: Some(id),
        }) => Some(ToolDetailRef::Spawn(id.clone())),
        _ => None,
    }
}

fn placeholder() -> ToolCallView {
    ToolCallView {
        kind: UiKind::Other,
        status: AgentStatus::Queued,
        verb: String::new(),
        target: None,
        opening: Opening::Collapsed,
        detail: None::<ToolDetail>,
    }
}

/// A pending permission request from this session or one below it. One
/// answer resolves it in every transcript and in Needs you.
#[component]
pub(crate) fn PendingApproval(
    model: Model,
    permission: ToolPermission,
    on_review: Rc<dyn Fn(String)>,
) -> Element {
    let data = {
        let permission = permission.clone();
        Derived::new(move || {
            adapt::approval_view(&model.snapshot.get(), &permission, &model.session.get())
        })
    };
    let approve: OnPress = {
        let permission = permission.clone();
        Rc::new(move || crate::sidebar::respond(model, &permission, true))
    };
    let reject: OnPress = {
        let permission = permission.clone();
        Rc::new(move || crate::sidebar::respond(model, &permission, false))
    };
    let requester = permission.session_id.clone();
    let review: OnPress = Rc::new(move || on_review(requester.clone()));
    view! {
        MessageRow speaker:"Needs you" {
            col height:min-content label:"Approval request" {
                ApprovalBlock data:(relay_ui::Live::from(data)) on-approve:(approve)
                    on-reject:(reject) on-review-diff:(review)
            }
        }
    }
}

/// A queued or paused turn with its cancel, edit and send-now actions.
#[component]
pub(crate) fn QueuedTurn(model: Model, submission: String) -> Element {
    let id = State::new(submission);
    let current = Derived::new(move || {
        model
            .snapshot
            .get()
            .submissions
            .into_iter()
            .find(|s| s.id == id.get())
    });
    let speaker = Derived::new(move || match current.get().map(|s| s.state) {
        Some(SubmissionState::Paused) => "Paused".to_string(),
        _ => "Queued".to_string(),
    });
    let position = Derived::new(move || {
        let snapshot = model.snapshot.get();
        snapshot
            .submissions
            .iter()
            .filter(|s| {
                s.session_id == model.session.get()
                    && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
            })
            .position(|s| s.id == id.get())
            .map(|i| format!("{:02}", i + 1))
            .unwrap_or_default()
    });
    view! {
        MessageRow speaker:(relay_ui::Live::from(speaker)) {
            row height:min-content min-width:0px label:"Queued message"
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                col width:{px(34.0)}px min-height:{px(80.0)}px shrink:0 gap:0px {
                    row height:{px(20.0)}px shrink:0 align:center justify:center {
                        { parent.adopt(&queued_position(model, position)); }
                    }
                    {
                        parent.adopt(&queued_action("Cancel queued message", relay_ui::icons::mark.remove, Tone::Signal(relay_ui::tokens::Signal::Failed), false, move || model.submit(Command::CancelTurn { submission_id: id.get_untracked() }, model.snapshot.get_untracked().revision, crate::model::Saved::Action)));
                        parent.adopt(&queued_action("Edit queued message", relay_ui::icons::mark.edit, Tone::Signal(relay_ui::tokens::Signal::NeedsYou), false, move || buffer::edit_queued(model, &id.get_untracked())));
                        parent.adopt(&queued_action("Send queued message now", relay_ui::icons::mark.send, Tone::Signal(relay_ui::tokens::Signal::FullSteam), true, move || buffer::promote(model, &id.get_untracked())));
                    }
                }
                el width:{px(1.0)}px height:fill shrink:0 nohit fill:ground.hairline {}
                col width:1fr min-width:0px height:min-content gap:0px {
                    col height:min-content gap:{px(8.0)}px
                        pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px) {
                        for (_, part) in { current.get().map(|s| s.parts.into_iter().map(|part| ((part.id.clone(), format!("{part:?}")), part)).collect::<Vec<_>>()).unwrap_or_default() } {
                            col height:min-content min-width:0px {
                                QueuedPart model:(model) part:(part.clone())
                            }
                        }
                        if current.get().is_some_and(|s| s.error.is_some()) {
                            Label
                                text:(Derived::new(move || current.get().and_then(|s| s.error).unwrap_or_default()))
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    }
                }
            }
        }
    }
}

/// Read-only structured content; attachments share the chat's preview and fetch path.
#[component]
fn QueuedPart(model: Model, part: Part) -> Element {
    match part.kind {
        PartKind::Text { text } => view! {
            col height:min-content min-width:0px {
                Label text:(text) role:(TextRole::Body)
            }
        },
        PartKind::Asset { asset } => view! {
            col height:min-content min-width:0px {
                InlineAsset model:(model) asset:(asset) part-id:(None) controller:(None)
                    mirror:false
            }
        },
        PartKind::Skill { skill } => view! {
            col height:min-content {
                Label text:(format!("${}", skill.name)) role:(TextRole::Body)
            }
        },
        PartKind::Reply { anchor, parts } => view! {
            col height:min-content min-width:0px gap:{px(6.0)}px pad:(left:{px(14.0)}px)
                stroke:(width:{px(2.0)} color:ground.hairline edges:left) {
                Label text:(format!("↩ {}", anchor.quote)) role:(TextRole::Caption)
                    tone:(Tone::Muted)
                for (_, nested) in { parts.iter().cloned().map(|part| (part.id.clone(), part)).collect::<Vec<_>>() } {
                    col height:min-content min-width:0px {
                        QueuedPart model:(model) part:(nested.clone())
                    }
                }
            }
        },
    }
}

/// The ledger: the session's harness, its model (reported, else selected,
/// else the harness default) and the effort its catalog describes.
pub(crate) fn ledger_view(model: Model, open: bool) -> LedgerView {
    let snapshot = model.snapshot.get();
    let catalogs = model.catalogs.get();
    let id = model.session.get();
    let session = snapshot.sessions.iter().find(|s| s.id == id);
    let director = session
        .map(|s| s.agent_config_id.clone())
        .unwrap_or_else(|| model.worker_director.get());
    let harness = session
        .and_then(|s| s.worker.as_ref().map(|w| w.harness))
        .or_else(|| {
            snapshot
                .agent_configs
                .iter()
                .find(|d| d.id == director)
                .and_then(|d| snapshot.effective_profile(d).ok())
                .map(|p| p.harness)
        })
        .unwrap_or_default();
    let (identity, mut effort) = session
        .map(|s| {
            let mut s = s.clone();
            s.selection = model.harness_selection();
            adapt::session_model(&s, &catalogs)
        })
        .unwrap_or_else(|| {
            adapt::selection_model(
                &model.harness_selection(),
                None,
                &id,
                Some(harness),
                &catalogs,
            )
        });
    if let Some(name) = model.acknowledged_effort(&id)
        && let Some(effort) = effort.as_mut()
        && let Some(step) =
            (0..effort.levels + effort.tiers).find(|&step| effort.step_name(step) == name)
    {
        effort.current = step;
    }
    let pending_effort = model.pending_effort(&id).and_then(|name| {
        let effort = effort?;
        (0..effort.levels + effort.tiers).find(|&step| effort.step_name(step) == name)
    });
    LedgerView {
        pending_effort,
        harness: adapt::harness(harness),
        model: identity.unwrap_or_else(|| ModelIdentity::new("Default", "", ModelHue::Neutral)),
        // Without the model's steps, name the selected effort alone.
        effort_label: effort
            .is_none()
            .then(|| model.harness_selection().effort)
            .flatten(),
        effort: effort.unwrap_or_default(),
        collapsed: !open,
    }
}

/// Selects an effort step of the session's model.
pub(crate) fn choose_effort(model: Model, step: u8) {
    let snapshot = model.snapshot.get_untracked();
    let id = model.session.get_untracked();
    let session = snapshot.sessions.iter().find(|s| s.id == id);
    let current = model.harness_selection();
    let model_id = current.model.clone().or_else(|| {
        session
            .and_then(|s| s.worker.as_ref())
            .and_then(|w| w.model.clone())
    });
    let catalogs = model.catalogs.get_untracked();
    let chosen = catalogs
        .get(&id)
        .and_then(|c| c.as_ref().ok())
        .and_then(|c| c.model(model_id.as_deref()));
    let Some(chosen) = chosen else {
        model.notice.set("Choose a session model first".into());
        return;
    };
    let Some(effort) = adapt::effort_choice(chosen, step) else {
        return;
    };
    model.submit(
        Command::SetHarnessSelection {
            session_id: id,
            selection: HarnessSelection {
                model: model_id,
                effort: Some(effort),
                fast: current.fast,
                plan: current.plan,
            },
        },
        snapshot.revision,
        crate::model::Saved::Action,
    );
}

/// Center the painted digits, whose ink is asymmetric inside the font's line box.
fn queued_position(model: Model, position: Derived<String>) -> Element {
    let host = view! {
        el width:fill height:fill nohit label:{position.get()} font-family:monospace
            font-size:{px(TextRole::Caption.size())}px font-weight:500 font-color:ground.muted {}
    };
    host.role(Role::Label);
    let repaint = host.clone();
    Effect::new(move || {
        let _ = position.get();
        repaint.content_dirty();
    });
    let fonts = model.ui.get_untracked().fonts();
    host.paint(move |ctx| {
        let mut fonts = fonts.borrow_mut();
        let shaped = fonts.shape(&position.get_untracked(), &ctx.text_style, None);
        let origin = Vector2::new(ctx.rect.center().x - shaped.metrics().size.width / 2.0, 0.0);
        let mut run = shaped.place(&mut fonts, ctx.text_color.clone(), origin, ctx.scale);
        let Some(first) = run.glyphs.first() else {
            return;
        };
        let bounds = run
            .glyphs
            .iter()
            .fold(first.dest, |bounds, glyph| bounds.union(glyph.dest));
        let offset = Vector2::new(0.0, ctx.rect.center().y - bounds.center().y);
        for glyph in &mut run.glyphs {
            glyph.dest = glyph.dest.translate(offset);
        }
        ctx.scene.glyphs(run);
    });
    host
}

/// A compact action in the queue-number gutter, described on hover and focus.
fn queued_action(
    label: &'static str,
    icon: SvgToken,
    tone: Tone,
    stretch: bool,
    handler: impl Fn() + 'static,
) -> Element {
    view! {
        Tooltip text:(label)
            height:(if stretch { Dimension::Fr(1.0) } else { Dimension::Px(px(20.0)) })
            min-height:(px(20.0)) {
            row width:{px(34.0)}px min-height:{px(20.0)}px shrink:0 {
                CellButton label:(label) rule:(Rule::Top) horizontal-inset:(0.0)
                    on-press:(press(handler)) {
                    row width:max-content height:min-content
                        font-color:{mosaic::core::theme::color(tone.token())} {
                        { parent.adopt(&relay_ui::icons::icon_mark(icon, 12.0)); }
                    }
                }
            }
        }
    }
}
