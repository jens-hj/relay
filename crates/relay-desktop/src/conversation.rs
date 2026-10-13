//! A Relay-owned document interaction controller over Mosaic's native text primitives.
use crate::command_ui::{CommandPanel, CommandPanelProps};
use crate::panels::{BoundedPanel, BoundedPanelProps};
use crate::transcript::{
    PendingApproval, PendingApprovalProps, QueuedTurn, QueuedTurnProps, SessionHead,
    SessionHeadProps, ToolMessage, ToolMessageProps, choose_effort, ledger_view,
};
use crate::ui::{Notice, NoticeProps};
use crate::ui::{Readout, ReadoutProps, UsageReadout, UsageReadoutProps};
use crate::{
    buffer,
    controls::{ButtonStyle, button},
    model::{EditTarget, Model},
};
use mosaic::core::theme::color;
use mosaic::{
    prelude::*,
    text::{CaretMotion, EditBuffer},
    widgets::input::TextInputContents,
};
use relay_core::*;
use relay_ui::prelude::{
    AgentText, AgentTextProps, Composer, ComposerProps, FingerprintStrip, FingerprintStripProps,
    LedgerActions, MessageRow, MessageRowProps, Segments, SegmentsProps, ThinkingLine,
    ThinkingLineProps, UserMessage, UserMessageProps,
};
use relay_ui::style::*;
use relay_ui::tokens::{ground, px, status};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Source {
        message: String,
        start: usize,
        end: Option<usize>,
    },
    Draft(String),
}

type StagedPreedit = (String, String, Option<(usize, usize)>);

#[derive(Clone)]
struct History {
    parts: Vec<Part>,
    focus: Option<(String, usize)>,
}
#[derive(Default)]
pub struct Controller {
    surfaces: BTreeMap<String, (Element, Rc<RefCell<EditBuffer>>)>,
    messages: BTreeMap<String, Element>,
    undo: Vec<History>,
    redo: Vec<History>,
    caret: Option<(String, usize)>,
    selection: Option<DraftSelection>,
    pointer_anchor: Option<(String, usize, bool)>,
    requested: Option<(String, usize)>,
    session: String,
    search_hit: Option<(String, usize)>,
    preedit: Option<StagedPreedit>,
    viewport: Option<Scroll>,
    draft_viewport: Option<Scroll>,
    resize_follow: bool,
    history_anchor: Option<(String, f32)>,
    history_scroll_seen: bool,
}

type Shared = Rc<RefCell<Controller>>;
pub(crate) type ControllerState = State<Shared>;

fn text_part(parts: &[Part], id: &str) -> Option<String> {
    for part in parts {
        if part.id == id
            && let PartKind::Text { text } = &part.kind
        {
            return Some(text.clone());
        }
        if let PartKind::Reply { parts, .. } = &part.kind
            && let Some(text) = text_part(parts, id)
        {
            return Some(text);
        }
    }
    None
}
fn replace_text(parts: &mut [Part], id: &str, text: &str) -> bool {
    for part in parts {
        if part.id == id
            && let PartKind::Text { text: value } = &mut part.kind
        {
            *value = text.into();
            return true;
        }
        if let PartKind::Reply { parts, .. } = &mut part.kind
            && replace_text(parts, id, text)
        {
            return true;
        }
    }
    false
}
fn current_text(model: Model, location: &Location) -> String {
    match location {
        Location::Draft(id) => {
            text_part(&buffer::parts(model, &model.session.get()), id).unwrap_or_default()
        }
        Location::Source {
            message,
            start,
            end,
        } => model
            .snapshot
            .get()
            .messages
            .iter()
            .find(|m| &m.id == message)
            .and_then(|m| {
                m.body
                    .get(*start..end.unwrap_or(m.body.len()))
                    .map(str::to_owned)
            })
            .unwrap_or_default(),
    }
}
fn remember(controller: &Shared, parts: Vec<Part>) {
    let mut controller = controller.borrow_mut();
    if controller.undo.last().map(|h| &h.parts) != Some(&parts) {
        let focus = controller.caret.clone();
        controller.undo.push(History { parts, focus });
        if controller.undo.len() > 256 {
            controller.undo.remove(0);
        }
    }
    controller.redo.clear();
}
fn update_text(model: Model, controller: &Shared, id: &str, text: String) {
    let session = model.session.get_untracked();
    let mut parts = buffer::parts(model, &session);
    remember(controller, parts.clone());
    if !replace_text(&mut parts, id, &text) {
        parts.push(Part {
            id: id.into(),
            kind: PartKind::Text { text },
        });
    }
    buffer::edit(model, &session, parts);
}

fn accept_completion(model: Model, controller: &Shared, index: usize, mirror: bool) {
    let Some(completion) = model.completion.get_untracked() else {
        return;
    };
    let Some(choice) = completion.choices.get(index).cloned() else {
        return;
    };
    model.completion.set(None);
    if let Some(command) = choice.command {
        if command.dispatch == CommandDispatch::Unavailable {
            model.notice.set(command.reason.unwrap_or_default());
            return;
        }
        // Keep command text as a draft until a command flow is submitted.
        insert_parts(
            model,
            controller,
            &completion.part,
            (completion.start, completion.end),
            vec![Part::text(format!("/{} ", command.name))],
            mirror,
        );
        if command.dispatch == CommandDispatch::Flow {
            crate::command_ui::open(model, &command.name);
        }
    } else if let Some(skill) = choice.skill {
        insert_parts(
            model,
            controller,
            &completion.part,
            (completion.start, completion.end),
            vec![
                Part {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: PartKind::Skill { skill },
                },
                Part::text(" "),
            ],
            mirror,
        );
    }
}

pub fn begin_reply(
    model: Model,
    controller: &Shared,
    location: &Location,
    selection: (usize, usize),
    insert: &str,
) {
    let Location::Source { message, start, .. } = location else {
        return;
    };
    if model.history.get_untracked().preview_ids.contains(message) {
        model.load_message(message.clone());
        model
            .notice
            .set("Loading the full message. Select the passage again when it is ready.".into());
        return;
    }
    let snapshot = model.snapshot.get_untracked();
    let Some(source) = snapshot.messages.iter().find(|m| &m.id == message) else {
        return;
    };
    let (mut begin, mut end) = (start + selection.0, start + selection.1);
    if begin == end {
        begin = source.body[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);
        end = source.body[end..]
            .find('\n')
            .map(|i| end + i)
            .unwrap_or(source.body.len());
    }
    let Some(quote) = source.body.get(begin..end) else {
        return;
    };
    let text = Part::text(insert);
    let text_id = text.id.clone();
    let reply = Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Reply {
            anchor: relay_core::Anchor {
                message_id: message.clone(),
                start: begin,
                end,
                quote: quote.into(),
            },
            parts: vec![text],
        },
    };
    let session = model.session.get_untracked();
    let mut parts = buffer::parts(model, &session);
    remember(controller, parts.clone());
    parts.push(reply);
    parts.push(Part::text(""));
    controller.borrow_mut().requested = Some((format!("inline-{text_id}"), insert.len()));
    buffer::edit(model, &session, parts);
}

fn insert_parts(
    model: Model,
    controller: &Shared,
    id: &str,
    selection: (usize, usize),
    inserted: Vec<Part>,
    mirror: bool,
) {
    fn insert(
        parts: &mut Vec<Part>,
        id: &str,
        selection: (usize, usize),
        inserted: &[Part],
    ) -> Option<String> {
        for index in 0..parts.len() {
            if parts[index].id == id
                && let PartKind::Text { text } = &parts[index].kind
            {
                let before = text.get(..selection.0)?.to_owned();
                let after = text.get(selection.1..)?.to_owned();
                let tail = Part::text(after);
                let tail_id = tail.id.clone();
                let mut next = vec![Part {
                    id: id.into(),
                    kind: PartKind::Text { text: before },
                }];
                next.extend(inserted.iter().cloned());
                next.push(tail);
                parts.splice(index..=index, next);
                return Some(tail_id);
            }
            if let PartKind::Reply { parts, .. } = &mut parts[index].kind
                && let Some(id) = insert(parts, id, selection, inserted)
            {
                return Some(id);
            }
        }
        None
    }
    let session = model.session.get_untracked();
    let mut parts = buffer::parts(model, &session);
    if text_part(&parts, id).is_none() {
        parts.push(Part {
            id: id.into(),
            kind: PartKind::Text {
                text: String::new(),
            },
        });
    }
    remember(controller, parts.clone());
    if let Some(tail) = insert(&mut parts, id, selection, &inserted) {
        controller.borrow_mut().requested = Some((
            format!("{}-{tail}", if mirror { "inline" } else { "draft" }),
            0,
        ));
    }
    buffer::edit(model, &session, parts);
}

fn document_edge(controller: &Shared, fonts: &mut mosaic::text::FontContext, end: bool) {
    let state = controller.borrow();
    let mut surfaces = state.surfaces.values().collect::<Vec<_>>();
    surfaces.sort_by(|a, b| {
        a.0.layout_rect()
            .origin
            .y
            .total_cmp(&b.0.layout_rect().origin.y)
    });
    if let Some((field, editor)) = if end {
        surfaces.last()
    } else {
        surfaces.first()
    } {
        let offset = if end { editor.borrow().text().len() } else { 0 };
        editor
            .borrow_mut()
            .set_selection_bytes(fonts, offset, offset);
        field.focus();
        field.reveal();
        field.paint_dirty();
    }
}

fn neighboring(
    controller: &Shared,
    key: &str,
    direction: i32,
    fonts: &mut mosaic::text::FontContext,
) {
    let controller = controller.borrow();
    let mut entries = controller.surfaces.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.1.0
            .layout_rect()
            .origin
            .y
            .total_cmp(&b.1.0.layout_rect().origin.y)
    });
    if let Some(index) = entries.iter().position(|(id, _)| id.as_str() == key) {
        let next = index as i32 + direction;
        if next >= 0
            && let Some((_, (element, buffer))) = entries.get(next as usize)
        {
            let end = if direction < 0 {
                buffer.borrow().text().len()
            } else {
                0
            };
            buffer.borrow_mut().set_selection_bytes(fonts, end, end);
            element.focus();
            element.reveal();
            element.paint_dirty();
        }
    }
}

#[derive(Clone)]
struct DraftSelection {
    anchor: (String, usize),
    head: (String, usize),
}

