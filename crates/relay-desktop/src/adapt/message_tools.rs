//! Join message delivery receipts to observed calls without changing stored history.
use relay_core::{Message, ToolDetail, ToolKind, ToolState};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The equality cutoff for the shared index. Keep complete records: replay
/// deduplication and conflicting receipts depend on more than count or ID.
pub fn message_tool_sources(messages: &[Message]) -> Vec<Message> {
    messages.iter().filter(|m| is_source(m)).cloned().collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Work {
    #[cfg(test)]
    visits: usize,
    #[cfg(test)]
    decodes: usize,
}

impl Work {
    fn visit(&mut self) {
        #[cfg(test)]
        {
            self.visits += 1;
        }
    }

    fn json(&mut self, text: &str) -> Option<Value> {
        #[cfg(test)]
        {
            self.decodes += 1;
        }
        serde_json::from_str(text).ok()
    }
}

pub(super) fn is_send(message: &Message) -> bool {
    message.tool.as_ref().is_some_and(|tool| {
        tool.kind == ToolKind::Message
            && matches!(
                tool.name.as_str(),
                "mcp__relay__message_agent" | "message_agent"
            )
    })
}

fn legacy_send(message: &Message) -> bool {
    message.tool.is_none()
        && message.kind == "command_execution"
        && message
            .body
            .split_once('\n')
            .is_some_and(|(name, _)| matches!(name, "mcp__relay__message_agent" | "message_agent"))
}

fn is_source(message: &Message) -> bool {
    is_send(message) || legacy_send(message)
}

fn sent(args: &Value, read: Option<bool>) -> Option<SentMessage> {
    Some(SentMessage {
        target: args["agent"].as_str().map(str::to_owned),
        text: args["text"].as_str()?.to_owned(),
        read,
    })
}

/// The message subset of with_legacy_tool, decoded once for both transcript
/// presentation and details. Unknown legacy outcomes never gain a run/receipt.
fn legacy_view(source: &Message, work: &mut Work) -> (Message, Option<SentMessage>) {
    let (name, input) = source.body.split_once('\n').unwrap();
    let args = work.json(input).filter(Value::is_object);
    let exchange = args.as_ref().and_then(|args| sent(args, None));
    let target = args.as_ref().map(|args| {
        [
            "command",
            "file_path",
            "path",
            "pattern",
            "query",
            "url",
            "description",
        ]
        .into_iter()
        .find_map(|key| args[key].as_str())
        .unwrap_or(name)
        .to_owned()
    });
    let valid = args.is_some();
    let mut view = source.clone();
    view.body = if valid {
        String::new()
    } else {
        source.body.clone()
    };
    view.tool = Some(relay_core::ToolCall {
        run_id: String::new(),
        name: if valid {
            name.into()
        } else {
            "Recorded tool".into()
        },
        kind: if valid {
            ToolKind::Message
        } else {
            ToolKind::Other
        },
        state: ToolState::Unknown,
        input: if valid { input.into() } else { String::new() },
        target: target.unwrap_or_else(|| "Recorded tool output".into()),
        directory: None,
        exit_code: None,
        started_ms: None,
        finished_ms: None,
        detail: None,
    });
    (view, exchange)
}

fn input(message: &Message, work: &mut Work) -> Option<Value> {
    let input = &message.tool.as_ref()?.input;
    if input.is_empty() {
        return None;
    }
    let value = work.json(input)?;
    // Codex may serialize arguments as a JSON string; Claude records an object.
    let value = match value.as_str() {
        Some(text) => work.json(text)?,
        None => value,
    };
    value.is_object().then_some(value)
}

/// Only the direct tool result and MCP's structured/text result envelopes are
/// receipts. Never search arbitrary nested queue data or the sent text for IDs.
fn submission(message: &Message, work: &mut Work) -> Option<String> {
    if message.tool.as_ref()?.state != ToolState::Completed || message.body.is_empty() {
        return None;
    }
    let result = work.json(&message.body)?;
    if result["isError"] == true {
        return None;
    }
    let mut values = vec![&result];
    if let Some(structured) = result.get("structuredContent") {
        values.push(structured);
    }
    let text_results: Vec<Value> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| work.json(block["text"].as_str()?))
        .collect();
    values.extend(&text_results);
    let mut ids = BTreeSet::new();
    for value in values {
        if let Some(id) = value.get("submission") {
            let id = id.as_str()?;
            if value["queued"] != true || uuid::Uuid::parse_str(id).is_err() {
                return None;
            }
            ids.insert(id.to_owned());
        }
    }
    (ids.len() == 1).then(|| ids.pop_first().unwrap())
}

