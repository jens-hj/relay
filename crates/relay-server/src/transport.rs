//! Per-connection projections. Watch revisions may coalesce; diff against delivered state.
use super::*;
use axum::extract::{Path as ApiPath, Query};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct Projection {
    session: String,
    generation: u64,
    oldest: Option<String>,
    newest: Option<String>,
    details: BTreeSet<String>,
    reviews: BTreeSet<String>,
}

fn pending(submission: &Submission) -> bool {
    matches!(
        submission.state,
        SubmissionState::Queued
            | SubmissionState::Launching
            | SubmissionState::Running
            | SubmissionState::Paused
    )
}

fn active_run_ids(snapshot: &Snapshot) -> std::collections::BTreeMap<String, String> {
    let active = snapshot
        .sessions
        .iter()
        .filter(|session| {
            session.worker.as_ref().is_some_and(|worker| {
                matches!(worker.status, WorkerStatus::Running | WorkerStatus::Queued)
            })
        })
        .map(|session| session.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut ids = std::collections::BTreeMap::new();
    for message in
        snapshot.messages.iter().rev().filter(|message| {
            message.kind == "prompt" && active.contains(message.session_id.as_str())
        })
    {
        if let Some(id) = message.id.strip_prefix("prompt-") {
            ids.entry(message.session_id.clone())
                .or_insert_with(|| id.into());
        }
    }
    ids
}

fn progress_summary(snapshot: &Snapshot, session: &str) -> Option<Box<ProgressSummary>> {
    let running = snapshot.sessions.iter().any(|s| {
        s.id == session
            && s.worker
                .as_ref()
                .is_some_and(|w| w.status == WorkerStatus::Running)
    });
    if !running {
        return None;
    }
    let run = active_run_ids(snapshot).remove(session)?;
    // This namespace is produced only from Codex's explicit public summary fields.
    // Legacy generic reasoning rows have no public/current-run contract.
    let prefix = format!("codex-summary-{run}-");
    // Item order is first public appearance; paragraph order is summaryIndex,
    // independent of arrival/insertion order. Late paragraphs of an older item
    // cannot replace the current item, nor can an index-zero delta replace one.
    let mut seen = BTreeSet::new();
    let mut current = None;
    let mut selected = None;
    let mut selected_index = 0;
    for message in &snapshot.messages {
        if message.session_id != session
            || message.kind != "reasoning"
            || message.body.trim().is_empty()
        {
            continue;
        }
        let Some((item, index)) = message
            .id
            .strip_prefix(&prefix)
            .and_then(|id| id.rsplit_once('-'))
        else {
            continue;
        };
        let Ok(index) = index.parse::<u64>() else {
            continue;
        };
        if index >= app_server::SUMMARY_PARTS {
            continue;
        }
        if seen.insert(item) {
            current = Some(item);
            selected_index = index;
            selected = Some(message);
        } else if current == Some(item) && index > selected_index {
            selected_index = index;
            selected = Some(message);
        }
    }
    let message = selected?;
    let text = message
        .body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut end = text.len().min(4096);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(Box::new(ProgressSummary {
        run_id: run,
        text: text[..end].into(),
    }))
}

/// Whole-transcript tool counts, derived from the complete stored snapshot on
/// every projection so they match whatever history page the client loaded.
fn session_tool_counts(snapshot: &Snapshot, session: &str) -> Option<Vec<relay_core::ToolCount>> {
    (!session.is_empty()).then(|| relay_core::tool_counts(&snapshot.messages, session))
}

fn historical_submissions(
    snapshot: &Snapshot,
    messages: &[relay_core::Message],
) -> Vec<Submission> {
    let prompts = messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<BTreeSet<_>>();
    let runs = messages
        .iter()
        .filter_map(|message| message.tool.as_ref().map(|tool| tool.run_id.as_str()))
        .collect::<BTreeSet<_>>();
    snapshot
        .submissions
        .iter()
        .filter(|submission| {
            !pending(submission)
                && (prompts.contains(format!("prompt-{}", submission.id).as_str())
                    || runs.contains(submission.id.as_str()))
        })
        .map(|submission| {
            let mut receipt = submission.clone();
            receipt.parts.clear();
            receipt
        })
        .collect()
}

pub(crate) fn metadata(snapshot: &Snapshot) -> Snapshot {
    let mut result = snapshot.clone();
    result.messages.clear();
    result.comments.clear();
    result.submissions.retain(pending);
    result
        .tool_permissions
        .retain(|p| p.decision.is_none() && !p.expired);
    for session in &mut result.sessions {
        for workspace in &mut session.workspaces {
            workspace.changes = None;
        }
        if let Some(worker) = &mut session.worker {
            worker.changes = None;
        }
    }
    result
}

pub(crate) fn history_page(
    snapshot: &Snapshot,
    session: &str,
    before: Option<&str>,
    around: Option<&str>,
    after: Option<&str>,
) -> Result<HistoryPage, Error> {
    if [before, around, after]
        .iter()
        .filter(|cursor| cursor.is_some())
        .count()
        > 1
    {
        return Err(Error::invalid("Choose one history cursor"));
    }
    if !snapshot.sessions.iter().any(|s| s.id == session) {
        return Err(Error::invalid("Session not found"));
    }
    let messages = snapshot
        .messages
        .iter()
        .filter(|m| m.session_id == session)
        .collect::<Vec<_>>();
    let start = after
        .map(|id| {
            messages
                .iter()
                .position(|m| m.id == id)
                .map(|i| i + 1)
                .ok_or_else(|| Error::invalid("History cursor not found"))
        })
        .transpose()?
        .unwrap_or(0);
    let end = if after.is_some() {
        (start + HISTORY_PAGE_MESSAGES).min(messages.len())
    } else {
        match (before, around) {
            (Some(id), _) => messages
                .iter()
                .position(|m| m.id == id)
                .ok_or_else(|| Error::invalid("History cursor not found"))?,
            (_, Some(id)) => (messages
                .iter()
                .position(|m| m.id == id)
                .ok_or_else(|| Error::invalid("Message not found"))?
                + HISTORY_PAGE_MESSAGES / 2
                + 1)
            .min(messages.len()),
            _ => messages.len(),
        }
    };
    let mut selected = Vec::new();
    let mut comments = Vec::new();
    let mut previews = BTreeSet::new();
    let mut bytes = 1024;
    let candidates: Vec<_> = if let Some(id) = around {
        let center = messages
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| Error::invalid("Message not found"))?;
        let mut indices = vec![center];
        for distance in 1..messages.len() {
            if let Some(index) = center.checked_sub(distance) {
                indices.push(index);
            }
            if center + distance < messages.len() {
                indices.push(center + distance);
            }
            if indices.len() >= HISTORY_PAGE_MESSAGES {
                break;
            }
        }
        indices
            .into_iter()
            .take(HISTORY_PAGE_MESSAGES)
            .map(|index| messages[index])
            .collect()
    } else if after.is_some() {
        messages[start..end].to_vec()
    } else {
        messages[..end]
            .iter()
            .rev()
            .take(HISTORY_PAGE_MESSAGES)
            .copied()
            .collect()
    };
    for source in candidates {
        let (message, is_preview) = preview(source);
        let mut related = snapshot
            .comments
            .iter()
            .filter(|c| c.message_id == source.id)
            .cloned()
            .collect::<Vec<_>>();
        let comment_size = serde_json::to_vec(&related).map_err(Error::internal)?.len();
        if comment_size > MESSAGE_PREVIEW_BYTES {
            related.clear();
            previews.insert(source.id.clone());
        }
        let size = serde_json::to_vec(&(&message, &related))
            .map_err(Error::internal)?
            .len();
        if !selected.is_empty() && bytes + size > HISTORY_PAGE_BYTES - 16 * 1024 {
            break;
        }
        bytes += size;
        comments.extend(related);
        if is_preview {
            previews.insert(source.id.clone());
        }
        selected.push(message);
    }
    if around.is_some() {
        selected.sort_by_key(|m| messages.iter().position(|source| source.id == m.id));
    } else if after.is_none() {
        selected.reverse();
    }
    let oldest = selected.first().map(|m| m.id.clone());
    let has_more = oldest
        .as_ref()
        .is_some_and(|id| messages.first().is_some_and(|m| &m.id != id));
    let newest = selected.last().map(|m| m.id.clone());
    let has_newer = newest
        .as_ref()
        .is_some_and(|id| messages.last().is_some_and(|m| &m.id != id));
    let mut page = HistoryPage {
        revision: snapshot.revision,
        submissions: historical_submissions(snapshot, &selected),
        messages: selected,
        comments,
        history: HistoryState {
            session_id: session.into(),
            oldest_message_id: oldest,
            has_more,
            newest_message_id: newest,
            has_newer,
            preview_ids: previews,
            generation: 0,
            active_run_ids: active_run_ids(snapshot),
            progress_summary: progress_summary(snapshot, session),
            tool_counts: session_tool_counts(snapshot, session),
        },
    };
    // Include cursor, preview references, comments and receipt metadata in the budget.
    while serde_json::to_vec(&page).map_err(Error::internal)?.len() > HISTORY_PAGE_BYTES - 16 * 1024
    {
        if page.messages.len() <= 1 {
            return Err(Error::invalid("History item is too large to preview"));
        }
        let remove_newest = after.is_some()
            || around.is_some_and(|id| {
                page.messages
                    .first()
                    .is_some_and(|message| message.id == id)
            });
        let removed = if remove_newest {
            page.messages.pop().unwrap()
        } else {
            page.messages.remove(0)
        };
        page.comments
            .retain(|comment| comment.message_id != removed.id);
        page.history.preview_ids.remove(&removed.id);
        page.submissions = historical_submissions(snapshot, &page.messages);
        page.history.oldest_message_id = page.messages.first().map(|message| message.id.clone());
        page.history.newest_message_id = page.messages.last().map(|message| message.id.clone());
        page.history.has_more = page
            .history
            .oldest_message_id
            .as_ref()
            .is_some_and(|id| messages.first().is_some_and(|message| &message.id != id));
        page.history.has_newer = page
            .history
            .newest_message_id
            .as_ref()
            .is_some_and(|id| messages.last().is_some_and(|message| &message.id != id));
    }
    Ok(page)
}