fn selection_slice(
    parts: &[Part],
    selection: &DraftSelection,
) -> Option<(usize, usize, usize, usize)> {
    let a = parts.iter().position(|p| p.id == selection.anchor.0)?;
    let b = parts.iter().position(|p| p.id == selection.head.0)?;
    if a < b || (a == b && selection.anchor.1 <= selection.head.1) {
        Some((a, selection.anchor.1, b, selection.head.1))
    } else {
        Some((b, selection.head.1, a, selection.anchor.1))
    }
}
fn selection_text(parts: &[Part], selection: &DraftSelection) -> Option<String> {
    if let Some((a, start, b, end)) = selection_slice(parts, selection) {
        let mut picked = parts[a..=b].to_vec();
        if let PartKind::Text { text } = &mut picked[b - a].kind {
            *text = text.get(..end)?.into();
        }
        if let PartKind::Text { text } = &mut picked[0].kind {
            *text = text.get(start..)?.into();
        }
        return Some(plain_text(&picked));
    }
    parts.iter().find_map(|p| {
        if let PartKind::Reply { parts, .. } = &p.kind {
            selection_text(parts, selection)
        } else {
            None
        }
    })
}
fn selection_range(parts: &[Part], selection: &DraftSelection, id: &str) -> Option<(usize, usize)> {
    if let Some((a, start, b, end)) = selection_slice(parts, selection) {
        let index = parts.iter().position(|p| p.id == id)?;
        if index < a || index > b {
            return None;
        }
        let PartKind::Text { text } = &parts[index].kind else {
            return None;
        };
        return Some((
            if index == a { start } else { 0 },
            if index == b { end } else { text.len() },
        ));
    }
    parts.iter().find_map(|p| {
        if let PartKind::Reply { parts, .. } = &p.kind {
            selection_range(parts, selection, id)
        } else {
            None
        }
    })
}
fn replace_selection(
    parts: &mut Vec<Part>,
    selection: &DraftSelection,
    text: &str,
) -> Option<(String, usize)> {
    if let Some((a, start, b, end)) = selection_slice(parts, selection) {
        let PartKind::Text { text: before } = &parts[a].kind else {
            return None;
        };
        let PartKind::Text { text: after } = &parts[b].kind else {
            return None;
        };
        let next = format!("{}{text}{}", before.get(..start)?, after.get(end..)?);
        let id = parts[a].id.clone();
        parts.splice(
            a..=b,
            [Part {
                id: id.clone(),
                kind: PartKind::Text { text: next },
            }],
        );
        return Some((id, start + text.len()));
    }
    for part in parts {
        if let PartKind::Reply { parts, .. } = &mut part.kind
            && let Some(next) = replace_selection(parts, selection, text)
        {
            return Some(next);
        }
    }
    None
}
fn adjacent_text(parts: &[Part], id: &str, direction: i32) -> Option<(String, usize)> {
    if let Some(index) = parts.iter().position(|p| p.id == id) {
        let candidates: Box<dyn Iterator<Item = &Part>> = if direction < 0 {
            Box::new(parts[..index].iter().rev())
        } else {
            Box::new(parts[index + 1..].iter())
        };
        return candidates
            .filter_map(|p| {
                if let PartKind::Text { text } = &p.kind {
                    Some((p.id.clone(), if direction < 0 { text.len() } else { 0 }))
                } else {
                    None
                }
            })
            .next();
    }
    parts.iter().find_map(|p| {
        if let PartKind::Reply { parts, .. } = &p.kind {
            adjacent_text(parts, id, direction)
        } else {
            None
        }
    })
}
fn edit_selection(model: Model, controller: &Shared, text: &str, mirror: bool) -> bool {
    let Some(selection) = controller.borrow().selection.clone() else {
        return false;
    };
    let session = model.session.get_untracked();
    let mut parts = buffer::parts(model, &session);
    let before = parts.clone();
    let Some((id, offset)) = replace_selection(&mut parts, &selection, text) else {
        return false;
    };
    remember(controller, before);
    let mut state = controller.borrow_mut();
    state.selection = None;
    state.requested = Some((
        format!("{}-{id}", if mirror { "inline" } else { "draft" }),
        offset,
    ));
    drop(state);
    buffer::edit(model, &session, parts);
    true
}

