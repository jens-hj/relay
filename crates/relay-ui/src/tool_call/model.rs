//! Optional, presentation-only tool data. Missing measurements stay absent.
use crate::composer::{Effort, ModelIdentity};
use crate::prelude::{AgentStatus, Harness};
use crate::tokens::ToolKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Opening {
    #[default]
    Collapsed,
    Summary,
    Expanded,
}
impl Opening {
    pub fn next(self) -> Self {
        match self {
            Self::Collapsed => Self::Summary,
            Self::Summary => Self::Expanded,
            Self::Expanded => Self::Collapsed,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Collapsed => "Collapsed. Open summary",
            Self::Summary => "Summary. Expand fully",
            Self::Expanded => "Expanded. Collapse",
        }
    }
    pub fn marks(self) -> usize {
        match self {
            Self::Collapsed => 1,
            Self::Summary => 2,
            Self::Expanded => 3,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCallView {
    pub kind: ToolKind,
    pub status: AgentStatus,
    pub verb: String,
    pub target: Option<String>,
    pub opening: Opening,
    pub detail: Option<ToolDetail>,
}
crate::value::live_value!(ToolCallView);
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeLine {
    pub number: Option<u32>,
    pub text: String,
    pub change: LineChange,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineChange {
    #[default]
    Context,
    Added,
    Removed,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hunk {
    pub line: Option<u32>,
    pub label: Option<String>,
    pub added: Option<u32>,
    pub removed: Option<u32>,
    pub lines: Option<Vec<CodeLine>>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchFile {
    pub path: Option<String>,
    pub hits: Option<u32>,
    pub matches: Option<Vec<CodeLine>>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestFailure {
    pub name: String,
    pub trace: Option<Vec<CodeLine>>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanStep {
    pub title: String,
    pub done: Option<bool>,
    pub owner: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    Allow,
    Ask,
    Deny,
}
impl PermissionMode {
    pub fn word(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Permission {
    pub capability: String,
    pub mode: PermissionMode,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exchange {
    pub who: Option<String>,
    pub text: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct LaneSegment {
    pub status: AgentStatus,
    pub fraction: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WaitAgent {
    pub name: String,
    pub harness: Option<Harness>,
    pub status: Option<AgentStatus>,
    pub lane: Option<Vec<LaneSegment>>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ToolDetail {
    Read {
        start: Option<u32>,
        end: Option<u32>,
        total: Option<u32>,
        found: Option<String>,
        lines: Option<Vec<CodeLine>>,
    },
    Edit {
        added: Option<u32>,
        removed: Option<u32>,
        hunks: Option<Vec<Hunk>>,
    },
    Create {
        lines: Option<u32>,
        first_line: Option<String>,
        content: Option<Vec<CodeLine>>,
    },
    Search {
        query: Option<String>,
        scope: Option<String>,
        files: Option<Vec<SearchFile>>,
    },
    Run {
        command: Option<String>,
        exit_code: Option<i32>,
        duration: Option<String>,
        tail: Option<Vec<CodeLine>>,
        full: Option<Vec<CodeLine>>,
    },
    Test {
        target: Option<String>,
        passed: Option<u32>,
        failed: Option<u32>,
        failures: Option<Vec<TestFailure>>,
    },
    Fetch {
        url: Option<String>,
        status: Option<u16>,
        title: Option<String>,
        read: Option<String>,
        used: Option<String>,
        kept: Option<String>,
    },
    Plan {
        title: Option<String>,
        steps: Option<Vec<PlanStep>>,
    },
    Spawn {
        child: Option<String>,
        harness: Option<Harness>,
        model: Option<ModelIdentity>,
        effort: Option<Effort>,
        task: Option<String>,
        context: Option<String>,
        reports_to: Option<String>,
        permissions: Option<Vec<Permission>>,
    },
    Message {
        target: Option<String>,
        read: Option<bool>,
        exchange: Option<Vec<Exchange>>,
    },
    Wait {
        agents: Option<Vec<WaitAgent>>,
    },
    Approval {
        requester: Option<String>,
        command: Option<String>,
        why: Option<String>,
        touches: Option<String>,
        blocks: Option<String>,
    },
    Result {
        quantities: Option<String>,
        from: Option<String>,
        summary: Option<String>,
        full: Option<String>,
        attachments: Option<Vec<String>>,
        used_by: Option<String>,
    },
}
/// Normalized lane widths, ignoring invalid/missing measurements.
pub fn lane_widths(segments: &[LaneSegment]) -> Vec<f32> {
    let sum: f32 = segments
        .iter()
        .map(|s| {
            if s.fraction.is_finite() {
                s.fraction.max(0.0)
            } else {
                0.0
            }
        })
        .sum();
    segments
        .iter()
        .map(|s| {
            if sum > 0.0 && s.fraction.is_finite() {
                s.fraction.max(0.0) / sum
            } else {
                0.0
            }
        })
        .collect()
}