impl Projection {
    pub(crate) fn view(&self, snapshot: &Snapshot) -> (Snapshot, HistoryState) {
        let mut view = metadata(snapshot);
        let mut history = HistoryState {
            session_id: self.session.clone(),
            generation: self.generation,
            active_run_ids: active_run_ids(snapshot),
            progress_summary: progress_summary(snapshot, &self.session),
            tool_counts: session_tool_counts(snapshot, &self.session),
            ..Default::default()
        };
        let messages = snapshot
            .messages
            .iter()
            .filter(|m| m.session_id == self.session)
            .collect::<Vec<_>>();
        let start = self
            .oldest
            .as_ref()
            .and_then(|id| messages.iter().position(|m| &m.id == id))
            .unwrap_or_else(|| {
                if self.session.is_empty() {
                    messages.len()
                } else {
                    messages.len().saturating_sub(HISTORY_PAGE_MESSAGES)
                }
            });
        let end = self
            .newest
            .as_ref()
            .and_then(|id| messages.iter().position(|m| &m.id == id))
            .map_or(messages.len(), |i| i + 1);
        for source in &messages[start.min(end)..end] {
            let (message, is_preview) = if self.details.contains(&source.id) {
                ((*source).clone(), false)
            } else {
                preview(source)
            };
            if is_preview {
                history.preview_ids.insert(source.id.clone());
            }
            view.messages.push(message);
        }
        history.oldest_message_id = view.messages.first().map(|m| m.id.clone());
        history.has_more = start > 0;
        history.newest_message_id = view.messages.last().map(|m| m.id.clone());
        history.has_newer = end < messages.len();
        let ids = view
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<BTreeSet<_>>();
        for id in &ids {
            let related = snapshot
                .comments
                .iter()
                .filter(|c| c.message_id == *id)
                .cloned()
                .collect::<Vec<_>>();
            let size = serde_json::to_vec(&related).map_or(usize::MAX, |v| v.len());
            if size <= MESSAGE_PREVIEW_BYTES || self.details.contains(*id) {
                view.comments.extend(related);
            } else {
                history.preview_ids.insert((*id).into());
            }
        }
        view.submissions
            .extend(historical_submissions(snapshot, &view.messages));
        for session in &mut view.sessions {
            if self.reviews.contains(&session.id)
                && let Some(source) = snapshot.sessions.iter().find(|s| s.id == session.id)
            {
                session.workspaces = source.workspaces.clone();
                if let (Some(worker), Some(original)) = (&mut session.worker, &source.worker) {
                    worker.changes = original.changes.clone();
                }
            }
        }
        (view, history)
    }

