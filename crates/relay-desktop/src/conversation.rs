//! A Relay-owned document interaction controller over Mosaic's native text primitives.
use crate::command_ui::{CommandPanel, CommandPanelProps};
use crate::panels::{BoundedPanel, BoundedPanelProps};
use crate::transcript::{
    PendingApproval, PendingApprovalProps, QueuedTurn, QueuedTurnProps, SessionHead,
    SessionHeadProps, ToolMessage, ToolMessageProps, choose_effort, ledger_view,
};
use crate::ui::{Notice, NoticeProps};
use crate::ui::{
    Readout, ReadoutProps, UsageReadout, UsageReadoutProps, action_cell, icon_action_cell, press,
};
use crate::{
    buffer,
    model::{EditTarget, Model},
};
use mosaic::core::theme::color;
use mosaic::{
    prelude::*,
    text::{CaretMotion, EditBuffer},
    widgets::input::TextInputContents,
};
use relay_core::*;
use relay_ui::composer::ComposerHandle;
use relay_ui::prelude::{
    AgentText, AgentTextProps, Composer, ComposerProps, FingerprintStrip, FingerprintStripProps,
    LedgerActions, MessageRow, MessageRowProps, ReplyStrip, ReplyStripProps, Segments,
    SegmentsProps, ThinkingLine, ThinkingLineProps, UserMessage, UserMessageProps,
};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Ellipsis, EllipsisProps, IconCell, IconCellProps, Label,
    LabelProps, LinkButton, LinkButtonProps, Rule, TextRole, Tone, Tooltip, TooltipProps,
};
use relay_ui::surfaces::{TextInput, TextInputProps};
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
        part: Option<String>,
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
    source_locations: BTreeMap<String, Location>,
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
    manual_reading: bool,
    /// Set when a session opens: its first layout lands at the end even if
    /// the previous session was left scrolled elsewhere.
    open_at_end: bool,
    /// Set by a disclosure toggle: the growth it causes keeps the toggled
    /// control in place instead of following the end.
    hold_reading: bool,
    index: Option<Derived<Rc<TranscriptIndex>>>,
    window: Option<HistoryWindow>,
    expanded_rows: BTreeMap<String, bool>,
    pub(crate) tool_openings: BTreeMap<String, relay_ui::prelude::Opening>,
    source_request: Option<(String, u64)>,
    source_search: bool,
    asset_previews: BTreeMap<(String, Option<String>, bool), bool>,
    retained_session: Option<String>,
    native_selection_rows: Option<Vec<String>>,
    /// Part id of the composer's trailing empty text when the draft has none.
    empty_draft: String,
}

/// Only history participates in windowing. The surrounding document, including
/// the live composer, keeps its original element and reactive scope.
#[derive(Clone)]
struct HistoryWindow {
    revision: State<u64>,
    geometry: State<(f32, f32, f32)>, // offset, viewport height, history origin
    requested: State<Option<String>>,
    heights: Rc<RefCell<BTreeMap<String, HistoryHeight>>>,
    projection: Rc<RefCell<Option<Rc<TranscriptIndex>>>>,
}

#[derive(Clone, Copy)]
struct HistoryHeight {
    pixels: f32,
    stale: bool,
}

#[derive(Clone, PartialEq)]
struct HistorySlot {
    key: (bool, String), // true for a message; false for a spacer
    height: f32,
}

impl HistoryWindow {
    fn new() -> Self {
        Self {
            revision: State::new(0),
            geometry: State::new((0.0, 900.0, 0.0)),
            requested: State::new(None),
            heights: Rc::default(),
            projection: Rc::default(),
        }
    }

    fn height(&self, id: &str) -> f32 {
        self.heights
            .borrow()
            .get(id)
            .map_or(px(120.0), |height| height.pixels)
    }

    /// A disposed/dirty row's last height is only an estimate. Preserve it
    /// until layout supplies a correction: replacing it with an arbitrary
    /// default would move the reader before any new geometry was available.
    fn invalidate(&self, id: &str) {
        if let Some(height) = self.heights.borrow_mut().get_mut(id) {
            height.stale = true;
        }
    }

    fn invalidate_all(&self) {
        for height in self.heights.borrow_mut().values_mut() {
            height.stale = true;
        }
    }

    fn reconcile(&self, next: Rc<TranscriptIndex>) {
        let mut previous = self.projection.borrow_mut();
        self.heights.borrow_mut().retain(|id, height| {
            let Some(entry) = next.rows.get(id) else {
                return false;
            };
            if previous.as_ref().and_then(|index| index.rows.get(id)) != Some(entry) {
                height.stale = true;
            }
            true
        });
        *previous = Some(next);
    }

    fn report_height(&self, id: &str, pixels: f32) -> (f32, bool) {
        let delta = pixels - self.height(id);
        let revalidated = self
            .heights
            .borrow()
            .get(id)
            .is_none_or(|height| height.stale);
        // Also revalidate equal-height reports. A new element always reports
        // its first layout, even if new content happens to occupy the same height.
        self.heights.borrow_mut().insert(
            id.to_owned(),
            HistoryHeight {
                pixels,
                stale: false,
            },
        );
        (delta, revalidated)
    }

    fn slots(&self, order: &[String], pinned: &[String]) -> Vec<HistorySlot> {
        self.revision.get();
        let (offset, viewport, origin) = self.geometry.get();
        let requested = self.requested.get();
        let mut y = origin;
        let mut gap = 0.0;
        let mut slots = Vec::new();
        // A viewport of overscan on each side keeps wheel/touch scrolling
        // smooth without mounting the rest of the document.
        let overscan = viewport.max(px(600.0));
        for id in order {
            let height = self.height(id);
            let visible = order.len() <= 40
                || (y + height >= offset - overscan && y <= offset + viewport + overscan)
                || requested.as_ref() == Some(id)
                || pinned.contains(id);
            if visible {
                if gap > 0.0 {
                    slots.push(HistorySlot {
                        key: (false, id.clone()),
                        height: gap,
                    });
                    gap = 0.0;
                }
                slots.push(HistorySlot {
                    key: (true, id.clone()),
                    height,
                });
            } else {
                gap += height;
            }
            y += height;
        }
        if gap > 0.0 {
            slots.push(HistorySlot {
                key: (false, String::new()),
                height: gap,
            });
        }
        slots
    }

    fn changed(&self) {
        self.revision
            .set(self.revision.get_untracked().wrapping_add(1));
    }
}

fn pinned_history(controller: &Shared, ui: &Ui) -> Vec<String> {
    let mut state = controller.borrow_mut();
    // Capture once per selection, never absorb rows mounted by later scrolling.
    let mounted = state.messages.keys().cloned().collect();
    state.capture_native_selection(ui.selected_text().is_some(), mounted);
    let mut pinned = state
        .source_locations
        .iter()
        .filter_map(|(key, location)| {
            let (field, buffer) = state.surfaces.get(key)?;
            let (a, b) = buffer.borrow().selection_bytes();
            if (field.interaction().focused() || a != b)
                && let Location::Source { message, .. } = location
            {
                Some(message.clone())
            } else {
                None
            }
        })
        .chain(state.history_anchor.as_ref().map(|(id, _)| id.clone()))
        .collect::<Vec<_>>();
    pinned.extend(state.native_selection_rows.iter().flatten().cloned());
    for (id, row) in &state.messages {
        if row.interaction().focus_within() {
            pinned.push(id.clone());
        }
    }
    for (key, (field, buffer)) in &state.surfaces {
        if !key.starts_with("inline-") {
            continue;
        }
        let (a, b) = buffer.borrow().selection_bytes();
        if field.interaction().focused() || a != b {
            for (id, row) in &state.messages {
                if row.layout_rect().contains(field.layout_rect().center()) {
                    pinned.push(id.clone());
                }
            }
        }
    }
    pinned
}

pub(crate) type Shared = Rc<RefCell<Controller>>;
pub(crate) type ControllerState = State<Shared>;

impl Controller {
    fn capture_native_selection(&mut self, selected: bool, mounted: Vec<String>) {
        if !selected {
            self.native_selection_rows = None;
        } else if self.native_selection_rows.is_none() {
            self.native_selection_rows = Some(mounted);
        }
    }

    fn prune_history_state(
        &mut self,
        session: &str,
        index: &TranscriptIndex,
        assets: &[(String, Option<String>)],
    ) {
        if self.retained_session.as_deref() != Some(session) {
            self.expanded_rows.clear();
            self.tool_openings.clear();
            self.asset_previews.clear();
            self.native_selection_rows = None;
            self.retained_session = Some(session.to_owned());
            if let Some(window) = &self.window {
                window.heights.borrow_mut().clear();
            }
        }
        self.expanded_rows
            .retain(|id, _| index.rows.contains_key(id));
        self.tool_openings
            .retain(|id, _| index.rows.contains_key(id));
        self.asset_previews.retain(|(id, part, _), _| {
            assets
                .iter()
                .any(|(asset, current)| asset == id && current == part)
        });
        if let Some(rows) = &mut self.native_selection_rows {
            rows.retain(|id| index.rows.contains_key(id));
        }
    }

    pub(crate) fn invalidate_history_height(&self, id: &str) {
        if let Some(window) = &self.window {
            window.invalidate(id);
        }
    }
}

fn preview_assets(parts: &[Part], draft: bool) -> Vec<(String, Option<String>)> {
    parts
        .iter()
        .flat_map(|part| match &part.kind {
            PartKind::Asset { asset } => vec![(asset.id.clone(), draft.then(|| part.id.clone()))],
            PartKind::Reply { parts, .. } => preview_assets(parts, draft),
            _ => vec![],
        })
        .collect()
}

/// One indexed projection per conversation. Row consumers read a single entry;
/// changes to message bodies do not invalidate the list's stable key sequence.
#[derive(Clone, Default, PartialEq)]
struct TranscriptIndex {
    order: Vec<String>,
    rows: BTreeMap<String, TranscriptEntry>,
}

#[derive(Clone, PartialEq)]
struct TranscriptEntry {
    message: Message,
    comments: Vec<Comment>,
    speaker: String,
    /// The agent session that sent a prompt, when it still exists.
    speaker_session: Option<String>,
    continues: bool,
    spacing: f32,
}

fn transcript_index(model: Model) -> Derived<Rc<TranscriptIndex>> {
    Derived::new(move || {
        let session = model.session.get();
        model.snapshot.with(|snapshot| {
            let mut result = TranscriptIndex::default();
            let mut comments: BTreeMap<&str, Vec<Comment>> = BTreeMap::new();
            for comment in &snapshot.comments {
                comments
                    .entry(&comment.message_id)
                    .or_default()
                    .push(comment.clone());
            }
            let mut previous: Option<(String, bool)> = None;
            for message in crate::adapt::transcript_messages(&snapshot.messages, &session) {
                let (speaker, speaker_session) = if message.kind == "prompt" {
                    prompt_speaker(snapshot, message)
                } else {
                    (message.author.clone(), None)
                };
                let tool = message.tool.is_some()
                    || matches!(message.kind.as_str(), "command_execution" | "file_change");
                let continues = previous.as_ref().is_some_and(|(name, _)| *name == speaker);
                let entry = TranscriptEntry {
                    message: crate::tool_activity::with_legacy_tool(message.clone()),
                    comments: comments.remove(message.id.as_str()).unwrap_or_default(),
                    speaker: if continues {
                        String::new()
                    } else {
                        speaker.clone()
                    },
                    speaker_session: speaker_session.filter(|_| !continues),
                    continues,
                    spacing: match previous.as_ref() {
                        None => 0.0,
                        Some((_, true)) if tool => 6.0,
                        Some(_) => 28.0,
                    },
                };
                previous = Some((speaker, tool));
                result.order.push(message.id.clone());
                result.rows.insert(message.id.clone(), entry);
            }
            Rc::new(result)
        })
    })
}

/// A prompt's speaker: the user, or the sending agent named by its place in
/// the session tree relative to the receiving session. A deleted sender is
/// shown as "Agent" without a link.
fn prompt_speaker(snapshot: &Snapshot, message: &Message) -> (String, Option<String>) {
    // Relay notices are prompts Relay queues for itself, never the user or an agent.
    if message.author == "Relay" {
        return ("Relay".into(), None);
    }
    let Sender::Agent { session_id } = &message.sender else {
        return ("You".into(), None);
    };
    let Some(sender) = snapshot.sessions.iter().find(|s| &s.id == session_id) else {
        return ("Agent".into(), None);
    };
    let receiver_parent = snapshot
        .sessions
        .iter()
        .find(|s| s.id == message.session_id)
        .and_then(|s| s.parent_session_id.as_deref());
    let sender_parent = sender.parent_session_id.as_deref();
    let relationship = if receiver_parent == Some(sender.id.as_str()) {
        Some("Parent")
    } else if sender_parent == Some(message.session_id.as_str()) {
        Some("Child")
    } else if sender_parent.is_some() && sender_parent == receiver_parent {
        Some("Sibling")
    } else {
        None
    };
    let label = match relationship {
        Some(relationship) => format!("{relationship} · {}", sender.title),
        None => sender.title.clone(),
    };
    (label, Some(sender.id.clone()))
}

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
        Location::Source { message, .. } => {
            let index = model
                .conversation_controller
                .get_untracked()
                .and_then(|controller| controller.borrow().index);
            let read = |m: &Message| {
                source_range(m, location)
                    .and_then(|(start, end)| m.body.get(start..end).map(str::to_owned))
            };
            if let Some(index) = index {
                return index
                    .with(|index| {
                        index
                            .rows
                            .get(message)
                            .and_then(|entry| read(&entry.message))
                    })
                    .unwrap_or_default();
            }
            model
                .snapshot
                .with(|snapshot| {
                    snapshot
                        .messages
                        .iter()
                        .find(|m| &m.id == message)
                        .and_then(read)
                })
                .unwrap_or_default()
        }
    }
}