fn synthetic(message: &Message) -> bool {
    is_send(message)
        && message.author == "Relay"
        && message.kind == "tool"
        && message.id.starts_with("agent-message-")
        && message.body.is_empty()
        && message.parts.is_empty()
        && message.tool.as_ref().is_some_and(|tool| {
            tool.input.is_empty()
                && tool.state == ToolState::Completed
                && matches!(tool.detail, Some(ToolDetail::Message { .. }))
        })
}

type DeliveryKey = (String, String, String);

fn key(message: &Message, receipt_id: String) -> Option<DeliveryKey> {
    let run = &message.tool.as_ref()?.run_id;
    (!message.session_id.is_empty() && !run.is_empty())
        .then(|| (message.session_id.clone(), run.clone(), receipt_id))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SentMessage {
    pub target: Option<String>,
    pub text: String,
    pub read: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    source: Message,
    legacy: Option<Message>,
    sent: Option<SentMessage>,
    hidden: bool,
}

impl Row {
    fn matches(&self, message: &Message) -> bool {
        self.source == *message || self.legacy.as_ref() == Some(message)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Variants {
    first: usize,
    // Only conflicting copies of the same session/row ID need a structural
    // key. Ordered serialized keys avoid scanning even a large corrupt bucket.
    others: BTreeMap<Vec<u8>, usize>,
}

fn structural_key(message: &Message) -> Vec<u8> {
    serde_json::to_vec(message).expect("transcript messages serialize")
}

/// One parsed correlation projection shared by transcript selection and tool
/// details. Construction visits records a bounded number of times; lookups
/// never parse JSON, rebuild the index, or scan the loaded history.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MessageTools {
    rows: Vec<Row>,
    identities: BTreeMap<String, BTreeMap<String, Variants>>,
    #[cfg(test)]
    work: Work,
    #[cfg(test)]
    reads: std::cell::Cell<usize>,
}

impl MessageTools {
    pub fn new(messages: &[Message]) -> Self {
        let mut result = Self::default();
        let mut work = Work::default();
        for message in messages {
            work.visit();
            if !is_source(message) || result.slot(message).is_some() {
                continue;
            }
            let (legacy, sent) = if legacy_send(message) {
                let (view, sent) = legacy_view(message, &mut work);
                (Some(view), sent)
            } else {
                (None, None)
            };
            let slot = result.rows.len();
            let identities = result
                .identities
                .entry(message.session_id.clone())
                .or_default();
            match identities.entry(message.id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Variants {
                        first: slot,
                        others: BTreeMap::new(),
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().others.insert(structural_key(message), slot);
                    if let Some(view) = &legacy {
                        entry.get_mut().others.insert(structural_key(view), slot);
                    }
                }
            }
            result.rows.push(Row {
                source: message.clone(),
                legacy,
                sent,
                hidden: false,
            });
        }
        let mut native_ids: BTreeMap<DeliveryKey, BTreeSet<String>> = BTreeMap::new();
        let mut receipts: BTreeMap<DeliveryKey, (usize, bool)> = BTreeMap::new();
        let mut candidates = Vec::new();
        for (slot, row) in result.rows.iter_mut().enumerate() {
            work.visit();
            let submission = submission(&row.source, &mut work);
            if row.legacy.is_none() {
                row.sent = input(&row.source, &mut work)
                    .and_then(|args| sent(&args, submission.as_ref().map(|_| false)));
            }
            let candidate =
                submission.and_then(|id| key(&row.source, format!("agent-message-{id}")));
            if let Some(key) = &candidate {
                // Even a call with missing arguments makes a repeated result
                // ambiguous. Distinct native IDs, never text, identify sends.
                native_ids
                    .entry(key.clone())
                    .or_default()
                    .insert(row.source.id.clone());
            }
            candidates.push(candidate);
            if synthetic(&row.source)
                && let Some(key) = key(&row.source, row.source.id.clone())
            {
                receipts
                    .entry(key)
                    .and_modify(|(_, conflict)| *conflict = true)
                    .or_insert((slot, false));
            }
        }
        let mut matches = Vec::new();
        for (slot, row) in result.rows.iter().enumerate() {
            work.visit();
            let Some(candidate) = &candidates[slot] else {
                continue;
            };
            let Some(target) = row.sent.as_ref().and_then(|sent| sent.target.as_ref()) else {
                continue;
            };
            if native_ids[candidate].len() != 1 {
                continue;
            }
            let Some(&(receipt, false)) = receipts.get(candidate) else {
                continue;
            };
            work.visit();
            if let Some(ToolDetail::Message {
                target_session_id,
                read,
            }) = &result.rows[receipt].source.tool.as_ref().unwrap().detail
                && target_session_id
                    .as_ref()
                    .is_none_or(|recipient| recipient == target)
            {
                matches.push((slot, receipt, *read));
            }
        }
        for (slot, receipt, read) in matches {
            work.visit();
            result.rows[slot].sent.as_mut().unwrap().read = Some(read);
            work.visit();
            result.rows[receipt].hidden = true;
        }
        #[cfg(test)]
        {
            work.visits += result.reads.get();
            result.work = work;
            result.reads.set(0);
        }
        result
    }

    fn row(&self, slot: usize) -> &Row {
        #[cfg(test)]
        self.reads.set(self.reads.get() + 1);
        &self.rows[slot]
    }

    fn slot(&self, message: &Message) -> Option<usize> {
        if !is_source(message) {
            return None;
        }
        let group = self.identities.get(&message.session_id)?.get(&message.id)?;
        if self.row(group.first).matches(message) {
            return Some(group.first);
        }
        let slot = *group.others.get(&structural_key(message))?;
        // Keep full equality as the final guard for conflicting copies.
        self.row(slot).matches(message).then_some(slot)
    }

    pub(super) fn sent(&self, message: &Message) -> Option<&SentMessage> {
        self.row(self.slot(message)?).sent.as_ref()
    }

    pub fn presentation(&self, message: &Message) -> Message {
        if legacy_send(message)
            && let Some(slot) = self.slot(message)
            && let Some(view) = &self.row(slot).legacy
        {
            return view.clone();
        }
        crate::tool_activity::with_legacy_tool(message.clone())
    }

    pub fn transcript_messages<'a>(
        &self,
        messages: &'a [Message],
        session: &str,
    ) -> Vec<&'a Message> {
        let mut seen = BTreeSet::new();
        relay_core::transcript_messages(messages, session)
            .into_iter()
            .filter(|message| {
                // Untyped legacy records were never deduplicated or hidden.
                if !is_send(message) {
                    return true;
                }
                self.slot(message)
                    .is_none_or(|slot| !self.row(slot).hidden && seen.insert(slot))
            })
            .collect()
    }
}

