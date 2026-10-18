//! Structured, persisted tool activity shared by the harness adapters and chat.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Create,
    Search,
    #[serde(alias = "terminal")]
    Run,
    Test,
    #[serde(alias = "web")]
    Fetch,
    Plan,
    #[serde(alias = "agent")]
    Spawn,
    Message,
    Wait,
    Result,
    Other,
}

impl ToolKind {
    pub fn from_name(name: &str) -> Self {
        let normalized = name.to_ascii_lowercase();
        let name = normalized.rsplit("__").next().unwrap_or(&normalized);
        match name {
            "bash" | "shell" | "exec_command" | "write_stdin" | "commandexecution" => Self::Run,
            "read" | "read_file" => Self::Read,
            "edit" | "write" | "multiedit" | "notebookedit" | "apply_patch" | "filechange" => {
                Self::Edit
            }
            "grep" | "glob" | "search" | "filesearch" => Self::Search,
            "websearch" | "webfetch" | "web_search" | "web.run" => Self::Fetch,
            "todowrite" | "update_plan" => Self::Plan,
            "agent" | "task" | "collabagenttoolcall" | "spawn_agent" => Self::Spawn,
            "message_agent" => Self::Message,
            "wait_for_agents" => Self::Wait,
            "report_result" => Self::Result,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Unknown,
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub run_id: String,
    pub name: String,
    pub kind: ToolKind,
    pub state: ToolState,
    /// Original JSON input, or the command for a native terminal item.
    pub input: String,
    pub target: String,
    pub directory: Option<String>,
    pub exit_code: Option<i64>,
    /// Server-observed bounds in Unix milliseconds.
    #[serde(default)]
    pub started_ms: Option<u64>,
    #[serde(default)]
    pub finished_ms: Option<u64>,
    /// Measured facts for compact summaries. Absent when the harness did not report them.
    #[serde(default)]
    pub detail: Option<ToolDetail>,
}

/// Bound on every list carried by a tool detail.
pub const TOOL_DETAIL_LIMIT: usize = 200;
const TOOL_DETAIL_TEXT: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub line: u64,
    #[serde(default)]
    pub label: Option<String>,
    pub added: u64,
    pub removed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFile {
    pub path: String,
    pub hits: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub title: String,
    pub done: bool,
    #[serde(default)]
    pub owner_session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDetail {
    Read {
        #[serde(default)]
        start_line: Option<u64>,
        #[serde(default)]
        end_line: Option<u64>,
        #[serde(default)]
        total_lines: Option<u64>,
    },
    Edit {
        #[serde(default)]
        hunks: Vec<Hunk>,
        added: u64,
        removed: u64,
    },
    Create {
        lines: u64,
    },
    Search {
        #[serde(default)]
        query: String,
        #[serde(default)]
        files: Vec<SearchFile>,
    },
    Run {
        #[serde(default)]
        exit_code: Option<i64>,
        #[serde(default)]
        duration_ms: Option<u64>,
    },
    Test {
        #[serde(default)]
        passed: Option<u64>,
        #[serde(default)]
        failed: Option<u64>,
        #[serde(default)]
        failures: Vec<String>,
    },
    Fetch {
        #[serde(default)]
        status: Option<u16>,
        #[serde(default)]
        title: Option<String>,
    },
    Plan {
        #[serde(default)]
        steps: Vec<PlanStep>,
    },
    Spawn {
        #[serde(default)]
        child_session_id: Option<String>,
    },
    Message {
        #[serde(default)]
        target_session_id: Option<String>,
        #[serde(default)]
        read: bool,
    },
    Wait {
        #[serde(default)]
        session_ids: Vec<String>,
    },
    Result {
        #[serde(default)]
        from_session_id: Option<String>,
        #[serde(default)]
        summary: Option<String>,
    },
}

impl ToolDetail {
    /// Truncate lists and text so persisted and published payloads stay bounded.
    pub fn bounded(mut self) -> Self {
        match &mut self {
            Self::Edit { hunks, .. } => {
                hunks.truncate(TOOL_DETAIL_LIMIT);
                for hunk in hunks {
                    if let Some(label) = &mut hunk.label {
                        truncate_text(label, TOOL_DETAIL_TEXT);
                    }
                }
            }
            Self::Search { files, query } => {
                files.truncate(TOOL_DETAIL_LIMIT);
                truncate_text(query, TOOL_DETAIL_TEXT);
            }
            Self::Test { failures, .. } => {
                failures.truncate(TOOL_DETAIL_LIMIT);
                failures
                    .iter_mut()
                    .for_each(|f| truncate_text(f, TOOL_DETAIL_TEXT));
            }
            Self::Plan { steps } => {
                steps.truncate(TOOL_DETAIL_LIMIT);
                steps
                    .iter_mut()
                    .for_each(|s| truncate_text(&mut s.title, TOOL_DETAIL_TEXT));
            }
            Self::Wait { session_ids } => session_ids.truncate(TOOL_DETAIL_LIMIT),
            Self::Fetch { title: Some(t), .. } => truncate_text(t, TOOL_DETAIL_TEXT),
            Self::Result {
                summary: Some(s), ..
            } => truncate_text(s, 4 * TOOL_DETAIL_TEXT),
            _ => {}
        }
        self
    }
}

fn truncate_text(text: &mut String, max: usize) {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

/// Line counts of text, matching how editors number a file.
pub fn line_count(text: &str) -> u64 {
    text.lines().count() as u64
}

/// Added and removed lines in a unified diff, with one hunk per `@@` header.
pub fn diff_stats(diff: &str) -> ToolDetail {
    let (mut added, mut removed) = (0, 0);
    let mut hunks: Vec<Hunk> = Vec::new();
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            // "@@ -a,b +c,d @@ label"
            let (range, label) = header.split_once("@@").unwrap_or((header, ""));
            let start = range
                .split_whitespace()
                .find_map(|r| r.strip_prefix('+'))
                .and_then(|r| r.split(',').next()?.parse().ok())
                .unwrap_or(0);
            let label = label.trim();
            hunks.push(Hunk {
                line: start,
                label: (!label.is_empty()).then(|| label.to_owned()),
                added: 0,
                removed: 0,
            });
        } else if line.starts_with("+++") || line.starts_with("---") {
        } else if line.starts_with('+') {
            added += 1;
            if let Some(hunk) = hunks.last_mut() {
                hunk.added += 1;
            }
        } else if line.starts_with('-') {
            removed += 1;
            if let Some(hunk) = hunks.last_mut() {
                hunk.removed += 1;
            }
        }
    }
    ToolDetail::Edit {
        hunks,
        added,
        removed,
    }
    .bounded()
}

/// Recognize common test-runner invocations inside a shell command.
pub fn is_test_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let words = lower
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '\'' | '"'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    words.iter().enumerate().any(|(i, word)| {
        let base = word.rsplit('/').next().unwrap_or(word);
        let next = words.get(i + 1).copied().unwrap_or("");
        match base {
            "pytest" | "jest" | "vitest" | "rspec" | "phpunit" | "ctest" => true,
            "cargo" => matches!(next, "test" | "nextest" | "t"),
            "go" | "mix" | "just" | "deno" | "dotnet" | "swift" | "zig" => next == "test",
            "npm" | "pnpm" | "yarn" | "bun" => {
                matches!(next, "test" | "t")
                    || (next == "run" && words.get(i + 2).is_some_and(|w| w.starts_with("test")))
            }
            "python" | "python3" => next == "-m" && words.get(i + 2) == Some(&"pytest"),
            _ => false,
        }
    })
}

/// Best-effort pass/fail counts from common test-runner summaries. Returns
/// `None` when no recognized summary is present.
pub fn parse_test_summary(output: &str) -> Option<ToolDetail> {
    let mut passed = None::<u64>;
    let mut failed = None::<u64>;
    let mut failures = Vec::new();
    fn add(slot: &mut Option<u64>, n: u64) {
        *slot = Some(slot.unwrap_or(0) + n);
    }
    fn count(tokens: &[&str], passed: &mut Option<u64>, failed: &mut Option<u64>) {
        for pair in tokens.windows(2) {
            if let Ok(n) = pair[0].parse::<u64>() {
                match pair[1] {
                    "passed" => add(passed, n),
                    "failed" => add(failed, n),
                    _ => {}
                }
            }
        }
    }
    for line in output.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let tokens = |text: &str| -> Vec<String> {
            text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';'))
                .filter(|w| !w.is_empty())
                .map(|w| w.trim_end_matches('.').to_owned())
                .collect()
        };
        // cargo: "test result: ok. 12 passed; 0 failed; ..."
        // nextest: "Summary [ 1.2s] 10 tests run: 9 passed, 1 failed, 0 skipped"
        // jest: "Tests:  1 failed, 9 passed, 10 total"
        // vitest: "Tests  9 passed (9)"
        // pytest: "==== 1 failed, 9 passed in 1.2s ===="
        let summary = if let Some(rest) = lower.strip_prefix("test result:") {
            Some(rest.to_owned())
        } else if lower.contains(" tests run:") || lower.starts_with("tests:") {
            lower.split_once(':').map(|(_, rest)| rest.to_owned())
        } else if lower.starts_with("tests ")
            || (lower.starts_with('=')
                && lower.contains(" in ")
                && (lower.contains("passed") || lower.contains("failed")))
        {
            Some(lower.trim_matches('=').to_owned())
        } else {
            None
        };
        if let Some(summary) = summary {
            let tokens = tokens(&summary);
            count(
                &tokens.iter().map(String::as_str).collect::<Vec<_>>(),
                &mut passed,
                &mut failed,
            );
        } else if let Some(name) = trimmed
            .strip_prefix("test ")
            .and_then(|r| r.strip_suffix(" ... FAILED"))
        {
            failures.push(name.to_owned());
        } else if let Some(rest) = trimmed.strip_prefix("FAILED ") {
            failures.push(rest.split(" - ").next().unwrap_or(rest).to_owned());
        } else if let Some(rest) = trimmed.strip_prefix("--- FAIL: ") {
            failures.push(rest.split_whitespace().next().unwrap_or(rest).to_owned());
            add(&mut failed, 1);
        } else if trimmed.starts_with("--- PASS: ") {
            add(&mut passed, 1);
        }
    }
    failures.dedup();
    (passed.is_some() || failed.is_some()).then(|| {
        ToolDetail::Test {
            passed,
            failed,
            failures,
        }
        .bounded()
    })
}