fn source_range(message: &Message, location: &Location) -> Option<(usize, usize)> {
    let Location::Source {
        message: message_id,
        start,
        end,
        part,
    } = location
    else {
        return None;
    };
    if &message.id != message_id {
        return None;
    }
    fn find(parts: &[Part], id: &str, mut offset: usize) -> Option<(usize, usize)> {
        for part in parts {
            match &part.kind {
                PartKind::Text { text } if part.id == id => {
                    return Some((offset, offset + text.len()));
                }
                PartKind::Reply { anchor, parts } => {
                    if let Some(range) = find(
                        parts,
                        id,
                        offset + format!("Reply to “{}”:\n", anchor.quote).len(),
                    ) {
                        return Some(range);
                    }
                }
                _ => {}
            }
            offset += plain_text(std::slice::from_ref(part)).len();
        }
        None
    }
    match part {
        Some(id) => find(&message.parts, id, 0),
        None => Some((*start, end.unwrap_or(message.body.len()))),
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
            model
                .command_feedback
                .set(command.reason.unwrap_or_default());
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
    let Location::Source { message, .. } = location else {
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
    let Some((start, _)) = source_range(source, location) else {
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

/// Capture the insertion point before a browser file read yields.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct AttachmentTarget {
    session: String,
    controller: Shared,
    part: String,
    selection: (usize, usize),
    text: String,
    mirror: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn attachment_target(model: Model) -> Option<AttachmentTarget> {
    let controller = model.conversation_controller.get_untracked()?;
    let state = controller.borrow();
    let editable = |key: &str| key.starts_with("draft-") || key.starts_with("inline-");
    let target = state
        .surfaces
        .iter()
        .find(|(key, (field, _))| editable(key) && field.interaction().focused())
        .or_else(|| {
            state
                .caret
                .as_ref()
                .and_then(|(key, _)| state.surfaces.get_key_value(key))
                .filter(|(key, _)| editable(key))
        })
        .or_else(|| {
            state
                .surfaces
                .iter()
                .filter(|(key, _)| key.starts_with("draft-"))
                .max_by(|(_, (a, _)), (_, (b, _))| {
                    a.layout_rect()
                        .origin
                        .y
                        .total_cmp(&b.layout_rect().origin.y)
                })
        })?;
    let (key, (_, editor)) = target;
    let mirror = key.starts_with("inline-");
    let part = key
        .strip_prefix(if mirror { "inline-" } else { "draft-" })?
        .to_owned();
    let editor = editor.borrow();
    Some(AttachmentTarget {
        session: model.session.get_untracked(),
        controller: controller.clone(),
        part,
        selection: editor.selection_bytes(),
        text: editor.text(),
        mirror,
    })
}

#[cfg(any(target_arch = "wasm32", test))]
impl AttachmentTarget {
    pub(crate) fn insert(self, model: Model, parts: Vec<Part>) -> Result<(), String> {
        if model.session.get_untracked() != self.session
            || model
                .conversation_controller
                .get_untracked()
                .is_none_or(|current| !Rc::ptr_eq(&current, &self.controller))
        {
            return Err("Conversation changed while reading files; paste them again.".into());
        }
        let current =
            text_part(&buffer::parts(model, &self.session), &self.part).unwrap_or_default();
        // Keystrokes arriving during the file read belong after the pasted
        // image. Preserve them when they were inserted at the captured cursor.
        let continued = current.len().saturating_sub(self.text.len());
        if current != self.text
            && !(self.selection.0 == self.selection.1
                && current.len() >= self.text.len()
                && current.starts_with(&self.text[..self.selection.0])
                && current.ends_with(&self.text[self.selection.1..]))
        {
            return Err("Draft changed while reading files; paste them again.".into());
        }
        insert_parts(
            model,
            &self.controller,
            &self.part,
            self.selection,
            parts,
            self.mirror,
        );
        if continued > 0
            && let Some((_, offset)) = &mut self.controller.borrow_mut().requested
        {
            *offset = continued;
        }
        Ok(())
    }
}

fn source_targets(index: &TranscriptIndex) -> Vec<(String, relay_core::Anchor)> {
    fn parts(
        id: &str,
        values: &[Part],
        offset: usize,
        out: &mut Vec<(String, relay_core::Anchor)>,
    ) {
        for (_, (part, start)) in located_parts(values, offset) {
            match part.kind {
                PartKind::Text { text } => out.push((
                    format!("source-{id}-{}", part.id),
                    relay_core::Anchor {
                        message_id: id.into(),
                        start,
                        end: start + text.len(),
                        quote: text,
                    },
                )),
                PartKind::Reply {
                    anchor,
                    parts: nested,
                } => parts(
                    id,
                    &nested,
                    start + format!("Reply to “{}”:\n", anchor.quote).len(),
                    out,
                ),
                _ => {}
            }
        }
    }
    let mut result = Vec::new();
    for id in &index.order {
        let message = &index.rows[id].message;
        if message.tool.is_some() {
            continue;
        }
        if message.parts.is_empty() {
            result.push((
                format!("source-{id}-0"),
                relay_core::Anchor {
                    message_id: id.clone(),
                    start: 0,
                    end: message.body.len(),
                    quote: message.body.clone(),
                },
            ));
        } else {
            parts(id, &message.parts, 0, &mut result);
        }
    }
    result
}

fn document_edge(
    model: Model,
    controller: &Shared,
    fonts: &mut mosaic::text::FontContext,
    end: bool,
) {
    if !end {
        let index = controller.borrow().index;
        if let Some(index) = index
            && let Some((_, mut anchor)) = index
                .with_untracked(|index| source_targets(index))
                .into_iter()
                .next()
        {
            anchor.end = anchor.start;
            anchor.quote.clear();
            controller.borrow_mut().source_search = false;
            model.source_anchor.set(Some(anchor));
            return;
        }
    }
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
    model: Model,
    controller: &Shared,
    key: &str,
    direction: i32,
    fonts: &mut mosaic::text::FontContext,
) {
    // Preserve navigation among recorded parts and editable reply mirrors in
    // the same row before crossing a window boundary through the model.
    let local = {
        let state = controller.borrow();
        let mut entries = state.surfaces.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            a.1.0
                .layout_rect()
                .origin
                .y
                .total_cmp(&b.1.0.layout_rect().origin.y)
        });
        entries
            .iter()
            .position(|(id, _)| id.as_str() == key)
            .and_then(|position| {
                let next = position as i32 + direction;
                let (_, (field, buffer)) =
                    (next >= 0).then(|| entries.get(next as usize)).flatten()?;
                let current = &state.surfaces[key].0;
                state
                    .messages
                    .values()
                    .any(|row| {
                        let rect = row.layout_rect();
                        rect.contains(current.layout_rect().center())
                            && rect.contains(field.layout_rect().center())
                    })
                    .then(|| (field.clone(), buffer.clone()))
            })
    };
    if let Some((field, buffer)) = local {
        let offset = if direction < 0 {
            buffer.borrow().text().len()
        } else {
            0
        };
        buffer
            .borrow_mut()
            .set_selection_bytes(fonts, offset, offset);
        field.focus();
        field.reveal();
        field.paint_dirty();
        return;
    }
    let index = controller.borrow().index;
    if let Some(index) = index {
        let sources = index.with_untracked(|index| source_targets(index));
        let position = sources.iter().position(|(id, _)| id == key);
        let target = if let Some(position) = position {
            let next = position as i32 + direction;
            (next >= 0).then(|| sources.get(next as usize)).flatten()
        } else if key.starts_with("draft-") && direction < 0 {
            let state = controller.borrow();
            let first = state
                .surfaces
                .iter()
                .filter(|(key, _)| key.starts_with("draft-"))
                .min_by(|a, b| {
                    a.1.0
                        .layout_rect()
                        .origin
                        .y
                        .total_cmp(&b.1.0.layout_rect().origin.y)
                });
            if first.is_some_and(|(id, _)| id == key) {
                sources.last()
            } else {
                None
            }
        } else if key.starts_with("inline-") {
            let state = controller.borrow();
            let row = state
                .messages
                .iter()
                .find(|(_, row)| row.interaction().focus_within())
                .map(|(id, _)| id.clone());
            row.and_then(|row| {
                index.with_untracked(|index| {
                    let position = index.order.iter().position(|id| id == &row)?;
                    if direction < 0 {
                        sources.iter().rev().find(|(_, anchor)| {
                            index
                                .order
                                .iter()
                                .position(|id| id == &anchor.message_id)
                                .is_some_and(|i| i < position)
                        })
                    } else {
                        sources.iter().find(|(_, anchor)| {
                            index
                                .order
                                .iter()
                                .position(|id| id == &anchor.message_id)
                                .is_some_and(|i| i > position)
                        })
                    }
                })
            })
        } else {
            None
        };
        if let Some((_, anchor)) = target {
            let mut anchor = anchor.clone();
            if direction < 0 {
                anchor.start = anchor.end;
            } else {
                anchor.end = anchor.start;
            }
            anchor.quote.clear();
            controller.borrow_mut().source_search = false;
            model.source_anchor.set(Some(anchor));
            return;
        }
    }
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
    let indexed = controller.borrow().index;
    if let Some(index) = indexed {
        let matches = index.with(|index| {
            index
                .order
                .iter()
                .flat_map(|id| {
                    index.rows[id]
                        .message
                        .body
                        .match_indices(&query)
                        .map(|(start, _)| (id.clone(), start))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });
        if matches.is_empty() {
            return;
        }
        let previous = controller.borrow().search_hit.clone();
        let position = matches
            .iter()
            .position(|hit| Some(hit) == previous.as_ref())
            .map(|i| (i as i32 + direction).rem_euclid(matches.len() as i32) as usize)
            .unwrap_or(if direction < 0 { matches.len() - 1 } else { 0 });
        let (id, start) = matches[position].clone();
        controller.borrow_mut().search_hit = Some((id.clone(), start));
        go_to_source(
            model,
            controller,
            &relay_core::Anchor {
                message_id: id,
                start,
                end: start,
                quote: String::new(),
            },
            false,
        );
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

fn go_to_source(model: Model, controller: &Shared, anchor: &relay_core::Anchor, focus: bool) {
    controller.borrow_mut().source_search = !focus;
    controller.borrow_mut().history_anchor = None;
    controller.borrow_mut().manual_reading = false;
    controller.borrow_mut().resize_follow = false;
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
        if model.source_anchor.get_untracked().as_ref() != Some(anchor) {
            model.source_anchor.set(Some(anchor.clone()));
        }
        let request = (
            anchor.message_id.clone(),
            model.history_generation.get_untracked(),
        );
        let send = controller.borrow().source_request.as_ref() != Some(&request)
            || !model.history_error.get_untracked().is_empty();
        if send
            && model.request_history(relay_core::StreamRequest::Around {
                session_id: model.session.get_untracked(),
                generation: model.history_generation.get_untracked(),
                message_id: anchor.message_id.clone(),
            })
        {
            controller.borrow_mut().source_request = Some(request);
        }
        return;
    }
    controller.borrow_mut().source_request = None;
    let prefix = format!("source-{}-", anchor.message_id);
    let snapshot = model.snapshot.get_untracked();
    let Some(message) = snapshot.messages.iter().find(|m| m.id == anchor.message_id) else {
        return;
    };
    let state = controller.borrow();
    let selected = state
        .surfaces
        .iter()
        .filter_map(|(key, (field, editor))| {
            key.starts_with(&prefix)
                .then(|| state.source_locations.get(key))
                .flatten()
                .and_then(|location| source_range(message, location).map(|(start, _)| start))
                .filter(|offset| *offset <= anchor.start)
                .map(|offset| (offset, field.clone(), editor.clone()))
        })
        .max_by_key(|(offset, _, _)| *offset);
    drop(state);
    // Tool rows own selectable presentation rather than BufferText surfaces.
    // Reveal their measured row once it exists, instead of leaving an anchor
    // pending forever while looking for a text editor they never create.
    if selected.is_none() && message.tool.is_some() {
        let row = controller
            .borrow()
            .messages
            .get(&anchor.message_id)
            .cloned();
        if let Some(row) = row
            && model
                .ui
                .get_untracked()
                .inspection_rect(row.id())
                .is_some_and(|rect| rect.size.height > 0.0)
        {
            row.reveal();
            model.source_anchor.set(None);
            controller.borrow_mut().source_search = false;
            if let Some(window) = controller.borrow().window.clone() {
                window.requested.set(None);
            }
            return;
        }
    }
    if let Some((offset, field, editor)) = selected {
        if !model
            .ui
            .get_untracked()
            .inspection_rect(field.id())
            .is_some_and(|rect| rect.size.height > 0.0)
        {
            if model.source_anchor.get_untracked().as_ref() != Some(anchor) {
                model.source_anchor.set(Some(anchor.clone()));
            }
            mount_history_message(controller, &anchor.message_id);
            return;
        }
        if focus {
            let fonts = model.ui.get_untracked().fonts();
            editor.borrow_mut().set_selection_bytes(
                &mut fonts.borrow_mut(),
                anchor.start - offset,
                anchor.end - offset,
            );
            field.focus();
        }
        field.reveal();
        field.paint_dirty();
        model.source_anchor.set(None);
        controller.borrow_mut().source_search = false;
        if let Some(window) = controller.borrow().window.clone() {
            window.requested.set(None);
        }
    } else {
        if model.source_anchor.get_untracked().as_ref() != Some(anchor) {
            model.source_anchor.set(Some(anchor.clone()));
        }
        mount_history_message(controller, &anchor.message_id);
    }
}

fn mount_history_message(controller: &Shared, id: &str) {
    let state = controller.borrow();
    let (Some(index), Some(window), Some(viewport)) =
        (state.index, state.window.clone(), state.viewport.clone())
    else {
        return;
    };
    let top = index.with_untracked(|index| {
        let position = index.order.iter().position(|key| key == id)?;
        Some(
            index.order[..position]
                .iter()
                .map(|key| window.height(key))
                .sum::<f32>(),
        )
    });
    drop(state);
    if let Some(top) = top {
        if window.requested.get_untracked().as_deref() != Some(id) {
            window.requested.set(Some(id.to_owned()));
        }
        let (_, _, origin) = window.geometry.get_untracked();
        viewport.scroll_to(Vector2::new(0.0, top + origin));
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
    let index = transcript_index(model);
    controller.get_untracked().borrow_mut().index = Some(index);
    let message_keys = Derived::new(move || index.with(|index| index.order.clone()));
    let history_window = HistoryWindow::new();
    controller.get_untracked().borrow_mut().window = Some(history_window.clone());
    let projection_window = history_window.clone();
    let history_assets = Derived::new(move || {
        index.with(|index| {
            index
                .rows
                .values()
                .flat_map(|entry| preview_assets(&entry.message.parts, false))
                .collect::<Vec<_>>()
        })
    });
    Effect::new(move || {
        let next = index.get();
        let session = model.session.get();
        mosaic::core::reactive::untracked(|| {
            let draft = buffer::parts(model, &session);
            let assets = history_assets
                .get_untracked()
                .into_iter()
                .chain(preview_assets(&draft, true))
                .collect::<Vec<_>>();
            controller
                .get_untracked()
                .borrow_mut()
                .prune_history_state(&session, &next, &assets);
            projection_window.reconcile(next);
        });
    });
    Effect::new(move || {
        let assets = history_assets
            .get()
            .into_iter()
            .chain(preview_assets(
                &buffer::parts(model, &model.session.get()),
                true,
            ))
            .collect::<Vec<_>>();
        controller
            .get_untracked()
            .borrow_mut()
            .asset_previews
            .retain(|(id, part, _), _| {
                assets
                    .iter()
                    .any(|(asset, current)| asset == id && current == part)
            });
    });
    let window_slots = {
        let window = controller
            .get_untracked()
            .borrow()
            .window
            .clone()
            .expect("history window");
        Derived::new(move || {
            window.slots(
                &message_keys.get(),
                &pinned_history(&controller.get_untracked(), &model.ui.get_untracked()),
            )
        })
    };
    let reveal_controller = controller.get_untracked();
    let reveal_window = history_window.clone();
    Effect::new(move || {
        let anchor = model.source_anchor.get();
        let remote = model.remote_search.get();
        let focused = model.focused_message.get();
        message_keys.get();
        reveal_window.revision.get();
        mosaic::core::reactive::untracked(|| {
            if let Some(anchor) = anchor {
                let focus = !reveal_controller.borrow().source_search;
                go_to_source(model, &reveal_controller, &anchor, focus);
            } else if remote.as_ref() == Some(&focused) && !focused.is_empty() {
                let target = reveal_controller
                    .borrow()
                    .messages
                    .get(&focused)
                    .cloned()
                    .filter(|field| {
                        model
                            .ui
                            .get_untracked()
                            .inspection_rect(field.id())
                            .is_some_and(|rect| rect.size.height > 0.0)
                    });
                if let Some(field) = target {
                    reveal_controller.borrow_mut().history_anchor = None;
                    field.reveal();
                    reveal_window.requested.set(None);
                    model.focused_message.set(String::new());
                } else {
                    mount_history_message(&reveal_controller, &focused);
                }
            }
        });
    });
    let mounted_controller = controller.get_untracked();
    model
        .conversation_controller
        .set(Some(mounted_controller.clone()));
    mosaic::core::reactive::on_cleanup(move || {
        if model
            .conversation_controller
            .get_untracked()
            .is_some_and(|current| Rc::ptr_eq(&current, &mounted_controller))
        {
            model.conversation_controller.set(None);
        }
    });
    let empty = State::new(uuid::Uuid::new_v4().to_string());
    controller.get_untracked().borrow_mut().empty_draft = empty.get_untracked();
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
            // A newly opened session starts at its latest messages.
            documents.manual_reading = false;
            documents.open_at_end = true;
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
            if state.manual_reading {
                return;
            }
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
                            && ((w.thread_id.is_none()
                                && s.conversations.is_empty()
                                && !s.awaiting_first_message)
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
    let editor = view! {
        col height:min-content min-height:{px(48.0)}px gap:{px(8.0)}px label:"Next message" {
            if discovery_feedback.get().is_some() && model.command_feedback.get().is_empty() {
                col height:min-content gap:{px(4.0)}px label:"Command discovery status" {
                    text font-size:{px(12.0)}px {discovery_feedback.get().unwrap_or_default()}
                    if model.catalogs.get().get(&model.session.get()).is_some_and(|c| c.is_err()) {
                        row width:{px(120.0)}px height:min-content {
                            { parent.adopt(&action_cell("Retry command discovery", "Retry", || false, move || crate::command_ui::refresh(model, true))); }
                        }
                    }
                }
            }
            col height:min-content min-width:0px gap:{px(6.0)}px {
                for (_, part) in {
                    composer_parts(buffer::parts(model,&model.session.get()), &empty.get()).into_iter().map(|p|(p.id.clone(),p)).collect::<Vec<_>>()
                } {
                    col height:min-content {
                        DraftPart model:(model) controller:(controller) part:(part.clone())
                            mirror:false
                    }
                }
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
    // Progress hints show at full strength; the idle key hint recedes.
    let hint_feedback = Derived::new(move || {
        let state = model.buffer.get();
        !state.uploads.is_empty()
            || state
                .documents
                .get(&model.session.get())
                .is_some_and(|doc| {
                    doc.finalize.is_some()
                        || doc.submitting
                        || doc.saving.is_some()
                        || !state.connected
                })
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
                .map(|s| s.agent_config_id.clone())
                .unwrap_or_else(|| model.worker_director.get_untracked());
            model.open_profile(EditTarget::Config(director));
        }),
        change_model: Rc::new(move || crate::command_ui::open(model, "model")),
        set_effort: Rc::new(move |step| choose_effort(model, step)),
        toggle: Rc::new(move || ledger_open.set(!ledger_open.get_untracked())),
    };
    // Filled once the composer is built; pressing Send focuses its button.
    let composer_element = Rc::new(RefCell::new(None::<Element>));
    let send_composer = composer_element.clone();
    let send_controller = controller.get_untracked();
    let send: relay_ui::prelude::OnPress = Rc::new(move || {
        let focused = send_composer
            .borrow()
            .as_ref()
            .is_some_and(|composer| composer.interaction().focus_within());
        send_keeping_focus(model, &send_controller, focused);
    });
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
        crate::adapt::session_fingerprint(
            &model.snapshot.get().messages,
            &model.history.get(),
            &model.session.get(),
        )
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
    let thinking_summary = Derived::new(move || {
        let session = model.session.get();
        crate::transcript::progress_summary(
            &model.snapshot.get(),
            &model.history.get(),
            &session,
            model.active_run_ids.get().get(&session).map(String::as_str),
        )
    });
    let thinking_run = Derived::new(move || {
        let session = model.session.get();
        format!(
            "{session}/{}",
            model
                .active_run_ids
                .get()
                .get(&session)
                .map_or("", String::as_str)
        )
    });
    let thinking_hue = Derived::new(move || ledger.get().model.hue);
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
    let inline_composer = view! {
            row height:min-content shrink:0
                pad:(left:{px(if compact.get() {12.0} else {28.0})}px right:{px(if compact.get() {12.0} else {36.0})}px bottom:{px(20.0)}px)
                justify:center {
                col width:1fr max-width:{px(if compact.get() {848.0} else {994.0})}px height:min-content
                    pad:(right:{px(6.0)}px) gap:0px label:"Inline composer" {
            if model.buffer.get().approval_needed {
                BoundedPanel limit:(auxiliary_limit) {
                    grid height:min-content gap:0px align:center
                        stroke:(width:{px(1.0)} color:ground.hairline edges:top)
                        cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0), GridTrack::MaxContent])}} {
                        row height:min-content min-width:0px
                            pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px) {
                            Label
                                text:"This agent’s profile requires approval to implement this turn."
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                        { parent.adopt(&action_cell("Approve this turn and send", "Approve and send", || false, move || { model.worker_approval.set(true); buffer::send(model); })); }
                    }
                }
            }
            if recovery.get() {
                BoundedPanel limit:(auxiliary_limit) {
                    col height:min-content gap:0px
                        stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                        row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px) {
                            Label
                                text:(Derived::new(move || model.buffer.get().documents.get(&model.session.get()).map(|d| if d.error.is_empty() {"Recovered draft available".into()} else {d.error.clone()}).unwrap_or_default()))
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                        grid height:min-content gap:0px
                            cols:{GridTracks::auto_fit(GridTrack::minmax(px(90.0).into(), GridTrack::fr(1.0)))} {
                            {
                                parent.adopt(&action_cell("Load shared draft", "Load shared", || false, move || buffer::resolve(model, false)));
                                parent.adopt(&action_cell("Restore local draft", "Restore local", || false, move || buffer::resolve(model, true)));
                                parent.adopt(&action_cell("Retry draft save", "Retry", || false, move || buffer::retry_save(model)));
                            }
                        }
                    }
                }
            }
                    if !model.command_flow.get().is_empty() {
                        col height:min-content shrink:0 fill:ground.panel
                            stroke:(width:{px(1.0)} color:ground.rule edges:(top left right))
                            label:"Harness command panel" {
                            // Each step starts at the top with its own focus entry point.
                            for (_, _) in {vec![((model.command_flow.get(), model.command_model.get()), ())]} {
                                BoundedPanel limit:(auxiliary_limit) {
                                    CommandPanel model:(model)
                                }
                            }
                        }
                    }
                    if model.completion.get().is_some() {
                        col height:min-content shrink:0 fill:ground.panel
                            stroke:(width:{px(1.0)} color:ground.rule edges:(top left right))
                            label:"Command suggestions panel" {
                            BoundedPanel limit:(auxiliary_limit) {
                                col height:min-content shrink:0 gap:0px
                                    label:"Inline suggestions" {
                                    // Key by content, not position: filtering must replace the displayed row.
                                    for (key, choice) in {model.completion.get().map(|c|c.choices.into_iter().map(|choice|((choice.label.clone(),choice.description.clone()),choice)).collect::<Vec<_>>()).unwrap_or_default()} {
                                        let picked_label=key.0.clone();
                                        let choice=State::new(choice.clone());
                                        row height:{px(32.0)}px min-width:0px shrink:0
                                            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                                            {
                                                let picked = picked_label.clone();
                                                let current = Derived::new(move || model.completion.get().is_some_and(|c| c.choices.get(c.index).is_some_and(|c| c.label == choice.get().label)));
                                                parent.adopt(&view! {
                                                    CellButton label:(Derived::new(move || choice.get().label)) start:true selected:(current)
                                                        horizontal-inset:(12.0)
                                                        on-press:(press(move || if let Some(index) = model.completion.get_untracked().and_then(|c| c.choices.iter().position(|c| c.label == picked)) { accept_completion(model, &controller.get_untracked(), index, false); })) {
                                                        Ellipsis text:(Derived::new(move || format!("{} · {}", choice.get().label, choice.get().description.split_whitespace().collect::<Vec<_>>().join(" "))))
                                                            role:(TextRole::Label)
                                                    }
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !model.command_feedback.get().is_empty() {
                        row height:{px(32.0)}px min-width:0px shrink:0
                            fill:ground.panel pad:(horizontal:{px(12.0)}px vertical:0px) align:center
                            stroke:(width:{px(1.0)} color:ground.rule edges:(top left right))
                            label:"Command feedback" {
                            Ellipsis text:(model.command_feedback) role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    }

                    Composer inline:true stacked:(relay_ui::Live::from(compact)) settings:(relay_ui::Live::from(ledger)) ledger-actions:(ledger_actions)
                        gutter-width:(relay_ui::Live::fixed(212.0))
                        disabled:(relay_ui::Live::from(Derived::new(move || session.get().is_some_and(|s| s.archived))))
                        pending:(relay_ui::Live::from(Derived::new(move || model.submission_pending())))
                        on-send:(send) on-attach:(attach) editor:(Some(editor))
                        has-content:(Some(relay_ui::Live::from(has_content)))
                        hint:(relay_ui::Live::from(hint))
                        hint-feedback:(relay_ui::Live::from(hint_feedback)) as composer
                    { composer.label("Message composer"); *composer_element.borrow_mut() = Some(composer.clone()); }
                }
            }
    };
    let root = view! {
        col width:1fr gap:0px
            @layout:{move |rect: Rect| { doc_width.set(rect.size.width); doc_height.set(rect.size.height); }} {
            SessionHead model:(model) menu:(menu) compact:(compact)
            if !tools.get().is_empty() {
                FingerprintStrip tools:(relay_ui::Live::from(tools))
            }
            Notice model:(model)
            if menu.get() {
                BoundedPanel limit:(auxiliary_limit) {
                    col height:min-content gap:0px shrink:0
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        if compact.get() && session.get().and_then(|s|s.worker).is_some_and(|w|matches!(w.status,WorkerStatus::Running|WorkerStatus::Queued)) {
                            { parent.adopt(&action_cell("Stop worker", "Stop worker", move || !model.connected.get() || model.busy.get(), move || model.stop_worker())); }
                        }
                        if session.get().is_some_and(|s| model.snapshot.get().descendants(&s.id).iter().any(|d| d.worker.as_ref().is_some_and(|w| matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued)))) {
                            { parent.adopt(&action_cell("Stop this agent and all its subagents", "Stop all", move || !model.connected.get() || model.busy.get(), move || model.stop_all())); }
                        }
                        grid height:min-content gap:0px
                            stroke:(width:{px(1.0)} color:ground.hairline edges:top offset:{px(-1.0)})
                            cols:{GridTracks::new(vec![GridTrack::fr(1.0); if compact.get() { 3 } else if cfg!(target_arch = "wasm32") { 6 } else { 5 }])} {
                            if cfg!(target_arch = "wasm32") {
                                { parent.adopt(&icon_action_cell("Add files to draft", "Add files", relay_ui::icons::call.create, || false, move || crate::platform::pick_files(model))); }
                            }
                            {
                                let none = move || session.get().is_none();
                                parent.adopt(&icon_action_cell("Linked issue", "Issue", relay_ui::icons::command.board, none, move || model.open_worker_issue()));
                                parent.adopt(&icon_action_cell("Root agent profile", "Profile", relay_ui::icons::command.profile, || false, move || model.open_profile(EditTarget::Config(session.get_untracked().map(|s| s.agent_config_id).unwrap_or_else(|| model.worker_director.get_untracked())))));
                                parent.adopt(&icon_action_cell("Toggle change review", "Changes", relay_ui::icons::call.edit, none, move || {
                                    if details.get_untracked() != "changes" {
                                        model.request_history(relay_core::StreamRequest::Review { session_id: model.session.get_untracked() });
                                    }
                                    details.set(if details.get_untracked() == "changes" { String::new() } else { "changes".into() });
                                }));
                                parent.adopt(&icon_action_cell("Session usage and provenance", "Details", relay_ui::icons::call.result, none, move || details.set(if details.get_untracked() == "usage" { String::new() } else { "usage".into() })));
                                parent.adopt(&icon_action_cell("Search transcript", "Find", relay_ui::icons::command.search, none, move || { model.searching.set(true); menu.set(false); }));
                            }
                        }
                        row height:{px(36.0)}px {
                            CellButton label:"Toggle chat archive"
                                disabled:(Derived::new(move || !model.connected.get() || model.busy.get()))
                                on-press:(press(move || {
                                if let Some(s) = session.get_untracked() { model.submit(Command::SetSessionArchived { session_id: s.id, archived: !s.archived }, model.snapshot.get_untracked().revision, crate::model::Saved::Action); }
                            })) {
                                { parent.adopt(&relay_ui::icons::icon_mark(if session.get().is_some_and(|s| s.archived) { relay_ui::icons::command.sync } else { relay_ui::icons::mark.archive }, 14.0)); }
                                Label
                                    text:(Derived::new(move || if session.get().is_some_and(|s| s.archived) { "Unarchive chat".to_string() } else { "Archive chat".to_string() }))
                                    role:(TextRole::Label)
                            }
                        }
                        if session.get().is_some_and(|s| !s.fixture && s.worker.is_some()) {
                            let current = session.get_untracked().unwrap();
                            let session_id = current.id.clone();
                            let snapshot = model.snapshot.get();
                            let inherited = snapshot.session_profile(&current).ok().map(|p|p.execution.approval).unwrap_or_default();
                            let worker = current.worker.unwrap();
                            row height:min-content
                                pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px) {
                                Label
                                    text:(format!("{} · {} · {}",match worker.harness{Harness::Codex=>"Codex",Harness::ClaudeCode=>"Claude Code"},worker.execution.as_ref().map(|e|e.approval).unwrap_or(inherited).label(),if worker.execution.is_some(){"Worker override"}else{"Inherited from director"}))
                                    role:(TextRole::Caption) tone:(Tone::Muted)
                            }
                            Tooltip text:"Execution changes apply to the next turn."
                                width:(Dimension::Fill) {
                                grid height:min-content gap:0px align:center
                                    stroke:(width:{px(1.0)} color:ground.hairline edges:top)
                                    cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0),GridTrack::MaxContent])}} {
                                    let mode_session = session_id.clone();
                                    let mode_index = Derived::new(move || {
                                        let snapshot = model.snapshot.get();
                                        let session = snapshot.sessions.iter().find(|s| s.id == mode_session);
                                        let inherited = session.and_then(|s| snapshot.session_profile(s).ok()).map(|p| p.execution.approval).unwrap_or_default();
                                        let mode = session.and_then(|s| s.worker.as_ref()).and_then(|w| w.execution.as_ref()).map(|e| e.approval).unwrap_or(inherited);
                                        ApprovalMode::ALL.iter().position(|m| *m == mode).unwrap_or(0)
                                    });
                                    let choose_session = session_id.clone();
                                    let choose: Rc<dyn Fn(usize)> = Rc::new(move |slot: usize| {
                                        model.submit(Command::SetWorkerExecution{session_id:choose_session.clone(),execution:Some(ExecutionSettings{approval:ApprovalMode::ALL[slot]})},model.snapshot.get_untracked().revision,crate::model::Saved::Action);
                                    });
                                    row height:{px(40.0)}px min-width:0px {
                                        Segments name:("Worker approval".to_string())
                                            options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                                            icons:(crate::controls::approval_icons())
                                            tones:(crate::controls::approval_tones())
                                            selected:(relay_ui::Live::from(mode_index))
                                            on-select:(choose)
                                            disabled:(relay_ui::Live::from(Derived::new(move || !model.connected.get() || model.busy.get())))
                                    }
                                    {
                                        let inherit_session = session_id.clone();
                                        parent.adopt(&icon_action_cell("Inherit worker execution settings", "Inherit", relay_ui::icons::command.sync, move || !model.connected.get() || model.busy.get(), move || model.submit(Command::SetWorkerExecution { session_id: inherit_session.clone(), execution: None }, model.snapshot.get_untracked().revision, crate::model::Saved::Action)));
                                    }
                                }
                            }
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
                    if session.get().is_some_and(|s|s.worker.as_ref().is_some_and(|w|!matches!(w.status,WorkerStatus::Queued|WorkerStatus::Running) && ((w.thread_id.is_none() && s.conversations.is_empty() && !s.awaiting_first_message) || w.worktree.is_none()))) {
                        grid height:min-content gap:{px(8.0)}px
                            cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0), GridTrack::MaxContent])}}
                            pad:(horizontal:{px(24.0)}px vertical:0px) shrink:0 {
                            text width:1fr label:"Worker cannot continue" font-size:{px(12.0)}px
                                {session.get().and_then(|s|s.worker).and_then(|w|w.error).unwrap_or_else(||"No resumable thread was recorded".into())}
                            { parent.adopt(&action_cell("Start new worker from linked issue", "Open issue", || false, move || model.open_worker_issue())); }
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
                                Label text:(Derived::new(move || block.get().title))
                                    role:(TextRole::Label) weight:600
                                grid
                                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(160.0).into(), GridTrack::fr(1.0)))}
                                    height:min-content gap:{px(10.0)}px {
                                    for (_, field) in {block.get().rows.into_iter().map(|r| (r.0, r.0)).collect::<Vec<_>>()} {
                                        let name: &'static str = field;
                                        Readout key:(name.to_string())
                                            value:(Derived::new(move || block.get().rows.into_iter().find(|r| r.0 == name).map(|r| r.1).unwrap_or_default()))
                                    }
                                }
                                if block.get().note.is_some() {
                                    row height:min-content font-size:{px(12.0)}px {
                                        text {block.get().note.unwrap_or_default()}
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
                row height:{px(40.0)}px shrink:0 align:center
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    row width:1fr min-width:0px {
                        {
                            let field = view! {
                                TextInput value:(model.search) label:"Find in conversation"
                                    placeholder:"Find in conversation…" height:(Some(40.0)) framed:false
                            };
                            parent.adopt(&field);
                            field.focus();
                        }
                    }
                    {
                        for (label, text, step) in [("Previous search match", "↑", -1), ("Next search match", "↓", 1)] {
                            parent.adopt(&view! {
                                row width:{px(40.0)}px height:fill shrink:0 {
                                    CellButton label:(label) rule:(Rule::Left) horizontal-inset:(0.0)
                                        on-press:(press(move || find_next(model, &controller.get_untracked(), step))) {
                                        Label text:(text) role:(TextRole::Label)
                                    }
                                }
                            });
                        }
                        parent.adopt(&view! {
                            row width:{px(40.0)}px height:fill shrink:0 {
                                CellButton label:"Close search" rule:(Rule::Left) horizontal-inset:(0.0)
                                    on-press:(press(move || { model.searching.set(false); model.search.set(String::new()); })) {
                                    Label text:"×" role:(TextRole::Label)
                                }
                            }
                        });
                    }
                }
            }
            scroll {
                col height:min-content {
                    row height:min-content justify:center
                        pad:(left:{px(if compact.get() {12.0} else {28.0})}px right:{px(if compact.get() {12.0} else {36.0})}px top:{px(28.0)}px bottom:{px(24.0)}px) {
                        col width:1fr max-width:{px(if compact.get() {848.0} else {994.0})}px
                            height:min-content
                            pad:(left:{px(if compact.get() {0.0} else {84.0})}px right:{px(if compact.get() {0.0} else {62.0})}px) {
                            col width:1fr max-width:{px(848.0)}px height:min-content
                                gap:{px(28.0)}px label:"Transcript" {
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
                                            row width:{px(100.0)}px height:min-content {
                                                { parent.adopt(&action_cell("Retry loading history", "Retry", || false, move || model.retry_history())); }
                                            }
                                        }
                                    }
                                }
                                if model.snapshot.get().messages.iter().any(|m|m.session_id==model.session.get()) {
                                    col height:min-content min-width:0px gap:0px {
                                        {
                                            parent.keyed(
                                                move || window_slots.get().into_iter().map(|slot| (slot.key, ())),
                                                move |parent, key, ()| {
                                                    if key.0 {
                                                        parent.adopt(&view! {
                                                            TranscriptMessage model:(model) message-id:(key.1.clone()) controller:(controller)
                                                        });
                                                    } else {
                                                        let gap_key = State::new(key.clone());
                                                        parent.adopt(&view! {
                                                            el height:{window_slots.with(|slots| slots.iter().find(|s| s.key == gap_key.get()).map_or(0.0, |s| s.height))}px
                                                                shrink:0 nohit {}
                                                        });
                                                    }
                                                },
                                            );
                                        }
                                    } as history_content
                                    {
                                        let window = controller.get_untracked().borrow().window.clone().expect("history window");
                                        let content = history_content.clone();
                                        let previous_width = Rc::new(Cell::new(0.0f32));
                                        history_content.on_layout(move |rect| {
                                            let old_width = previous_width.replace(rect.size.width);
                                            if (old_width - rect.size.width).abs() > 0.5 {
                                                // Retain a reading anchor while all the old wrap
                                                // measurements are replaced with estimates.
                                                let shared = controller.get_untracked();
                                                let mut state = shared.borrow_mut();
                                                if old_width > 0.0 && state.history_anchor.is_none()
                                                    && let Some(viewport) = &state.viewport {
                                                    let root = viewport.root().layout_rect();
                                                    if viewport.offset().y < viewport.content().layout_rect().size.height - root.size.height - px(4.0) {
                                                        state.history_anchor = state.messages.iter().map(|(id, row)| (id, row.layout_rect()))
                                                            .filter(|(_, rect)| rect.size.height > 0.0 && rect.max_y() > root.origin.y && rect.origin.y < root.max_y())
                                                            .min_by(|a, b| a.1.origin.y.total_cmp(&b.1.origin.y))
                                                            .map(|(id, rect)| (id.clone(), rect.origin.y));
                                                    }
                                                }
                                                drop(state);
                                                window.heights.borrow_mut().clear();
                                                window.changed();
                                            }
                                            let viewport = controller.get_untracked().borrow().viewport.clone();
                                            if let Some(viewport) = viewport {
                                                let origin = content.layout_rect().origin.y - viewport.content().layout_rect().origin.y;
                                                let (offset, height, _) = window.geometry.get_untracked();
                                                window.geometry.set((offset, height, origin));
                                            }
                                        });
                                    }
                                }
                                if session.get().is_some_and(|s| s.archived) {
                                    col height:min-content label:"Archived chat message" {
                                        MessageRow speaker:"Relay"
                                            icon:(Some(relay_ui::icons::mark.relay)) {
                                            AgentText {
                                                Label
                                                    text:"This chat has been archived. Unarchive it to continue."
                                            }
                                        }
                                    }
                                }
                                if thinking.get() {
                                    ThinkingLine effort:(relay_ui::Live::from(thinking_level))
                                        summary:(relay_ui::Live::from(thinking_summary))
                                        run-key:(relay_ui::Live::from(thinking_run))
                                        on-toggle:(Rc::new(move || {
                                            // A disclosure is a reading action; pointer-down may
                                            // otherwise resume automatic caret/bottom following, and
                                            // the toggled control stays put while it grows.
                                            let shared = controller.get_untracked();
                                            let mut state = shared.borrow_mut();
                                            state.manual_reading = true;
                                            state.hold_reading = true;
                                        }) as relay_ui::prelude::OnPress)
                                        hue:(relay_ui::Live::from(thinking_hue))
                                }
                                for (_, permission) in {approvals.get().into_iter().map(|p|(p.id.clone(),p)).collect::<Vec<_>>()} {
                                    col height:min-content {
                                        PendingApproval model:(model)
                                            permission:(permission.clone())
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
                                        row width:{px(160.0)}px height:min-content {
                                            { parent.adopt(&action_cell("Resume paused queue", "Resume queue", || false, move || model.submit(Command::ResumeQueue { session_id: model.session.get_untracked() }, model.snapshot.get_untracked().revision, crate::model::Saved::Action))); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    el width:fill height:{px(1.0)}px fill:ground.rule label:"Draft boundary"
                        nohit {}
                    { parent.adopt(&inline_composer); }
                }
            } as document_scroll
            {
                document_scroll.root().label("Transcript viewport");
                document_scroll.root().on_layout(move |rect| viewport_size.set(rect.size));
                controller.get_untracked().borrow_mut().viewport=Some(document_scroll.clone());
                controller.get_untracked().borrow_mut().draft_viewport = Some(document_scroll.clone());
                let window = controller.get_untracked().borrow().window.clone().expect("history window");
                let window_scroll = document_scroll.clone();
                Effect::new(move || {
                    let offset = window_scroll.offset();
                    let size = viewport_size.get();
                    mosaic::core::reactive::untracked(|| {
                        let (_, _, origin) = window.geometry.get_untracked();
                        window.geometry.set((offset.y, size.height, origin));
                    });
                });
                let gesture_controller=controller.get_untracked();
                document_scroll.root().on_pointer(move |event,_| {
                    match event.kind {
                        PointerEventKind::Wheel { .. } => gesture_controller.borrow_mut().manual_reading = true,
                        PointerEventKind::Down(_) => gesture_controller.borrow_mut().manual_reading = false,
                        PointerEventKind::Move if event.pointer_type == PointerType::Touch => gesture_controller.borrow_mut().manual_reading = true,
                        _ => {}
                    }
                    if matches!(event.kind,PointerEventKind::Wheel{..}|PointerEventKind::Down(_)) || (event.pointer_type==PointerType::Touch && matches!(event.kind,PointerEventKind::Move)) {let mut state=gesture_controller.borrow_mut();state.history_scroll_seen=true;state.history_anchor=None;}
                });
                let paging_scroll=document_scroll.clone();
                let paging_controller=controller.get_untracked();
                let paging_window=paging_controller.borrow().window.clone().expect("history window");
                Effect::new(move || {
                    let offset=paging_scroll.offset();
                    paging_window.revision.get();
                    mosaic::core::reactive::untracked(|| {
                        if !paging_controller.borrow().history_scroll_seen || model.transport_requests.get_untracked().is_none() || model.history_loading.get_untracked() {return;}
                        let root=paging_scroll.root().layout_rect();
                        if offset.y<px(200.0) {
                            if !model.history.get_untracked().has_more {return;}
                            let ui=model.ui.get_untracked();
                            let anchor=paging_controller.borrow().messages.iter().filter_map(|(key,field)|ui.inspection_rect(field.id()).map(|rect|(key.clone(),rect))).filter(|(_,rect)|rect.size.height>0.0 && rect.max_y()>=root.origin.y && rect.origin.y<root.max_y()).min_by(|(_,a),(_,b)|a.origin.y.total_cmp(&b.origin.y)).map(|(key,rect)|(key,rect.origin.y));
                            if anchor.is_none() {return;}
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
                    let extent_changed = previous.get().is_none_or(|(content, viewport)| (content - rect.size.height).abs() > 0.5 || (viewport - height).abs() > 0.5);
                    let focused = layout_controller.borrow().surfaces.iter().find(|(_, (field, _))| field.interaction().focused()).map(|(key, (field, _))| (key.starts_with("draft-"), field.clone()));
                    // Revealing only the caret can leave the rest of the input
                    // below the viewport, so draft focus must preserve bottom-follow.
                    let focused_draft = focused.as_ref().is_some_and(|(draft, _)| *draft);
                    // Pinning follows where the offset rests, not the gesture
                    // that moved it: a reader who scrolls back to the end
                    // follows again. A newly opened session starts there.
                    let opened = std::mem::take(&mut layout_controller.borrow_mut().open_at_end);
                    let held = extent_changed && std::mem::take(&mut layout_controller.borrow_mut().hold_reading);
                    let was_at_bottom = opened || (!held && previous.get().is_none_or(|(content, viewport)|
                        follow_scroll.offset().y >= (content - viewport).max(0.0) - px(4.0)));
                    let revealing = model.source_anchor.get_untracked().is_some();
                    let follow_caret = !layout_controller.borrow().manual_reading && !revealing
                        && ((focused_draft && (was_at_bottom || !model.command_flow.get_untracked().is_empty() || model.completion.get_untracked().is_some())) || (!focused_draft && focused.as_ref().is_some_and(|(_, field)| {
                            let field = field.layout_rect(); let viewport = follow_scroll.root().layout_rect();
                            field.max_y() > viewport.origin.y && field.origin.y < viewport.max_y()
                        })));
                    if extent_changed && follow_caret && let Some((_, field)) = &focused { layout_controller.borrow_mut().resize_follow = true; field.paint_dirty(); }
                    previous.set(Some((rect.size.height, height)));
                    if extent_changed && focused.is_some() && follow_caret
                        && let Some(caret) = model.ui.get_untracked().ime_cursor_area() {
                        follow_scroll.reveal(caret_with_margin(Vector2::ZERO, caret));
                    }
                    let anchor=if !model.history_loading.get_untracked() {layout_controller.borrow().history_anchor.clone()} else {None};
                    if let Some((key,y))=anchor {
                        if let Some(field)=layout_controller.borrow().messages.get(&key) && let Some(rect)=model.ui.get_untracked().inspection_rect(field.id()) {
                            follow_scroll.scroll_to(Vector2::new(0.0,follow_scroll.offset().y+rect.origin.y-y));
                        }
                    } else if was_at_bottom && (focused.is_none() || focused_draft) && !model.history.get_untracked().has_newer {
                        follow_scroll.scroll_to(Vector2::new(0.0, (rect.size.height - height).max(0.0)));
                    }
                    if let Some(anchor)=model.source_anchor.get_untracked()
                        && !model.history.get_untracked().preview_ids.contains(&anchor.message_id)
                        && model.snapshot.get_untracked().messages.iter().any(|m|m.id==anchor.message_id)
                    {
                        let focus = !layout_controller.borrow().source_search;
                        go_to_source(model,&layout_controller,&anchor,focus);
                    } else if let Some(id)=model.remote_search.get_untracked() {
                        let target=layout_controller.borrow().messages.get(&id).filter(|field|model.ui.get_untracked().inspection_rect(field.id()).is_some()).cloned();
                        if model.focused_message.get_untracked()==id {
                            if let Some(field)=target {layout_controller.borrow_mut().history_anchor=None;field.reveal();model.focused_message.set(String::new());}
                            else {mount_history_message(&layout_controller, &id);}
                        }
                    }
                });
            }
        }
    };
    let cleanup_controller = controller.get_untracked();
    mosaic::core::reactive::on_cleanup(move || {
        cleanup_controller.borrow_mut().viewport = None;
    });
    let keys_controller = controller.get_untracked();
    let restore_controller = keys_controller.clone();
    let was_open = Rc::new(Cell::new(false));
    Effect::new(move || {
        let open = !model.command_flow.get().is_empty();
        if was_open.replace(open)
            && !open
            && let Some((_, (field, _))) = restore_controller
                .borrow()
                .surfaces
                .iter()
                .find(|(key, _)| key.starts_with("draft-"))
        {
            let restore = field.clone();
            field.animate(move |_| {
                restore.reveal();
                restore.focus();
                mosaic::widgets::Tick::Done
            });
        }
    });
    root.on_key(move |event, ctx| {
        keys_controller.borrow_mut().history_anchor = None;
        if matches!(event.kind, KeyEventKind::Down { .. }) && event.key == Key::Escape {
            model.command_feedback.set(String::new());
            model.notice.set(String::new());
        }
        if matches!(event.kind, KeyEventKind::Down { .. })
            && event.key == Key::Escape
            && !model.command_flow.get_untracked().is_empty()
        {
            model.command_flow.set(String::new());
            ctx.stop_propagation();
        }
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
    let index = controller
        .get_untracked()
        .borrow()
        .index
        .unwrap_or_else(|| transcript_index(model));
    let entry =
        Derived::new(move || index.with(|index| index.rows.get(&message_id.get()).cloned()));
    let spacing =
        Derived::new(move || entry.with(|entry| entry.as_ref().map_or(0.0, |entry| entry.spacing)));
    let message =
        Derived::new(move || entry.with(|entry| entry.as_ref().map(|entry| entry.message.clone())));
    let expanded = State::new(
        controller
            .get_untracked()
            .borrow()
            .expanded_rows
            .get(&message_id.get_untracked())
            .copied()
            .unwrap_or(false),
    );
    Effect::new(move || {
        if model
            .source_anchor
            .get()
            .is_some_and(|anchor| anchor.message_id == message_id.get_untracked())
        {
            expanded.set(true);
        }
    });
    Effect::new(move || {
        let value = expanded.get();
        let shared = controller.get_untracked();
        let mut state = shared.borrow_mut();
        let id = message_id.get_untracked();
        if state.expanded_rows.insert(id.clone(), value) != Some(value) {
            state.invalidate_history_height(&id);
        }
    });
    let initial = message.get_untracked();
    let detail = initial.as_ref().is_some_and(|m| {
        matches!(
            m.kind.as_str(),
            "issue-context" | "command_execution" | "file_change" | "reasoning"
        )
    });
    let prompt = initial.as_ref().is_some_and(|m| m.kind == "prompt");
    let tool = initial.as_ref().is_some_and(|m| m.tool.is_some());
    let continues = move || entry.with(|entry| entry.as_ref().is_some_and(|entry| entry.continues));
    let speaker = Derived::new(move || {
        entry.with(|entry| {
            entry
                .as_ref()
                .map(|entry| entry.speaker.clone())
                .unwrap_or_default()
        })
    });
    // Agent-sent prompts link to their sender's session.
    let on_speaker = entry
        .with_untracked(|entry| {
            entry
                .as_ref()
                .and_then(|entry| entry.speaker_session.clone())
        })
        .map(|_| -> relay_ui::prelude::OnPress {
            Rc::new(move || {
                if let Some(id) = entry.with_untracked(|entry| {
                    entry
                        .as_ref()
                        .and_then(|entry| entry.speaker_session.clone())
                }) {
                    model.open_session(id);
                }
            })
        });
    // Relay's own messages (the context it sends with each turn) carry the
    // Relay mark rather than the session's harness glyph.
    let from_relay = initial.as_ref().is_some_and(|m| m.author == "Relay");
    let speaker_icon = (from_relay && !mosaic::core::reactive::untracked(continues))
        .then_some(relay_ui::icons::mark.relay);
    // Agent messages carry their session's harness glyph when it is known.
    let harness = (!prompt && !from_relay && !mosaic::core::reactive::untracked(continues))
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
                    Label text:"Message Preview" role:(TextRole::Caption) tone:(Tone::Muted)
                    LinkButton text:"Load full message"
                        on-press:(press(move || model.load_message(message_id.get_untracked())))
                }
            }
            if detail && message.get().is_none_or(|m|m.tool.is_none()) {
                col height:min-content {
                    row width:max-content height:{px(28.0)}px pad:(bottom:{px(8.0)}px) {
                        button @click:{expanded.set(!expanded.get_untracked())}
                            label:"Toggle tool activity" width:max-content height:fill radius:0px
                            align:center pad:(horizontal:{px(8.0)}px vertical:0px)
                            fill:{if expanded.get() { color(ground.selected) } else { Color::TRANSPARENT }}
                            stroke:(width:{px(1.0)} color:ground.rule-strong offset:{px(-1.0)})
                            hover { fill:ground.hover } pressed { fill:ground.pressed }
                            focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                            Label
                                text:(Derived::new(move || message.get().map(|m|match m.kind.as_str(){"issue-context"=>"Context Sent to Agent","file_change"=>"File Changes","reasoning"=>"Reasoning",_=>"Tool Activity"}.to_owned()).unwrap_or_default()))
                                role:(TextRole::Label) weight:500
                        }
                    }
                }
            }
            if message.get().is_none_or(|m|m.tool.is_none()) && (!detail || expanded.get()) {
                col height:min-content {
                    if message.get().is_some_and(|m|m.parts.is_empty()) {
                        col height:min-content {
                            BufferText model:(model) controller:(controller)
                                location:(Location::Source {message:message_id.get(),start:0,end:None,part:None})
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
            for (_, comment) in {entry.with(|entry| entry.as_ref().map(|entry| entry.comments.clone()).unwrap_or_default()).into_iter().map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                col height:min-content pad:(left:{px(14.0)}px top:{px(8.0)}px) gap:{px(5.0)}px
                    selectable stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                    Label text:(comment.author.clone()) role:(TextRole::Caption) tone:(Tone::Muted)
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
                col height:min-content min-width:0px gap:0px {
                    ToolMessage model:(model) message:(message)
                    { parent.adopt(&body); }
                }
            }
        }
    } else if prompt {
        view! {
            MessageRow speaker:(relay_ui::Live::from(speaker)) on-speaker:(on_speaker) {
                UserMessage {
                    { parent.adopt(&body); }
                }
            }
        }
    } else {
        view! {
            MessageRow speaker:(relay_ui::Live::from(speaker)) harness:(harness)
                icon:(speaker_icon) {
                AgentText {
                    { parent.adopt(&body); }
                }
            }
        }
    };
    // Anchor paging to the content, excluding spacing that can change when
    // earlier messages arrive.
    let content_anchor = row.clone();
    let root = view! {
        col height:min-content min-width:0px pad:(top:{px(spacing.get())}px) {
            { parent.adopt(&row); }
        }
    };
    let shared = controller.get_untracked();
    let id = message_id.get_untracked();
    if let Some(window) = shared.borrow().window.clone() {
        let measure_id = id.clone();
        let measure_controller = shared.clone();
        root.on_layout(move |rect| {
            let delta = rect.size.height - window.height(&measure_id);
            if delta.abs() > 0.5 {
                let state = measure_controller.borrow();
                let viewport = state.viewport.clone();
                let (_, _, origin) = window.geometry.get_untracked();
                let old_bottom = state.index.and_then(|index| {
                    index.with_untracked(|index| {
                        let position = index.order.iter().position(|id| id == &measure_id)?;
                        Some(
                            origin
                                + index.order[..=position]
                                    .iter()
                                    .map(|id| window.height(id))
                                    .sum::<f32>(),
                        )
                    })
                });
                let anchored = state.history_anchor.is_some();
                drop(state);
                window.report_height(&measure_id, rect.size.height);
                if !anchored && let Some(viewport) = viewport {
                    let offset = viewport.offset();
                    let bottom = (viewport.content().layout_rect().size.height
                        - viewport.root().layout_rect().size.height)
                        .max(0.0);
                    if offset.y < bottom - px(4.0) && old_bottom.is_some_and(|end| end <= offset.y)
                    {
                        viewport.scroll_to(Vector2::new(offset.x, (offset.y + delta).max(0.0)));
                    }
                }
                window.changed();
            } else if window.report_height(&measure_id, rect.size.height).1 {
                window.changed();
            }
        });
    }
    shared
        .borrow_mut()
        .messages
        .insert(id.clone(), content_anchor);
    mosaic::core::reactive::on_cleanup(move || {
        let mut state = shared.borrow_mut();
        state.messages.remove(&id);
        state.invalidate_history_height(&id);
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
                        location:(Location::Source {message:message_id.clone(),start:offset,end:Some(offset+text.len()),part:Some(part.id.clone())})
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
                    IconCell label:"Remove skill"
                        on-press:(press(move || remove_draft_part(model, &controller.get_untracked(), &id.get_untracked(), mirror))) {
                        Label text:"×" role:(TextRole::Label)
                    }
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
                    ReplyStrip
                        quote:(Derived::new(move || anchor.get().quote.split_whitespace().collect::<Vec<_>>().join(" ")))
                        on-source:(press(move || go_to_source(model, &source_controller.get_untracked(), &anchor.get_untracked(), true)))
                        on-remove:(press(move || remove_draft_part(model, &source_controller.get_untracked(), &remove.get_untracked(), false)))
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

/// The composer always ends in text, so the next words have a field to go to.
fn composer_parts(mut parts: Vec<Part>, empty: &str) -> Vec<Part> {
    if !parts
        .last()
        .is_some_and(|p| matches!(p.kind, PartKind::Text { .. }))
    {
        let id = parts
            .last()
            .map(|part| format!("{empty}-after-{}", part.id))
            .unwrap_or_else(|| empty.to_owned());
        parts.push(Part {
            id,
            kind: PartKind::Text {
                text: String::new(),
            },
        });
    }
    parts
}

/// Sending replaces the draft text, remounting its field, and disables the
/// pressed Send button; either drops keyboard focus. When the composer had
/// focus, hand it to the trailing draft text so typing continues there.
fn send_keeping_focus(model: Model, controller: &Shared, focused: bool) {
    buffer::send(model);
    if !focused {
        return;
    }
    let empty = controller.borrow().empty_draft.clone();
    let parts = composer_parts(buffer::parts(model, &model.session.get_untracked()), &empty);
    let Some(Part {
        id,
        kind: PartKind::Text { text },
    }) = parts.last()
    else {
        return;
    };
    let key = format!("draft-{id}");
    let mounted = controller
        .borrow()
        .surfaces
        .get(&key)
        .map(|(field, _)| field.clone());
    if let Some(field) = mounted {
        field.focus();
        field.reveal();
    } else {
        // The field mounts when the draft change flushes and takes focus then.
        controller.borrow_mut().requested = Some((key, text.len()));
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
pub(crate) fn InlineAsset(
    model: Model,
    asset: Asset,
    part_id: Option<String>,
    controller: Option<ControllerState>,
    mirror: bool,
) -> Element {
    buffer::fetch(model, &asset);
    let preview_controller = model.conversation_controller.get_untracked();
    let preview_key = (asset.id.clone(), part_id.clone(), mirror);
    let preview = State::new(
        preview_controller
            .as_ref()
            .and_then(|controller| {
                controller
                    .borrow()
                    .asset_previews
                    .get(&preview_key)
                    .copied()
            })
            .unwrap_or_else(|| asset.media_type.starts_with("image/")),
    );
    Effect::new(move || {
        let value = preview.get();
        if let Some(controller) = &preview_controller {
            let mut state = controller.borrow_mut();
            if state.asset_previews.insert(preview_key.clone(), value) != Some(value)
                && let Some(window) = &state.window
            {
                window.invalidate_all();
            }
        }
    });
    let asset = State::new(asset);
    let part_id = State::new(part_id);
    let root = view! {
        col height:min-content min-width:0px gap:0px
            label:(format!("Attachment {}", asset.get_untracked().name))
            stroke:(width:{px(1.0)} color:ground.rule-strong offset:{px(-1.0)}) {
            row height:{px(32.0)}px gap:0px align:center
                stroke:(width:{if preview.get() {px(1.0)} else {0.0}} color:ground.rule-strong edges:bottom) {
                row width:1fr min-width:0px {
                    CellButton label:(format!("Preview {}", asset.get_untracked().name))
                        selected:(preview) start:true horizontal-inset:(8.0)
                        on-press:(press(move || { buffer::fetch(model, &asset.get_untracked()); preview.set(!preview.get_untracked()); })) {
                        Label
                            text:(Derived::new(move || format!("{} · {} KiB", asset.get().name, asset.get().size.div_ceil(1024))))
                            role:(TextRole::Label)
                    }
                }
                if part_id.get().is_some() {
                    row width:{px(32.0)}px shrink:0
                        stroke:(width:{px(1.0)} color:ground.rule-strong edges:left) {
                        CellButton label:"Remove inline file" horizontal-inset:(0.0)
                            on-press:(press(move || if let Some(controller) = controller { remove_draft_part(model, &controller.get_untracked(), part_id.get_untracked().as_ref().unwrap(), mirror); })) {
                            Label text:"×" role:(TextRole::Label)
                        }
                    }
                }
            }
            if preview.get() {
                col height:min-content min-width:0px pad:{px(8.0)}px {
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

// Custom editor surfaces must shape with the same metrics during measurement,
// selection and painting. Inherited leading at paint time can overflow a field
// that was measured with TextStyle's default leading.
fn buffer_text_style() -> TextStyle {
    TextStyle::new(px(13.0))
        .family(mosaic::text::FontFamily::Monospace)
        .line_height(mosaic::text::LineHeight::Scale(1.7))
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
    field.text_style(buffer_text_style());
    let key = match &location {
        Location::Source {
            message,
            start,
            part,
            ..
        } => format!(
            "source-{message}-{}",
            part.clone().unwrap_or_else(|| start.to_string())
        ),
        Location::Draft(id) => format!("{}-{id}", if mirror { "inline" } else { "draft" }),
    };
    field.focusable(true).accepts_text(true).selectable(false);
    field.label(match &location {
        Location::Source { message, .. } => format!("Message {message}"),
        Location::Draft(id) => format!("Draft text {id}"),
    });
    field.role(mosaic::widgets::a11y::Role::TextInput);
    let fonts = model.ui.get_untracked().fonts();
    let editor = Rc::new(RefCell::new(EditBuffer::new(buffer_text_style())));
    let source_cursor = Rc::new(Cell::new((0, 0)));
    let markdown = matches!(location, Location::Source { .. }).then(|| {
        Rc::new(RefCell::new(crate::markdown::document(&current_text(
            model, &location,
        ))))
    });
    editor
        .borrow_mut()
        .set_text(&mut fonts.borrow_mut(), &current_text(model, &location));
    controller
        .borrow_mut()
        .surfaces
        .insert(key.clone(), (field.clone(), editor.clone()));
    if matches!(location, Location::Source { .. }) {
        controller
            .borrow_mut()
            .source_locations
            .insert(key.clone(), location.clone());
    }
    let cleanup = controller.clone();
    let cleanup_key = key.clone();
    mosaic::core::reactive::on_cleanup(move || {
        cleanup.borrow_mut().surfaces.remove(&cleanup_key);
        cleanup.borrow_mut().source_locations.remove(&cleanup_key);
    });
    let repaint = field.clone();
    let observed = editor.clone();
    let observe_fonts = fonts.clone();
    let observe_location = location.clone();
    let focus_controller = controller.clone();
    let focus_key = key.clone();
    let follow = Rc::new(Cell::new(false));
    let focus_follow = follow.clone();
    let observe_markdown = markdown.clone();
    let text_location = observe_location.clone();
    let source_text = Derived::new(move || current_text(model, &text_location));
    let previous_content = Rc::new(RefCell::new(None::<(String, String)>));
    field.style_dyn(move || {
        // Draft transactions also deliver pending focus and IME requests when
        // the source bytes have not changed (for example, crossing a part).
        if matches!(observe_location, Location::Draft(_)) {
            model.buffer.with(|_| ());
        }
        let text = source_text.get();
        if let Some(document) = &observe_markdown {
            document.borrow_mut().update(&text);
        }
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
        let mut changed_preedit = false;
        if let Some((key, text, cursor)) = staged
            && key == focus_key
        {
            editor.set_preedit(&mut observe_fonts.borrow_mut(), &text, cursor);
            focus_controller.borrow_mut().preedit = None;
            changed_preedit = true;
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
        let content = (text, query);
        if changed_preedit || previous_content.borrow().as_ref() != Some(&content) {
            *previous_content.borrow_mut() = Some(content);
            repaint.content_dirty();
        }
        Style::default()
            .width(Dimension::Fill)
            .height(Dimension::MinContent)
            .shrink(0.0)
    });
    let measured = editor.clone();
    let measure_fonts = fonts.clone();
    let measure_markdown = markdown.clone();
    field.measure(move |available| {
        let mut fonts = measure_fonts.borrow_mut();
        if let Some(document) = &measure_markdown {
            let size = document
                .borrow_mut()
                .size(&mut fonts, available.width.definite());
            return Size::new(size.width.max(1.0), size.height.max(px(24.0)));
        }
        let mut buffer = measured.borrow_mut();
        buffer.set_style(&mut fonts, buffer_text_style());
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
    let layout_markdown = markdown.clone();
    let layout_cursor = source_cursor.clone();
    field.on_layout(move |rect| {
        if layout_interaction.focused() && layout_follow.replace(false) {
            let mut fonts = layout_fonts.borrow_mut();
            let mut editor = layout_editor.borrow_mut();
            editor.set_wrap_width(&mut fonts, Some(rect.size.width));
            let caret = if let Some(document) = &layout_markdown {
                document.borrow_mut().caret(
                    &mut fonts,
                    rect.size.width,
                    crate::markdown::selection(&editor, layout_cursor.get()).1,
                )
            } else {
                editor.caret_rect(&mut fonts)
            };
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
    let paint_markdown = markdown.clone();
    let paint_cursor = source_cursor.clone();
    field.paint(move |ctx| {
        let mut fonts = paint_fonts.borrow_mut();
        let mut buffer = painted.borrow_mut();
        buffer.set_style(&mut fonts, buffer_text_style());
        buffer.set_wrap_width(&mut fonts, Some(ctx.rect.size.width));
        let origin = ctx.rect.origin;
        if let Some(document) = &paint_markdown {
            let mut document = document.borrow_mut();
            let width = ctx.rect.size.width;
            document.paint_under(ctx.scene, &mut fonts, width, origin);
            let query = if model.searching.get_untracked() {
                model.search.get_untracked()
            } else {
                String::new()
            };
            if !query.is_empty() {
                for (offset, _) in buffer.text().match_indices(&query) {
                    let range = document.display_offset(offset)
                        ..document.display_offset(offset + query.len());
                    document.fill(
                        ctx.scene,
                        &mut fonts,
                        width,
                        origin,
                        range,
                        color(status.needs_you),
                    );
                }
            }
        }
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
                        &buffer_text_style(),
                        Some(ctx.rect.size.width),
                    )
                    .selection_rects(start, end)
            } else if let Some(document) = &paint_markdown {
                let mut document = document.borrow_mut();
                let (start, end) = buffer.selection_bytes();
                let start = document.display_offset(start);
                let end = document.display_offset(end);
                document.rects(&mut fonts, ctx.rect.size.width, start..end)
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
        if let Some(document) = &paint_markdown {
            document.borrow_mut().paint_text(
                ctx.scene,
                &mut fonts,
                ctx.rect.size.width,
                origin,
                ctx.scale,
                ctx.text_color.clone(),
            );
        } else {
            ctx.scene
                .glyphs(buffer.place(&mut fonts, ctx.text_color.clone(), origin, ctx.scale));
        }
        if interaction.focused() {
            let caret = if let Some(document) = &paint_markdown {
                document.borrow_mut().caret(
                    &mut fonts,
                    ctx.rect.size.width,
                    crate::markdown::selection(&buffer, paint_cursor.get()).1,
                )
            } else {
                buffer.caret_rect(&mut fonts)
            };
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
    let ime_markdown = markdown.clone();
    let ime_cursor = source_cursor.clone();
    field.ime_area(move || {
        let rect = if let Some(document) = &ime_markdown {
            document.borrow_mut().caret(
                &mut ime_fonts.borrow_mut(),
                ime_field.layout_rect().size.width,
                crate::markdown::selection(&ime.borrow(), ime_cursor.get()).1,
            )
        } else {
            ime.borrow_mut().caret_rect(&mut ime_fonts.borrow_mut())
        };
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
    let pointer_markdown = markdown.clone();
    let pointer_cursor = source_cursor.clone();
    let pressed_link = Rc::new(RefCell::new(None::<(String, Vector2)>));
    field.on_pointer(move |event, ctx| {
        let mut fonts = pointer_fonts.borrow_mut();
        let mut editor = pointer.borrow_mut();
        match event.kind {
            PointerEventKind::Down(PointerButton::Primary) => {
                ctx.request_focus();
                pointer_controller.borrow_mut().selection = None;
                dragging.set(true);
                if let Some(document) = &pointer_markdown {
                    let rect = pointer_field.layout_rect();
                    let point = event.position - rect.origin;
                    let mut document = document.borrow_mut();
                    *pressed_link.borrow_mut() = document
                        .link_at(&mut fonts, rect.size.width, point)
                        .map(|url| (url, event.position));
                    let offset = document.hit(&mut fonts, rect.size.width, point);
                    let anchor = if event.modifiers.shift {
                        crate::markdown::selection(&editor, pointer_cursor.get()).0
                    } else {
                        offset
                    };
                    let (anchor, offset) = document.selection_range(anchor, offset);
                    editor.set_selection_bytes(&mut fonts, anchor, offset);
                    pointer_cursor.set((anchor, offset));
                } else {
                    editor.caret_to(
                        &mut fonts,
                        event.position - pointer_field.layout_rect().origin,
                        event.modifiers.shift,
                    );
                }
                if let Location::Draft(id) = &pointer_location {
                    pointer_controller.borrow_mut().pointer_anchor =
                        Some((id.clone(), editor.caret_offset(), mirror));
                }
                if ctx.press_count() >= 2
                    && let Some(document) = &pointer_markdown
                {
                    let (start, end) = document.borrow_mut().unit(
                        &mut fonts,
                        pointer_field.layout_rect().size.width,
                        editor.caret_offset(),
                        ctx.press_count() > 2,
                    );
                    editor.set_selection_bytes(&mut fonts, start, end);
                    pointer_cursor.set((start, end));
                } else if ctx.press_count() == 2 {
                    editor.select_word(&mut fonts);
                } else if ctx.press_count() > 2 {
                    editor.select_line(&mut fonts);
                }
                ctx.stop_propagation();
            }
            PointerEventKind::Move if dragging.get() => {
                if pressed_link.borrow().as_ref().is_some_and(|(_, origin)| {
                    (event.position.x - origin.x).abs() >= px(4.0)
                        || (event.position.y - origin.y).abs() >= px(4.0)
                }) {
                    pressed_link.borrow_mut().take();
                }
                if let Some(document) = &pointer_markdown {
                    let rect = pointer_field.layout_rect();
                    let mut document = document.borrow_mut();
                    let offset =
                        document.hit(&mut fonts, rect.size.width, event.position - rect.origin);
                    let anchor = crate::markdown::selection(&editor, pointer_cursor.get()).0;
                    let (anchor, offset) = document.selection_range(anchor, offset);
                    editor.set_selection_bytes(&mut fonts, anchor, offset);
                    pointer_cursor.set((anchor, offset));
                } else {
                    editor.caret_to(
                        &mut fonts,
                        event.position - pointer_field.layout_rect().origin,
                        true,
                    );
                }
            }
            PointerEventKind::Up(PointerButton::Primary) | PointerEventKind::Cancel => {
                if matches!(event.kind, PointerEventKind::Up(_))
                    && let Some((url, origin)) = pressed_link.borrow_mut().take()
                    && (event.position.x - origin.x).abs() < px(4.0)
                    && (event.position.y - origin.y).abs() < px(4.0)
                    && !event.modifiers.shift
                    && ctx.press_count() == 1
                    && let Some(document) = &pointer_markdown
                {
                    document.borrow().open_link(&url);
                }
                pressed_link.borrow_mut().take();
                dragging.set(false);
                editor.finish_selection();
            }
            // Horizontal (or Shift+vertical) wheel input scrolls wide tables;
            // other wheel input reaches the transcript.
            PointerEventKind::Wheel { delta } => {
                if let Some(document) = &pointer_markdown {
                    let rect = pointer_field.layout_rect();
                    let delta = if event.modifiers.shift && delta.x == 0.0 {
                        Vector2::new(delta.y, 0.0)
                    } else {
                        delta
                    };
                    if document.borrow_mut().scroll(
                        &mut fonts,
                        rect.size.width,
                        event.position - rect.origin,
                        delta,
                    ) {
                        ctx.stop_propagation();
                    }
                }
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
        {
            let mut state = ime_controller.borrow_mut();
            state.manual_reading = false;
            state.history_anchor = None;
        }
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
    let key_markdown = markdown.clone();
    let key_cursor = source_cursor.clone();
    let key_draft = key.starts_with("draft-");
    field.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        key_follow.set(true);
        {
            let mut state = key_controller.borrow_mut();
            state.manual_reading = false;
            state.history_anchor = None;
        }
        if event.key == Key::Escape {
            model.command_feedback.set(String::new());
            model.notice.set(String::new());
        }
        let command = event.modifiers.command();
        if command && event.key == Key::Enter {
            if !matches!(event.kind, KeyEventKind::Down { repeat: true }) {
                send_keeping_focus(model, &key_controller, key_draft);
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
            document_edge(model, &key_controller, &mut fonts, event.key == Key::End);
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
                let before = if key_markdown.is_some() {
                    crate::markdown::selection(&editor, key_cursor.get()).1
                } else {
                    editor.caret_offset()
                };
                if let Some(document) = &key_markdown {
                    let (a, b) = editor.selection_bytes();
                    let offset = if !event.modifiers.shift
                        && a != b
                        && matches!(motion, CaretMotion::Left | CaretMotion::Right)
                    {
                        if matches!(motion, CaretMotion::Left) {
                            a
                        } else {
                            b
                        }
                    } else {
                        document.borrow_mut().motion(
                            &mut fonts,
                            key_field.layout_rect().size.width,
                            before,
                            motion,
                        )
                    };
                    let anchor = if event.modifiers.shift {
                        crate::markdown::selection(&editor, key_cursor.get()).0
                    } else {
                        offset
                    };
                    let (anchor, offset) = document.borrow().selection_range(anchor, offset);
                    editor.set_selection_bytes(&mut fonts, anchor, offset);
                    key_cursor.set((anchor, offset));
                } else {
                    editor.move_caret(&mut fonts, motion, event.modifiers.shift);
                }
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
                let after = if key_markdown.is_some() {
                    key_cursor.get().1
                } else {
                    editor.caret_offset()
                };
                let at_boundary = if let Some(document) = &key_markdown {
                    let document = document.borrow();
                    let before = document.display_offset(before);
                    let after = document.display_offset(after);
                    let end = document.display_len();
                    before.min(end) == after.min(end)
                } else {
                    after == before
                };
                if at_boundary
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
                        model,
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
    /// Wrapped prose under the rows, such as why the model was chosen.
    pub note: Option<String>,
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

/// A spawned session's model choice. The reason is the parent's stored
/// `selection_rationale`, verbatim. The grant keeps no copy of the harness,
/// model or effort chosen then, so those rows show the session's current
/// configuration, which may have changed since. A grant without a rationale
/// (fixed-model policy, older servers) says so rather than inventing one.
fn model_choice(snapshot: &Snapshot, session: &Session) -> Option<ProvenanceGroup> {
    let grant = session.spawn.as_ref()?;
    let mut rows = Vec::new();
    if let Some(parent) = session
        .parent_session_id
        .as_deref()
        .and_then(|id| snapshot.sessions.iter().find(|s| s.id == id))
    {
        rows.push(("Spawned By", parent.title.clone()));
    }
    if let Some(worker) = &session.worker {
        rows.push((
            "Current Harness",
            match worker.harness {
                Harness::Codex => "Codex",
                Harness::ClaudeCode => "Claude Code",
            }
            .to_string(),
        ));
    }
    if let Some(model) = session
        .selection
        .model
        .as_deref()
        .or(session.worker.as_ref().and_then(|w| w.model.as_deref()))
    {
        rows.push(("Current Model", adapt::model_identity(model).short_name()));
    }
    if let Some(effort) = &session.selection.effort {
        rows.push(("Current Effort", effort.clone()));
    }
    Some(ProvenanceGroup {
        key: "model-choice".into(),
        title: "Model Choice".into(),
        rows,
        changes: None,
        note: Some(match &grant.selection_rationale {
            Some(reason) => format!("Parent's reason at spawn:\n{reason}"),
            None => "No reason recorded at spawn".into(),
        }),
    })
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
                note: None,
            }
        })
        .collect();
    if let Some(group) = model_choice(snapshot, session) {
        groups.insert(0, group);
    }
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
            title: "Worker Run".into(),
            rows,
            changes: (changes && session.workspaces.is_empty())
                .then(|| change_text(worker.changes.as_ref())),
            note: None,
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_selection_ownership_does_not_absorb_later_windows() {
        let mut state = Controller::default();
        state.capture_native_selection(true, vec!["a".into(), "b".into()]);
        for n in 0..800 {
            state.capture_native_selection(true, vec![format!("later-{n}")]);
        }
        assert_eq!(
            state.native_selection_rows.as_deref(),
            Some(["a".into(), "b".into()].as_slice())
        );
        state.capture_native_selection(false, vec![]);
        assert!(state.native_selection_rows.is_none());
        state.capture_native_selection(true, vec!["c".into()]);
        assert_eq!(state.native_selection_rows, Some(vec!["c".into()]));
    }

    #[test]
    fn history_retained_state_prunes_removals_and_session_changes() {
        let mut state = Controller::default();
        let mut index = TranscriptIndex::default();
        state.prune_history_state("first", &index, &[]);
        for id in ["keep", "removed"] {
            state.expanded_rows.insert(id.into(), true);
            state
                .tool_openings
                .insert(id.into(), relay_ui::prelude::Opening::Expanded);
        }
        state
            .asset_previews
            .insert(("keep-asset".into(), None, false), true);
        state.asset_previews.insert(
            ("keep-asset".into(), Some("removed-part".into()), true),
            true,
        );
        state
            .asset_previews
            .insert(("removed-asset".into(), None, false), true);
        state.capture_native_selection(true, vec!["keep".into(), "removed".into()]);
        index.rows.insert(
            "keep".into(),
            TranscriptEntry {
                message: projected_message("keep", "Agent", "assistant"),
                comments: vec![],
                speaker: "Agent".into(),
                speaker_session: None,
                continues: false,
                spacing: 0.0,
            },
        );
        state.prune_history_state("first", &index, &[("keep-asset".into(), None)]);
        assert_eq!(state.expanded_rows.len(), 1);
        assert!(state.expanded_rows.contains_key("keep"));
        assert_eq!(state.tool_openings.len(), 1);
        assert!(state.tool_openings.contains_key("keep"));
        assert_eq!(state.asset_previews.len(), 1);
        assert_eq!(state.native_selection_rows, Some(vec!["keep".into()]));
        state.prune_history_state("second", &index, &[("keep-asset".into(), None)]);
        assert!(state.expanded_rows.is_empty());
        assert!(state.tool_openings.is_empty());
        assert!(state.asset_previews.is_empty());
        assert!(state.native_selection_rows.is_none());
    }

    #[test]
    fn history_cache_invalidates_projection_versions_without_moving_estimates() {
        mosaic::core::builtins::install();
        install_theme(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::select(false, false),
            1.0,
        ));
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
            let model = Model::new(&ui, sender);
            model.session.set("indexed".into());
            model.snapshot.update(|snapshot| {
                snapshot.messages = vec![
                    projected_message("a", "Agent", "assistant"),
                    projected_message("b", "Agent", "assistant"),
                ]
            });
            let index = transcript_index(model);
            let window = HistoryWindow::new();
            window.reconcile(index.get());
            window.report_height("a", 480.0);
            window.report_height("b", 160.0);
            model.snapshot.update(|snapshot| {
                snapshot.messages[0].body = "Changed body".into();
                snapshot.messages[0].parts = vec![Part::text("Changed body")];
            });
            window.reconcile(index.get());
            assert!(window.heights.borrow()["a"].stale);
            assert!(!window.heights.borrow()["b"].stale);
            assert_eq!(
                window.height("a"),
                480.0,
                "invalidating unseen content must retain its estimate"
            );
            assert_eq!(window.report_height("a", 480.0), (0.0, true));
            assert!(
                !window.heights.borrow()["a"].stale,
                "equal-height layout must revalidate"
            );
            model.snapshot.update(|snapshot| {
                snapshot.comments.push(Comment {
                    id: "comment".into(),
                    message_id: "a".into(),
                    quote: None,
                    author: "Reader".into(),
                    body: "New comment".into(),
                    created_at: 1,
                })
            });
            window.reconcile(index.get());
            assert!(window.heights.borrow()["a"].stale);
            assert_eq!(window.report_height("a", 720.0), (240.0, true));
            let state = Controller {
                window: Some(window.clone()),
                ..Default::default()
            };
            state.invalidate_history_height("b");
            assert!(window.heights.borrow()["b"].stale);
            assert_eq!(window.height("b"), 160.0);
            model
                .snapshot
                .update(|snapshot| snapshot.messages.remove(0));
            window.reconcile(index.get());
            assert!(!window.heights.borrow().contains_key("a"));
            model.session.set("other".into());
            window.reconcile(index.get());
            assert!(
                window.heights.borrow().is_empty(),
                "session changes must discard old measurements"
            );
        });
        scope.dispose();
    }

    fn projected_message(id: &str, author: &str, kind: &str) -> Message {
        Message {
            id: id.into(),
            session_id: "indexed".into(),
            author: author.into(),
            sender: Sender::User,
            kind: kind.into(),
            body: format!("Body {id}"),
            parts: vec![],
            tool: None,
        }
    }

    fn assert_projection(
        index: Derived<Rc<TranscriptIndex>>,
        expected: &[(&str, &str, bool, f32)],
    ) {
        let actual = index.get();
        assert_eq!(
            actual.order,
            expected
                .iter()
                .map(|row| row.0.to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(actual.rows.len(), expected.len());
        for &(id, speaker, continues, spacing) in expected {
            let row = &actual.rows[id];
            assert_eq!(
                (&*row.speaker, row.continues, row.spacing),
                (speaker, continues, spacing),
                "row {id}"
            );
        }
    }

    #[test]
    fn transcript_projection_distinguishes_empty_speakers_from_continuations() {
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
            let model = Model::new(&ui, sender);
            model.session.set("indexed".into());
            model.snapshot.update(|snapshot| {
                snapshot.messages = vec![
                    projected_message("first", "", "assistant"),
                    projected_message("same", "", "assistant"),
                    projected_message("named", "Agent", "assistant"),
                    projected_message("empty-after-named", "", "assistant"),
                    projected_message("prompt", "", "prompt"),
                    projected_message("next-prompt", "Different raw author", "prompt"),
                    projected_message("relay-notice", "Relay", "prompt"),
                ]
            });
            let index = transcript_index(model);
            assert_projection(
                index,
                &[
                    ("first", "", false, 0.0),
                    ("same", "", true, 28.0),
                    ("named", "Agent", false, 28.0),
                    ("empty-after-named", "", false, 28.0),
                    ("prompt", "You", false, 28.0),
                    ("next-prompt", "", true, 28.0),
                    ("relay-notice", "Relay", false, 28.0),
                ],
            );
            let empty =
                Derived::new(move || index.with(|index| index.rows["empty-after-named"].clone()));
            let changes = Rc::new(Cell::new(0));
            let observed = changes.clone();
            Effect::new(move || {
                empty.get();
                observed.set(observed.get() + 1);
            });
            mosaic::core::reactive::flush();
            model
                .snapshot
                .update(|snapshot| snapshot.messages[2].author.clear());
            mosaic::core::reactive::flush();
            assert!(empty.get().continues);
            assert_eq!(empty.get().speaker, "");
            assert_eq!(
                changes.get(),
                2,
                "continuation-only change must cross the row equality cutoff"
            );
            model
                .snapshot
                .update(|snapshot| snapshot.messages[2].author = "Agent".into());
            mosaic::core::reactive::flush();
            assert!(!empty.get().continues);
            assert_eq!(changes.get(), 3);
        });
        scope.dispose();
    }

    #[test]
    fn transcript_projection_updates_neighbors_after_predecessor_and_structure_changes() {
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
            let model = Model::new(&ui, sender);
            model.session.set("indexed".into());
            model.snapshot.update(|snapshot| {
                snapshot.messages = vec![
                    projected_message("a", "Agent", "command_execution"),
                    projected_message("b", "Agent", "file_change"),
                    projected_message("c", "Other", "assistant"),
                ]
            });
            let index = transcript_index(model);
            let b = Derived::new(move || index.with(|index| index.rows["b"].clone()));
            let updates = Rc::new(Cell::new(0));
            let observed = updates.clone();
            Effect::new(move || {
                b.get();
                observed.set(observed.get() + 1);
            });
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("a", "Agent", false, 0.0),
                    ("b", "", true, 6.0),
                    ("c", "Other", false, 28.0),
                ],
            );
            model
                .snapshot
                .update(|snapshot| snapshot.messages[0].author = "Other".into());
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("a", "Other", false, 0.0),
                    ("b", "Agent", false, 6.0),
                    ("c", "Other", false, 28.0),
                ],
            );
            assert_eq!(updates.get(), 2);
            model
                .snapshot
                .update(|snapshot| snapshot.messages[0].kind = "assistant".into());
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("a", "Other", false, 0.0),
                    ("b", "Agent", false, 28.0),
                    ("c", "Other", false, 28.0),
                ],
            );
            assert_eq!(updates.get(), 3);
            model.snapshot.update(|snapshot| {
                snapshot
                    .messages
                    .insert(0, projected_message("p", "Agent", "command_execution"))
            });
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("p", "Agent", false, 0.0),
                    ("a", "Other", false, 28.0),
                    ("b", "Agent", false, 28.0),
                    ("c", "Other", false, 28.0),
                ],
            );
            assert_eq!(
                updates.get(),
                3,
                "unaffected row should keep its equality cutoff"
            );
            model.snapshot.update(|snapshot| {
                snapshot.messages.remove(1);
            });
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("p", "Agent", false, 0.0),
                    ("b", "", true, 6.0),
                    ("c", "Other", false, 28.0),
                ],
            );
            assert_eq!(updates.get(), 4);
            model
                .snapshot
                .update(|snapshot| snapshot.messages.reverse());
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("c", "Other", false, 0.0),
                    ("b", "Agent", false, 28.0),
                    ("p", "", true, 6.0),
                ],
            );
            assert_eq!(updates.get(), 5);
            model.snapshot.update(|snapshot| {
                snapshot
                    .messages
                    .insert(0, projected_message("older", "Agent", "assistant"))
            });
            mosaic::core::reactive::flush();
            assert_projection(
                index,
                &[
                    ("older", "Agent", false, 0.0),
                    ("c", "Other", false, 28.0),
                    ("b", "Agent", false, 28.0),
                    ("p", "", true, 6.0),
                ],
            );
            assert_eq!(updates.get(), 5);
        });
        scope.dispose();
    }

    #[test]
    fn transcript_projection_reconciles_spawn_receipts_comments_and_session_switches() {
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
            let model = Model::new(&ui, sender);
            let mut snapshot = demo_snapshot(AgentProfile::default());
            demo_orchestration(&mut snapshot);
            let mut native = snapshot
                .messages
                .iter()
                .find(|m| m.id == "t-spawn")
                .unwrap()
                .clone();
            native.id = "native-spawn".into();
            let mut receipt = native.clone();
            receipt.id = "agent-tool-receipt".into();
            receipt.author = "Relay".into();
            receipt.kind = "tool".into();
            let mut foreign = projected_message("foreign", "Other", "assistant");
            foreign.session_id = "other".into();
            snapshot.messages = vec![receipt.clone(), foreign];
            snapshot.comments = vec![Comment {
                id: "receipt-comment".into(),
                message_id: receipt.id.clone(),
                quote: None,
                author: "Reader".into(),
                body: "Keep fallback".into(),
                created_at: 1,
            }];
            model.snapshot.set(snapshot);
            model.session.set(native.session_id.clone());
            let index = transcript_index(model);
            assert_eq!(index.get().order, [receipt.id.clone()]);
            assert_eq!(
                index.get().rows[&receipt.id].comments[0].body,
                "Keep fallback"
            );
            model.snapshot.update(|snapshot| {
                snapshot.messages.push(native.clone());
                snapshot.comments.push(Comment {
                    id: "native-comment".into(),
                    message_id: native.id.clone(),
                    quote: Some("quoted".into()),
                    author: "Reader".into(),
                    body: "Original".into(),
                    created_at: 2,
                });
            });
            assert_eq!(index.get().order, [native.id.clone()]);
            assert_eq!(
                index.get().rows[&native.id].comments[0].id,
                "native-comment"
            );
            model.snapshot.update(|snapshot| {
                let comment = &mut snapshot.comments[1];
                comment.body = "Edited".into();
                comment.quote = None;
                comment.author = "Editor".into();
                snapshot.messages.reverse();
            });
            let row = index.get().rows[&native.id].clone();
            assert_eq!(index.get().order, [native.id.clone()]);
            assert_eq!(
                (
                    &*row.comments[0].body,
                    &*row.comments[0].author,
                    &row.comments[0].quote
                ),
                ("Edited", "Editor", &None)
            );
            model
                .snapshot
                .update(|snapshot| snapshot.messages.retain(|m| m.id != native.id));
            assert_eq!(index.get().order, [receipt.id.clone()]);
            assert_eq!(
                index.get().rows[&receipt.id].comments[0].body,
                "Keep fallback"
            );
            model.snapshot.update(|snapshot| {
                snapshot
                    .messages
                    .iter_mut()
                    .find(|m| m.id == receipt.id)
                    .unwrap()
                    .body = "Meaningful receipt text".into();
                snapshot.messages.push(native.clone());
            });
            assert_eq!(index.get().order, [receipt.id.clone(), native.id.clone()]);
            model.session.set("other".into());
            assert_projection(index, &[("foreign", "Other", false, 0.0)]);
            assert!(index.get().rows["foreign"].comments.is_empty());
            model.session.set(native.session_id.clone());
            assert_eq!(index.get().order, [receipt.id.clone(), native.id.clone()]);
            model.snapshot.update(|snapshot| snapshot.comments.clear());
            assert!(index.get().rows.values().all(|row| row.comments.is_empty()));
            assert_eq!(model.snapshot.get_untracked().messages.len(), 3);
        });
        scope.dispose();
    }

    #[test]
    fn transcript_projection_isolates_keys_and_unchanged_rows_from_streaming() {
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
            let model = Model::new(&ui, sender);
            model.session.set("indexed".into());
            model.snapshot.update(|snapshot| {
                snapshot.messages = (0..100)
                    .map(|index| Message {
                        id: format!("row-{index}"),
                        session_id: "indexed".into(),
                        author: "Agent".into(),
                        sender: Sender::User,
                        kind: "assistant".into(),
                        body: "Text".into(),
                        parts: vec![],
                        tool: None,
                    })
                    .collect();
            });
            let index = transcript_index(model);
            let keys = Derived::new(move || index.with(|index| index.order.clone()));
            let first = Derived::new(move || index.with(|index| index.rows["row-0"].clone()));
            let tail = Derived::new(move || index.with(|index| index.rows["row-99"].clone()));
            let counts = Rc::new(Cell::new([0; 3]));
            for (position, read) in [
                Box::new(move || {
                    keys.get();
                }) as Box<dyn Fn()>,
                Box::new(move || {
                    first.get();
                }),
                Box::new(move || {
                    tail.get();
                }),
            ]
            .into_iter()
            .enumerate()
            {
                let counts = counts.clone();
                Effect::new(move || {
                    read();
                    let mut next = counts.get();
                    next[position] += 1;
                    counts.set(next);
                });
            }
            mosaic::core::reactive::flush();
            assert_eq!(counts.get(), [1, 1, 1]);
            model.snapshot.update(|snapshot| {
                snapshot
                    .messages
                    .last_mut()
                    .unwrap()
                    .body
                    .push_str(" streamed")
            });
            mosaic::core::reactive::flush();
            assert_eq!(counts.get(), [1, 1, 2]);
            model.snapshot.update(|snapshot| snapshot.revision += 1);
            mosaic::core::reactive::flush();
            assert_eq!(counts.get(), [1, 1, 2]);
        });
        scope.dispose();
    }

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

#[cfg(test)]
mod layout_regression {
    use super::*;
    use mosaic::core::reactive::flush;
    use std::time::Duration;

    #[test]
    fn transcript_text_fits_panels_after_wrapping_streaming_and_resize() {
        mosaic::core::builtins::install();
        install_theme(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::Dark,
            1.0,
        ));
        install_theme(&relay_ui::icons::icons());
        let scope = Scope::new(|| {});
        let ui = scope.run(Ui::new);
        let mut fonts = mosaic::text::FontContext::embedded_only();
        relay_ui::fonts::install(&mut fonts);
        ui.set_fonts(fonts);
        relay_ui::attach(&ui);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let model = scope.run(|| Model::new(&ui, sender));
        let journal = format!(
            "journalctl --user\n{}",
            format!(
                "Oct 10 systemd: /nix/store/{} /home/user/workspace/deploy.py\n",
                "long-unbroken-path".repeat(15)
            )
            .repeat(10)
        );
        model.snapshot.update(|s| {
            for (id, kind, body) in [
                ("journal", "prompt", journal),
                (
                    "context",
                    "issue-context",
                    "Source context follows the pasted journal".into(),
                ),
                (
                    "fence",
                    "assistant",
                    "Completed.\n```bash\njournalctl --user -fu relay\n```".into(),
                ),
                (
                    "reply",
                    "assistant",
                    "Completion is reported after checks pass.".into(),
                ),
            ] {
                s.messages.push(Message {
                    id: id.into(),
                    session_id: "layout".into(),
                    author: "Relay".into(),
                    sender: Sender::User,
                    kind: kind.into(),
                    parts: if id == "fence" {
                        vec![Part::text(&body)]
                    } else {
                        vec![]
                    },
                    body,
                    tool: None,
                });
            }
        });
        let controller = scope.run(|| State::new(Rc::new(RefCell::new(Controller::default()))));
        scope.run(|| {
            let _ambient = ui.enter();
            ui.mount(&view! {
                col height:min-content gap:28px font-family:monospace {
                    TranscriptMessage model:(model) message-id:"journal" controller:(controller)
                    TranscriptMessage model:(model) message-id:"context" controller:(controller)
                    TranscriptMessage model:(model) message-id:"fence" controller:(controller)
                    TranscriptMessage model:(model) message-id:"reply" controller:(controller)
                }
            });
        });
        for (width, chunk) in [
            (480.0, "\n[Streamed"),
            (1100.0, " reply](https://example.com)"),
            (480.0, "\n\n**Done**"),
            (848.0, "\n\n```rust\nlet x = 1;\n```"),
        ] {
            model.snapshot.update(|s| {
                s.messages
                    .iter_mut()
                    .find(|m| m.id == "reply")
                    .unwrap()
                    .body
                    .push_str(chunk)
            });
            for _ in 0..12 {
                flush();
                ui.tick(Duration::from_millis(16));
                ui.frame(Size::new(width, 8000.0), 1.0);
            }
            let state = controller.get_untracked();
            let state = state.borrow();
            let inspection = ui.inspection_snapshot();
            let mut previous_bottom = 0.0;
            for id in ["journal", "context", "fence", "reply"] {
                let panel = state.messages[id].layout_rect();
                assert!(
                    panel.origin.y >= previous_bottom - 0.5,
                    "overlapping row {id} at {width}"
                );
                previous_bottom = panel.origin.y + panel.size.height;
                let mut descendants = vec![state.messages[id].id()];
                while let Some(node_id) = descendants.pop() {
                    let node = inspection
                        .nodes
                        .iter()
                        .find(|node| node.id == node_id)
                        .unwrap();
                    descendants.extend(node.children.iter().copied());
                    if matches!(
                        node.role,
                        mosaic::widgets::a11y::Role::Label | mosaic::widgets::a11y::Role::TextInput
                    ) {
                        let rect = node.rect;
                        assert!(
                            rect.origin.x >= panel.origin.x - 0.5
                                && rect.origin.y >= panel.origin.y - 0.5
                                && rect.origin.x + rect.size.width
                                    <= panel.origin.x + panel.size.width + 0.5
                                && rect.origin.y + rect.size.height <= previous_bottom + 0.5,
                            "text node outside {id} panel at {width}: {:?}",
                            node.label
                        );
                    }
                }
                for (key, (field, buffer)) in &state.surfaces {
                    if !key.starts_with(&format!("source-{id}-")) {
                        continue;
                    }
                    let rect = field.layout_rect();
                    let metrics = crate::markdown::document(&buffer.borrow().text())
                        .size(&mut ui.fonts().borrow_mut(), Some(rect.size.width));
                    assert!(
                        metrics.height <= rect.size.height + 0.5,
                        "{id} at {width}: painted height {} exceeds measured {}",
                        metrics.height,
                        rect.size.height
                    );
                    assert!(
                        metrics.width <= rect.size.width + 0.5,
                        "text spills horizontally: {id}"
                    );
                    assert!(
                        rect.origin.y + metrics.height <= previous_bottom + 0.5,
                        "text outside panel: {id}"
                    );
                }
            }
        }
        scope.dispose();
    }
}
