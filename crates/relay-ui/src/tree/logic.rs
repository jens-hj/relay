//! The tree's pure logic: tallies, child status bars, approvals, focus
//! paths, and flattening the tree into the lines the sidebar draws.

use super::AgentNode;
use crate::model::{AgentStatus, Effort, Harness, ModelIdentity};

/// How many agents are in each state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StatusTally {
    pub working: usize,
    pub waiting: usize,
    pub failed: usize,
    pub queued: usize,
    pub done: usize,
}

impl StatusTally {
    /// Counts `node` and everything below it.
    pub fn of_subtree(node: &AgentNode) -> Self {
        let mut tally = StatusTally::default();
        tally.add_subtree(node);
        tally
    }

    /// Counts everything below `node`, not `node` itself.
    pub fn of_descendants(node: &AgentNode) -> Self {
        let mut tally = StatusTally::default();
        for child in &node.children {
            tally.add_subtree(child);
        }
        tally
    }

    /// Counts every node of a forest.
    pub fn of_forest(roots: &[AgentNode]) -> Self {
        let mut tally = StatusTally::default();
        for root in roots {
            tally.add_subtree(root);
        }
        tally
    }

    fn add_subtree(&mut self, node: &AgentNode) {
        self.add(node.status);
        for child in &node.children {
            self.add_subtree(child);
        }
    }

    pub fn add(&mut self, status: AgentStatus) {
        *self.slot(status) += 1;
    }

    fn slot(&mut self, status: AgentStatus) -> &mut usize {
        match status {
            AgentStatus::Working => &mut self.working,
            AgentStatus::Waiting => &mut self.waiting,
            AgentStatus::Failed => &mut self.failed,
            AgentStatus::Queued => &mut self.queued,
            AgentStatus::Done => &mut self.done,
        }
    }

    pub fn get(&self, status: AgentStatus) -> usize {
        match status {
            AgentStatus::Working => self.working,
            AgentStatus::Waiting => self.waiting,
            AgentStatus::Failed => self.failed,
            AgentStatus::Queued => self.queued,
            AgentStatus::Done => self.done,
        }
    }

    pub fn total(&self) -> usize {
        self.working + self.waiting + self.failed + self.queued + self.done
    }
}

/// One segment of a child status bar, in bar units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarSegment {
    pub status: AgentStatus,
    pub x: f32,
    pub width: f32,
}

/// Splits a `length`-unit bar by status in the order live, waiting,
/// failed, queued, done. Each segment takes its share of the length less a
/// 1-unit gap, so neighbours never touch; empty states take no segment.
pub fn status_segments(tally: &StatusTally, length: f32) -> Vec<BarSegment> {
    let total = tally.total();
    if total == 0 {
        return Vec::new();
    }
    let mut x = 0.0;
    let mut segments = Vec::new();
    for status in AgentStatus::ALL {
        let count = tally.get(status);
        if count == 0 {
            continue;
        }
        let share = length * count as f32 / total as f32;
        segments.push(BarSegment {
            status,
            x,
            width: (share - 1.0).max(1.0),
        });
        x += share;
    }
    segments
}

/// An agent that is waiting for approval, with the names of its ancestors.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Approval {
    pub id: String,
    pub name: String,
    pub harness: Harness,
    pub model: Option<ModelIdentity>,
    /// Ancestor names, root first.
    pub path: Vec<String>,
}

/// Every waiting agent of a forest, in tree order, however deep.
pub fn approvals(roots: &[AgentNode]) -> Vec<Approval> {
    fn walk(node: &AgentNode, path: &mut Vec<String>, out: &mut Vec<Approval>) {
        if node.status == AgentStatus::Waiting {
            out.push(Approval {
                id: node.id.clone(),
                name: node.name.clone(),
                harness: node.harness,
                model: node.model.clone(),
                path: path.clone(),
            });
        }
        path.push(node.name.clone());
        for child in &node.children {
            walk(child, path, out);
        }
        path.pop();
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, &mut Vec::new(), &mut out);
    }
    out
}

/// Where a focused branch sits: its node, its ancestors' names, and its
/// true depth (roots are depth 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Branch<'a> {
    pub node: &'a AgentNode,
    pub path: Vec<String>,
    pub depth: usize,
}

/// Finds the node with `id`.
pub fn find_branch<'a>(roots: &'a [AgentNode], id: &str) -> Option<Branch<'a>> {
    fn walk<'a>(node: &'a AgentNode, id: &str, path: &mut Vec<String>) -> Option<Branch<'a>> {
        if node.id == id {
            return Some(Branch {
                node,
                depth: path.len(),
                path: path.clone(),
            });
        }
        path.push(node.name.clone());
        for child in &node.children {
            if let Some(found) = walk(child, id, path) {
                return Some(found);
            }
        }
        path.pop();
        None
    }
    roots
        .iter()
        .find_map(|root| walk(root, id, &mut Vec::new()))
}

/// Levels below `node`: 0 for a leaf.
pub fn levels_below(node: &AgentNode) -> usize {
    node.children
        .iter()
        .map(|child| 1 + levels_below(child))
        .max()
        .unwrap_or(0)
}

/// One elbow column of a line, 14px wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Guide {
    /// An ancestor with later siblings: a vertical rule through the row.
    Pass,
    /// An ancestor that was the last child: nothing.
    Blank,
    /// This line has later siblings: rule through plus a tick to the right.
    Branch,
    /// This line is the last child: rule down to mid-height and the tick.
    Last,
}

