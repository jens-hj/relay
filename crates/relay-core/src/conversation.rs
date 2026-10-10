use crate::Snapshot;
use serde::{Deserialize, Serialize};

pub const ASSET_LIMIT: usize = 20 * 1024 * 1024;
pub const DRAFT_ASSET_LIMIT: u64 = 64 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 64 * 1024;

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
        PartKind::Asset { .. } | PartKind::Skill { .. } => true,
        PartKind::Reply { parts, .. } => has_content(parts),
    })
}

pub fn plain_text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match &p.kind {
            PartKind::Text { text } => text.clone(),
            PartKind::Skill { skill } => format!("${}", skill.name),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirectorProfile, demo_snapshot};

    #[test]
    fn anchors_distinguish_repeated_passages_and_reject_invalid_unicode_ranges() {
        let mut snapshot = demo_snapshot(DirectorProfile::default());
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
        let snapshot = demo_snapshot(DirectorProfile::default());
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
    }
}
