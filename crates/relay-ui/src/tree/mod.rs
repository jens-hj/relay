//! The agent tree: the sidebar, its status filter, the Needs you cards,
//! focus breadcrumbs, and rows that recurse over any depth.

mod logic;
mod row;
mod sidebar;
#[cfg(test)]
mod tests;

pub use logic::{
    AgentLine, Approval, BarSegment, Branch, Flatten, FoldedLine, Guide, StatusTally, TreeLine,
    approvals, find_branch, flatten, levels_below, status_segments,
};
pub use row::{
    AgentTreeRow, AgentTreeRowProps, ChildStatusBar, ChildStatusBarProps, Elbows, ElbowsProps,
    FoldedBranch, FoldedBranchProps, TreeNav, segment_token, tally_label,
};
pub use sidebar::{
    AgentSidebar, AgentSidebarProps, FocusCrumb, FocusCrumbProps, NeedsYou, NeedsYouCard,
    NeedsYouCardProps, NeedsYouProps, StatusCounts, StatusCountsProps,
};

use crate::model::{AgentStatus, Effort, Harness, ModelIdentity};
use std::rc::Rc;

/// One agent and everything it spawned. Being a manager is only having
/// children, so the same type serves at every depth.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentNode {
    /// Stable identity, passed back to callbacks.
    pub id: String,
    pub name: String,
    pub status: AgentStatus,
    pub harness: Harness,
    /// Hidden when unknown.
    pub model: Option<ModelIdentity>,
    /// Hidden when unknown.
    pub effort: Option<Effort>,
    pub children: Vec<AgentNode>,
    /// Whether the children are shown.
    pub expanded: bool,
    /// The open agent.
    pub selected: bool,
}

impl AgentNode {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        status: AgentStatus,
        harness: Harness,
    ) -> Self {
        AgentNode {
            id: id.into(),
            name: name.into(),
            status,
            harness,
            model: None,
            effort: None,
            children: Vec::new(),
            expanded: true,
            selected: false,
        }
    }

    pub fn model(mut self, model: ModelIdentity, effort: Effort) -> Self {
        self.model = Some(model);
        self.effort = Some(effort);
        self
    }

    pub fn children(mut self, children: Vec<AgentNode>) -> Self {
        self.children = children;
        self
    }

    pub fn collapsed(mut self) -> Self {
        self.expanded = false;
        self
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    /// Waiting agents anywhere below this one.
    pub fn waiting_below(&self) -> usize {
        StatusTally::of_descendants(self).waiting
    }
}

/// Called with an agent's id.
pub type OnAgent = Rc<dyn Fn(String)>;
/// Called with an agent's id and whether its children should show.
pub type OnToggle = Rc<dyn Fn(String, bool)>;
/// Called with the branch to focus, or `None` for the whole tree.
pub type OnFocus = Rc<dyn Fn(Option<String>)>;
/// Called with the status filter, or `None` to show every agent.
pub type OnFilter = Rc<dyn Fn(Option<AgentStatus>)>;