fn find_next(model: Model, controller: &Shared, direction: i32) {
    let query = model.search.get_untracked();
    if query.is_empty() {
        controller.borrow_mut().search_hit = None;
        return;
    }
    if model.transport_requests.get_untracked().is_some() {
        model.search_history(direction < 0);
        return;
    }
    let mut entries = controller
        .borrow()
        .surfaces
        .iter()
        .filter(|(key, _)| key.starts_with("source-"))
        .map(|(key, (field, editor))| (key.clone(), field.clone(), editor.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.1.layout_rect()
            .origin
            .y
            .total_cmp(&b.1.layout_rect().origin.y)
    });
    let matches = entries
        .into_iter()
        .flat_map(|(key, field, editor)| {
            editor
                .borrow()
                .text()
                .match_indices(&query)
                .map(|(start, _)| (key.clone(), start, field.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return;
    }
    let previous = controller.borrow().search_hit.clone();
    let index = matches
        .iter()
        .position(|(key, start, _)| previous.as_ref() == Some(&(key.clone(), *start)))
        .map(|i| (i as i32 + direction).rem_euclid(matches.len() as i32) as usize)
        .unwrap_or(0);
    let (key, start, field) = &matches[index];
    controller.borrow_mut().search_hit = Some((key.clone(), *start));
    field.reveal();
    field.paint_dirty();
}

fn go_to_source(model: Model, controller: &Shared, anchor: &relay_core::Anchor) {
    controller.borrow_mut().history_anchor = None;
    if model.transport_requests.get_untracked().is_some()
        && (model
            .history
            .get_untracked()
            .preview_ids
            .contains(&anchor.message_id)
            || !model
                .snapshot
                .get_untracked()
                .messages
                .iter()
                .any(|m| m.id == anchor.message_id))
    {
        model.source_anchor.set(Some(anchor.clone()));
        model.request_history(relay_core::StreamRequest::Around {
            session_id: model.session.get_untracked(),
            generation: model.history_generation.get_untracked(),
            message_id: anchor.message_id.clone(),
        });
        return;
    }
    let prefix = format!("source-{}-", anchor.message_id);
    let selected = controller
        .borrow()
        .surfaces
        .iter()
        .filter_map(|(key, (field, editor))| {
            key.strip_prefix(&prefix)
                .and_then(|offset| offset.parse::<usize>().ok())
                .filter(|offset| *offset <= anchor.start)
                .map(|offset| (offset, field.clone(), editor.clone()))
        })
        .max_by_key(|(offset, _, _)| *offset);
    if let Some((offset, field, editor)) = selected {
        let fonts = model.ui.get_untracked().fonts();
        editor.borrow_mut().set_selection_bytes(
            &mut fonts.borrow_mut(),
            anchor.start - offset,
            anchor.end - offset,
        );
        field.focus();
        field.reveal();
        field.paint_dirty();
    }
}

fn delete_boundary(parts: &mut Vec<Part>, id: &str, backward: bool) -> Option<(String, usize)> {
    for index in 0..parts.len() {
        if parts[index].id == id {
            let PartKind::Text { text } = parts[index].kind.clone() else {
                return None;
            };
            if backward && index > 0 {
                let previous = parts[index - 1].clone();
                if let PartKind::Text { text: before } = previous.kind {
                    let offset = before.len();
                    parts[index - 1].kind = PartKind::Text {
                        text: format!("{before}{text}"),
                    };
                    parts.remove(index);
                    return Some((previous.id, offset));
                }
                parts.remove(index - 1);
                return Some((id.into(), 0));
            }
            if !backward && index + 1 < parts.len() {
                let next = parts.remove(index + 1);
                if let PartKind::Text { text: after } = next.kind {
                    parts[index].kind = PartKind::Text {
                        text: format!("{text}{after}"),
                    };
                }
                return Some((id.into(), text.len()));
            }
            return None;
        }
        if let PartKind::Reply { parts, .. } = &mut parts[index].kind
            && let Some(next) = delete_boundary(parts, id, backward)
        {
            return Some(next);
        }
    }
    None
}

#[component]
pub fn Conversation(model: Model) -> Element {
    let controller = State::new(Rc::new(RefCell::new(Controller::default())));
    let empty = State::new(uuid::Uuid::new_v4().to_string());
    let menu = State::new(false);
    let discovery_feedback = Derived::new(move || crate::command_ui::discovery_feedback(model));
    let details = State::new(String::new());
    let session = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .into_iter()
            .find(|s| s.id == model.session.get())
    });
    let session_usage = Derived::new(move || {
        session
            .get()
            .and_then(|s| s.worker)
            .and_then(|w| w.usage.or(w.last_usage))
    });
    let removed = Derived::new(move || {
        let snapshot = model.snapshot.get();
        session
            .get()
            .and_then(|s| s.issue_id)
            .and_then(|id| snapshot.issues.iter().find(|i| i.id == id))
            .is_some_and(|issue| !snapshot.visible_task(&issue.id))
    });
    let review: Rc<dyn Fn(String)> = Rc::new(move |id: String| {
        if id == model.session.get_untracked() {
            model.request_history(relay_core::StreamRequest::Review { session_id: id });
            details.set("changes".into());
        } else {
            model.open_session(id);
        }
    });
    let documents = controller.get_untracked();
    Effect::new(move || {
        let id = model.session.get();
        let mut documents = documents.borrow_mut();
        if documents.session != id {
            documents.session = id;
            documents.undo.clear();
            documents.redo.clear();
            documents.requested = None;
            documents.selection = None;
            documents.history_anchor = None;
            documents.history_scroll_seen = false;
        }
    });
    let completion_controller = controller.get_untracked();
    Effect::new(move || {
        let catalogs = model.catalogs.get();
        let id = model.session.get_untracked();
        if catalogs.get(&id).is_some_and(Result::is_ok) {
            mosaic::core::reactive::untracked(|| {
                let state = completion_controller.borrow();
                if let Some((key, (_, editor))) = state
                    .surfaces
                    .iter()
                    .find(|(_, (field, _))| field.interaction().focused())
                    && let Some(part) = key.strip_prefix("draft-")
                {
                    let editor = editor.borrow();
                    crate::command_ui::complete(model, part, &editor.text(), editor.caret_offset());
                }
            });
        }
    });
    let search_controller = controller.get_untracked();
    Effect::new(move || {
        let _query = model.search.get();
        if model.searching.get() {
            mosaic::core::reactive::untracked(|| find_next(model, &search_controller, 1));
        }
    });
    // Narrow conversations (small windows, large scales) stack the author
    // column above each entry and wrap card actions. Only track templates
    // change, so transcript and draft surfaces are never rebuilt.
    let doc_width = State::new(1200.0f32);
    let doc_height = State::new(900.0f32);
    let viewport_size = State::new(Size::ZERO);
    let resize_controller = controller.get_untracked();
    Effect::new(move || {
        let _size = viewport_size.get();
        mosaic::core::reactive::untracked(|| {
            let mut state = resize_controller.borrow_mut();
            let Some(field) = state
                .surfaces
                .values()
                .find(|(field, _)| field.interaction().focused())
                .map(|(field, _)| field.clone())
            else {
                return;
            };
            // A keyboard resize needs to reveal the current insertion point even
            // when no key was pressed. Wait for paint so wrapping and content
            // extents have settled; do not follow during manual scrolling.
            state.resize_follow = true;
            drop(state);
            field.paint_dirty();
        });
    });
    let recovery = Derived::new(move || {
        model
            .buffer
            .get()
            .documents
            .get(&model.session.get())
            .is_some_and(|d| {
                d.conflict
                    || !d.error.is_empty()
                    || (!d.recovery_reviewed && !d.recovery.is_empty())
            })
    });
    let warning = Derived::new(move || {
        removed.get()
            || session.get().is_some_and(|s| {
                s.worker.as_ref().is_some_and(|w| {
                    w.error.is_some()
                        || (!matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)
                            && ((w.thread_id.is_none() && s.conversations.is_empty())
                                || w.worktree.is_none()))
                })
            })
    });
    // Auxiliary disclosures share a height budget. Even several open panels
    // leave space for the document; Tab reveals controls in their own scroll.
    let auxiliary_limit = Derived::new(move || {
        let count = usize::from(menu.get())
            + usize::from(!details.get().is_empty())
            + usize::from(model.buffer.get().approval_needed)
            + usize::from(recovery.get())
            + usize::from(warning.get())
            + usize::from(!model.command_flow.get().is_empty());
        ((doc_height.get() - px(160.0)).max(0.0) * 0.6 / count.max(1) as f32).min(px(280.0))
    });
    let compact = Derived::new(move || doc_width.get() < px(720.0));
    let draft_extent = State::new(0.0f32);
    let editor = view! {
        col width:fill height:min-content min-height:{px(48.0)}px gap:{px(8.0)}px label:"Next message" {
        if discovery_feedback.get().is_some() {
            col height:min-content gap:{px(4.0)}px
                label:"Command discovery status" {
                text font-size:{px(12.0)}px
                    {discovery_feedback.get().unwrap_or_default()}
                if model.catalogs.get().get(&model.session.get()).is_some_and(|c| c.is_err()) {
                    button #relay.action
                        @click:{crate::command_ui::refresh(model,true);}
                        label:"Retry command discovery" "Retry"
                }
            }
        }
        if model.completion.get().is_some() {
            col height:min-content shrink:0 gap:0px
                stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)})
                label:"Inline suggestions" {
                // Key by content, not position: filtering must replace the displayed row.
                for (key, choice) in {model.completion.get().map(|c|c.choices.into_iter().map(|choice|((choice.label.clone(),choice.description.clone()),choice)).collect::<Vec<_>>()).unwrap_or_default()} {
                    let picked_label=key.0.clone();
                    let choice=State::new(choice.clone());
                    button #relay.action
                        @click:{
                            if let Some(index)=model.completion.get_untracked().and_then(|c|c.choices.iter().position(|c|c.label==picked_label)) {
                                accept_completion(model,&controller.get_untracked(),index,false);
                            }
                        }
                        width:fill min-width:0px height:{px(32.0)}px justify:start
                        pad:(horizontal:{px(12.0)}px vertical:0px)
                        stroke:(width:{px(1.0)} color:ground.rule edges:bottom offset:{px(-1.0)})
                        fill:{color(if model.completion.get().is_some_and(|c|c.choices.get(c.index).is_some_and(|c|c.label==choice.get().label)){ground.selected}else{ground.panel})}
                        focused { stroke:(width:{px(2.0)} color:ground.ink offset:{px(-2.0)}) }
                        label:{choice.get().label} {
                        text width:fill text-wrap:none
                            {format!("{} · {}",choice.get().label,choice.get().description.split_whitespace().collect::<Vec<_>>().join(" "))}
                    } as suggestion
                    { suggestion.clips(true); }
                }
            }
        }
        scroll {
            col height:min-content min-width:0px gap:{px(6.0)}px
                @layout:{move |rect:Rect| draft_extent.set(rect.size.height)} {
                for (_, part) in {
                    let parts=buffer::parts(model,&model.session.get());
                    let parts=if parts.is_empty(){vec![Part{id:empty.get(),kind:PartKind::Text{text:String::new()}}]}else{parts};
                    parts.into_iter().map(|p|(p.id.clone(),p)).collect::<Vec<_>>()
                } {
                    col height:min-content {
                        DraftPart model:(model) controller:(controller)
                            part:(part.clone()) mirror:false
                    }
                }
            }
        } as draft_scroll
        {
            draft_scroll.root().style_dyn(move || Style::stack().width(Dimension::Fill).height(draft_extent.get().min((doc_height.get() - px(200.0)).clamp(px(24.0), px(240.0)))).basis(Dimension::Auto).grow(0.0).shrink(0.0));
            controller.get_untracked().borrow_mut().draft_viewport = Some(draft_scroll.clone());
        }
    }
    };
    let editor_area = editor.clone();
    let has_content =
        Derived::new(move || has_content(&buffer::parts(model, &model.session.get())));
    let hint = Derived::new(move || {
        let state = model.buffer.get();
        if !state.uploads.is_empty() {
            "Uploading inline files…"
        } else if let Some(doc) = state.documents.get(&model.session.get()) {
            if doc.finalize.is_some() || doc.submitting {
                "Sending…"
            } else if doc.saving.is_some() {
                "Saving draft…"
            } else if !state.connected {
                "Draft retained locally"
            } else {
                "Ctrl/Cmd+Enter sends · Enter adds a line"
            }
        } else {
            "Ctrl/Cmd+Enter sends · Enter adds a line"
        }
        .to_string()
    });
    let ledger_open = State::new(true);
    // Narrow conversations start with the ledger folded to its strip.
    Effect::new(move || {
        if compact.get() {
            ledger_open.set(false);
        }
    });
    let ledger = Derived::new(move || ledger_view(model, ledger_open.get()));
    let ledger_actions = LedgerActions {
        change_harness: Rc::new(move || {
            let director = model
                .snapshot
                .get_untracked()
                .sessions
                .iter()
                .find(|s| s.id == model.session.get_untracked())
                .map(|s| s.director_id.clone())
                .unwrap_or_else(|| model.worker_director.get_untracked());
            model.open_profile(EditTarget::Director(director));
        }),
        change_model: Rc::new(move || crate::command_ui::open(model, "model")),
        set_effort: Rc::new(move |step| choose_effort(model, step)),
        toggle: Rc::new(move || ledger_open.set(!ledger_open.get_untracked())),
    };
    let send: relay_ui::prelude::OnPress = Rc::new(move || buffer::send(model));
    let attach: relay_ui::prelude::OnPress = Rc::new(move || {
        if cfg!(target_arch = "wasm32") {
            crate::platform::pick_files(model);
        } else {
            model
                .notice
                .set("Paste or drop files into the message to attach them.".into());
        }
    });
    let tools = Derived::new(move || {
        crate::adapt::fingerprint(&model.snapshot.get().messages, &model.session.get())
    });
    let approvals = Derived::new(move || {
        crate::adapt::transcript_permissions(&model.snapshot.get(), &model.session.get())
    });
    let thinking = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let id = model.session.get();
        let working = snapshot.sessions.iter().any(|s| {
            s.id == id
                && crate::adapt::session_status(&snapshot, s)
                    == relay_ui::prelude::AgentStatus::Working
        });
        let calling = snapshot.messages.iter().any(|m| {
            m.session_id == id
                && m.tool
                    .as_ref()
                    .is_some_and(|t| t.state == ToolState::Running)
        });
        working && !calling
    });
    let thinking_level = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .iter()
            .find(|s| s.id == model.session.get())
            .and_then(|s| crate::adapt::session_model(s, &model.catalogs.get()).1)
            .map_or(1, |effort| effort.level())
    });
    let root = view! {
        col width:1fr gap:0px
            @layout:{move |rect: Rect| { doc_width.set(rect.size.width); doc_height.set(rect.size.height); }} {
            SessionHead model:(model) menu:(menu) compact:(compact)
            if !tools.get().is_empty() {
                FingerprintStrip tools:(relay_ui::Live::from(tools))
            }
            Notice model:(model)
            if !model.command_flow.get().is_empty() {
                BoundedPanel limit:(auxiliary_limit) {
                    CommandPanel model:(model)
                }
            }
            if menu.get() {
                BoundedPanel limit:(auxiliary_limit) {
                    col height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px)
                        gap:{px(10.0)}px shrink:0
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        if compact.get() && session.get().and_then(|s|s.worker).is_some_and(|w|matches!(w.status,WorkerStatus::Running|WorkerStatus::Queued)) {
                            row height:min-content {
                                button #relay.action @click:{model.stop_worker();}
                                    label:"Stop worker"
                                    disabled:{!model.connected.get() || model.busy.get()}
                                    "Stop worker"
                            }
                        }
                        grid height:min-content gap:{px(8.0)}px
                            cols:{GridTracks::auto_fit(GridTrack::minmax(px(90.0).into(), GridTrack::fr(1.0)))} {
                            if cfg!(target_arch = "wasm32") {
                                button #relay.action @click:{crate::platform::pick_files(model);}
                                    label:"Add files to draft" "Add files"
                            }
                            button #relay.action @click:{model.open_worker_issue();}
                                disabled:{session.get().is_none()} label:"Linked issue" "Issue"
                            button #relay.action
                                @click:{model.open_profile(EditTarget::Director(session.get_untracked().map(|s|s.director_id).unwrap_or_else(||model.worker_director.get_untracked())));}
                                label:"Director profile" "Profile"
                            button #relay.action
                                @click:{if details.get_untracked()!="changes" {model.request_history(relay_core::StreamRequest::Review{session_id:model.session.get_untracked()});}details.set(if details.get_untracked()=="changes" {String::new()}else{"changes".into()});}
                                disabled:{session.get().is_none()} label:"Toggle change review"
                                "Changes"
                            button #relay.action
                                @click:{details.set(if details.get_untracked()=="usage" {String::new()}else{"usage".into()});}
                                disabled:{session.get().is_none()}
                                label:"Session usage and provenance" "Details"
                            button #relay.action @click:{model.searching.set(true);menu.set(false);}
                                disabled:{session.get().is_none()} label:"Search transcript" "Find"
                        }
                        if session.get().is_some_and(|s| !s.fixture && s.worker.is_some()) {
                            let current = session.get_untracked().unwrap();
                            let session_id = current.id.clone();
                            let snapshot = model.snapshot.get();
                            let inherited = snapshot.directors.iter().find(|d| d.id == current.director_id).and_then(|d|snapshot.effective_profile(d).ok()).map(|p|p.execution.approval).unwrap_or_default();
                            let worker = current.worker.unwrap();
                            text font-size:{px(12.0)}px font-color:{color(ground.muted)}
                                {format!("{} · {} · {}",match worker.harness{Harness::Codex=>"Codex",Harness::ClaudeCode=>"Claude Code"},worker.execution.as_ref().map(|e|e.approval).unwrap_or(inherited).label(),if worker.execution.is_some(){"Worker override"}else{"Inherited from director"})}
                            grid height:min-content gap:{px(8.0)}px align:center
                                cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0),GridTrack::MaxContent])}} {
                                let mode_session = session_id.clone();
                                let mode_index = Derived::new(move || {
                                    let snapshot = model.snapshot.get();
                                    let session = snapshot.sessions.iter().find(|s| s.id == mode_session);
                                    let inherited = session.and_then(|s| snapshot.directors.iter().find(|d| d.id == s.director_id)).and_then(|d| snapshot.effective_profile(d).ok()).map(|p| p.execution.approval).unwrap_or_default();
                                    let mode = session.and_then(|s| s.worker.as_ref()).and_then(|w| w.execution.as_ref()).map(|e| e.approval).unwrap_or(inherited);
                                    ApprovalMode::ALL.iter().position(|m| *m == mode).unwrap_or(0)
                                });
                                let choose_session = session_id.clone();
                                let choose: Rc<dyn Fn(usize)> = Rc::new(move |slot: usize| {
                                    model.submit(Command::SetWorkerExecution{session_id:choose_session.clone(),execution:Some(ExecutionSettings{approval:ApprovalMode::ALL[slot]})},model.snapshot.get_untracked().revision,crate::model::Saved::Action);
                                });
                                Segments name:("Worker approval".to_string())
                                    options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                                    selected:(relay_ui::Live::from(mode_index)) on-select:(choose)
                                    disabled:(relay_ui::Live::from(Derived::new(move || !model.connected.get() || model.busy.get())))
                                button #relay.action
                                    @click:{model.submit(Command::SetWorkerExecution{session_id:session_id.clone(),execution:None},model.snapshot.get_untracked().revision,crate::model::Saved::Action);}
                                    disabled:{!model.connected.get() || model.busy.get()}
                                    label:"Inherit worker execution settings" "Inherit"
                            }
                            text font-size:{px(11.0)}px font-color:ground.muted
                                "Execution changes apply to the next turn."
                        }
                    }
                }
            }
            if warning.get() {
                BoundedPanel limit:(auxiliary_limit) {
                    if removed.get() {
                        row height:min-content pad:(horizontal:{px(24.0)}px vertical:0px) shrink:0 {
                            text label:"Session issue removed from board" font-size:{px(12.0)}px
                                font-color:ground.muted
                                "Issue no longer on this board. New turns require restoration and sync."
                        }
                    }
                    if session.get().is_some_and(|s|s.worker.as_ref().is_some_and(|w|!matches!(w.status,WorkerStatus::Queued|WorkerStatus::Running) && ((w.thread_id.is_none() && s.conversations.is_empty()) || w.worktree.is_none()))) {
                        grid height:min-content gap:{px(8.0)}px
                            cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0), GridTrack::MaxContent])}}
                            pad:(horizontal:{px(24.0)}px vertical:0px) shrink:0 {
                            text width:1fr label:"Worker cannot continue" font-size:{px(12.0)}px
                                {session.get().and_then(|s|s.worker).and_then(|w|w.error).unwrap_or_else(||"No resumable thread was recorded".into())}
                            button #relay.action @click:{model.open_worker_issue();}
                                label:"Start new worker from linked issue" "Open issue"
                        }
                    } else {
                        col height:min-content shrink:0 {
                            if session.get().and_then(|s|s.worker).is_some_and(|w|w.error.is_some()) {
                                row height:min-content pad:(horizontal:{px(24.0)}px vertical:0px) {
                                    text font-size:{px(12.0)}px font-color:{color(status.failed)}
                                        {session.get().and_then(|s|s.worker).and_then(|w|w.error).unwrap_or_default()}
                                }
                            }
                        }
                    }
                }
            }
            if !details.get().is_empty() {
                BoundedPanel limit:(auxiliary_limit) {
                    col height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(10.0)}px)
                        gap:{px(12.0)}px selectable label:"Session details" {
                        if details.get() == "usage" {
                            text font-size:{px(12.0)}px font-color:ground.muted
                                "Cache status is unknown: the harness does not report whether the provider still retains this conversation. Last prompt tokens show the most recent measured input, not a prediction for the next message. Re-caching may include replies, tool output, and new input; compaction may reduce it."
                            if session_usage.get().is_some() {
                                UsageReadout usage:(session_usage)
                            } else {
                                text font-size:{px(12.0)}px font-color:ground.muted
                                    "Usage unavailable for the latest turn"
                            }
                        }
                        for (_, group) in {session.get().map(|s| provenance(&model.snapshot.get(), &s, details.get() == "changes")).unwrap_or_default().into_iter().map(|g| (g.key.clone(), g)).collect::<Vec<_>>()} {
                            let key = group.key.clone();
                            let fallback = group.clone();
                            let block = Derived::new(move || session.get().map(|s| provenance(&model.snapshot.get(), &s, details.get() == "changes")).unwrap_or_default().into_iter().find(|g| g.key == key).unwrap_or_else(|| fallback.clone()));
                            col height:min-content gap:{px(8.0)}px pad:(top:{px(10.0)}px)
                                stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                                row #relay.title height:min-content font-size:{px(13.0)}px {
                                    text {block.get().title}
                                }
                                grid
                                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(160.0).into(), GridTrack::fr(1.0)))}
                                    height:min-content gap:{px(10.0)}px {
                                    for (_, field) in {block.get().rows.into_iter().map(|r| (r.0, r.0)).collect::<Vec<_>>()} {
                                        let name: &'static str = field;
                                        Readout key:(name.to_string())
                                            value:(Derived::new(move || block.get().rows.into_iter().find(|r| r.0 == name).map(|r| r.1).unwrap_or_default()))
                                    }
                                }
                                if block.get().changes.is_some() {
                                    row height:min-content font-size:{px(12.0)}px {
                                        text {block.get().changes.unwrap_or_default()}
                                    }
                                }
                            }
                        }
                        if session.get().is_some_and(|s| s.workspaces.is_empty() && s.worker.is_none()) {
                            text font-size:{px(12.0)}px font-color:ground.muted
                                "No execution metadata"
                        }
                    }
                }
            }
            if model.searching.get() {
                row height:min-content pad:(horizontal:{px(24.0)}px vertical:0px) shrink:0 {
                    input #relay.field placeholder:"Find in conversation…" model.search as search
                    {search.focus();}
                    button #relay.tree-control
                        @click:{find_next(model,&controller.get_untracked(),-1);}
                        label:"Previous search match" "↑"
                    button #relay.tree-control
                        @click:{find_next(model,&controller.get_untracked(),1);}
                        label:"Next search match" "↓"
                    button #relay.tree-control
                        @click:{model.searching.set(false);model.search.set(String::new());}
                        label:"Close search" "×"
                }
            }
            scroll {
                row height:min-content justify:center
                    pad:(left:{px(if compact.get() {12.0} else {28.0})}px right:{px(if compact.get() {12.0} else {36.0})}px top:{px(28.0)}px bottom:{px(24.0)}px) {
                    col width:1fr max-width:{px(848.0)}px height:min-content gap:{px(28.0)}px
                        label:"Transcript" {
                        if model.searching.get() && !model.search.get().is_empty() && !model.search_snippet.get().is_empty() {
                            row height:min-content {
                                text font-size:{px(12.0)}px font-color:ground.muted
                                    {model.search_snippet.get()}
                            }
                        }
                        if model.transport_requests.get().is_some() {
                            row height:{px(22.0)}px align:center {
                                text font-size:{px(12.0)}px font-color:ground.muted
                                    {if !model.history_error.get().is_empty() {model.history_error.get()} else if model.history_loading.get() {"Loading conversation…".into()} else {String::new()}}
                                if !model.history_error.get().is_empty() {
                                    button #relay.action @click:{model.retry_history();}
                                        label:"Retry loading history" "Retry"
                                }
                            }
                        }
                        for (_, message) in {model.snapshot.get().messages.into_iter().filter(|m|m.session_id==model.session.get()).map(|m|(m.id.clone(),m)).collect::<Vec<_>>()} {
                            col height:min-content {
                                TranscriptMessage model:(model) message-id:(message.id.clone())
                                    controller:(controller)
                            }
                        }
                        if thinking.get() {
                            ThinkingLine effort:(relay_ui::Live::from(thinking_level))
                        }
                        for (_, permission) in {approvals.get().into_iter().map(|p|(p.id.clone(),p)).collect::<Vec<_>>()} {
                            col height:min-content {
                                PendingApproval model:(model) permission:(permission.clone())
                                    on-review:(review.clone())
                            }
                        }
                        for (_, queued) in {model.snapshot.get().submissions.into_iter().filter(|s|s.session_id==model.session.get() && matches!(s.state,SubmissionState::Queued|SubmissionState::Paused)).map(|s|(s.id.clone(),s)).collect::<Vec<_>>()} {
                            col height:min-content {
                                QueuedTurn model:(model) submission:(queued.id.clone())
                            }
                        }
                        if model.snapshot.get().submissions.iter().any(|s|s.session_id==model.session.get() && s.state==SubmissionState::Paused) && session.get().and_then(|s|s.worker).is_some_and(|w|!matches!(w.status,WorkerStatus::Queued|WorkerStatus::Running)) {
                            row height:min-content pad:(left:{px(128.0)}px) {
                                button #relay.action
                                    @click:{model.submit(Command::ResumeQueue{session_id:model.session.get_untracked()},model.snapshot.get_untracked().revision,crate::model::Saved::Action);}
                                    label:"Resume paused queue" "Resume queue"
                            }
                        }
                    }
                }
            } as document_scroll
            {
                document_scroll.root().on_layout(move |rect| viewport_size.set(rect.size));
                controller.get_untracked().borrow_mut().viewport=Some(document_scroll.clone());
                let gesture_controller=controller.get_untracked();
                document_scroll.root().on_pointer(move |event,_| {
                    if matches!(event.kind,PointerEventKind::Wheel{..}|PointerEventKind::Down(_)) || (event.pointer_type==PointerType::Touch && matches!(event.kind,PointerEventKind::Move)) {let mut state=gesture_controller.borrow_mut();state.history_scroll_seen=true;state.history_anchor=None;}
                });
                let paging_scroll=document_scroll.clone();
                let paging_controller=controller.get_untracked();
                Effect::new(move || {
                    let offset=paging_scroll.offset();
                    mosaic::core::reactive::untracked(|| {
                        if !paging_controller.borrow().history_scroll_seen || model.transport_requests.get_untracked().is_none() || model.history_loading.get_untracked() {return;}
                        let root=paging_scroll.root().layout_rect();
                        if offset.y<px(200.0) {
                            if !model.history.get_untracked().has_more {return;}
                            let ui=model.ui.get_untracked();
                            let anchor=paging_controller.borrow().messages.iter().filter_map(|(key,field)|ui.inspection_rect(field.id()).map(|rect|(key.clone(),rect))).filter(|(_,rect)|rect.origin.y+rect.size.height>=root.origin.y).min_by(|(_,a),(_,b)|a.origin.y.total_cmp(&b.origin.y)).map(|(key,rect)|(key,rect.origin.y));
                            if model.load_older() {paging_controller.borrow_mut().history_anchor=anchor;}
                        } else if offset.y+root.size.height>=paging_scroll.content().layout_rect().size.height-px(200.0) {
                            model.load_newer();
                        }
                    });
                });
                let layout_controller=controller.get_untracked();
                let follow_scroll = document_scroll.clone();
                let previous = Rc::new(Cell::new(None::<(f32, f32)>));
                document_scroll.content().on_layout(move |rect| {
                    let height = follow_scroll.root().layout_rect().size.height;
                    let was_at_bottom = previous.get().is_none_or(|(content, viewport)|
                        follow_scroll.offset().y >= (content - viewport).max(0.0) - px(4.0));
                    previous.set(Some((rect.size.height, height)));
                    let anchor=if !model.history_loading.get_untracked() {layout_controller.borrow().history_anchor.clone()} else {None};
                    if let Some((key,y))=anchor {
                        if let Some(field)=layout_controller.borrow().messages.get(&key) && let Some(rect)=model.ui.get_untracked().inspection_rect(field.id()) {
                            follow_scroll.scroll_to(Vector2::new(0.0,follow_scroll.offset().y+rect.origin.y-y));
                        }
                    } else if was_at_bottom && !model.history.get_untracked().has_newer {
                        follow_scroll.scroll_to(Vector2::new(0.0, (rect.size.height - height).max(0.0)));
                    }
                    if let Some(anchor)=model.source_anchor.get_untracked()
                        && !model.history.get_untracked().preview_ids.contains(&anchor.message_id)
                        && model.snapshot.get_untracked().messages.iter().any(|m|m.id==anchor.message_id)
                    {
                        go_to_source(model,&layout_controller,&anchor);
                        model.source_anchor.set(None);
                    } else if let Some(id)=model.remote_search.get_untracked() {
                        let target=layout_controller.borrow().messages.get(&id).filter(|field|model.ui.get_untracked().inspection_rect(field.id()).is_some()).cloned();
                        if let Some(field)=target && model.focused_message.get_untracked()==id {layout_controller.borrow_mut().history_anchor=None;field.reveal();model.focused_message.set(String::new());}
                    }
                });
            }
            if model.buffer.get().approval_needed {
                BoundedPanel limit:(auxiliary_limit) {
                    grid height:min-content gap:{px(10.0)}px align:center
                        pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px)
                        cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0), GridTrack::MaxContent])}} {
                        row #relay.caption height:min-content min-width:0px {
                            text "This director requires approval to implement this turn."
                        }
                        button #relay.action
                            @click:{model.worker_approval.set(true);buffer::send(model);}
                            label:"Approve this turn and send" "Approve and send"
                    }
                }
            }
            if recovery.get() {
                BoundedPanel limit:(auxiliary_limit) {
                    col height:min-content gap:{px(8.0)}px
                        pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) {
                        row #relay.caption height:min-content {
                            text
                                {model.buffer.get().documents.get(&model.session.get()).map(|d| if d.error.is_empty() {"Recovered draft available".into()} else {d.error.clone()}).unwrap_or_default()}
                        }
                        grid height:min-content gap:{px(8.0)}px
                            cols:{GridTracks::auto_fit(GridTrack::minmax(px(90.0).into(), GridTrack::fr(1.0)))} {
                            button #relay.action @click:{buffer::resolve(model,false);}
                                label:"Load shared draft" "Load shared"
                            button #relay.action @click:{buffer::resolve(model,true);}
                                label:"Restore local draft" "Restore local"
                            button #relay.action @click:{buffer::retry_save(model);}
                                label:"Retry draft save" "Retry"
                        }
                    }
                }
            }
            row height:min-content shrink:0
                pad:(left:{px(if compact.get() {0.0} else {28.0})}px right:{px(if compact.get() {0.0} else {36.0})}px bottom:{px(if compact.get() {0.0} else {20.0})}px)
                justify:center {
                col width:1fr max-width:{px(848.0)}px height:min-content {
                    Composer settings:(relay_ui::Live::from(ledger)) ledger-actions:(ledger_actions)
                        on-send:(send) on-attach:(attach) editor:(Some(editor))
                        has-content:(Some(relay_ui::Live::from(has_content)))
                        hint:(relay_ui::Live::from(hint))
                }
            }
        }
    };
    let cleanup_controller = controller.get_untracked();
    mosaic::core::reactive::on_cleanup(move || {
        cleanup_controller.borrow_mut().viewport = None;
    });
    let keys_controller = controller.get_untracked();
    root.on_key(move |_, _| {
        keys_controller.borrow_mut().history_anchor = None;
    });
    let drag_controller = controller.get_untracked();
    root.on_pointer(move |event, _| match event.kind {
        PointerEventKind::Wheel { .. } => {
            drag_controller.borrow_mut().history_scroll_seen = true;
        }
        PointerEventKind::Down(PointerButton::Primary) => {
            drag_controller.borrow_mut().history_scroll_seen = true;
            let state = drag_controller.borrow();
            // A press in the composer's message area below the draft text,
            // down to its key hint, continues the draft at its end.
            let area = editor_area.layout_rect();
            if event.position.x < area.origin.x - px(16.0)
                || event.position.x > area.origin.x + area.size.width
                || event.position.y < area.origin.y
                || event.position.y > area.origin.y + area.size.height + px(36.0)
            {
                return;
            }
            let target = state
                .surfaces
                .iter()
                .filter(|(key, _)| key.starts_with("draft-"))
                .max_by(|(_, (a, _)), (_, (b, _))| {
                    a.layout_rect()
                        .origin
                        .y
                        .total_cmp(&b.layout_rect().origin.y)
                })
                .map(|(_, (field, editor))| (field.clone(), editor.clone()));
            drop(state);
            if let Some((field, editor)) = target
                && event.position.y
                    >= field.layout_rect().origin.y + field.layout_rect().size.height
            {
                drag_controller.borrow_mut().selection = None;
                let fonts = model.ui.get_untracked().fonts();
                let end = editor.borrow().text().len();
                editor
                    .borrow_mut()
                    .set_selection_bytes(&mut fonts.borrow_mut(), end, end);
                field.focus();
                field.reveal();
                field.paint_dirty();
            }
        }
        PointerEventKind::Move => {
            let Some((anchor, offset, mirror)) = drag_controller.borrow().pointer_anchor.clone()
            else {
                return;
            };
            let prefix = if mirror { "inline-" } else { "draft-" };
            let target = drag_controller
                .borrow()
                .surfaces
                .iter()
                .find(|(key, (field, _))| {
                    key.starts_with(prefix) && field.layout_rect().contains(event.position)
                })
                .map(|(key, (field, editor))| (key.clone(), field.clone(), editor.clone()));
            if let Some((key, field, editor)) = target {
                let font_context = model.ui.get_untracked().fonts();
                editor.borrow_mut().caret_to(
                    &mut font_context.borrow_mut(),
                    event.position - field.layout_rect().origin,
                    false,
                );
                let head = (
                    key.strip_prefix(prefix).unwrap().to_owned(),
                    editor.borrow().caret_offset(),
                );
                if head.0 != anchor {
                    let selection = DraftSelection {
                        anchor: (anchor, offset),
                        head,
                    };
                    if selection_text(
                        &buffer::parts(model, &model.session.get_untracked()),
                        &selection,
                    )
                    .is_some()
                    {
                        drag_controller.borrow_mut().selection = Some(selection);
                        for (field, _) in drag_controller.borrow().surfaces.values() {
                            field.paint_dirty();
                        }
                    }
                }
            }
        }
        PointerEventKind::Up(_) | PointerEventKind::Cancel => {
            drag_controller.borrow_mut().pointer_anchor = None
        }
        _ => {}
    });
    root
}