    pub(crate) fn request(
        &mut self,
        snapshot: &Snapshot,
        request: StreamRequest,
    ) -> Result<Option<StreamEvent>, Error> {
        let (session, generation) = match &request {
            StreamRequest::Subscribe {
                session_id,
                generation,
            }
            | StreamRequest::Older {
                session_id,
                generation,
                ..
            }
            | StreamRequest::Newer {
                session_id,
                generation,
                ..
            }
            | StreamRequest::Around {
                session_id,
                generation,
                ..
            }
            | StreamRequest::Detail {
                session_id,
                generation,
                ..
            }
            | StreamRequest::Search {
                session_id,
                generation,
                ..
            } => (session_id, *generation),
            StreamRequest::Review { session_id } => {
                self.reviews.insert(session_id.clone());
                return Ok(None);
            }
        };
        if !matches!(request, StreamRequest::Subscribe { .. })
            && (self.session != *session || self.generation != generation)
        {
            return Ok(None);
        }
        match request {
            StreamRequest::Subscribe {
                session_id,
                generation,
            } => {
                self.session = session_id;
                self.generation = generation;
                self.newest = None;
                self.details.clear();
                self.reviews.clear();
                self.oldest = if self.session.is_empty() {
                    None
                } else {
                    history_page(snapshot, &self.session, None, None, None)?
                        .history
                        .oldest_message_id
                };
            }
            StreamRequest::Older { before, .. } => {
                if self.oldest.as_deref() != Some(&before) {
                    return Ok(None);
                }
                self.oldest = history_page(snapshot, &self.session, Some(&before), None, None)?
                    .history
                    .oldest_message_id
                    .or_else(|| self.oldest.clone());
            }
            StreamRequest::Newer { after, .. } => {
                if self.newest.as_deref() != Some(&after) {
                    return Ok(None);
                }
                let page = history_page(snapshot, &self.session, None, None, Some(&after))?;
                self.newest = page
                    .history
                    .has_newer
                    .then_some(page.history.newest_message_id)
                    .flatten();
            }
            StreamRequest::Around { message_id, .. } => {
                let page = history_page(snapshot, &self.session, None, Some(&message_id), None)?;
                self.oldest = page.history.oldest_message_id;
                self.newest = page
                    .history
                    .has_newer
                    .then_some(page.history.newest_message_id)
                    .flatten();
                self.details.insert(message_id);
            }
            StreamRequest::Detail { message_id, .. } => {
                if !snapshot
                    .messages
                    .iter()
                    .any(|m| m.session_id == self.session && m.id == message_id)
                {
                    return Err(Error::invalid("Message not found"));
                }
                self.details.insert(message_id);
            }
            StreamRequest::Search {
                query,
                after,
                backwards,
                ..
            } => {
                if query.len() > 4096 {
                    return Err(Error::invalid("Search is too long"));
                }
                let messages = snapshot
                    .messages
                    .iter()
                    .filter(|m| m.session_id == self.session)
                    .collect::<Vec<_>>();
                let hits = messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        m.body.contains(&query)
                            || plain_text(&m.parts).contains(&query)
                            || m.tool.as_ref().is_some_and(|t| t.input.contains(&query))
                    })
                    .collect::<Vec<_>>();
                let position = after
                    .as_ref()
                    .and_then(|id| messages.iter().position(|m| &m.id == id));
                let found = if backwards {
                    hits.iter()
                        .rev()
                        .find(|(i, _)| position.is_none_or(|p| *i < p))
                        .or(hits.last())
                } else {
                    hits.iter()
                        .find(|(i, _)| position.is_none_or(|p| *i > p))
                        .or(hits.first())
                };
                let message_id = found.map(|(_, m)| m.id.clone());
                if let Some(id) = &message_id {
                    self.details.clear();
                    let page = history_page(snapshot, &self.session, None, Some(id), None)?;
                    self.oldest = page.history.oldest_message_id;
                    self.newest = page
                        .history
                        .has_newer
                        .then_some(page.history.newest_message_id)
                        .flatten();
                }
                let snippet = found
                    .map(|(_, m)| search_snippet(m, &query))
                    .unwrap_or_default();
                return Ok(Some(StreamEvent::SearchResult {
                    session_id: self.session.clone(),
                    generation: self.generation,
                    query,
                    message_id,
                    snippet,
                }));
            }
            StreamRequest::Review { .. } => unreachable!(),
        }
        Ok(None)
    }
}

