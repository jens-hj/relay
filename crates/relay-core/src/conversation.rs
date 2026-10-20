use crate::Snapshot;
use serde::{Deserialize, Serialize};

pub const ASSET_LIMIT: usize = 20 * 1024 * 1024;
pub const DRAFT_ASSET_LIMIT: u64 = 64 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 64 * 1024;
pub const MENTION_LABEL_LIMIT: usize = 256;
pub const MENTION_ID_LIMIT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub message_id: String,
    pub start: usize,
    pub end: usize,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub size: u64,
}

/// A human-selected reference to another agent session. The ID is authoritative; the
/// label is the "<project name> • <session title>" captured when it was selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMention {
    pub session_id: String,
    pub label: String,
}

impl SessionMention {
    /// Harness-facing text, shared by every harness: the label plus the stable ID.
    pub fn reference(&self) -> String {
        format!("@{} [Relay session {}]", self.label, self.session_id)
    }
}

/// "<project> • <title>", truncated on a char boundary to MENTION_LABEL_LIMIT bytes.
pub fn mention_label(project: &str, title: &str) -> String {
    let mut label = format!("{project} • {title}");
    if label.len() > MENTION_LABEL_LIMIT {
        let mut end = MENTION_LABEL_LIMIT;
        while !label.is_char_boundary(end) {
            end -= 1;
        }
        label.truncate(end);
    }
    label
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    #[serde(flatten)]
    pub kind: PartKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartKind {
    Text { text: String },
    Skill { skill: crate::SkillReference },
    SessionMention { mention: SessionMention },
    Asset { asset: Asset },
    Reply { anchor: Anchor, parts: Vec<Part> },
}

impl Part {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Text { text: text.into() },
        }
    }
}

pub fn has_content(parts: &[Part]) -> bool {
    parts.iter().any(|part| match &part.kind {
        PartKind::Text { text } => !text.trim().is_empty(),
        PartKind::Asset { .. } | PartKind::Skill { .. } | PartKind::SessionMention { .. } => true,
        PartKind::Reply { parts, .. } => has_content(parts),
    })
}