/// A turn can end before individual tools publish results. Preserve uncertainty
/// rather than leaving an old call running or claiming that it succeeded.
pub fn interrupt_tools(snapshot: &mut crate::Snapshot, session: &str, run: Option<&str>) -> bool {
    let mut changed = false;
    for message in &mut snapshot.messages {
        if message.session_id == session
            && let Some(tool) = &mut message.tool
            && run.is_none_or(|run| tool.run_id == run)
            && tool.state == ToolState::Running
        {
            tool.state = ToolState::Interrupted;
            changed = true;
        }
    }
    changed
}

/// Calls of one tool category across a session's whole transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCount {
    pub kind: ToolKind,
    pub count: u32,
}

/// The category an older transcript's untyped activity message is shown as.
/// Typed tool messages and ordinary messages have none.
pub fn legacy_tool_kind(message: &crate::Message) -> Option<ToolKind> {
    if message.tool.is_some()
        || !matches!(message.kind.as_str(), "command_execution" | "file_change")
    {
        return None;
    }
    Some(if message.body.starts_with("Command: ") {
        ToolKind::Run
    } else if message.kind == "file_change" {
        ToolKind::Edit
    } else if let Some((name, input)) = message.body.split_once('\n')
        && serde_json::from_str::<serde_json::Value>(input).is_ok_and(|v| v.is_object())
    {
        ToolKind::from_name(name)
    } else {
        ToolKind::Other
    })
}

