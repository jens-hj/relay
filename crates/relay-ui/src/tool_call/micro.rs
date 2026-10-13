use super::*;
pub(super) fn micrographic(vm: &ToolCallView) -> Element {
    let row = view! {
        row width:max-content height:min-content align:center gap:{px(5.0)}px shrink:0 {}
    };
    let Some(d) = &vm.detail else {
        return row;
    };
    let label = |text: String| {
        view! {
            Label text:(text) role:(TextRole::Caption) tone:(Tone::Muted)
        }
    };
    let square = |colour: ColorToken, fill: bool, height: f32| {
        view! {
            el width:{px(4.0)}px height:{px(height)}px
                fill:{if fill { mosaic::core::theme::color(colour) } else { Color::TRANSPARENT }}
                stroke:(width:{px(1.0)} color:(colour)) {}
        }
    };
    match d {
        ToolDetail::Read {
            start, end, total, ..
        } => {
            if let (Some(start), Some(end)) = (start, end) {
                if let Some(total) = total.filter(|n| *n > 0) {
                    let ruler = view! {
                        row width:{px(38.0)}px height:{px(5.0)}px fill:ground.track {}
                    };
                    let begin = (*start as f32 - 1.0).max(0.0).min(total as f32) / total as f32;
                    let span = (end.saturating_sub(*start) + 1) as f32 / total as f32;
                    ruler.adopt(&view! {
                        el width:{px(38.0 * begin)}px height:fill {}
                    });
                    ruler.adopt(&view! {
                        el width:{px(38.0 * span.min(1.0 - begin))}px height:fill
                            fill:(vm.kind.token()) {}
                    });
                    row.adopt(&ruler);
                }
                row.adopt(&label(format!("L{start}–{end}")));
            }
        }
        ToolDetail::Edit { added, removed, .. } => {
            if let Some(n) = added {
                row.adopt(&label(format!("+{n}")));
            }
            if let Some(n) = removed {
                row.adopt(&view! {
                    Label text:(format!("−{n}")) role:(TextRole::Caption)
                        tone:(Tone::Colour(status.failed))
                });
            }
            if let (Some(a), Some(r)) = (added, removed) {
                let total = a.saturating_add(*r);
                if total > 0 {
                    let filled = (*a as f64 / total as f64 * 5.0).round() as usize;
                    for i in 0..5 {
                        row.adopt(&square(
                            if i < filled {
                                ground.muted
                            } else {
                                status.failed
                            },
                            i < filled,
                            4.0,
                        ));
                    }
                }
            }
        }
        ToolDetail::Create { lines, .. } => {
            row.adopt(&view! {
                ToolIcon kind:(ToolKind::Create)
            });
            if let Some(n) = lines {
                row.adopt(&label(format!("new · {n} lines")));
            }
        }
        ToolDetail::Search {
            files: Some(files), ..
        } => {
            let max = files
                .iter()
                .filter_map(|f| f.hits)
                .max()
                .unwrap_or(1)
                .max(1);
            for file in files {
                if let Some(n) = file.hits {
                    row.adopt(&square(
                        vm.kind.token(),
                        true,
                        2.0 + 10.0 * n as f32 / max as f32,
                    ));
                }
            }
            if files.iter().all(|f| f.hits.is_some()) {
                row.adopt(&label(format!(
                    "{} in {}",
                    files
                        .iter()
                        .filter_map(|f| f.hits)
                        .map(u64::from)
                        .sum::<u64>(),
                    files.len()
                )));
            }
        }
        ToolDetail::Run {
            exit_code,
            duration,
            ..
        } => {
            if let Some(n) = exit_code {
                row.adopt(&view! {
                    Chip text:(format!("exit {n}"))
                        tone:(Some(if *n == 0 { ground.muted } else { status.failed }))
                });
            }
            if let Some(t) = duration {
                row.adopt(&label(t.clone()));
            }
        }
        ToolDetail::Test { passed, failed, .. } => {
            row.adopt(&super::body::test_grid(*passed, *failed, 1.5, 40));
            if let Some(n) = failed {
                let counter = view! {
                    Label text:(format!("{n} failed")) role:(TextRole::Caption)
                        tone:(if *n > 0 { Tone::Signal(Signal::Failed) } else { Tone::Muted })
                };
                row.adopt(&counter);
            }
        }
        ToolDetail::Fetch {
            status: Some(n), ..
        } => {
            row.adopt(&view! {
                Chip text:(n.to_string())
                    tone:(Some(if *n >= 400 { status.failed } else { ground.muted }))
            });
        }
        ToolDetail::Plan {
            steps: Some(steps), ..
        } => {
            for step in steps {
                if let Some(done) = step.done {
                    row.adopt(&square(
                        if done { ground.muted } else { ground.outline },
                        done,
                        4.0,
                    ));
                }
            }
            if steps.iter().all(|s| s.done.is_some()) {
                row.adopt(&label(format!(
                    "{}/{}",
                    steps.iter().filter(|s| s.done == Some(true)).count(),
                    steps.len()
                )));
            }
        }
        ToolDetail::Spawn {
            harness,
            model,
            effort,
            ..
        } => {
            if let Some(h) = harness {
                row.adopt(&view! {
                    HarnessGlyph harness:(*h) size:12.0
                });
            }
            if let Some(m) = model {
                row.adopt(&label(m.short_name()));
                if let Some(e) = effort {
                    row.adopt(&view! {
                        EffortTrack levels:(e.levels) current:(e.selected()) tiers:(e.tiers)
                            colour:(m.hue.token()) size:(TrackSize::Compact)
                    });
                }
            }
        }
        ToolDetail::Message { read, .. } => {
            if *read == Some(true) {
                row.adopt(&label("read".into()));
            }
        }
        ToolDetail::Wait {
            agents: Some(agents),
        } => {
            for agent in agents {
                if let Some(s) = agent.status {
                    row.adopt(&view! {
                        StatusSquare status:(s)
                    });
                }
            }
            if agents.iter().all(|a| a.status.is_some()) {
                row.adopt(&label(format!(
                    "{} of {}",
                    agents
                        .iter()
                        .filter(|a| a.status == Some(AgentStatus::Done))
                        .count(),
                    agents.len()
                )));
            }
        }
        ToolDetail::Approval { .. } if vm.status == AgentStatus::Waiting => {
            row.adopt(&view! {
                Chip text:"pending" tone:(Some(status.needs_you))
            });
        }
        ToolDetail::Result {
            quantities,
            attachments,
            ..
        } => {
            let mut facts = Vec::new();
            if let Some(q) = quantities {
                facts.push(q.clone());
            }
            if let Some(files) = attachments {
                facts.push(format!(
                    "{} {}",
                    files.len(),
                    if files.len() == 1 { "file" } else { "files" }
                ));
            }
            if !facts.is_empty() {
                row.adopt(&label(facts.join(" · ")));
            }
        }
        _ => {}
    }
    row
}