pub fn plain_text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match &p.kind {
            PartKind::Text { text } => text.clone(),
            PartKind::Skill { skill } => format!("${}", skill.name),
            PartKind::SessionMention { mention } => mention.reference(),
            PartKind::Asset { asset } => format!("[{}]", asset.name),
            PartKind::Reply { anchor, parts } => {
                format!("Reply to “{}”:\n{}", anchor.quote, plain_text(parts))
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn assets(parts: &[Part]) -> Vec<&Asset> {
    let mut found = Vec::new();
    for part in parts {
        match &part.kind {
            PartKind::Asset { asset } => found.push(asset),
            PartKind::Reply { parts, .. } => found.extend(assets(parts)),
            _ => {}
        }
    }
    found
}

/// Every mention in order, recursing into Reply.parts but never into Anchor.quote text.
pub fn session_mentions(parts: &[Part]) -> Vec<&SessionMention> {
    let mut found = Vec::new();
    for part in parts {
        match &part.kind {
            PartKind::SessionMention { mention } => found.push(mention),
            PartKind::Reply { parts, .. } => found.extend(session_mentions(parts)),
            _ => {}
        }
    }
    found
}

pub fn validate_parts(snapshot: &Snapshot, session: &str, parts: &[Part]) -> Result<(), String> {
    fn validate(
        snapshot: &Snapshot,
        session: &str,
        parts: &[Part],
        nested: bool,
        ids: &mut std::collections::HashSet<String>,
        text: &mut usize,
    ) -> Result<(), String> {
        for part in parts {
            uuid::Uuid::parse_str(&part.id).map_err(|_| "Part ID must be a UUID")?;
            if !ids.insert(part.id.clone()) {
                return Err("Duplicate part ID".into());
            }
            match &part.kind {
                PartKind::Text { text: value } => *text += value.len(),
                PartKind::Skill { skill } => {
                    if skill.id.is_empty()
                        || skill.id.len() > 4096
                        || skill.name.is_empty()
                        || skill.name.len() > 256
                    {
                        return Err("Invalid skill reference".into());
                    }
                    *text += skill.name.len() + 1;
                }
                // The target is checked when a message is accepted, so stale
                // drafts stay loadable.
                PartKind::SessionMention { mention } => {
                    if mention.session_id.is_empty()
                        || mention.session_id.len() > MENTION_ID_LIMIT
                        || mention.session_id.chars().any(char::is_control)
                        || mention.label.is_empty()
                        || mention.label.len() > MENTION_LABEL_LIMIT
                        || mention.label.chars().any(char::is_control)
                    {
                        return Err("Invalid session mention".into());
                    }
                    *text += mention.label.len() + mention.session_id.len();
                }
                PartKind::Asset { asset } => {
                    uuid::Uuid::parse_str(&asset.id).map_err(|_| "Asset ID must be a UUID")?;
                    if asset.size > ASSET_LIMIT as u64 {
                        return Err("File exceeds 20 MiB".into());
                    }
                }
                PartKind::Reply { anchor, parts } => {
                    if nested {
                        return Err("Nested draft replies are not supported".into());
                    }
                    let message = snapshot
                        .messages
                        .iter()
                        .find(|m| m.id == anchor.message_id && m.session_id == session)
                        .ok_or("Reply source not found in this session")?;
                    if message.body.get(anchor.start..anchor.end) != Some(anchor.quote.as_str()) {
                        return Err("Reply source range changed".into());
                    }
                    *text += anchor.quote.len();
                    validate(snapshot, session, parts, true, ids, text)?;
                }
            }
        }
        Ok(())
    }
    let mut ids = std::collections::HashSet::new();
    let mut text = 0;
    validate(snapshot, session, parts, false, &mut ids, &mut text)?;
    if ids.len() > 1024 || text > TEXT_LIMIT {
        return Err("Draft exceeds 64 KiB of text or 1024 parts".into());
    }
    if assets(parts).iter().map(|a| a.size).sum::<u64>() > DRAFT_ASSET_LIMIT {
        return Err("Draft files exceed 64 MiB".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub session_id: String,
    pub revision: u64,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveDraft {
    pub request_id: String,
    pub expected_revision: u64,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Queued,
    Launching,
    Running,
    Completed,
    Interrupted,
    Failed,
    Cancelled,
    Paused,
}

/// One unfinished turn of a session's queue as a client saw it. A queue
/// resume that names these still applies after unrelated workspace changes,
/// but not after this queue changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedTurn {
    pub submission_id: String,
    pub state: SubmissionState,
    /// The request that last edited the turn, which changes with each edit.
    pub last_edit_request: Option<String>,
    /// The reason the turn was paused, which changes when it is paused again.
    pub error: Option<String>,
}

impl crate::Snapshot {
    /// A session's unfinished turns, in queue order.
    pub fn queued_turns(&self, session_id: &str) -> Vec<QueuedTurn> {
        self.submissions
            .iter()
            .filter(|s| {
                s.session_id == session_id
                    && matches!(
                        s.state,
                        SubmissionState::Queued
                            | SubmissionState::Paused
                            | SubmissionState::Launching
                            | SubmissionState::Running
                    )
            })
            .map(|s| QueuedTurn {
                submission_id: s.id.clone(),
                state: s.state.clone(),
                last_edit_request: s.last_edit_request.clone(),
                error: s.error.clone(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub id: String,
    pub session_id: String,
    pub parts: Vec<Part>,
    pub state: SubmissionState,
    pub approve_implementation: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub interrupts_run: Option<String>,
    #[serde(default)]
    pub last_edit_request: Option<String>,
    #[serde(default)]
    pub sender: crate::Sender,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentProfile, demo_snapshot};

    #[test]
    fn anchors_distinguish_repeated_passages_and_reject_invalid_unicode_ranges() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        snapshot.messages[0].body = "λ same λ same".into();
        let message = &snapshot.messages[0];
        let reply = |start, end| Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::Reply {
                anchor: Anchor {
                    message_id: message.id.clone(),
                    start,
                    end,
                    quote: "same".into(),
                },
                parts: vec![Part::text("Change this one")],
            },
        };
        let parts = vec![reply(3, 7), reply(11, 15)];
        assert!(validate_parts(&snapshot, &message.session_id, &parts).is_ok());
        assert!(validate_parts(&snapshot, &message.session_id, &[reply(1, 7)]).is_err());
        assert!(validate_parts(&snapshot, "another-session", &parts).is_err());
        let mut duplicate = parts.clone();
        duplicate[1].id = duplicate[0].id.clone();
        assert!(validate_parts(&snapshot, &message.session_id, &duplicate).is_err());
    }

    #[test]
    fn ordered_context_survives_serialization_and_bounds_include_reply_quotes() {
        let snapshot = demo_snapshot(AgentProfile::default());
        let parts = vec![
            Part::text("Before "),
            Part {
                id: uuid::Uuid::new_v4().to_string(),
                kind: PartKind::Asset {
                    asset: Asset {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: "diagram.png".into(),
                        media_type: "image/png".into(),
                        size: 10,
                    },
                },
            },
            Part::text(" after"),
        ];
        assert_eq!(plain_text(&parts), "Before [diagram.png] after");
        let restored: Vec<Part> =
            serde_json::from_str(&serde_json::to_string(&parts).unwrap()).unwrap();
        assert_eq!(restored, parts);
        assert_eq!(assets(&parts).len(), 1);
        assert!(
            validate_parts(
                &snapshot,
                "session-plan",
                &[Part::text("x".repeat(TEXT_LIMIT + 1))]
            )
            .is_err()
        );
        let legacy =
            r#"{"id":"m","session_id":"s","author":"You","kind":"prompt","body":"Keep history"}"#;
        let message: crate::Message = serde_json::from_str(legacy).unwrap();
        assert!(message.parts.is_empty());
        assert_eq!(message.body, "Keep history");
        assert_eq!(message.sender, crate::Sender::User);
        let legacy = r#"{"id":"q","session_id":"s","parts":[],"state":"queued","approve_implementation":false,"error":null}"#;
        let submission: Submission = serde_json::from_str(legacy).unwrap();
        assert_eq!(submission.sender, crate::Sender::User);
        let sent = crate::Sender::Agent {
            session_id: "parent".into(),
        };
        let json = serde_json::to_string(&sent).unwrap();
        assert_eq!(json, r#"{"kind":"agent","session_id":"parent"}"#);
        assert_eq!(serde_json::from_str::<crate::Sender>(&json).unwrap(), sent);
    }

    fn mention(session_id: &str, label: &str) -> Part {
        Part {
            id: uuid::Uuid::new_v4().to_string(),
            kind: PartKind::SessionMention {
                mention: SessionMention {
                    session_id: session_id.into(),
                    label: label.into(),
                },
            },
        }
    }

    #[test]
    fn session_mentions_serialize_render_and_validate_bounds() {
        let snapshot = demo_snapshot(AgentProfile::default());
        let part = mention("session-other", "Relay • Review");
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "session_mention");
        assert_eq!(json["mention"]["session_id"], "session-other");
        assert_eq!(json["mention"]["label"], "Relay • Review");
        assert_eq!(serde_json::from_value::<Part>(json).unwrap(), part);
        let legacy: Vec<Part> = serde_json::from_str(
            r#"[{"id":"a","type":"text","text":"Hi"},{"id":"b","type":"skill","skill":{"id":"s","name":"review"}}]"#,
        )
        .unwrap();
        assert!(session_mentions(&legacy).is_empty());
        let parts = vec![Part::text("Ask "), part.clone(), Part::text(" first")];
        assert_eq!(
            plain_text(&parts),
            "Ask @Relay • Review [Relay session session-other] first"
        );
        assert!(has_content(std::slice::from_ref(&part)));
        // A mention of a missing session is still a loadable draft.
        assert!(validate_parts(&snapshot, "session-plan", &parts).is_ok());
        for invalid in [
            mention("", "Label"),
            mention("session-x", ""),
            mention(&"x".repeat(MENTION_ID_LIMIT + 1), "Label"),
            mention("session-x", &"x".repeat(MENTION_LABEL_LIMIT + 1)),
            mention("session\nx", "Label"),
            mention("session-x", "Label\u{7}"),
        ] {
            assert!(validate_parts(&snapshot, "session-plan", &[invalid]).is_err());
        }
        let label = "x".repeat(MENTION_LABEL_LIMIT);
        let budget = TEXT_LIMIT - label.len() - "session-x".len();
        let full = vec![Part::text("y".repeat(budget)), mention("session-x", &label)];
        assert!(validate_parts(&snapshot, "session-plan", &full).is_ok());
        let over = vec![
            Part::text("y".repeat(budget + 1)),
            mention("session-x", &label),
        ];
        assert!(validate_parts(&snapshot, "session-plan", &over).is_err());
        let label = mention_label("Project", &"é".repeat(200));
        assert!(label.len() <= MENTION_LABEL_LIMIT && label.starts_with("Project • é"));
        assert_eq!(mention_label("Relay", "Review"), "Relay • Review");
    }

    #[test]
    fn session_mentions_recurse_into_replies_but_not_quotes() {
        let parts = vec![
            mention("session-a", "A"),
            Part {
                id: uuid::Uuid::new_v4().to_string(),
                kind: PartKind::Reply {
                    anchor: Anchor {
                        message_id: "m".into(),
                        start: 0,
                        end: 35,
                        quote: "@B [Relay session session-quoted]".into(),
                    },
                    parts: vec![Part::text("See "), mention("session-b", "B")],
                },
            },
            Part::text("@C [Relay session session-c]"),
        ];
        let ids: Vec<_> = session_mentions(&parts)
            .into_iter()
            .map(|m| m.session_id.as_str())
            .collect();
        assert_eq!(ids, ["session-a", "session-b"]);
    }
}