#[derive(Deserialize)]
pub(crate) struct PageQuery {
    before: Option<String>,
    around: Option<String>,
    after: Option<String>,
}

pub(crate) async fn history(
    State(workspace): State<Workspace>,
    ApiPath(session): ApiPath<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<HistoryPage>, Error> {
    history_page(
        &workspace.snapshots.borrow(),
        &session,
        query.before.as_deref(),
        query.around.as_deref(),
        query.after.as_deref(),
    )
    .map(Json)
}

pub(crate) async fn detail(
    State(workspace): State<Workspace>,
    ApiPath((session, id)): ApiPath<(String, String)>,
) -> Result<Json<crate::Message>, Error> {
    workspace
        .snapshots
        .borrow()
        .messages
        .iter()
        .find(|m| m.session_id == session && m.id == id)
        .cloned()
        .map(Json)
        .ok_or_else(|| Error::invalid("Message not found"))
}

pub(crate) async fn workspace(State(workspace): State<Workspace>) -> Json<Snapshot> {
    Json(metadata(&workspace.snapshots.borrow()))
}

fn search_snippet(message: &relay_core::Message, query: &str) -> String {
    let text = if message.body.contains(query) {
        message.body.clone()
    } else if let Some(tool) = &message.tool
        && tool.input.contains(query)
    {
        tool.input.clone()
    } else {
        plain_text(&message.parts)
    };
    let Some(position) = text.find(query) else {
        return String::new();
    };
    let prefix = text[..position]
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let suffix = text[position..].chars().take(160).collect::<String>();
    format!("{prefix}{suffix}")
}

#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    query: String,
    after: Option<String>,
}

