//! Pure mappings from relay-core data to relay-ui view models. Views read
//! the snapshot through these functions; nothing here touches the network
//! or the UI, so every projection is unit-tested in `tests.rs`.
//!
//! Readouts stay truthful: a value the server did not measure maps to an
//! absent field, never to a guess.

mod context;
mod identity;
mod tools;
mod tree;
mod usage;

#[cfg(test)]
mod tests;

pub use context::context_view;
pub use identity::{effort_choice, harness, selection_model, session_model};
#[cfg(test)]
pub use tools::fingerprint;
pub use tools::{
    approval_view, session_fingerprint, tool_status, tool_view, transcript_messages,
    transcript_permissions,
};
pub use tree::{
    agent_forest, ancestor_path, issue_status, needs_you, session_status, subtree_statuses,
};
pub use usage::harness_usage;