#[cfg(test)]
pub fn transcript_messages<'a>(messages: &'a [Message], session: &str) -> Vec<&'a Message> {
    MessageTools::new(messages).transcript_messages(messages, session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapt::{message_pair, tool_view};
    use relay_core::Snapshot;
    use relay_ui::prelude::{Opening, ToolDetail as View};

    #[test]
    fn legacy_message_presentation_is_cached_and_preserves_unknown_fallbacks() {
        let (observed, receipt) =
            message_pair("00000000-0000-4000-8000-000000000001", "legacy-send");
        for input in [
            observed.tool.as_ref().unwrap().input.as_str(),
            "{}",
            "[1]",
            "{malformed",
        ] {
            let mut legacy = observed.clone();
            legacy.tool = None;
            legacy.kind = "command_execution".into();
            legacy.body = format!("mcp__relay__message_agent\n{input}");
            let messages = vec![legacy.clone(), receipt.clone(), legacy.clone()];
            let index = MessageTools::new(&messages);
            assert_eq!(index.work.decodes, 1);
            assert_eq!(
                index.transcript_messages(&messages, "session-plan").len(),
                3
            );
            let expected = crate::tool_activity::with_legacy_tool(legacy.clone());
            for _ in 0..20 {
                let view = index.presentation(&legacy);
                assert_eq!(view, expected);
                let detail = tool_view(
                    &Snapshot::default(),
                    &view,
                    &index,
                    Opening::Expanded,
                    &BTreeMap::new(),
                )
                .unwrap();
                if input == observed.tool.as_ref().unwrap().input {
                    assert!(matches!(
                        detail.detail,
                        Some(View::Message {
                            read: None,
                            exchange: Some(_),
                            ..
                        })
                    ));
                }
            }
            assert_eq!(
                index.work.decodes, 1,
                "legacy presentation/detail must not reparse"
            );
        }
    }

    #[test]
    fn long_history_parses_each_send_once_and_shares_projection_with_details() {
        const SENDS: usize = 5_000;
        let mut messages = Vec::new();
        for n in 0..SENDS {
            let id = format!("00000000-0000-4000-8000-{n:012x}");
            let (observed, receipt) = message_pair(&id, &format!("native-{n}"));
            let mut prose = observed.clone();
            prose.id = format!("prose-{n}");
            prose.tool = None;
            prose.kind = "assistant".into();
            messages.extend([prose, receipt, observed]);
        }
        // Exact event replay must not add parser work or duplicate rows.
        messages.extend(messages[..90].to_vec());
        let index = MessageTools::new(&messages);
        assert_eq!(index.work.decodes, 2 * SENDS);
        assert!(index.work.visits <= 8 * messages.len());
        let visible = index.transcript_messages(&messages, "session-plan");
        assert_eq!(visible.iter().filter(|m| m.tool.is_some()).count(), SENDS);
        let snapshot = Snapshot::default();
        let catalogs = BTreeMap::new();
        for observed in visible.iter().filter(|m| m.tool.is_some()) {
            let view =
                tool_view(&snapshot, observed, &index, Opening::Expanded, &catalogs).unwrap();
            assert!(matches!(
                view.detail,
                Some(View::Message {
                    read: Some(false),
                    exchange: Some(_),
                    ..
                })
            ));
        }
        assert_eq!(
            index.work.decodes,
            2 * SENDS,
            "projection/details must reuse decoded results"
        );
        assert!(index.reads.get() <= 2 * (messages.len() + SENDS));
        eprintln!(
            "message index budget: loaded={}, sends={SENDS}, build_visits={}, JSON_decodes={}, lookup_record_reads={}",
            messages.len(),
            index.work.visits,
            index.work.decodes,
            index.reads.get()
        );
    }

    #[test]
    fn conflicting_copies_use_indexed_variants_and_preserve_exact_replay_semantics() {
        const COPIES: usize = 2_000;
        let (observed, receipt) =
            message_pair("00000000-0000-4000-8000-000000000001", "one-native-id");
        let mut messages = vec![receipt];
        for n in 0..COPIES {
            let mut variant = observed.clone();
            variant.body = serde_json::json!({"submission": "00000000-0000-4000-8000-000000000001", "queued": true, "queue": {"pending": n}}).to_string();
            messages.push(variant);
        }
        messages.push(messages[COPIES].clone());
        let index = MessageTools::new(&messages);
        assert_eq!(index.work.decodes, 2 * COPIES);
        assert!(index.work.visits <= 8 * messages.len());
        assert_eq!(
            index.transcript_messages(&messages, "session-plan").len(),
            COPIES
        );
        assert!(index.reads.get() <= 3 * messages.len());
        // The exact same result on a DISTINCT native ID is ambiguous.
        messages.last_mut().unwrap().id = "different-native-id".into();
        let ambiguous = MessageTools::new(&messages);
        assert_eq!(
            ambiguous
                .transcript_messages(&messages, "session-plan")
                .len(),
            COPIES + 2
        );
    }
}