pub(crate) async fn search(
    State(workspace): State<Workspace>,
    ApiPath(session): ApiPath<String>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchPage>, Error> {
    if query.query.len() > 4096 {
        return Err(Error::invalid("Search is too long"));
    }
    let snapshot = workspace.snapshots.borrow();
    if !snapshot.sessions.iter().any(|s| s.id == session) {
        return Err(Error::invalid("Session not found"));
    }
    let messages = snapshot
        .messages
        .iter()
        .filter(|m| m.session_id == session)
        .collect::<Vec<_>>();
    let start = query
        .after
        .as_ref()
        .map(|id| {
            messages
                .iter()
                .position(|m| &m.id == id)
                .map(|i| i + 1)
                .ok_or_else(|| Error::invalid("Search cursor not found"))
        })
        .transpose()?
        .unwrap_or(0);
    let mut hits = Vec::new();
    if !query.query.is_empty() {
        for message in &messages[start..] {
            let snippet = search_snippet(message, &query.query);
            if snippet.is_empty() {
                continue;
            }
            hits.push(SearchHit {
                message_id: message.id.clone(),
                snippet,
            });
            if hits.len() == 21 {
                break;
            }
        }
    }
    let next = if hits.len() > 20 {
        hits.truncate(20);
        hits.last().map(|h| h.message_id.clone())
    } else {
        None
    };
    Ok(Json(SearchPage {
        revision: snapshot.revision,
        hits,
        next,
    }))
}

pub(crate) async fn ping() -> impl IntoResponse {
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        StatusCode::NO_CONTENT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::{
        Message as ClientMessage, client::IntoClientRequest, http::HeaderValue,
    };

    fn archive() -> Snapshot {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        snapshot.messages.clear();
        snapshot.comments.clear();
        for index in 0..1000 {
            snapshot.messages.push(relay_core::Message {
                id: format!("history-{index}"),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                sender: Sender::User,
                kind: "assistant".into(),
                body: format!("{index}: {}", "λ".repeat(1800)),
                parts: vec![],
                tool: None,
            });
        }
        snapshot
    }

    fn subscribe(projection: &mut Projection, snapshot: &Snapshot, generation: u64) {
        projection
            .request(
                snapshot,
                StreamRequest::Subscribe {
                    session_id: "session-plan".into(),
                    generation,
                },
            )
            .unwrap();
    }

    #[test]
    fn page_bounds_order_unicode_and_cursor_validation() {
        let mut snapshot = archive();
        snapshot.messages.last_mut().unwrap().body = "🐱".repeat(100_000);
        let page = history_page(&snapshot, "session-plan", None, None, None).unwrap();
        assert_eq!(page.messages.len(), 50);
        assert_eq!(page.messages[0].id, "history-950");
        assert!(page.history.preview_ids.contains("history-999"));
        assert!(serde_json::to_vec(&page).unwrap().len() <= HISTORY_PAGE_BYTES);
        let older = history_page(
            &snapshot,
            "session-plan",
            page.history.oldest_message_id.as_deref(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(older.messages.last().unwrap().id, "history-949");
        let around =
            history_page(&snapshot, "session-plan", None, Some("history-30"), None).unwrap();
        assert!(around.messages.iter().any(|m| m.id == "history-30"));
        assert!(around.history.has_newer);
        assert!(history_page(&snapshot, "session-plan", Some("missing"), None, None).is_err());
        assert!(history_page(&snapshot, "unknown", None, None, None).is_err());
    }

    #[test]
    fn subscribed_changes_coalesce_without_archive_transfers() {
        let mut snapshot = archive();
        let mut projection = Projection::default();
        subscribe(&mut projection, &snapshot, 7);
        let (initial, history) = projection.view(&snapshot);
        assert_eq!(initial.messages.len(), 50);
        assert_eq!(history.generation, 7);
        assert!(metadata(&snapshot).messages.is_empty());
        // Multiple skipped watch revisions still compare against the delivered baseline.
        snapshot.revision += 3;
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str("new live output");
        let (updated, history) = projection.view(&snapshot);
        let event = StreamEvent::Update {
            base_revision: initial.revision,
            revision: updated.revision,
            delta: Delta::between(
                &serde_json::to_value(&initial).unwrap(),
                &serde_json::to_value(&updated).unwrap(),
            ),
            history,
        };
        assert!(serde_json::to_vec(&event).unwrap().len() < 1024);
        let mut actual = initial;
        event.apply(&mut actual).unwrap();
        assert_eq!(actual, updated);
        projection
            .request(
                &snapshot,
                StreamRequest::Older {
                    session_id: "session-plan".into(),
                    generation: 6,
                    before: "history-950".into(),
                },
            )
            .unwrap();
        assert_eq!(projection.view(&snapshot).0.messages.len(), 50);
        projection
            .request(
                &snapshot,
                StreamRequest::Older {
                    session_id: "session-plan".into(),
                    generation: 7,
                    before: "history-950".into(),
                },
            )
            .unwrap();
        assert_eq!(projection.view(&snapshot).0.messages.len(), 100);
        // Search jumps to a bounded window, rather than transferring the gap to the tail.
        projection
            .request(
                &snapshot,
                StreamRequest::Search {
                    session_id: "session-plan".into(),
                    generation: 7,
                    query: "30: ".into(),
                    after: None,
                    backwards: false,
                },
            )
            .unwrap();
        assert_eq!(projection.view(&snapshot).0.messages.len(), 50);
        assert!(projection.view(&snapshot).1.has_newer);
    }

    #[test]
    fn empty_subscription_receives_first_message_and_details_preserve_source() {
        let mut snapshot = archive();
        let message = snapshot.messages.pop().unwrap();
        snapshot.messages.clear();
        let mut projection = Projection::default();
        subscribe(&mut projection, &snapshot, 1);
        snapshot.messages.push(message);
        assert_eq!(projection.view(&snapshot).0.messages.len(), 1);
        snapshot.messages[0].body = "🐱".repeat(10000);
        let (_, history) = projection.view(&snapshot);
        assert!(history.preview_ids.contains("history-999"));
        projection
            .request(
                &snapshot,
                StreamRequest::Detail {
                    session_id: "session-plan".into(),
                    generation: 1,
                    message_id: "history-999".into(),
                },
            )
            .unwrap();
        let (view, history) = projection.view(&snapshot);
        assert!(history.preview_ids.is_empty());
        assert_eq!(view.messages[0], snapshot.messages[0]);
    }

    #[test]
    fn active_run_identity_survives_paging_the_prompt_out_of_view() {
        let mut snapshot = archive();
        snapshot.sessions[0].worker = Some(WorkerRun {
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
        snapshot.messages[0].id = "prompt-active-run".into();
        snapshot.messages[0].kind = "prompt".into();
        let mut projection = Projection::default();
        subscribe(&mut projection, &snapshot, 1);
        let (view, history) = projection.view(&snapshot);
        assert_eq!(view.messages.len(), 50);
        assert!(!view.messages.iter().any(|message| message.kind == "prompt"));
        assert_eq!(
            history
                .active_run_ids
                .get("session-plan")
                .map(String::as_str),
            Some("active-run")
        );
    }

    fn progress_archive() -> Snapshot {
        let mut snapshot = archive();
        snapshot.sessions[0].worker = Some(WorkerRun {
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
        snapshot.messages[0].id = "prompt-active-run".into();
        snapshot.messages[0].kind = "prompt".into();
        let mut summary = snapshot.messages.last().unwrap().clone();
        summary.id = "codex-summary-active-run-item-0".into();
        summary.kind = "reasoning".into();
        summary.body = "Checking the adapter λ".into();
        snapshot.messages.push(summary);
        snapshot
    }

    #[test]
    fn progress_uses_logical_indices_and_latest_first_seen_item() {
        let mut snapshot = progress_archive();
        snapshot.messages.last_mut().unwrap().id = "codex-summary-active-run-item-1".into();
        let mut paragraph = snapshot.messages.last().unwrap().clone();
        paragraph.id = "codex-summary-active-run-item-0".into();
        paragraph.body = "Zero arrived later".into();
        snapshot.messages.push(paragraph.clone());
        assert_eq!(
            progress_summary(&snapshot, "session-plan").unwrap().text,
            "Checking the adapter λ"
        );
        snapshot
            .messages
            .last_mut()
            .unwrap()
            .body
            .push_str(" with a delta");
        assert_eq!(
            progress_summary(&snapshot, "session-plan").unwrap().text,
            "Checking the adapter λ"
        );
        paragraph.id = "codex-summary-active-run-next-item-0".into();
        paragraph.body = "Current item".into();
        snapshot.messages.push(paragraph.clone());
        paragraph.id = "codex-summary-active-run-item-2".into();
        paragraph.body = "Older item late paragraph".into();
        snapshot.messages.push(paragraph.clone());
        assert_eq!(
            progress_summary(&snapshot, "session-plan").unwrap().text,
            "Current item"
        );
        paragraph.id = "codex-summary-active-run-next-item-31".into();
        paragraph.body = "Logical final paragraph".into();
        snapshot.messages.push(paragraph.clone());
        paragraph.id = "codex-summary-active-run-next-item-18446744073709551615".into();
        paragraph.body = "Invalid index".into();
        snapshot.messages.push(paragraph);
        assert_eq!(
            progress_summary(&snapshot, "session-plan").unwrap().text,
            "Logical final paragraph"
        );
    }

    #[test]
    fn progress_metadata_survives_paging_and_rejects_stale_legacy_and_empty_rows() {
        let mut snapshot = progress_archive();
        snapshot.submissions.push(Submission {
            id: "queued-run".into(),
            session_id: "session-plan".into(),
            parts: vec![],
            state: SubmissionState::Queued,
            approve_implementation: false,
            error: None,
            interrupts_run: None,
            last_edit_request: None,
            sender: Sender::User,
        });
        let mut projection = Projection::default();
        subscribe(&mut projection, &snapshot, 1);
        projection
            .request(
                &snapshot,
                StreamRequest::Around {
                    session_id: "session-plan".into(),
                    generation: 1,
                    message_id: "history-30".into(),
                },
            )
            .unwrap();
        let (view, history) = projection.view(&snapshot);
        assert!(
            !view
                .messages
                .iter()
                .any(|m| m.kind == "prompt" || m.kind == "reasoning")
        );
        assert_eq!(
            history.progress_summary.as_ref().unwrap().text,
            "Checking the adapter λ"
        );
        let mut stale = snapshot.messages.last().unwrap().clone();
        stale.id = "codex-summary-prior-run-item-0".into();
        stale.body = "Stale completion".into();
        snapshot.messages.push(stale.clone());
        stale.id = "legacy-uuid".into();
        stale.body = "Uncertified reasoning".into();
        snapshot.messages.push(stale);
        snapshot
            .messages
            .iter_mut()
            .find(|m| m.id == "codex-summary-active-run-item-0")
            .unwrap()
            .body
            .push_str(" streamed");
        let (updated, history) = projection.view(&snapshot);
        assert_eq!(updated.messages, view.messages);
        assert_eq!(
            history.progress_summary.as_ref().unwrap().text,
            "Checking the adapter λ streamed"
        );
        snapshot
            .messages
            .iter_mut()
            .find(|m| m.id == "codex-summary-active-run-item-0")
            .unwrap()
            .body = "🐱".repeat(10000);
        assert!(
            projection
                .view(&snapshot)
                .1
                .progress_summary
                .unwrap()
                .text
                .len()
                <= 4096
        );
        snapshot
            .messages
            .iter_mut()
            .find(|m| m.id == "codex-summary-active-run-item-0")
            .unwrap()
            .body
            .clear();
        assert!(projection.view(&snapshot).1.progress_summary.is_none());
        snapshot
            .messages
            .iter_mut()
            .find(|m| m.id == "codex-summary-active-run-item-0")
            .unwrap()
            .body = "Current".into();
        snapshot.messages[0].id = "prompt-next-run".into();
        assert!(projection.view(&snapshot).1.progress_summary.is_none());
        snapshot.messages[0].id = "prompt-active-run".into();
        for status in [
            WorkerStatus::Completed,
            WorkerStatus::Failed,
            WorkerStatus::Interrupted,
            WorkerStatus::Stopped,
            WorkerStatus::Queued,
        ] {
            snapshot.sessions[0].worker.as_mut().unwrap().status = status;
            assert!(projection.view(&snapshot).1.progress_summary.is_none());
        }
    }

    fn tool(kind: ToolKind) -> relay_core::ToolCall {
        relay_core::ToolCall {
            run_id: "run".into(),
            name: format!("{kind:?}"),
            kind,
            state: ToolState::Running,
            input: String::new(),
            target: String::new(),
            directory: None,
            exit_code: None,
            started_ms: None,
            finished_ms: None,
            detail: None,
        }
    }

    /// 1000 messages: every fourth a typed Read/Edit/Run call, every tenth an
    /// older transcript's untyped command, plus another session's call.
    fn tool_archive() -> Snapshot {
        let mut snapshot = archive();
        let kinds = [ToolKind::Read, ToolKind::Edit, ToolKind::Run];
        for (index, message) in snapshot.messages.iter_mut().enumerate() {
            if index % 10 == 5 {
                message.kind = "command_execution".into();
                message.body = "Command: ls\nExit: 0\nok".into();
            } else if index % 4 == 0 {
                message.kind = "tool".into();
                message.tool = Some(tool(kinds[(index / 4) % 3]));
            }
        }
        let mut other = snapshot.messages[0].clone();
        other.id = "other-tool".into();
        other.session_id = "session-worker".into();
        snapshot.messages.push(other);
        snapshot
    }

    fn counts(history: &HistoryState) -> Vec<(ToolKind, u32)> {
        history
            .tool_counts
            .as_ref()
            .unwrap()
            .iter()
            .map(|c| (c.kind, c.count))
            .collect()
    }

    #[test]
    fn tool_counts_cover_the_whole_transcript_and_follow_new_tool_events() {
        let mut snapshot = tool_archive();
        // 250 typed calls cycle Read/Edit/Run; 100 legacy commands count as Run.
        let whole = vec![
            (ToolKind::Read, 84),
            (ToolKind::Edit, 83),
            (ToolKind::Run, 83 + 100),
        ];
        let mut projection = Projection::default();
        subscribe(&mut projection, &snapshot, 1);
        let (view, history) = projection.view(&snapshot);
        assert_eq!(view.messages.len(), 50);
        assert_eq!(counts(&history), whole);
        // Paging to older history changes the loaded window, not the counts.
        let oldest = history.oldest_message_id.unwrap();
        projection
            .request(
                &snapshot,
                StreamRequest::Older {
                    session_id: "session-plan".into(),
                    generation: 1,
                    before: oldest,
                },
            )
            .unwrap();
        let (view, history) = projection.view(&snapshot);
        assert!(view.messages.len() > 50);
        assert_eq!(counts(&history), whole);
        let page = history_page(&snapshot, "session-plan", None, Some("history-30"), None).unwrap();
        assert_eq!(counts(&page.history), whole);
        // A new call counts once, in the update that carries it.
        let mut call = snapshot.messages[0].clone();
        call.id = "tool-new".into();
        call.tool = Some(tool(ToolKind::Search));
        snapshot.messages.push(call);
        let (_, history) = projection.view(&snapshot);
        assert_eq!(counts(&history)[3], (ToolKind::Search, 1));
        // Finishing it and refining its category moves, never duplicates, the call.
        let last = snapshot.messages.last_mut().unwrap().tool.as_mut().unwrap();
        last.state = ToolState::Completed;
        last.kind = ToolKind::Test;
        let (_, history) = projection.view(&snapshot);
        let total: u32 = counts(&history).iter().map(|(_, n)| n).sum();
        assert_eq!(total, 351);
        assert_eq!(counts(&history)[3], (ToolKind::Test, 1));
        assert!(!counts(&history).iter().any(|(k, _)| *k == ToolKind::Search));
    }

    #[tokio::test]
    async fn stored_transcripts_report_whole_tool_counts_after_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut store = Store::open(&path, AgentProfile::default()).unwrap();
        store.save(&tool_archive()).unwrap();
        drop(store);
        let token = uuid::Uuid::new_v4().to_string();
        let app = router(&path, token.clone(), AgentProfile::default()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let page: HistoryPage = reqwest::Client::new()
            .get(format!(
                "http://{address}/v1/sessions/session-plan/history?around=history-30"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(page.messages.len() < 1000);
        assert_eq!(
            counts(&page.history),
            vec![
                (ToolKind::Read, 84),
                (ToolKind::Edit, 83),
                (ToolKind::Run, 183)
            ]
        );
        server.abort();
    }

    #[tokio::test]
    async fn progress_metadata_crosses_http_and_websocket_with_unloaded_live_tail() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut store = Store::open(&path, AgentProfile::default()).unwrap();
        store.save(&progress_archive()).unwrap();
        drop(store);
        let token = uuid::Uuid::new_v4().to_string();
        let app = router(&path, token.clone(), AgentProfile::default()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let page: HistoryPage = client
            .get(format!(
                "http://{address}/v1/sessions/session-plan/history?around=history-30"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!page.messages.iter().any(|m| m.kind == "reasoning"));
        assert_eq!(
            page.history.progress_summary.unwrap().text,
            "Checking the adapter λ"
        );
        let mut request = format!("ws://{address}/v1/events")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let mut state = Snapshot::default();
        let event: StreamEvent =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        event.apply(&mut state).unwrap();
        for request in [
            StreamRequest::Subscribe {
                session_id: "session-plan".into(),
                generation: 1,
            },
            StreamRequest::Around {
                session_id: "session-plan".into(),
                generation: 1,
                message_id: "history-30".into(),
            },
        ] {
            socket
                .send(ClientMessage::Text(
                    serde_json::to_string(&request).unwrap().into(),
                ))
                .await
                .unwrap();
            let event: StreamEvent =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            let history = event.apply(&mut state).unwrap().unwrap();
            assert_eq!(
                history.progress_summary.unwrap().text,
                "Checking the adapter λ"
            );
        }
        assert!(!state.messages.iter().any(|m| m.kind == "reasoning"));
        server.abort();
    }

    #[test]
    fn protocol_three_migration_preserves_archive_receipts_and_drafts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut store = Store::open(&path, AgentProfile::default()).unwrap();
        let mut original = archive();
        original.protocol_version = 3;
        store.save(&original).unwrap();
        store.connection.execute_batch("PRAGMA user_version=10; INSERT INTO receipts VALUES('preserved','request'); INSERT INTO drafts VALUES('preserved','draft');").unwrap();
        drop(store);
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        let migrated = store.snapshot().unwrap();
        original.protocol_version = PROTOCOL_VERSION;
        assert_eq!(migrated, original);
        for table in ["receipts", "drafts"] {
            let count: u32 = store
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1);
        }
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .unwrap(),
            21
        );
    }

    #[test]
    fn schema_eleven_migration_loads_protocol_four_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        let mut value = serde_json::to_value(archive()).unwrap();
        value["protocol_version"] = 4.into();
        value.as_object_mut().unwrap().remove("usage_limits");
        value.as_object_mut().unwrap().remove("usage_ledger");
        for session in value["sessions"].as_array_mut().unwrap() {
            for field in ["parent_session_id", "spawn", "compactions"] {
                session.as_object_mut().unwrap().remove(field);
            }
        }
        value["messages"][0]["tool"] = serde_json::json!({"run_id":"r","name":"Bash","kind":"terminal","state":"completed","input":"ls","target":"ls","directory":null,"exit_code":0});
        value["messages"][1]["tool"] = serde_json::json!({"run_id":"r","name":"WebSearch","kind":"web","state":"completed","input":"{}","target":"q","directory":null,"exit_code":null});
        store
            .connection
            .execute(
                "UPDATE workspace SET snapshot=?1 WHERE id=1",
                [value.to_string()],
            )
            .unwrap();
        store
            .connection
            .execute_batch(
                "PRAGMA user_version=11; INSERT INTO receipts VALUES('preserved','request');",
            )
            .unwrap();
        drop(store);
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        let migrated = store.snapshot().unwrap();
        assert_eq!(migrated.protocol_version, PROTOCOL_VERSION);
        assert_eq!(migrated.messages.len(), 1000);
        assert_eq!(
            migrated.messages[0].tool.as_ref().unwrap().kind,
            ToolKind::Run
        );
        assert_eq!(
            migrated.messages[1].tool.as_ref().unwrap().kind,
            ToolKind::Fetch
        );
        assert!(migrated.messages[0].tool.as_ref().unwrap().detail.is_none());
        assert!(migrated.usage_limits.is_empty() && migrated.usage_ledger.is_empty());
        assert!(
            migrated
                .sessions
                .iter()
                .all(|s| s.parent_session_id.is_none()
                    && s.spawn.is_none()
                    && s.compactions.is_empty())
        );
        let stored: String = store
            .connection
            .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!stored.contains("\"terminal\""));
        let receipts: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(receipts, 1);
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .unwrap(),
            21
        );
    }

    #[test]
    fn usage_records_are_published_as_record_deltas() {
        let mut old = metadata(&archive());
        old.protocol_version = PROTOCOL_VERSION;
        let mut new = old.clone();
        new.revision += 1;
        new.record_usage_limit(UsageLimit::new(
            Harness::Codex,
            UsageWindow::FiveHour,
            12.5,
            300,
            Some(100),
            1,
        ));
        new.record_usage(
            UsageEntry {
                id: "run".into(),
                session_id: "session-plan".into(),
                harness: Harness::ClaudeCode,
                model: Some("model".into()),
                input_tokens: 10,
                cached_input_tokens: 2,
                output_tokens: 3,
                completed_at: 1,
            },
            1,
        );
        let projected = metadata(&new);
        assert_eq!(projected.usage_limits, new.usage_limits);
        assert_eq!(projected.usage_ledger, new.usage_ledger);
        let delta = relay_core::Delta::between(
            &serde_json::to_value(&old).unwrap(),
            &serde_json::to_value(&projected).unwrap(),
        );
        let text = serde_json::to_string(&delta).unwrap();
        assert!(
            text.contains("\"usage_limits\":{\"op\":\"records\""),
            "{text}"
        );
        assert!(
            text.contains("\"usage_ledger\":{\"op\":\"records\""),
            "{text}"
        );
        let mut client = old.clone();
        relay_core::StreamEvent::Update {
            base_revision: old.revision,
            revision: new.revision,
            delta,
            history: Default::default(),
        }
        .apply(&mut client)
        .unwrap();
        assert_eq!(client, projected);
    }

    #[tokio::test]
    async fn small_acknowledgements_and_probes_work_over_throttled_http_and_websocket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut store = Store::open(&path, AgentProfile::default()).unwrap();
        let snapshot = archive();
        assert!(serde_json::to_vec(&snapshot).unwrap().len() > 3_350_000);
        store.save(&snapshot).unwrap();
        drop(store);
        let token = uuid::Uuid::new_v4().to_string();
        let app = router(&path, token.clone(), AgentProfile::default()).unwrap();
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            axum::serve(server, app).await.unwrap();
        });
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let Ok((incoming, _)) = proxy.accept().await else {
                    break;
                };
                connections.spawn(async move {
                    let outgoing = tokio::net::TcpStream::connect(address).await.unwrap();
                    let (mut source, mut sink) = incoming.into_split();
                    let (mut response, mut request) = outgoing.into_split();
                    let upload = async {
                        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
                        let _ = tokio::io::copy(&mut source, &mut request).await;
                    };
                    let download = async {
                        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
                        let mut buffer = [0; 1250];
                        loop {
                            let Ok(length) = response.read(&mut buffer).await else {
                                break;
                            };
                            if length == 0 {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs_f64(
                                length as f64 / 125_000.0,
                            ))
                            .await;
                            if sink.write_all(&buffer[..length]).await.is_err() {
                                break;
                            }
                        }
                    };
                    tokio::select! {_=upload=>{},_=download=>{}};
                });
            }
        });
        let client = reqwest::Client::new();
        let base = format!("http://{proxy_address}");
        let started = tokio::time::Instant::now();
        let response = client
            .get(format!("{base}/v1/ping"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(response.bytes().await.unwrap().is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let baseline: Snapshot = client
            .get(format!("{base}/v1/workspace"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let baseline_revision = baseline.revision;
        let command = CommandEnvelope {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: baseline_revision,
            command: Command::AddComment {
                message_id: "history-999".into(),
                quote: None,
                author: "Reviewer".into(),
                body: "Small acknowledgement".into(),
            },
        };
        let started = tokio::time::Instant::now();
        let response = client
            .post(format!("{base}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&command)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let bytes = response.bytes().await.unwrap();
        assert!(bytes.len() < 1024);
        let ack: CommandAck = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(ack.request_id, command.request_id);
        assert_eq!(ack.committed_revision, baseline_revision + 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let mut request = format!("ws://{proxy_address}/v1/events")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let reset = socket.next().await.unwrap().unwrap();
        assert!(reset.len() < 32 * 1024);
        let mut state = Snapshot::default();
        let reset: StreamEvent = serde_json::from_str(&reset.into_text().unwrap()).unwrap();
        reset.apply(&mut state).unwrap();
        assert!(state.messages.is_empty());
        socket
            .send(ClientMessage::Text(
                serde_json::to_string(&StreamRequest::Subscribe {
                    session_id: "session-plan".into(),
                    generation: 1,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let page = socket.next().await.unwrap().unwrap();
        assert!(page.len() < HISTORY_PAGE_BYTES);
        let event: StreamEvent = serde_json::from_str(&page.into_text().unwrap()).unwrap();
        event.apply(&mut state).unwrap();
        assert_eq!(state.messages.len(), 50);
        let page: HistoryPage = client
            .get(format!(
                "{base}/v1/sessions/session-plan/history?around=history-30"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(page.messages.iter().any(|m| m.id == "history-30"));
        assert!(serde_json::to_vec(&page).unwrap().len() < HISTORY_PAGE_BYTES);
        let results: SearchPage = client
            .get(format!(
                "{base}/v1/sessions/session-plan/search?query=30%3A"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(
            results
                .hits
                .iter()
                .any(|hit| hit.message_id == "history-30")
        );
        let invalid = client
            .get(format!(
                "{base}/v1/sessions/session-plan/history?before=history-50&after=history-30"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), 422);
        // A lost acknowledgement is retried verbatim without another mutation.
        let repeated: CommandAck = client
            .post(format!("{base}/v1/commands"))
            .bearer_auth(&token)
            .header("x-relay-protocol", PROTOCOL_VERSION.to_string())
            .json(&command)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(repeated.committed_revision, baseline_revision + 1);
        socket.close(None).await.unwrap();
        proxy_task.abort();
        server_task.abort();
    }
}
