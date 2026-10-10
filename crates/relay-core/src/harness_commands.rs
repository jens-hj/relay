//! Harness-neutral discovery and explicit invocation contracts.
use crate::{Harness, Part, PartKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReference {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    pub enabled: bool,
    /// Server-owned native identity; clients cannot supply filesystem paths.
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandDispatch {
    Direct,
    Flow,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    pub dispatch: CommandDispatch,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessModel {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub fast: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCatalog {
    pub session_id: String,
    pub harness: Harness,
    pub commands: Vec<HarnessCommand>,
    pub skills: Vec<HarnessSkill>,
    pub models: Vec<HarnessModel>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSelection {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
    #[serde(default)]
    pub plan: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedConversation {
    pub id: String,
    pub title: String,
    pub thread_id: String,
    pub selection: HarnessSelection,
}

pub fn canonical_command(name: &str) -> &str {
    match name {
        "approvals" | "allowed-tools" => "permissions",
        "reset" => "clear",
        "cost" => "usage",
        "branch" => "fork",
        _ => name,
    }
}

/// Recognize skill names in prose while excluding escapes, code and quoted lines.
pub fn skill_tokens(text: &str) -> Vec<(usize, usize, &str)> {
    let mut found = vec![];
    let mut code_delimiter = 0;
    let mut quote = false;
    let mut line_start = true;
    let mut escaped = false;
    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch == '\n' {
            line_start = true;
            quote = false;
            escaped = false;
            continue;
        }
        if line_start && ch.is_whitespace() {
            continue;
        }
        if line_start {
            quote = ch == '>';
            line_start = false;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && code_delimiter == 0 {
            escaped = true;
            continue;
        }
        if ch == '`' && !quote {
            let mut length = 1;
            while chars.peek().is_some_and(|(_, ch)| *ch == '`') {
                chars.next();
                length += 1;
            }
            if code_delimiter == 0 {
                code_delimiter = length;
            } else if code_delimiter == length {
                code_delimiter = 0;
            }
            continue;
        }
        if ch != '$' || code_delimiter != 0 || quote {
            continue;
        }
        if i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            continue;
        }
        let end = text[i + 1..]
            .char_indices()
            .find(|(_, c)| !c.is_alphanumeric() && !matches!(c, '_' | '-' | ':' | '.'))
            .map(|(n, _)| i + 1 + n)
            .unwrap_or(text.len());
        let end = i + 1 + text[i + 1..end].trim_end_matches('.').len();
        if end > i + 1 {
            found.push((i, end, &text[i + 1..end]));
        }
    }
    found
}

pub fn has_skill_references(parts: &[Part]) -> bool {
    parts.iter().any(|part| match &part.kind {
        PartKind::Text { text } => !skill_tokens(text).is_empty(),
        PartKind::Skill { .. } => true,
        PartKind::Reply { parts, .. } => has_skill_references(parts),
        _ => false,
    })
}

/// Canonicalize known typed references into durable inline parts.
pub fn resolve_skills(parts: &[Part], catalog: &HarnessCatalog) -> Result<Vec<Part>, String> {
    let mut result = vec![];
    for part in parts {
        match &part.kind {
            PartKind::Text { text } => {
                let mut offset = 0;
                let tokens = skill_tokens(text);
                for (start, end, name) in tokens {
                    let candidates: Vec<_> = catalog
                        .skills
                        .iter()
                        .filter(|s| s.enabled && s.name == name)
                        .collect();
                    if candidates.len() > 1 {
                        return Err(format!("Choose which ${name} skill to invoke"));
                    }
                    let Some(skill) = candidates.first() else {
                        continue;
                    };
                    if start > offset {
                        result.push(Part::text(&text[offset..start]));
                    }
                    result.push(Part {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: PartKind::Skill {
                            skill: SkillReference {
                                id: skill.id.clone(),
                                name: skill.name.clone(),
                            },
                        },
                    });
                    offset = end;
                }
                if offset == 0 {
                    result.push(part.clone());
                } else if offset < text.len() {
                    result.push(Part::text(&text[offset..]));
                }
            }
            PartKind::Skill { skill } => {
                if !catalog
                    .skills
                    .iter()
                    .any(|s| s.enabled && s.id == skill.id && s.name == skill.name)
                {
                    return Err(format!("Skill ${} is no longer available", skill.name));
                }
                result.push(part.clone());
            }
            PartKind::Reply { anchor, parts } => result.push(Part {
                id: part.id.clone(),
                kind: PartKind::Reply {
                    anchor: anchor.clone(),
                    parts: resolve_skills(parts, catalog)?,
                },
            }),
            _ => result.push(part.clone()),
        }
    }
    Ok(result)
}

pub fn selected_skills(parts: &[Part]) -> Vec<&SkillReference> {
    let mut result = vec![];
    for part in parts {
        match &part.kind {
            PartKind::Skill { skill } => result.push(skill),
            PartKind::Reply { parts, .. } => result.extend(selected_skills(parts)),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plain_text;
    fn catalog() -> HarnessCatalog {
        HarnessCatalog {
            skills: vec![HarnessSkill {
                id: "native-review".into(),
                name: "review".into(),
                description: String::new(),
                argument_hint: String::new(),
                enabled: true,
                path: None,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn prose_references_exclude_literals_and_keep_unicode_offsets() {
        let text = "λ $review and $review\n`$review` \\$review\n> $review\n```sh\n$review\n```\n$HOME/path";
        let tokens = skill_tokens(text);
        assert_eq!(
            tokens.iter().map(|(_, _, name)| *name).collect::<Vec<_>>(),
            vec!["review", "review", "HOME"]
        );
        for (a, b, name) in tokens {
            assert_eq!(&text[a..b], format!("${name}"));
        }
        assert_eq!(
            skill_tokens("``a `$review` b`` $review.")
                .iter()
                .map(|(_, _, name)| *name)
                .collect::<Vec<_>>(),
            vec!["review"]
        );
        let parts = resolve_skills(&[Part::text(text)], &catalog()).unwrap();
        assert_eq!(plain_text(&parts), text);
        assert_eq!(selected_skills(&parts).len(), 2);
        let json = serde_json::to_string(&parts).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Part>>(&json).unwrap(), parts);
        assert_eq!(resolve_skills(&parts, &catalog()).unwrap(), parts);
    }
    #[test]
    fn removed_and_ambiguous_selections_are_errors() {
        let mut catalog = catalog();
        let parts = resolve_skills(&[Part::text("$review")], &catalog).unwrap();
        catalog.skills[0].enabled = false;
        assert!(
            resolve_skills(&parts, &catalog)
                .unwrap_err()
                .contains("no longer available")
        );
        catalog.skills[0].enabled = true;
        let mut other = catalog.skills[0].clone();
        other.id = "another-review".into();
        catalog.skills.push(other);
        assert!(
            resolve_skills(&[Part::text("$review")], &catalog)
                .unwrap_err()
                .contains("Choose which")
        );
        assert_eq!(resolve_skills(&parts, &catalog).unwrap(), parts);
    }
}