#[component]
fn TranscriptMessage(model: Model, message_id: String, controller: ControllerState) -> Element {
    let message_id = State::new(message_id);
    let message = Derived::new(move || {
        model
            .snapshot
            .get()
            .messages
            .into_iter()
            .find(|m| m.id == message_id.get())
            .map(crate::tool_activity::with_legacy_tool)
    });
    let expanded = State::new(false);
    let initial = message.get_untracked();
    let detail = initial.as_ref().is_some_and(|m| {
        matches!(
            m.kind.as_str(),
            "issue-context" | "command_execution" | "file_change" | "reasoning"
        )
    });
    let prompt = initial.as_ref().is_some_and(|m| m.kind == "prompt");
    let tool = initial.as_ref().is_some_and(|m| m.tool.is_some());
    fn speaker_of(message: &Message) -> String {
        if message.kind == "prompt" {
            "You".to_string()
        } else {
            message.author.clone()
        }
    }
    // A run of messages from one speaker names it once, at the first.
    let continues = move || {
        let snapshot = model.snapshot.get();
        let id = message_id.get();
        let Some(index) = snapshot.messages.iter().position(|m| m.id == id) else {
            return false;
        };
        let current = &snapshot.messages[index];
        snapshot.messages[..index]
            .iter()
            .rev()
            .find(|m| m.session_id == current.session_id)
            .is_some_and(|previous| speaker_of(previous) == speaker_of(current))
    };
    let speaker = Derived::new(move || {
        if continues() {
            return String::new();
        }
        message.get().map(|m| speaker_of(&m)).unwrap_or_default()
    });
    // Agent messages carry their session's harness glyph when it is known.
    let harness = (!prompt && !mosaic::core::reactive::untracked(continues))
        .then(|| {
            let snapshot = model.snapshot.get_untracked();
            let session_id = initial.as_ref().map(|m| m.session_id.clone())?;
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.worker.as_ref())
                .map(|w| crate::adapt::harness(w.harness))
        })
        .flatten();
    let body = view! {
        col height:min-content min-width:0px gap:0px {
            if model.history.get().preview_ids.contains(&message_id.get()) {
                row height:min-content align:center gap:{px(8.0)}px {
                    text font-size:{px(12.0)}px font-color:ground.muted "Message preview"
                    button #relay.action @click:{model.load_message(message_id.get_untracked());}
                        label:"Load full message" "Load full message"
                }
            }
            if detail && message.get().is_none_or(|m|m.tool.is_none()) {
                col height:min-content {
                    row height:min-content pad:(bottom:{px(8.0)}px) {
                        button #relay.action @click:{expanded.set(!expanded.get_untracked());}
                            pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                            label:"Toggle tool activity"
                            {message.get().map(|m|match m.kind.as_str(){"issue-context"=>"Source context","file_change"=>"File changes","reasoning"=>"Reasoning",_=>"Tool activity"}.to_owned()).unwrap_or_default()}
                    }
                }
            }
            if message.get().is_none_or(|m|m.tool.is_none()) && (!detail || expanded.get()) {
                col height:min-content {
                    if message.get().is_some_and(|m|m.parts.is_empty()) {
                        col height:min-content {
                            BufferText model:(model) controller:(controller)
                                location:(Location::Source {message:message_id.get(),start:0,end:None})
                                mirror:false
                        }
                    } else {
                        col height:min-content gap:{px(8.0)}px {
                            for (_, entry) in {message.get().map(|m|located_parts(&m.parts,0)).unwrap_or_default()} {
                                col height:min-content {
                                    RecordedPart model:(model) controller:(controller)
                                        message-id:(message_id.get()) part:(entry.0.clone())
                                        offset:(entry.1)
                                }
                            }
                        }
                    }
                }
            }
            for (_, reply) in {buffer::parts(model,&model.session.get()).into_iter().filter(|p|matches!(&p.kind,PartKind::Reply {anchor,..} if anchor.message_id==message_id.get())).map(|p|(p.id.clone(),p)).collect::<Vec<_>>()} {
                col height:min-content pad:(top:{px(8.0)}px) {
                    DraftPart model:(model) controller:(controller) part:(reply.clone()) mirror:true
                }
            }
            for (_, comment) in {model.snapshot.get().comments.into_iter().filter(|c|c.message_id==message_id.get()).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                col height:min-content pad:(left:{px(14.0)}px top:{px(8.0)}px) gap:{px(5.0)}px
                    selectable stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                    row #relay.caption height:min-content {
                        text (comment.author.clone())
                    }
                    row height:min-content font-size:{px(14.0)}px {
                        text (comment.body.clone())
                    }
                }
            }
        }
    };
    let row = if tool {
        view! {
            MessageRow speaker:(relay_ui::Live::from(speaker)) harness:(harness) {
                ToolMessage model:(model) message:(message)
                { parent.adopt(&body); }
            }
        }
    } else if prompt {
        view! {
            MessageRow speaker:(relay_ui::Live::from(speaker)) {
                UserMessage {
                    { parent.adopt(&body); }
                }
            }
        }
    } else {
        view! {
            MessageRow speaker:(relay_ui::Live::from(speaker)) harness:(harness) {
                AgentText {
                    { parent.adopt(&body); }
                }
            }
        }
    };
    let root = view! {
        col height:min-content min-width:0px {
            { parent.adopt(&row); }
        }
    };
    let shared = controller.get_untracked();
    let id = message_id.get_untracked();
    shared
        .borrow_mut()
        .messages
        .insert(id.clone(), root.clone());
    mosaic::core::reactive::on_cleanup(move || {
        shared.borrow_mut().messages.remove(&id);
    });
    root
}

