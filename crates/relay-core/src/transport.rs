//! Wire projections and revisioned updates. Persistent snapshots remain complete.
use crate::{PROTOCOL_VERSION, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const HISTORY_PAGE_MESSAGES: usize = 50;
pub const HISTORY_PAGE_BYTES: usize = 256 * 1024;
pub const MESSAGE_PREVIEW_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandAck {
    pub request_id: String,
    pub committed_revision: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryState {
    pub session_id: String,
    pub oldest_message_id: Option<String>,
    pub has_more: bool,
    pub newest_message_id: Option<String>,
    pub has_newer: bool,
    pub preview_ids: BTreeSet<String>,
    pub generation: u64,
    /// Workspace control metadata remains available when prompts are outside loaded history.
    pub active_run_ids: BTreeMap<String, String>,
    /// Bounded provider summary for the selected session's active run, independent of paging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_summary: Option<Box<ProgressSummary>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressSummary {
    pub run_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryPage {
    pub revision: u64,
    pub messages: Vec<crate::Message>,
    pub comments: Vec<crate::Comment>,
    pub submissions: Vec<crate::Submission>,
    pub history: HistoryState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub message_id: String,
    pub snippet: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchPage {
    pub revision: u64,
    pub hits: Vec<SearchHit>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamRequest {
    Subscribe {
        session_id: String,
        generation: u64,
    },
    Older {
        session_id: String,
        generation: u64,
        before: String,
    },
    Newer {
        session_id: String,
        generation: u64,
        after: String,
    },
    Around {
        session_id: String,
        generation: u64,
        message_id: String,
    },
    Detail {
        session_id: String,
        generation: u64,
        message_id: String,
    },
    Review {
        session_id: String,
    },
    Search {
        session_id: String,
        generation: u64,
        query: String,
        after: Option<String>,
        backwards: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    Reset {
        protocol_version: u32,
        snapshot: Box<Snapshot>,
        history: HistoryState,
    },
    Update {
        base_revision: u64,
        revision: u64,
        delta: Delta,
        history: HistoryState,
    },
    SearchResult {
        session_id: String,
        generation: u64,
        query: String,
        message_id: Option<String>,
        snippet: String,
    },
    Error {
        message: String,
        generation: u64,
    },
}

/// Arrays of records are keyed by stable ID; append-only strings send only their suffix.
/// Other values use replacement. This also handles nested tool and worker state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Delta {
    Replace {
        value: Value,
    },
    Append {
        suffix: String,
    },
    Object {
        fields: BTreeMap<String, Delta>,
        removed: Vec<String>,
    },
    Records {
        upsert: BTreeMap<String, Delta>,
        removed: Vec<String>,
        order: Option<Vec<String>>,
    },
}

fn records(values: &[Value]) -> Option<BTreeMap<String, &Value>> {
    let mut result = BTreeMap::new();
    for value in values {
        let id = value.get("id")?.as_str()?.to_owned();
        if result.insert(id, value).is_some() {
            return None;
        }
    }
    Some(result)
}

impl Delta {
    pub fn between(old: &Value, new: &Value) -> Self {
        match (old, new) {
            (Value::String(a), Value::String(b)) if b.starts_with(a) => Self::Append {
                suffix: b[a.len()..].into(),
            },
            (Value::Object(a), Value::Object(b)) => Self::Object {
                fields: b
                    .iter()
                    .filter(|(k, v)| a.get(*k) != Some(*v))
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            a.get(k).map_or_else(
                                || Self::Replace { value: v.clone() },
                                |old| Self::between(old, v),
                            ),
                        )
                    })
                    .collect(),
                removed: a.keys().filter(|k| !b.contains_key(*k)).cloned().collect(),
            },
            (Value::Array(a), Value::Array(b)) if records(a).is_some() && records(b).is_some() => {
                let old = records(a).unwrap();
                let new = records(b).unwrap();
                let old_order = a
                    .iter()
                    .map(|v| v["id"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                let new_order = b
                    .iter()
                    .map(|v| v["id"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                Self::Records {
                    upsert: new
                        .iter()
                        .filter(|(k, v)| old.get(*k) != Some(*v))
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                old.get(k).map_or_else(
                                    || Self::Replace {
                                        value: (*v).clone(),
                                    },
                                    |old| Self::between(old, v),
                                ),
                            )
                        })
                        .collect(),
                    removed: old
                        .keys()
                        .filter(|k| !new.contains_key(*k))
                        .cloned()
                        .collect(),
                    order: (old_order != new_order).then_some(new_order),
                }
            }
            _ => Self::Replace { value: new.clone() },
        }
    }

    pub fn apply(&self, value: &mut Value) -> Result<(), String> {
        match self {
            Self::Replace { value: new } => *value = new.clone(),
            Self::Append { suffix } => value
                .as_str()
                .map(|s| format!("{s}{suffix}"))
                .map(|s| *value = Value::String(s))
                .ok_or("Invalid text update")?,
            Self::Object { fields, removed } => {
                let object = value.as_object_mut().ok_or("Invalid object update")?;
                for key in removed {
                    object.remove(key);
                }
                for (key, delta) in fields {
                    delta.apply(object.entry(key.clone()).or_insert(Value::Null))?;
                }
            }
            Self::Records {
                upsert,
                removed,
                order,
            } => {
                let array = value.as_array_mut().ok_or("Invalid record update")?;
                let mut map: BTreeMap<String, Value> = records(array)
                    .ok_or("Invalid record IDs")?
                    .into_iter()
                    .map(|(k, v)| (k, v.clone()))
                    .collect();
                let original = array
                    .iter()
                    .map(|v| v["id"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                for id in removed {
                    map.remove(id);
                }
                for (id, delta) in upsert {
                    let target = map.entry(id.clone()).or_insert(Value::Null);
                    delta.apply(target)?;
                    if target["id"].as_str() != Some(id) {
                        return Err("Record ID changed".into());
                    }
                }
                let ids = order.as_ref().unwrap_or(&original);
                let mut updated = Vec::new();
                for id in ids {
                    updated.push(map.remove(id).ok_or("Invalid record order")?);
                }
                if !map.is_empty() {
                    return Err("Missing record order".into());
                }
                *array = updated;
            }
        }
        Ok(())
    }
}

impl StreamEvent {
    /// Update a clone and deserialize before publishing, so malformed updates are atomic.
    pub fn apply(&self, snapshot: &mut Snapshot) -> Result<Option<HistoryState>, String> {
        match self {
            Self::Reset {
                protocol_version,
                snapshot: new,
                history,
            } => {
                if *protocol_version != PROTOCOL_VERSION || new.protocol_version != PROTOCOL_VERSION
                {
                    return Err("Incompatible Relay protocol; update client and server".into());
                }
                *snapshot = *new.clone();
                Ok(Some(history.clone()))
            }
            Self::Update {
                base_revision,
                revision,
                delta,
                history,
            } => {
                if *revision < snapshot.revision {
                    return Ok(None);
                }
                if *base_revision != snapshot.revision {
                    return Err("Workspace update gap; reconnecting".into());
                }
                let mut value =
                    serde_json::to_value(&*snapshot).map_err(|_| "Invalid workspace")?;
                delta.apply(&mut value)?;
                let updated: Snapshot =
                    serde_json::from_value(value).map_err(|_| "Invalid workspace update")?;
                if updated.revision != *revision || updated.protocol_version != PROTOCOL_VERSION {
                    return Err("Invalid update revision".into());
                }
                *snapshot = updated;
                Ok(Some(history.clone()))
            }
            _ => Ok(None),
        }
    }
}

pub fn preview(message: &crate::Message) -> (crate::Message, bool) {
    let size = serde_json::to_vec(message).map_or(usize::MAX, |v| v.len());
    if size <= MESSAGE_PREVIEW_BYTES {
        return (message.clone(), false);
    }
    let mut result = message.clone();
    fn shorten(text: &mut String) {
        let mut end = text.len().min(1024);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    shorten(&mut result.body);
    result.parts.clear();
    if let Some(tool) = &mut result.tool {
        shorten(&mut tool.input);
        shorten(&mut tool.target);
    }
    // Large details load with the full message.
    if serde_json::to_vec(&result).map_or(true, |v| v.len() > MESSAGE_PREVIEW_BYTES)
        && let Some(tool) = &mut result.tool
    {
        tool.detail = None;
    }
    (result, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_updates_preserve_order_remove_records_and_append_unicode() {
        let old = json!({"messages":[{"id":"a","body":"λ","tool":{"state":"running"}},{"id":"b","body":"gone"}],"aliases":{"old":"a"}});
        let new = json!({"messages":[{"id":"c","body":"new"},{"id":"a","body":"λ🐱","tool":{"state":"completed"}}],"aliases":{}});
        let delta = Delta::between(&old, &new);
        let mut actual = old;
        delta.apply(&mut actual).unwrap();
        assert_eq!(actual, new);
        assert!(serde_json::to_string(&delta).unwrap().contains("append"));
    }

    #[test]
    fn append_traffic_is_independent_of_existing_text_size() {
        let old = json!({"id":"a","body":"x".repeat(100_000)});
        let new = json!({"id":"a","body":format!("{}tail","x".repeat(100_000))});
        let delta = Delta::between(&old, &new);
        assert!(serde_json::to_vec(&delta).unwrap().len() < 200);
    }

    #[test]
    fn previews_keep_small_details_and_drop_large_ones() {
        let message = |steps: usize| crate::Message {
            id: "m".into(),
            session_id: "s".into(),
            author: "a".into(),
            kind: "tool_call".into(),
            body: "x".repeat(20_000),
            parts: vec![],
            tool: Some(crate::ToolCall {
                run_id: "r".into(),
                name: "TodoWrite".into(),
                kind: crate::ToolKind::Plan,
                state: crate::ToolState::Completed,
                input: String::new(),
                target: String::new(),
                directory: None,
                exit_code: None,
                started_ms: Some(1),
                finished_ms: Some(2),
                detail: Some(crate::ToolDetail::Plan {
                    steps: (0..steps)
                        .map(|i| crate::PlanStep {
                            title: format!("{i}{}", "s".repeat(400)),
                            done: false,
                            owner_session_id: None,
                        })
                        .collect(),
                }),
            }),
        };
        let (small, truncated) = preview(&message(2));
        assert!(truncated && small.tool.unwrap().detail.is_some());
        let (large, truncated) = preview(&message(200));
        assert!(truncated && large.tool.as_ref().unwrap().detail.is_none());
        assert!(serde_json::to_vec(&large).unwrap().len() <= MESSAGE_PREVIEW_BYTES);
    }

    #[test]
    fn invalid_update_does_not_modify_state() {
        let mut snapshot = crate::demo_snapshot(crate::AgentProfile::default());
        let before = snapshot.clone();
        let event = StreamEvent::Update {
            base_revision: snapshot.revision,
            revision: snapshot.revision + 1,
            delta: Delta::Replace { value: json!({}) },
            history: HistoryState::default(),
        };
        assert!(event.apply(&mut snapshot).is_err());
        assert_eq!(snapshot, before);
    }
}