/// A session's transcript messages. The server's durable spawn receipt is a
/// fallback for harnesses that omit tool events: prefer the observed call
/// when both identify the same child. Keep the underlying messages untouched
/// for history and replay.
pub fn transcript_messages<'a>(
    messages: &'a [crate::Message],
    session: &str,
) -> Vec<&'a crate::Message> {
    fn spawn(message: &crate::Message) -> Option<(&str, &str)> {
        let tool = message.tool.as_ref()?;
        if tool.kind != ToolKind::Spawn {
            return None;
        }
        match &tool.detail {
            Some(ToolDetail::Spawn {
                child_session_id: Some(child),
            }) => Some((&tool.run_id, child)),
            _ => None,
        }
    }
    let receipt = |m: &crate::Message| {
        m.author == "Relay"
            && m.id.starts_with("agent-tool-")
            && m.kind == "tool"
            && m.body.is_empty()
            && m.parts.is_empty()
    };
    let observed: std::collections::BTreeSet<_> = messages
        .iter()
        .filter(|m| m.session_id == session && !receipt(m))
        .filter_map(spawn)
        .collect();
    messages
        .iter()
        .filter(|m| m.session_id == session)
        .filter(|m| !receipt(m) || spawn(m).is_none_or(|key| !observed.contains(&key)))
        .collect()
}

