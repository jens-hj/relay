//! Relay's domain and versioned client/server contract. No UI or execution dependencies.

mod fixture;
mod profile;

pub use fixture::demo_snapshot;
pub use profile::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Gitlab,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRef {
    pub provider: Provider,
    pub repository: String,
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub fixture: bool,
    pub columns: Vec<BoardColumn>,
    pub defaults: DirectorProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub project_id: String,
    pub reference: IssueRef,
    pub title: String,
    pub body: String,
    pub column_id: String,
    pub labels: Vec<String>,
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Director {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub overrides: ProfileOverrides,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Director,
    Worker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub issue_id: Option<String>,
    pub director_id: String,
    pub title: String,
    pub role: SessionRole,
    pub fixture: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub author: String,
    pub kind: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub message_id: String,
    /// Optional quoted passage from the immutable message. Not a line-number anchor.
    pub quote: Option<String>,
    /// A display label supplied by a member of the token-trusted workspace.
    pub author: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub projects: Vec<Project>,
    pub issues: Vec<Issue>,
    pub directors: Vec<Director>,
    pub sessions: Vec<Session>,
    pub messages: Vec<Message>,
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub request_id: String,
    pub expected_revision: u64,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    UpdateProjectDefaults {
        project_id: String,
        profile: DirectorProfile,
    },
    CreateDirector {
        project_id: String,
        name: String,
        overrides: ProfileOverrides,
    },
    UpdateDirector {
        director_id: String,
        name: String,
        overrides: ProfileOverrides,
    },
    AddComment {
        message_id: String,
        quote: Option<String>,
        author: String,
        body: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl Snapshot {
    pub fn project(&self, id: &str) -> Result<&Project, String> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "Project not found".into())
    }

    pub fn effective_profile(&self, director: &Director) -> Result<DirectorProfile, String> {
        Ok(director
            .overrides
            .resolve(&self.project(&director.project_id)?.defaults))
    }

    pub fn validate_profile(
        &self,
        project_id: &str,
        profile: &DirectorProfile,
    ) -> Result<(), String> {
        self.project(project_id)?;
        profile.validate()?;
        if let DirectorScope::Issues { issue_ids } = &profile.scope {
            for id in issue_ids {
                if !self
                    .issues
                    .iter()
                    .any(|i| i.id == *id && i.project_id == project_id)
                {
                    return Err(format!("Scope issue {id} does not belong to this project"));
                }
            }
        }
        Ok(())
    }

    /// Apply only to a candidate snapshot: the server commits it atomically after validation.
    pub fn apply(&mut self, command: Command, id: &str, now: u64) -> Result<(), String> {
        match command {
            Command::UpdateProjectDefaults {
                project_id,
                profile,
            } => {
                self.validate_profile(&project_id, &profile)?;
                for director in self.directors.iter().filter(|d| d.project_id == project_id) {
                    self.validate_profile(&project_id, &director.overrides.resolve(&profile))?;
                }
                self.projects
                    .iter_mut()
                    .find(|p| p.id == project_id)
                    .unwrap()
                    .defaults = profile;
            }
            Command::CreateDirector {
                project_id,
                name,
                overrides,
            } => {
                validate_text("Director name", &name, 100)?;
                self.validate_profile(
                    &project_id,
                    &overrides.resolve(&self.project(&project_id)?.defaults),
                )?;
                self.directors.push(Director {
                    id: format!("director-{id}"),
                    project_id,
                    name: name.trim().into(),
                    overrides,
                });
            }
            Command::UpdateDirector {
                director_id,
                name,
                overrides,
            } => {
                validate_text("Director name", &name, 100)?;
                let director = self
                    .directors
                    .iter()
                    .find(|d| d.id == director_id)
                    .ok_or("Director not found")?;
                self.validate_profile(
                    &director.project_id,
                    &overrides.resolve(&self.project(&director.project_id)?.defaults),
                )?;
                let director = self
                    .directors
                    .iter_mut()
                    .find(|d| d.id == director_id)
                    .unwrap();
                director.name = name.trim().into();
                director.overrides = overrides;
            }
            Command::AddComment {
                message_id,
                quote,
                author,
                body,
            } => {
                validate_text("Comment", &body, 16_000)?;
                validate_text("Author", &author, 100)?;
                let message = self
                    .messages
                    .iter()
                    .find(|m| m.id == message_id)
                    .ok_or("Message not found")?;
                let quote = quote.filter(|q| !q.is_empty());
                if let Some(quote) = &quote {
                    validate_text("Quote", quote, 16_000)?;
                    if !message.body.contains(quote) {
                        return Err("Quote is not a passage from this message".into());
                    }
                }
                self.comments.push(Comment {
                    id: format!("comment-{id}"),
                    message_id,
                    quote,
                    author: author.trim().into(),
                    body: body.trim().into(),
                    created_at: now,
                });
            }
        }
        self.revision = self.revision.checked_add(1).ok_or("Revision exhausted")?;
        Ok(())
    }
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{label} must be nonempty and at most {max} bytes"));
    }
    Ok(())
}