/// Everything a tree row draws.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentLine {
    pub id: String,
    pub name: String,
    pub status: AgentStatus,
    pub harness: Harness,
    pub model: Option<ModelIdentity>,
    pub effort: Option<Effort>,
    /// The parent's id, `None` for a displayed root.
    pub parent: Option<String>,
    /// Elbow columns, one per displayed depth.
    pub guides: Vec<Guide>,
    /// Direct children; hidden when the children are folded.
    pub child_count: usize,
    /// Whether the row offers expand/collapse.
    pub expandable: bool,
    pub expanded: bool,
    pub selected: bool,
    /// Everything below, for the collapsed status bar.
    pub below: StatusTally,
}

impl AgentLine {
    /// A collapsed manager: model info gives way to the status bar.
    pub fn shows_summary(&self) -> bool {
        self.expandable && !self.expanded
    }
}

/// A branch folded past the fold depth.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FoldedLine {
    /// The agent whose children are folded; Focus re-roots on it.
    pub parent: String,
    pub guides: Vec<Guide>,
    /// Levels hidden below the parent.
    pub levels: usize,
    pub tally: StatusTally,
}

impl FoldedLine {
    /// "3 more levels · 6 agents".
    pub fn text(&self) -> String {
        let levels = if self.levels == 1 { "level" } else { "levels" };
        let total = self.tally.total();
        let agents = if total == 1 { "agent" } else { "agents" };
        format!("{} more {levels} · {total} {agents}", self.levels)
    }
}

/// One drawn line of the tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TreeLine {
    Agent(AgentLine),
    Folded(FoldedLine),
}

impl TreeLine {
    /// A stable identity across rebuilds.
    pub fn key(&self) -> String {
        match self {
            TreeLine::Agent(line) => line.id.clone(),
            TreeLine::Folded(line) => format!("{}\u{0}folded", line.parent),
        }
    }

    pub fn guides(&self) -> &[Guide] {
        match self {
            TreeLine::Agent(line) => &line.guides,
            TreeLine::Folded(line) => &line.guides,
        }
    }
}

/// What the flattened tree shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flatten {
    /// Displayed depth at which children fold into one line: children of a
    /// row at `fold_depth - 1` fold. Zero never folds.
    pub fold_depth: usize,
    /// Shows only agents in this state and the ancestors that lead to
    /// them, with every branch on the way open.
    pub filter: Option<AgentStatus>,
}

impl Default for Flatten {
    fn default() -> Self {
        Flatten {
            fold_depth: 4,
            filter: None,
        }
    }
}

fn matches(node: &AgentNode, filter: AgentStatus) -> bool {
    node.status == filter || node.children.iter().any(|child| matches(child, filter))
}

/// Flattens `roots` (displayed at depth 0) into drawn lines.
pub fn flatten(roots: &[AgentNode], options: Flatten) -> Vec<TreeLine> {
    let mut out = Vec::new();
    let visible: Vec<&AgentNode> = roots
        .iter()
        .filter(|node| options.filter.is_none_or(|filter| matches(node, filter)))
        .collect();
    let count = visible.len();
    for (index, node) in visible.into_iter().enumerate() {
        walk(node, None, &[], index + 1 == count, true, options, &mut out);
    }
    out
}

fn walk(
    node: &AgentNode,
    parent: Option<&str>,
    ancestors: &[Guide],
    last: bool,
    root: bool,
    options: Flatten,
    out: &mut Vec<TreeLine>,
) {
    let mut guides = ancestors.to_vec();
    if !root {
        guides.push(if last { Guide::Last } else { Guide::Branch });
    }
    let depth = guides.len();
    let folds = options.fold_depth > 0 && depth + 1 >= options.fold_depth;
    let children: Vec<&AgentNode> = node
        .children
        .iter()
        .filter(|child| options.filter.is_none_or(|filter| matches(child, filter)))
        .collect();
    let has_children = !children.is_empty();
    let expandable = has_children && !folds;
    let expanded = expandable && (node.expanded || options.filter.is_some());
    out.push(TreeLine::Agent(AgentLine {
        id: node.id.clone(),
        name: node.name.clone(),
        status: node.status,
        harness: node.harness,
        model: node.model.clone(),
        effort: node.effort,
        parent: parent.map(str::to_string),
        guides: guides.clone(),
        child_count: if folds { 0 } else { node.children.len() },
        expandable,
        expanded,
        selected: node.selected,
        below: StatusTally::of_descendants(node),
    }));
    // Columns below this line continue its rule only if it has later
    // siblings.
    let mut inner = ancestors.to_vec();
    if !root {
        inner.push(if last { Guide::Blank } else { Guide::Pass });
    }
    if has_children && folds {
        let mut guides = inner;
        guides.push(Guide::Last);
        out.push(TreeLine::Folded(FoldedLine {
            parent: node.id.clone(),
            guides,
            levels: levels_below(node),
            tally: StatusTally::of_descendants(node),
        }));
    } else if expanded {
        let count = children.len();
        for (index, child) in children.into_iter().enumerate() {
            walk(
                child,
                Some(&node.id),
                &inner,
                index + 1 == count,
                false,
                options,
                out,
            );
        }
    }
}