/// Calls per tool category in a session's transcript, in the order the
/// categories first appear. Older transcripts' activity messages count as the
/// typed calls the transcript shows them as.
pub fn tool_counts(messages: &[crate::Message], session: &str) -> Vec<ToolCount> {
    let mut counts: Vec<ToolCount> = Vec::new();
    for kind in transcript_messages(messages, session)
        .into_iter()
        .filter_map(|m| {
            m.tool
                .as_ref()
                .map(|t| t.kind)
                .or_else(|| legacy_tool_kind(m))
        })
    {
        match counts.iter_mut().find(|c| c.kind == kind) {
            Some(count) => count.count += 1,
            None => counts.push(ToolCount { kind, count: 1 }),
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_messages_and_structured_activity_round_trip() {
        let legacy: crate::Message = serde_json::from_str(r#"{"id":"m","session_id":"s","author":"Codex","kind":"command_execution","body":"old output"}"#).unwrap();
        assert!(legacy.tool.is_none());
        let old_tool: ToolCall = serde_json::from_str(r#"{"run_id":"r","name":"Bash","kind":"terminal","state":"completed","input":"ls","target":"ls","directory":null,"exit_code":0}"#).unwrap();
        assert_eq!(old_tool.kind, ToolKind::Run);
        assert!(old_tool.detail.is_none() && old_tool.started_ms.is_none());
        let mut message = legacy;
        message.tool = Some(ToolCall {
            run_id: "r".into(),
            name: "Bash".into(),
            kind: ToolKind::Test,
            state: ToolState::Failed,
            input: "false".into(),
            target: "false".into(),
            directory: Some("/workspace".into()),
            exit_code: Some(1),
            started_ms: Some(10),
            finished_ms: Some(20),
            detail: Some(ToolDetail::Test {
                passed: Some(3),
                failed: Some(1),
                failures: vec!["a::b".into()],
            }),
        });
        assert_eq!(
            serde_json::from_str::<crate::Message>(&serde_json::to_string(&message).unwrap())
                .unwrap(),
            message
        );
    }

    #[test]
    fn legacy_kinds_load_and_names_classify() {
        for (old, new) in [
            ("terminal", ToolKind::Run),
            ("web", ToolKind::Fetch),
            ("agent", ToolKind::Spawn),
            ("read", ToolKind::Read),
        ] {
            assert_eq!(
                serde_json::from_str::<ToolKind>(&format!("\"{old}\"")).unwrap(),
                new
            );
        }
        assert_eq!(serde_json::to_string(&ToolKind::Run).unwrap(), "\"run\"");
        for (name, kind) in [
            ("mcp__relay__spawn_agent", ToolKind::Spawn),
            ("mcp__relay__message_agent", ToolKind::Message),
            ("mcp__relay__wait_for_agents", ToolKind::Wait),
            ("mcp__relay__report_result", ToolKind::Result),
            ("TodoWrite", ToolKind::Plan),
            ("Bash", ToolKind::Run),
            ("WebFetch", ToolKind::Fetch),
            ("Task", ToolKind::Spawn),
            ("mcp__other__thing", ToolKind::Other),
        ] {
            assert_eq!(ToolKind::from_name(name), kind, "{name}");
        }
    }

    #[test]
    fn test_commands_are_recognized() {
        for command in [
            "cargo test -p relay-core",
            "cargo nextest run",
            "/bin/bash -lc 'cd x && pytest -q'",
            "python3 -m pytest",
            "go test ./...",
            "npx jest",
            "pnpm test",
            "npm run test:unit",
            "just test",
            "mix test",
            "bun test",
            "nix develop --command just test",
        ] {
            assert!(is_test_command(command), "{command}");
        }
        for command in [
            "cargo build",
            "ls tests",
            "echo test",
            "git log --grep test",
        ] {
            assert!(!is_test_command(command), "{command}");
        }
    }

    #[test]
    fn test_summaries_are_parsed() {
        assert_eq!(
            parse_test_summary(
                "test a ... ok\ntest b::c ... FAILED\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; finished in 0.01s\ntest result: ok. 4 passed; 0 failed;"
            ),
            Some(ToolDetail::Test {
                passed: Some(5),
                failed: Some(1),
                failures: vec!["b::c".into()]
            })
        );
        assert_eq!(
            parse_test_summary(
                "FAILED tests/t.py::test_x - assert 1\n==== 1 failed, 9 passed in 0.5s ===="
            ),
            Some(ToolDetail::Test {
                passed: Some(9),
                failed: Some(1),
                failures: vec!["tests/t.py::test_x".into()]
            })
        );
        assert_eq!(
            parse_test_summary("Tests:       2 failed, 8 passed, 10 total"),
            Some(ToolDetail::Test {
                passed: Some(8),
                failed: Some(2),
                failures: vec![]
            })
        );
        assert_eq!(
            parse_test_summary("     Summary [   1.2s] 10 tests run: 10 passed, 0 skipped"),
            Some(ToolDetail::Test {
                passed: Some(10),
                failed: None,
                failures: vec![]
            })
        );
        assert_eq!(parse_test_summary("compiling\nerror"), None);
    }

    #[test]
    fn details_are_bounded_and_diffs_counted() {
        let many = ToolDetail::Plan {
            steps: (0..500)
                .map(|i| PlanStep {
                    title: i.to_string(),
                    done: false,
                    owner_session_id: None,
                })
                .collect(),
        }
        .bounded();
        assert!(matches!(many, ToolDetail::Plan { steps } if steps.len() == TOOL_DETAIL_LIMIT));
        assert_eq!(
            diff_stats(
                "--- a/x\n+++ b/x\n@@ -1,2 +1,3 @@ fn main\n a\n-b\n+c\n+d\n@@ -9 +10 @@\n-e\n"
            ),
            ToolDetail::Edit {
                hunks: vec![
                    Hunk {
                        line: 1,
                        label: Some("fn main".into()),
                        added: 2,
                        removed: 1
                    },
                    Hunk {
                        line: 10,
                        label: None,
                        added: 0,
                        removed: 1
                    },
                ],
                added: 2,
                removed: 2,
            }
        );
    }
}
