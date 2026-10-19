use super::*;
/// Command lines shown before the summary opening folds the rest.
pub(super) const COMMAND_SUMMARY_LINES: usize = 6;
/// A command as one code line per source line, so a multi-line script
/// keeps its shape instead of being drawn as one tall line. Summary shows
/// the first lines and counts the rest; expanded shows all of them.
pub(super) fn command_lines(command: &str, expanded: bool) -> Option<(Vec<CodeLine>, usize)> {
    let mut lines: Vec<CodeLine> = command
        .lines()
        .map(|line| CodeLine {
            text: line.trim_end().to_owned(),
            ..Default::default()
        })
        .collect();
    while lines.last().is_some_and(|line| line.text.is_empty()) {
        lines.pop();
    }
    let skip = lines.iter().take_while(|line| line.text.is_empty()).count();
    lines.drain(..skip);
    if lines.is_empty() {
        return None;
    }
    let shown = if expanded {
        lines.len()
    } else {
        COMMAND_SUMMARY_LINES.min(lines.len())
    };
    let more = lines.len() - shown;
    lines.truncate(shown);
    Some((lines, more))
}
pub(super) fn body(vm: &ToolCallView, actions: ToolActions) -> Element {
    let expanded = vm.opening == Opening::Expanded;
    let body = view! {
        col height:min-content min-width:0px gap:{px(if expanded { 8.0 } else { 6.0 })}px
            pad:(top:{px(6.0)}px bottom:{px(6.0)}px left:{px(12.0)}px right:{px(12.0)}px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:top) {}
    };
    let Some(detail) = &vm.detail else {
        return body;
    };
    let text = |s: &str| {
        let s = s.to_owned();
        view! {
            text font-size:{px(12.0)}px line-height:1.55 font-color:ground.ink {s.clone()}
        }
    };
    let mut kv = Vec::new();
    let mut add = |key: &str, value: &Option<String>| {
        if let Some(value) = value {
            kv.push((key.to_owned(), value.clone()));
        }
    };
    match detail {
        ToolDetail::Read {
            start,
            end,
            total,
            found,
            lines,
        } => {
            if let (Some(a), Some(b)) = (start, end) {
                kv.push((
                    "range".into(),
                    format!(
                        "L{a}–{b}{}",
                        total.map(|n| format!(" of {n}")).unwrap_or_default()
                    ),
                ));
            }
            if let Some(found) = found {
                kv.push(("found".into(), found.clone()));
            }
            if expanded && let Some(lines) = lines {
                let more = start
                    .zip(*end)
                    .map(|(a, b)| {
                        b.saturating_sub(a)
                            .saturating_add(1)
                            .saturating_sub(lines.len() as u32) as usize
                    })
                    .unwrap_or(0);
                body.adopt(&view! {
                    CodeBlock lines:(lines.clone()) more:(more)
                });
                if let (Some(a), Some(b), Some(total)) = (start, end, total) {
                    let unread = total.saturating_sub(b.saturating_sub(*a).saturating_add(1));
                    body.adopt(&view! {
                        Label text:(format!("{unread} not read")) role:(TextRole::Label)
                            tone:(Tone::Muted)
                    });
                }
            }
        }
        ToolDetail::Edit { hunks, .. } => {
            if let Some(hunks) = hunks {
                for h in hunks {
                    let caption = [
                        h.line.map(|n| format!("L{n}")),
                        h.label.clone(),
                        h.added.map(|n| format!("+{n}")),
                        h.removed.map(|n| format!("−{n}")),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                    body.adopt(&text(&caption));
                    if expanded && let Some(lines) = &h.lines {
                        body.adopt(&view! {
                            CodeBlock lines:(lines.clone())
                        });
                    }
                }
            }
        }
        ToolDetail::Create {
            lines: count,
            first_line,
            content,
        } => {
            if expanded {
                if let Some(lines) = content {
                    let more = count
                        .map(|count| count.saturating_sub(lines.len() as u32) as usize)
                        .unwrap_or(0);
                    body.adopt(&view! {
                        CodeBlock lines:(lines.clone()) more:(more)
                    });
                }
            } else if let Some(line) = first_line {
                kv.push(("first".into(), line.clone()));
                if let Some(count) = count {
                    kv.push(("lines".into(), count.to_string()));
                }
            }
        }
        ToolDetail::Search {
            query,
            scope,
            files,
        } => {
            add("query", query);
            add("scope", scope);
            if let Some(files) = files {
                for file in files {
                    let Some(path) = &file.path else {
                        continue;
                    };
                    if let Some(hits) = file.hits {
                        let width = hits as f32
                            / files
                                .iter()
                                .filter_map(|f| f.hits)
                                .max()
                                .unwrap_or(1)
                                .max(1) as f32
                            * 80.0;
                        body.adopt(&view! {
                            el width:{px(width)}px height:{px(3.0)}px fill:(vm.kind.token()) {}
                        });
                    }
                    body.adopt(&text(&format!(
                        "{}{}",
                        path,
                        file.hits
                            .map(|n| format!(" · {n} matches"))
                            .unwrap_or_default()
                    )));
                    if expanded && let Some(lines) = &file.matches {
                        body.adopt(&view! {
                            CodeBlock lines:(lines.clone())
                        });
                    }
                }
                let hidden = files.iter().filter(|file| file.path.is_none()).count();
                if hidden > 0 {
                    body.adopt(&view! {
                        Label text:(format!("{hidden} more files")) role:(TextRole::Label)
                            tone:(Tone::Muted)
                    });
                }
            }
        }
        ToolDetail::Run {
            command,
            tail,
            full,
            ..
        } => {
            if let Some((lines, more)) = command.as_deref().and_then(|c| command_lines(c, expanded))
            {
                body.adopt(&view! {
                    CodeBlock lines:(lines) more:(more)
                });
            }
            let lines = if expanded {
                full.as_ref().or(tail.as_ref())
            } else {
                tail.as_ref()
            };
            if let Some(lines) = lines {
                body.adopt(&view! {
                    CodeBlock lines:(lines.clone())
                });
            }
        }
        ToolDetail::Test {
            target,
            passed,
            failed,
            failures,
        } => {
            add("target", target);
            if expanded {
                body.adopt(&test_grid(*passed, *failed, 6.0, 32));
            }
            if let Some(n) = passed {
                kv.push(("passed".into(), n.to_string()));
            }
            if let Some(n) = failed {
                kv.push(("failed".into(), n.to_string()));
            }
            if let Some(failures) = failures {
                for failure in failures {
                    body.adopt(&view! {
                        Label text:(failure.name.clone()) role:(TextRole::Label)
                            tone:(Tone::Colour(status.failed))
                    });
                    if expanded && let Some(lines) = &failure.trace {
                        body.adopt(&view! {
                            CodeBlock lines:(lines.clone())
                        });
                    }
                }
            }
        }
        ToolDetail::Fetch {
            url,
            title,
            read,
            used,
            kept,
            ..
        } => {
            if expanded {
                add("url", url);
                add("read", read);
            }
            add("title", title);
            add("used", used);
            if expanded {
                add("kept", kept);
            }
        }
        ToolDetail::Plan { title, steps } => {
            if let Some(title) = title
                && vm.target.as_deref() != Some(title.as_str())
            {
                body.adopt(&text(title));
            }
            if let Some(steps) = steps {
                for step in steps.clone() {
                    body.adopt(&view! {
                        row height:min-content gap:{px(8.0)}px align:center {
                            if step.done.is_some() {
                                el width:{px(5.0)}px height:{px(5.0)}px shrink:0 role:image
                                    label:(if step.done.unwrap() { "Completed step" } else { "Incomplete step" })
                                    fill:{if step.done.unwrap() { mosaic::core::theme::color(ground.muted) } else { Color::TRANSPARENT }}
                                    stroke:(width:{px(1.0)} color:ground.outline) {}
                            }
                            col width:1fr min-width:0px height:min-content {
                                Ellipsis text:(step.title.clone()) role:(TextRole::Label)
                            }
                            if expanded {
                                { if let Some(owner) = step.owner.clone() { parent.adopt(&view! { Label text:(owner) role:(TextRole::Caption) tone:(Tone::Muted) }); } }
                            }
                        }
                    });
                }
            }
        }
        ToolDetail::Spawn {
            task,
            context,
            rationale,
            reports_to,
            permissions,
            ..
        } => {
            add("task", task);
            if expanded {
                add("context", context);
                add("model choice", rationale);
                add("reports to", reports_to);
                if let Some(permissions) = permissions {
                    if !kv.is_empty() {
                        body.adopt(&view! {
                            KeyValue entries:(std::mem::take(&mut kv))
                        });
                    }
                    body.adopt(&view! {
                        Label text:"permissions" role:(TextRole::Caption) tone:(Tone::Muted)
                    });
                    for permission in permissions {
                        let tone = match permission.mode {
                            PermissionMode::Allow => Tone::Muted,
                            PermissionMode::Ask => Tone::Signal(Signal::NeedsYou),
                            PermissionMode::Deny => Tone::Signal(Signal::Failed),
                        };
                        body.adopt(&view! {
                            row height:min-content align:center gap:{px(8.0)}px {
                                col width:1fr min-width:0px height:min-content {
                                    Ellipsis text:(permission.capability.clone())
                                        role:(TextRole::Label)
                                }
                                Label text:(permission.mode.word()) role:(TextRole::Caption)
                                    tone:(tone)
                            }
                        });
                    }
                }
            } else if let Some(permissions) = permissions {
                let asks: Vec<_> = permissions
                    .iter()
                    .filter(|permission| permission.mode == PermissionMode::Ask)
                    .map(|permission| permission.capability.clone())
                    .collect();
                if !asks.is_empty() {
                    kv.push(("asks for".into(), asks.join(", ")));
                }
            }
        }
        ToolDetail::Message { exchange, .. } => {
            if let Some(exchange) = exchange {
                for entry in exchange.iter().take(if expanded { usize::MAX } else { 1 }) {
                    if expanded && let Some(who) = &entry.who {
                        body.adopt(&view! {
                            Label text:(who.clone()) role:(TextRole::Label) weight:600
                                tone:(Tone::Muted)
                        });
                    }
                    if let Some(s) = &entry.text {
                        body.adopt(&text(s));
                    }
                }
            }
        }
        ToolDetail::Wait { agents } => {
            if let Some(agents) = agents {
                body.adopt(&view! {
                    WaitLanes agents:(agents.clone()) expanded:(expanded)
                });
            }
        }
        ToolDetail::Approval {
            requester,
            command,
            why,
            touches,
            blocks,
        } => {
            if let Some((lines, more)) = command.as_deref().and_then(|c| command_lines(c, expanded))
            {
                body.adopt(&view! {
                    CodeBlock lines:(lines) more:(more)
                });
            }
            if expanded {
                add("requester", requester);
                add("why", why);
                add("touches", touches);
                add("blocks", blocks);
            }
            if !kv.is_empty() {
                body.adopt(&view! {
                    KeyValue entries:(std::mem::take(&mut kv))
                });
            }
            let row = view! {
                row height:min-content width:max-content gap:{px(6.0)}px {}
            };
            if let Some(approve) = actions.approve {
                row.adopt(&view! {
                    PrimaryButton text:(if expanded { "Approve once" } else { "Approve" })
                        on-press:(approve)
                });
            }
            if expanded && let Some(always) = actions.always {
                row.adopt(&view! {
                    SecondaryButton text:"Always for this path" on-press:(always)
                });
            }
            if let Some(reject) = actions.reject {
                row.adopt(&view! {
                    SecondaryButton text:"Reject" on-press:(reject)
                });
            }
            body.adopt(&row);
        }
        ToolDetail::Result {
            from,
            summary,
            full,
            attachments,
            used_by,
            ..
        } => {
            if let Some(summary) = if expanded {
                full.as_ref().or(summary.as_ref())
            } else {
                summary.as_ref()
            } {
                body.adopt(&text(summary));
            }
            if expanded {
                if vm.target.is_none() {
                    add("from", from);
                }
                add("used by", used_by);
                if let Some(files) = attachments {
                    body.adopt(&view! {
                        Label text:"attachments" role:(TextRole::Caption) tone:(Tone::Muted)
                    });
                    for file in files {
                        body.adopt(&view! {
                            row height:min-content gap:{px(8.0)}px align:center {
                                ToolIcon kind:(ToolKind::Create)
                                Ellipsis text:(file.clone()) role:(TextRole::Label)
                            }
                        });
                    }
                }
            }
        }
    }
    if !kv.is_empty() {
        body.adopt(&view! {
            KeyValue entries:(kv)
        });
    }
    body
}
#[component]
pub fn WaitLanes(agents: Vec<WaitAgent>, #[prop(default = true)] expanded: bool) -> Element {
    let lanes = view! {
        col height:min-content gap:{px(8.0)}px {}
    };
    let measured = agents.iter().any(|a| a.lane.is_some());
    for agent in agents {
        let row = view! {
            row height:min-content align:center gap:{px(8.0)}px {
                if agent.status.is_some() {
                    StatusSquare status:(agent.status.unwrap())
                }
                if agent.harness.is_some() {
                    HarnessGlyph harness:(agent.harness.unwrap()) size:12.0
                }
                col width:{px(140.0)}px shrink:0 height:min-content {
                    Ellipsis text:(agent.name.clone()) role:(TextRole::Label) tone:(Tone::Muted)
                }
            }
        };
        if expanded && let Some(segments) = &agent.lane {
            let lane = view! {
                row width:{px(160.0)}px height:{px(8.0)}px gap:{px(2.0)}px {}
            };
            for (segment, width) in segments.clone().into_iter().zip(lane_widths(segments)) {
                if width <= 0.0 {
                    continue;
                }
                let colour = match segment.status {
                    AgentStatus::Working => status.live,
                    AgentStatus::Waiting => status.needs_you,
                    AgentStatus::Failed => status.failed,
                    _ => ground.muted,
                };
                lane.adopt(&view! {
                    el width:{px(160.0 * width)}px height:fill shrink:1
                        fill:{if segment.status == AgentStatus::Queued { Color::TRANSPARENT } else { mosaic::core::theme::color(colour) }}
                        stroke:(width:{px(1.0)} color:(colour)) {}
                });
            }
            row.adopt(&lane);
        }
        lanes.adopt(&row);
    }
    if expanded && measured {
        lanes.adopt(&view! {
            row height:min-content justify:between {
                Label text:"started" role:(TextRole::Caption) tone:(Tone::Muted)
                Label text:"now" role:(TextRole::Caption) tone:(Tone::Muted)
            }
        });
    }
    lanes
}
pub(super) fn test_grid(
    passed: Option<u32>,
    failed: Option<u32>,
    side: f32,
    columns: usize,
) -> Element {
    let grid = view! {
        col height:min-content gap:{px(side / 2.0)}px {}
    };
    let total = passed.unwrap_or(0).saturating_add(failed.unwrap_or(0));
    for start in (0..total).step_by(columns) {
        let row = view! {
            row width:max-content height:min-content gap:{px(side / 2.0)}px {}
        };
        for index in start..(start + columns as u32).min(total) {
            let colour = if index < passed.unwrap_or(0) {
                ground.muted
            } else {
                status.failed
            };
            row.adopt(&view! {
                el width:{px(side)}px height:{px(side)}px fill:(colour) {}
            });
        }
        grid.adopt(&row);
    }
    grid
}
#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[CodeLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn commands_split_into_lines_and_fold_in_summary() {
        let (lines, more) = command_lines("ls -la", false).unwrap();
        assert_eq!((texts(&lines), more), (vec!["ls -la"], 0));
        let (lines, more) = command_lines("\n  python3 - <<PY\n  print(1)\nPY\n\n", true).unwrap();
        assert_eq!(texts(&lines), ["  python3 - <<PY", "  print(1)", "PY"]);
        assert_eq!(more, 0);
        let script = (0..10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, more) = command_lines(&script, false).unwrap();
        assert_eq!((lines.len(), more), (COMMAND_SUMMARY_LINES, 4));
        let (lines, more) = command_lines(&script, true).unwrap();
        assert_eq!((lines.len(), more), (10, 0));
        assert!(command_lines("  \n\n", true).is_none());
    }
}
