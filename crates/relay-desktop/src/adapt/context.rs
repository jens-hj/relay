//! A session's context window and compactions.

use super::identity::model_identity;
use relay_core::Session;
use relay_ui::context::{ContextView, tokens};

fn times(count: usize) -> String {
    match count {
        1 => "once".into(),
        2 => "twice".into(),
        n => format!("{n} times"),
    }
}

/// The context gauge of a session, when its harness reported both the
/// occupancy and the window. The breakdown is never estimated, so it stays
/// empty.
pub fn context_view(session: &Session) -> Option<ContextView> {
    let worker = session.worker.as_ref()?;
    let (used, window) = (worker.context_tokens?, worker.context_window?);
    if window == 0 {
        return None;
    }
    let mut view = ContextView::new(used, window, session.compactions.len() as u32);
    view.model = worker
        .model
        .as_deref()
        .map(|id| model_identity(id).short_name())
        .unwrap_or_default();
    if !session.compactions.is_empty() {
        view.history = format!("Compacted {}", times(session.compactions.len()));
        if let Some(at) = session.compactions.last().and_then(|c| c.at_tokens) {
            view.history.push_str(&format!(", last at {}", tokens(at)));
        }
    }
    Some(view)
}

/// A session's progress report with the likely range its confidence implies.
pub fn session_progress(report: &relay_core::ProgressReport) -> relay_ui::prelude::TaskProgress {
    let (low, high) = report.range();
    relay_ui::prelude::TaskProgress {
        percent: report.percent.min(100),
        low,
        high,
        confidence: report.confidence.min(100),
        completed: report.completed.clone(),
        remaining: report.remaining.clone(),
    }
}
