//! The conversation's relay-ui parts: the session header, tool calls, spawn
//! and approval blocks, queued turns, and the ledger's settings.

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
use relay_ui::prelude::{CellButton, CellButtonProps, Label, LabelProps, Rule, TextRole, Tone};
use relay_ui::tokens::{ToolKind as UiKind, ground, px};
use std::rc::Rc;

/// What the session header shows; rebuilt only when it changes.
#[derive(Clone, Debug, PartialEq)]
struct HeaderView {
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
                        status:(view.status) effort:(view.effort) context:(view.context.clone())
                        on-compact:(compact) below:(view.below.clone()) on-below:(below)
                        on-pause:(pause) paused:(view.paused) on-stop:(stop)
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
                col width:{px(34.0)}px shrink:0 align:center pad:(top:{px(9.0)}px)
                    stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                    Label text:(position) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                }
                col width:1fr min-width:0px height:min-content gap:0px {
                    col height:min-content gap:{px(8.0)}px
                        pad:(horizontal:{px(12.0)}px vertical:{px(9.0)}px) {
                        Label
                            text:(Derived::new(move || current.get().map(|s| plain_text(&s.parts)).unwrap_or_default()))
                            role:(TextRole::Body)
                        if current.get().is_some_and(|s| s.error.is_some()) {
                            Label
                                text:(Derived::new(move || current.get().and_then(|s| s.error).unwrap_or_default()))
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    }
                    row height:{px(32.0)}px gap:0px
                        stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                        {
                            parent.adopt(&queued_action("Cancel queued message", "Cancel", true, move || model.submit(Command::CancelTurn { submission_id: id.get_untracked() }, model.snapshot.get_untracked().revision, crate::model::Saved::Action)));
                            parent.adopt(&queued_action("Edit queued message", "Edit", false, move || buffer::edit_queued(model, &id.get_untracked())));
                            parent.adopt(&queued_action("Send queued message now", "Send now", false, move || buffer::promote(model, &id.get_untracked())));
                        }
                    }
                }
            }
        }
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
        .and_then(|c| c.models.iter().find(|m| Some(&m.id) == model_id.as_ref()));
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

/// An action cell of a queued message, divided from the one before it.
fn queued_action(
    label: &'static str,
    text: &'static str,
    first: bool,
    handler: impl Fn() + 'static,
) -> Element {
    let rule = if first { Rule::None } else { Rule::Left };
    view! {
        row width:max-content shrink:0 {
            CellButton label:(label) rule:(rule) horizontal-inset:(12.0) on-press:(press(handler)) {
                Label text:(text) role:(TextRole::Label) weight:500
            }
        }
    }
}
