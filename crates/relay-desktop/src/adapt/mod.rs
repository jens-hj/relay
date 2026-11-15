//! Pure mappings from relay-core data to relay-ui view models. Views read
//! the snapshot through these functions; nothing here touches the network
//! or the UI, so every projection is unit-tested in `tests.rs`.
//!
//! Readouts stay truthful: a value the server did not measure maps to an
//! absent field, never to a guess.

mod context;
mod identity;
mod message_tools;
mod tools;
mod tree;
mod usage;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::message_pair;

pub use context::{context_view, session_progress};
pub use identity::{effort_choice, harness, model_identity, selection_model, session_model};
#[cfg(test)]
pub use message_tools::transcript_messages;
pub use message_tools::{MessageTools, message_tool_sources};
#[cfg(test)]
pub use tools::fingerprint;
pub use tools::{
    approval_view, session_fingerprint, tool_status, tool_view, transcript_permissions,
};
pub use tree::{
    agent_forest, ancestor_path, issue_status, needs_you, session_status, subtree_statuses,
};
pub use usage::harness_usage;