fn located_parts(parts: &[Part], start: usize) -> Vec<(String, (Part, usize))> {
    let mut offset = start;
    parts
        .iter()
        .map(|part| {
            let entry = (part.id.clone(), (part.clone(), offset));
            offset += plain_text(std::slice::from_ref(part)).len();
            entry
        })
        .collect()
}

#[component]
fn RecordedPart(
    model: Model,
    controller: ControllerState,
    message_id: String,
    part: Part,
    offset: usize,
) -> Element {
    match part.kind {
        PartKind::Text { text } => {
            view! {
                col height:min-content {
                    BufferText model:(model) controller:(controller)
                        location:(Location::Source {message:message_id.clone(),start:offset,end:Some(offset+text.len())})
                        mirror:false
                }
            }
        }
        PartKind::Skill { skill } => {
            view! {
                row height:min-content {
                    text font-color:{color(ground.ink)} {format!("${}",skill.name)}
                }
            }
        }
        PartKind::Asset { asset } => {
            view! {
                col height:min-content {
                    InlineAsset model:(model) asset:(asset.clone()) part-id:(None) controller:(None)
                        mirror:false
                }
            }
        }
        PartKind::Reply { anchor, parts } => {
            let message_id = State::new(message_id);
            let parts = State::new(parts);
            let quote = State::new(anchor.quote.clone());
            let prefix = format!("Reply to “{}”:\n", anchor.quote).len();
            view! {
                col height:min-content gap:{px(6.0)}px pad:(left:{px(14.0)}px)
                    stroke:(width:{px(2.0)} color:ground.hairline edges:left) {
                    text font-size:{px(12.0)}px font-color:{color(ground.muted)}
                        {format!("↩ {}",quote.get())}
                    for (_, entry) in {located_parts(&parts.get(),offset+prefix)} {
                        col height:min-content {
                            RecordedPart model:(model) controller:(controller)
                                message-id:(message_id.get()) part:(entry.0.clone())
                                offset:(entry.1)
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DraftPart(model: Model, controller: ControllerState, part: Part, mirror: bool) -> Element {
    match part.kind {
        PartKind::Text { .. } => {
            view! {
                col height:min-content {
                    BufferText model:(model) controller:(controller)
                        location:(Location::Draft(part.id.clone())) mirror:(mirror)
                }
            }
        }
        PartKind::Skill { skill } => {
            let id = State::new(part.id.clone());
            view! {
                row height:min-content align:center gap:{px(6.0)}px {
                    text font-color:{color(ground.ink)} {format!("${}",skill.name)}
                    button #relay.action
                        @click:{remove_draft_part(model,&controller.get_untracked(),&id.get_untracked(),mirror);}
                        label:"Remove skill" "×"
                }
            }
        }
        PartKind::Asset { asset } => {
            view! {
                col height:min-content {
                    InlineAsset model:(model) asset:(asset.clone()) part-id:(Some(part.id.clone()))
                        controller:(Some(controller)) mirror:(mirror)
                }
            }
        }
        PartKind::Reply { anchor, parts } => {
            let anchor = State::new(anchor);
            let original = State::new(parts);
            let source_controller = controller;
            let remove = State::new(part.id.clone());
            let parts = Derived::new(move || {
                buffer::parts(model, &model.session.get())
                    .into_iter()
                    .find(|p| p.id == remove.get())
                    .and_then(|p| {
                        if let PartKind::Reply { parts, .. } = p.kind {
                            Some(parts)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| original.get())
            });
            view! {
                col height:min-content gap:{px(6.0)}px pad:(left:{px(14.0)}px)
                    stroke:(width:{px(2.0)} color:ground.rule edges:left) {
                    row height:min-content gap:{px(8.0)}px {
                        button #relay.tree-control
                            @click:{go_to_source(model,&source_controller.get_untracked(),&anchor.get_untracked());}
                            label:"Go to reply source"
                            {format!("↩ {}",anchor.get().quote.chars().take(80).collect::<String>())}
                        button #relay.tree-control
                            @click:{remove_draft_part(model,&source_controller.get_untracked(),&remove.get_untracked(),false);}
                            label:"Remove draft reply" "×"
                    }
                    for (_, nested) in {parts.get().into_iter().map(|p|(p.id.clone(),p)).collect::<Vec<_>>()} {
                        col height:min-content {
                            DraftPart model:(model) controller:(controller) part:(nested.clone())
                                mirror:(mirror)
                        }
                    }
                }
            }
        }
    }
}

fn remove_draft_part(model: Model, controller: &Shared, id: &str, mirror: bool) {
    let session = model.session.get_untracked();
    let mut parts = buffer::parts(model, &session);
    remember(controller, parts.clone());
    let target = adjacent_text(&parts, id, 1).or_else(|| adjacent_text(&parts, id, -1));
    remove_part(&mut parts, id);
    if let Some((id, offset)) = target {
        controller.borrow_mut().requested = Some((
            format!("{}-{id}", if mirror { "inline" } else { "draft" }),
            offset,
        ));
    }
    controller.borrow_mut().selection = None;
    buffer::edit(model, &session, parts);
}

#[component]
fn InlineAsset(
    model: Model,
    asset: Asset,
    part_id: Option<String>,
    controller: Option<ControllerState>,
    mirror: bool,
) -> Element {
    buffer::fetch(model, &asset);
    let preview = State::new(asset.media_type.starts_with("image/"));
    let asset = State::new(asset);
    let part_id = State::new(part_id);
    let root = view! {
        col height:min-content gap:{px(6.0)}px {
            row height:min-content gap:{px(8.0)}px align:center {
                button #relay.tree-control
                    @click:{buffer::fetch(model,&asset.get_untracked());preview.set(!preview.get_untracked());}
                    label:(format!("Preview {}",asset.get().name))
                    {format!("{} · {} KiB",asset.get().name,asset.get().size.div_ceil(1024))}
                if part_id.get().is_some() {
                    button #relay.tree-control
                        @click:{if let Some(controller)=controller {remove_draft_part(model,&controller.get_untracked(),part_id.get_untracked().as_ref().unwrap(),mirror);}}
                        label:"Remove inline file" "×"
                }
            }
            if preview.get() {
                col height:min-content {
                    if model.buffer.get().blobs.contains_key(&asset.get().id) {
                        col height:min-content {
                            if asset.get().media_type.starts_with("image/") {
                                img width:fill max-height:{px(360.0)}px fit:contain
                                    {ImageSource::encoded(model.buffer.get().blobs.get(&asset.get().id).unwrap().as_ref())}
                            } else {
                                text font-size:{px(12.0)}px
                                    {String::from_utf8(model.buffer.get().blobs.get(&asset.get().id).unwrap().iter().take(8192).copied().collect()).unwrap_or_else(|_|"Binary file · available to the agent at this position".into())}
                            }
                        }
                    } else {
                        text font-color:{color(ground.muted)} font-size:{px(12.0)}px
                            {model.buffer.get().fetch_errors.get(&asset.get().id).cloned().unwrap_or_else(||"Loading file…".into())}
                    }
                }
            }
        }
    };
    root.on_key(move |event, ctx| {
        if matches!(event.kind, KeyEventKind::Down { .. })
            && matches!(event.key, Key::Delete | Key::Backspace)
            && let Some(id) = part_id.get_untracked()
            && let Some(controller) = controller
        {
            remove_draft_part(model, &controller.get_untracked(), &id, mirror);
            ctx.stop_propagation();
        }
    });
    root
}

fn remove_part(parts: &mut Vec<Part>, id: &str) {
    parts.retain(|p| p.id != id);
    for part in parts {
        if let PartKind::Reply { parts, .. } = &mut part.kind {
            remove_part(parts, id);
        }
    }
}

fn caret_with_margin(origin: Vector2, caret: Rect) -> Rect {
    let margin = px(8.0);
    Rect::from_xywh(
        origin.x + caret.origin.x,
        origin.y + caret.origin.y - margin,
        caret.size.width,
        caret.size.height + margin * 2.0,
    )
}

#[component]
pub(crate) fn BufferText(
    model: Model,
    controller: ControllerState,
    location: Location,
    mirror: bool,
) -> Element {
    let controller = controller.get_untracked();
    let field = view! {
        el width:fill height:min-content shrink:0 font-size:{px(13.0)}px {}
    };
    let key = match &location {
        Location::Source { message, start, .. } => format!("source-{message}-{start}"),
        Location::Draft(id) => format!("{}-{id}", if mirror { "inline" } else { "draft" }),
    };
    field.focusable(true).accepts_text(true).selectable(false);
    field.label(match &location {
        Location::Source { message, .. } => format!("Message {message}"),
        Location::Draft(id) => format!("Draft text {id}"),
    });
    field.role(mosaic::widgets::a11y::Role::TextInput);
    let fonts = model.ui.get_untracked().fonts();
    let editor = Rc::new(RefCell::new(EditBuffer::new(TextStyle::new(px(13.0)))));
    editor
        .borrow_mut()
        .set_text(&mut fonts.borrow_mut(), &current_text(model, &location));
    controller
        .borrow_mut()
        .surfaces
        .insert(key.clone(), (field.clone(), editor.clone()));
    let cleanup = controller.clone();
    let cleanup_key = key.clone();
    mosaic::core::reactive::on_cleanup(move || {
        cleanup.borrow_mut().surfaces.remove(&cleanup_key);
    });
    let repaint = field.clone();
    let observed = editor.clone();
    let observe_fonts = fonts.clone();
    let observe_location = location.clone();
    let focus_controller = controller.clone();
    let focus_key = key.clone();
    let follow = Rc::new(Cell::new(false));
    let focus_follow = follow.clone();
    field.style_dyn(move || {
        let text = current_text(model, &observe_location);
        let mut editor = observed.borrow_mut();
        if editor.text() != text {
            let selection = editor.selection_bytes();
            editor.set_text(&mut observe_fonts.borrow_mut(), &text);
            editor.set_selection_bytes(
                &mut observe_fonts.borrow_mut(),
                selection.0.min(text.len()),
                selection.1.min(text.len()),
            );
        }
        let query = if model.searching.get() {
            model.search.get()
        } else {
            String::new()
        };
        let spans = if query.is_empty() || !matches!(observe_location, Location::Source { .. }) {
            vec![]
        } else {
            text.match_indices(&query)
                .map(|(start, _)| {
                    mosaic::text::ColorSpan::new(
                        start..start + query.len(),
                        mosaic::core::theme::color(status.needs_you),
                    )
                })
                .collect()
        };
        editor.set_color_spans(&mut observe_fonts.borrow_mut(), spans);
        let staged = focus_controller.borrow().preedit.clone();
        if let Some((key, text, cursor)) = staged
            && key == focus_key
        {
            editor.set_preedit(&mut observe_fonts.borrow_mut(), &text, cursor);
            focus_controller.borrow_mut().preedit = None;
        }
        drop(editor);
        let requested = focus_controller.borrow().requested.clone();
        if let Some((key, offset)) = requested
            && key == focus_key
        {
            focus_controller.borrow_mut().requested = None;
            observed.borrow_mut().set_selection_bytes(
                &mut observe_fonts.borrow_mut(),
                offset,
                offset,
            );
            repaint.focus();
            focus_follow.set(true);
            repaint.reveal();
        }
        repaint.content_dirty();
        Style::default()
            .width(Dimension::Fill)
            .height(Dimension::MinContent)
            .shrink(0.0)
    });
    let measured = editor.clone();
    let measure_fonts = fonts.clone();
    field.measure(move |available| {
        let mut fonts = measure_fonts.borrow_mut();
        let mut buffer = measured.borrow_mut();
        buffer.set_style(
            &mut fonts,
            TextStyle::new(px(13.0)).family(mosaic::text::FontFamily::Monospace),
        );
        buffer.set_wrap_width(&mut fonts, available.width.definite());
        let size = buffer.metrics(&mut fonts).size;
        Size::new(size.width.max(1.0), size.height.max(px(24.0)))
    });
    let painted = editor.clone();
    let paint_fonts = fonts.clone();
    let interaction = field.interaction();
    let paint_controller = controller.clone();
    let paint_location = location.clone();
    let paint_key = key.clone();
    let paint_follow = follow.clone();
    let layout_follow = follow.clone();
    let layout_editor = editor.clone();
    let layout_fonts = fonts.clone();
    let layout_controller = controller.clone();
    let layout_key = key.clone();
    let layout_interaction = field.interaction();
    field.on_layout(move |rect| {
        if layout_interaction.focused() && layout_follow.replace(false) {
            let mut fonts = layout_fonts.borrow_mut();
            let mut editor = layout_editor.borrow_mut();
            editor.set_wrap_width(&mut fonts, Some(rect.size.width));
            let caret = editor.caret_rect(&mut fonts);
            let state = layout_controller.borrow();
            let viewport = if layout_key.starts_with("draft-") {
                state.draft_viewport.as_ref()
            } else {
                state.viewport.as_ref()
            };
            if let Some(viewport) = viewport {
                viewport.reveal(caret_with_margin(rect.origin, caret));
            }
        }
    });
    field.paint(move |ctx| {
        let mut fonts = paint_fonts.borrow_mut();
        let mut buffer = painted.borrow_mut();
        buffer.set_style(&mut fonts, ctx.text_style.clone());
        buffer.set_wrap_width(&mut fonts, Some(ctx.rect.size.width));
        let origin = ctx.rect.origin;
        let shared = paint_controller
            .borrow()
            .selection
            .clone()
            .and_then(|selection| {
                if let Location::Draft(id) = &paint_location {
                    selection_range(
                        &buffer::parts(model, &model.session.get_untracked()),
                        &selection,
                        id,
                    )
                } else {
                    None
                }
            });
        if interaction.focused() || shared.is_some() {
            let rects = if let Some((start, end)) = shared {
                fonts
                    .shape(
                        &buffer.display_text(),
                        &ctx.text_style,
                        Some(ctx.rect.size.width),
                    )
                    .selection_rects(start, end)
            } else {
                buffer.selection_rects(&mut fonts)
            };
            for rect in rects {
                ctx.scene.shape(
                    Visual::new()
                        .fill(mosaic::core::theme::color(ground.rule_strong))
                        .shape(Rect::new(origin + rect.origin, rect.size)),
                );
            }
        }
        ctx.scene
            .glyphs(buffer.place(&mut fonts, ctx.text_color.clone(), origin, ctx.scale));
        if interaction.focused() {
            let caret = buffer.caret_rect(&mut fonts);
            let resized = std::mem::take(&mut paint_controller.borrow_mut().resize_follow);
            let state = paint_controller.borrow();
            let viewport = if paint_key.starts_with("draft-") {
                state.draft_viewport.as_ref()
            } else {
                state.viewport.as_ref()
            };
            if (paint_follow.replace(false) || resized)
                && let Some(viewport) = viewport
            {
                viewport.reveal(caret_with_margin(origin, caret));
            }
            ctx.scene.shape(
                Visual::new()
                    .fill(mosaic::core::theme::color(ground.ink))
                    .shape(Rect::from_xywh(
                        origin.x + caret.origin.x,
                        origin.y + caret.origin.y,
                        px(1.0),
                        caret.size.height,
                    )),
            );
        }
    });
    let ime = editor.clone();
    let ime_fonts = fonts.clone();
    let ime_field = field.clone();
    field.ime_area(move || {
        let rect = ime.borrow_mut().caret_rect(&mut ime_fonts.borrow_mut());
        Rect::new(ime_field.layout_rect().origin + rect.origin, rect.size)
    });
    let contents = editor.clone();
    field.text_contents(move || {
        let editor = contents.borrow();
        TextInputContents {
            text: editor.display_text(),
            selection: editor.selection_bytes(),
            compose: editor.preedit_bytes(),
        }
    });
    let dragging = Rc::new(Cell::new(false));
    let pointer = editor.clone();
    let pointer_fonts = fonts.clone();
    let pointer_field = field.clone();
    let pointer_controller = controller.clone();
    let pointer_key = key.clone();
    let pointer_location = location.clone();
    field.on_pointer(move |event, ctx| {
        let mut fonts = pointer_fonts.borrow_mut();
        let mut editor = pointer.borrow_mut();
        match event.kind {
            PointerEventKind::Down(PointerButton::Primary) => {
                ctx.request_focus();
                pointer_controller.borrow_mut().selection = None;
                dragging.set(true);
                editor.caret_to(
                    &mut fonts,
                    event.position - pointer_field.layout_rect().origin,
                    event.modifiers.shift,
                );
                if let Location::Draft(id) = &pointer_location {
                    pointer_controller.borrow_mut().pointer_anchor =
                        Some((id.clone(), editor.caret_offset(), mirror));
                }
                if ctx.press_count() == 2 {
                    editor.select_word(&mut fonts);
                } else if ctx.press_count() > 2 {
                    editor.select_line(&mut fonts);
                }
                ctx.stop_propagation();
            }
            PointerEventKind::Move if dragging.get() => editor.caret_to(
                &mut fonts,
                event.position - pointer_field.layout_rect().origin,
                true,
            ),
            PointerEventKind::Up(PointerButton::Primary) | PointerEventKind::Cancel => {
                dragging.set(false);
                editor.finish_selection();
            }
            _ => {}
        }
        pointer_controller.borrow_mut().caret = Some((pointer_key.clone(), editor.caret_offset()));
        pointer_field.paint_dirty();
    });
    let ime_editor = editor.clone();
    let ime_fonts = fonts.clone();
    let ime_location = location.clone();
    let ime_controller = controller.clone();
    let ime_field = field.clone();
    let ime_follow = follow.clone();
    field.on_ime(move |event, _| {
        let mut editor = ime_editor.borrow_mut();
        let mut fonts = ime_fonts.borrow_mut();
        if matches!(ime_location, Location::Source { .. }) {
            if let ImeEvent::Preedit { text, cursor } = event
                && !text.is_empty()
            {
                begin_reply(
                    model,
                    &ime_controller,
                    &ime_location,
                    editor.selection_bytes(),
                    "",
                );
                let target = ime_controller
                    .borrow()
                    .requested
                    .as_ref()
                    .map(|(key, _)| key.clone());
                if let Some(key) = target {
                    ime_controller.borrow_mut().preedit = Some((key, text.clone(), *cursor));
                }
            }
            if let ImeEvent::Commit(text) = event {
                begin_reply(
                    model,
                    &ime_controller,
                    &ime_location,
                    editor.selection_bytes(),
                    text,
                );
            }
            return;
        }
        match event {
            ImeEvent::Preedit { text, cursor } => editor.set_preedit(&mut fonts, text, *cursor),
            ImeEvent::Commit(text) => editor.insert(&mut fonts, text),
            ImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => editor.delete_surrounding(&mut fonts, *before_bytes, *after_bytes),
            ImeEvent::SetSelection { start, end } => {
                editor.set_selection_bytes(&mut fonts, *start, *end)
            }
        }
        if !matches!(
            event,
            ImeEvent::Preedit { .. } | ImeEvent::SetSelection { .. }
        ) && let Location::Draft(id) = &ime_location
        {
            update_text(model, &ime_controller, id, editor.text());
            crate::command_ui::complete(model, id, &editor.text(), editor.caret_offset());
        }
        ime_follow.set(true);
        ime_field.content_dirty();
    });
    let key_editor = editor.clone();
    let key_fonts = fonts.clone();
    let key_field = field.clone();
    let key_controller = controller.clone();
    let key_follow = follow.clone();
    field.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        key_follow.set(true);
        let command = event.modifiers.command();
        if command && event.key == Key::Enter {
            if !matches!(event.kind, KeyEventKind::Down { repeat: true }) {
                buffer::send(model);
            }
            ctx.stop_propagation();
            return;
        }
        if !command
            && model.completion.get_untracked().is_some()
            && !key_editor.borrow().has_preedit()
        {
            match event.key {
                Key::Escape => {
                    model.completion.set(None);
                    ctx.stop_propagation();
                    return;
                }
                Key::ArrowDown | Key::ArrowUp => {
                    model.completion.update(|value| {
                        if let Some(c) = value {
                            c.index = if event.key == Key::ArrowDown {
                                (c.index + 1) % c.choices.len()
                            } else {
                                (c.index + c.choices.len() - 1) % c.choices.len()
                            };
                        }
                    });
                    ctx.stop_propagation();
                    return;
                }
                Key::Enter | Key::Tab => {
                    let index = model
                        .completion
                        .get_untracked()
                        .map(|c| c.index)
                        .unwrap_or(0);
                    accept_completion(model, &key_controller, index, mirror);
                    ctx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        let mut editor = key_editor.borrow_mut();
        let mut fonts = key_fonts.borrow_mut();
        if command && !event.modifiers.shift && matches!(event.key, Key::Home | Key::End) {
            drop(editor);
            document_edge(&key_controller, &mut fonts, event.key == Key::End);
            ctx.stop_propagation();
            return;
        }
        let selection = editor.selection_bytes();
        key_controller.borrow_mut().caret = Some((key.clone(), editor.caret_offset()));
        if command && let Key::Character(key) = &event.key {
            match key.to_lowercase().as_str() {
                "c" => {
                    let selected = key_controller
                        .borrow()
                        .selection
                        .clone()
                        .and_then(|s| {
                            selection_text(
                                &buffer::parts(model, &model.session.get_untracked()),
                                &s,
                            )
                        })
                        .or_else(|| editor.selected_text());
                    if let Some(text) = selected {
                        model.ui.get_untracked().set_clipboard_text(text);
                    }
                    ctx.stop_propagation();
                    return;
                }
                "a" => {
                    key_controller.borrow_mut().selection = None;
                    editor.select_all(&mut fonts);
                    key_field.paint_dirty();
                    ctx.stop_propagation();
                    return;
                }
                "z" | "y" => {
                    key_controller.borrow_mut().selection = None;
                    let session = model.session.get_untracked();
                    let current = buffer::parts(model, &session);
                    let mut history = key_controller.borrow_mut();
                    let current = History {
                        parts: current,
                        focus: history.caret.clone(),
                    };
                    let next = if key.eq_ignore_ascii_case("y") || event.modifiers.shift {
                        history
                            .redo
                            .pop()
                            .inspect(|_| history.undo.push(current.clone()))
                    } else {
                        history
                            .undo
                            .pop()
                            .inspect(|_| history.redo.push(current.clone()))
                    };
                    drop(history);
                    if let Some(record) = next {
                        key_controller.borrow_mut().requested = record.focus;
                        buffer::edit(model, &session, record.parts);
                    }
                    ctx.stop_propagation();
                    return;
                }
                "v" => {
                    let inserted = buffer::paste(model);
                    match inserted {
                        Ok(parts)
                            if parts
                                .iter()
                                .all(|p| matches!(p.kind, PartKind::Text { .. })) =>
                        {
                            let text = plain_text(&parts);
                            if edit_selection(model, &key_controller, &text, mirror) {
                                ctx.stop_propagation();
                                return;
                            }
                            match &location {
                                Location::Draft(id) => {
                                    editor.insert(&mut fonts, &text);
                                    update_text(model, &key_controller, id, editor.text());
                                    crate::command_ui::complete(
                                        model,
                                        id,
                                        &editor.text(),
                                        editor.caret_offset(),
                                    );
                                    key_field.content_dirty();
                                }
                                _ => {
                                    begin_reply(model, &key_controller, &location, selection, &text)
                                }
                            }
                        }
                        Ok(parts) => match &location {
                            Location::Draft(id) => {
                                insert_parts(model, &key_controller, id, selection, parts, mirror)
                            }
                            _ => {
                                begin_reply(model, &key_controller, &location, selection, "");
                                let requested = key_controller.borrow().requested.clone();
                                if let Some((key, _)) = requested
                                    && let Some(id) = key.strip_prefix("inline-")
                                {
                                    insert_parts(model, &key_controller, id, (0, 0), parts, true);
                                }
                            }
                        },
                        Err(error) => model.notice.set(error),
                    }
                    ctx.stop_propagation();
                    return;
                }
                "x" => {
                    let selected = key_controller.borrow().selection.clone().and_then(|s| {
                        selection_text(&buffer::parts(model, &model.session.get_untracked()), &s)
                    });
                    if let Some(text) = selected {
                        model.ui.get_untracked().set_clipboard_text(text);
                        if edit_selection(model, &key_controller, "", mirror) {
                            ctx.stop_propagation();
                            return;
                        }
                    }
                    if let Location::Draft(_) = location {
                        if let Some(text) = editor.selected_text() {
                            model.ui.get_untracked().set_clipboard_text(text);
                        }
                        editor.insert(&mut fonts, "");
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        } else {
            let motion = match event.key {
                Key::ArrowLeft => Some(if command || event.modifiers.alt {
                    CaretMotion::WordLeft
                } else {
                    CaretMotion::Left
                }),
                Key::ArrowRight => Some(if command || event.modifiers.alt {
                    CaretMotion::WordRight
                } else {
                    CaretMotion::Right
                }),
                Key::ArrowUp => Some(CaretMotion::Up),
                Key::ArrowDown => Some(CaretMotion::Down),
                Key::Home => Some(if command {
                    CaretMotion::TextStart
                } else {
                    CaretMotion::LineStart
                }),
                Key::End => Some(if command {
                    CaretMotion::TextEnd
                } else {
                    CaretMotion::LineEnd
                }),
                _ => None,
            };
            if let Some(motion) = motion {
                let before = editor.caret_offset();
                editor.move_caret(&mut fonts, motion, event.modifiers.shift);
                if event.modifiers.shift
                    && let Location::Draft(id) = &location
                {
                    let mut selection = key_controller.borrow().selection.clone();
                    if editor.caret_offset() == before
                        && matches!(
                            motion,
                            CaretMotion::Left
                                | CaretMotion::Right
                                | CaretMotion::Up
                                | CaretMotion::Down
                        )
                    {
                        if let Some((next, offset)) = adjacent_text(
                            &buffer::parts(model, &model.session.get_untracked()),
                            id,
                            if matches!(motion, CaretMotion::Left | CaretMotion::Up) {
                                -1
                            } else {
                                1
                            },
                        ) {
                            let range = selection.get_or_insert_with(|| DraftSelection {
                                anchor: (id.clone(), {
                                    let (a, b) = editor.selection_bytes();
                                    if editor.caret_offset() == a { b } else { a }
                                }),
                                head: (id.clone(), before),
                            });
                            range.head = (next.clone(), offset);
                            let mut state = key_controller.borrow_mut();
                            state.requested = Some((
                                format!("{}-{next}", if mirror { "inline" } else { "draft" }),
                                offset,
                            ));
                            state.selection = selection;
                            drop(state);
                            model.buffer.update(|_| {});
                            ctx.stop_propagation();
                            return;
                        }
                    } else if let Some(range) = &mut selection {
                        range.head = (id.clone(), editor.caret_offset());
                        key_controller.borrow_mut().selection = selection;
                    }
                    for (field, _) in key_controller.borrow().surfaces.values() {
                        field.paint_dirty();
                    }
                } else if !event.modifiers.shift {
                    key_controller.borrow_mut().selection = None;
                }
                if editor.caret_offset() == before
                    && !event.modifiers.shift
                    && matches!(
                        motion,
                        CaretMotion::Left
                            | CaretMotion::Right
                            | CaretMotion::Up
                            | CaretMotion::Down
                    )
                {
                    drop(editor);
                    neighboring(
                        &key_controller,
                        &key,
                        if matches!(motion, CaretMotion::Left | CaretMotion::Up) {
                            -1
                        } else {
                            1
                        },
                        &mut fonts,
                    );
                }
                key_field.paint_dirty();
                ctx.stop_propagation();
                return;
            }
            let insert = match &event.key {
                Key::Character(text) if !command && !event.modifiers.alt => Some(text.as_str()),
                Key::Space => Some(" "),
                Key::Enter => Some("\n"),
                _ => None,
            };
            if let Some(text) = insert {
                if matches!(location, Location::Draft(_))
                    && edit_selection(model, &key_controller, text, mirror)
                {
                    ctx.stop_propagation();
                    return;
                }
                if editor.has_preedit() {
                    return;
                }
                if matches!(location, Location::Source { .. }) {
                    begin_reply(model, &key_controller, &location, selection, text);
                    ctx.stop_propagation();
                    return;
                }
                editor.insert(&mut fonts, text);
            } else if matches!(location, Location::Draft(_)) {
                if matches!(event.key, Key::Backspace | Key::Delete)
                    && edit_selection(model, &key_controller, "", mirror)
                {
                    ctx.stop_propagation();
                    return;
                }
                if let Location::Draft(id) = &location
                    && selection.0 == selection.1
                    && ((event.key == Key::Backspace && selection.0 == 0)
                        || (event.key == Key::Delete && selection.1 == editor.text().len()))
                {
                    let session = model.session.get_untracked();
                    let mut parts = buffer::parts(model, &session);
                    let before = parts.clone();
                    if let Some((target, offset)) =
                        delete_boundary(&mut parts, id, event.key == Key::Backspace)
                    {
                        remember(&key_controller, before);
                        key_controller.borrow_mut().requested = Some((
                            format!("{}-{target}", if mirror { "inline" } else { "draft" }),
                            offset,
                        ));
                        buffer::edit(model, &session, parts);
                        ctx.stop_propagation();
                        return;
                    }
                }
                match event.key {
                    Key::Backspace => {
                        if command || event.modifiers.alt {
                            editor.delete_word_backward(&mut fonts)
                        } else {
                            editor.delete_backward(&mut fonts)
                        }
                    }
                    Key::Delete => editor.delete_forward(&mut fonts),
                    _ => return,
                }
            } else {
                return;
            }
        }
        if let Location::Draft(id) = &location {
            update_text(model, &key_controller, id, editor.text());
            crate::command_ui::complete(model, id, &editor.text(), editor.caret_offset());
        }
        key_field.content_dirty();
        ctx.stop_propagation();
    });
    field
}

/// One block of session provenance: a workspace or the worker run, with
/// only the values that are actually recorded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProvenanceGroup {
    /// Stable identity: the workspace connection, or "worker" for the run.
    pub key: String,
    pub title: String,
    pub rows: Vec<(&'static str, String)>,
    pub changes: Option<String>,
}

fn change_text(changes: Option<&ChangeSet>) -> String {
    changes
        .map(|c| {
            format!(
                "{}\n\n{}{}",
                c.files.join("\n"),
                c.diff,
                if c.truncated {
                    "\n\nReview truncated"
                } else {
                    ""
                }
            )
        })
        .unwrap_or_else(|| "Changes unavailable".into())
}

pub(crate) fn provenance(
    snapshot: &Snapshot,
    session: &Session,
    changes: bool,
) -> Vec<ProvenanceGroup> {
    let mut groups: Vec<ProvenanceGroup> = session
        .workspaces
        .iter()
        .map(|w| {
            let title = snapshot
                .connections
                .iter()
                .find(|c| c.id == w.connection_id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| {
                    if w.connection_id.starts_with("workspace-") {
                        "Project workspace".into()
                    } else {
                        w.connection_id.clone()
                    }
                });
            let mut rows = vec![
                (
                    "Kind",
                    if w.repository {
                        "Repository"
                    } else {
                        "Directory"
                    }
                    .to_string(),
                ),
                ("Path", w.path.clone()),
            ];
            if let Some(branch) = &w.branch {
                rows.push(("Branch", branch.clone()));
            }
            if let Some(commit) = &w.base_commit {
                rows.push(("Base", commit.chars().take(12).collect()));
            }
            ProvenanceGroup {
                key: w.connection_id.clone(),
                title,
                rows,
                changes: changes.then(|| change_text(w.changes.as_ref())),
            }
        })
        .collect();
    if let Some(worker) = &session.worker {
        let mut rows = vec![(
            "Harness",
            match worker.harness {
                Harness::Codex => "Codex",
                Harness::ClaudeCode => "Claude Code",
            }
            .to_string(),
        )];
        for (key, value) in [
            ("Thread", &worker.thread_id),
            ("Worktree", &worker.worktree),
            ("Branch", &worker.branch),
            ("Base", &worker.base_commit),
        ] {
            if let Some(value) = value {
                rows.push((key, value.clone()));
            }
        }
        groups.push(ProvenanceGroup {
            key: "worker".into(),
            title: "Worker run".into(),
            rows,
            changes: (changes && session.workspaces.is_empty())
                .then(|| change_text(worker.changes.as_ref())),
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_selection_replaces_text_and_objects_together_without_breaking_unicode() {
        let left = Part::text("λ before");
        let right = Part::text("after β");
        let object = Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Asset {
                asset: Asset {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "image.png".into(),
                    media_type: "image/png".into(),
                    size: 1,
                },
            },
        };
        let mut parts = vec![left.clone(), object, right.clone()];
        let range = DraftSelection {
            anchor: (left.id.clone(), 3),
            head: (right.id.clone(), 5),
        };
        assert_eq!(
            selection_text(&parts, &range).unwrap(),
            "before[image.png]after"
        );
        assert_eq!(
            replace_selection(&mut parts, &range, "replacement"),
            Some((left.id.clone(), 14))
        );
        assert_eq!(plain_text(&parts), "λ replacement β");
        assert_eq!(parts.len(), 1);
        assert_eq!(
            delete_boundary(&mut vec![left.clone(), right], &left.id, false),
            Some((left.id, 9))
        );
    }
}
