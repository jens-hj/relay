//! Tool calls are transcript entries with their own input, output and state.
use crate::{
    conversation::{BufferText, BufferTextProps, ControllerState, Location},
    labels::{RunState, StatusGlyph, StatusGlyphProps},
    model::Model,
    panels::{BoundedPanel, BoundedPanelProps},
    styles::*,
    theme::*,
};
use mosaic::{core::theme::color, prelude::*};
use relay_core::{Message, ToolKind, ToolState};

fn category(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Terminal => "Terminal",
        ToolKind::Read => "Read file",
        ToolKind::Edit => "File changes",
        ToolKind::Search => "Search",
        ToolKind::Web => "Web",
        ToolKind::Agent => "Agent",
        ToolKind::Other => "Tool",
    }
}

fn swatch(kind: ToolKind) -> ColorToken {
    match kind {
        ToolKind::Terminal | ToolKind::Agent => tint.lilac,
        ToolKind::Read | ToolKind::Web => tint.sky,
        ToolKind::Edit => tint.mint,
        ToolKind::Search | ToolKind::Other => tint.sand,
    }
}

fn state(message: &Option<Message>, snapshot: &relay_core::Snapshot) -> RunState {
    if let Some(message) = message
        && let Some(tool) = &message.tool
        && tool.state == ToolState::Running
        && snapshot.tool_permissions.iter().any(|p| {
            if p.session_id != message.session_id
                || p.run_id != tool.run_id
                || p.decision.is_some()
                || p.expired
            {
                return false;
            }
            let input: serde_json::Value = serde_json::from_str(&p.description).unwrap_or_default();
            input["itemId"]
                .as_str()
                .is_some_and(|id| message.id == format!("codex-{}-{id}", tool.run_id))
                || (p.tool == tool.name && p.description == tool.input)
        })
    {
        return RunState::Waiting;
    }
    match message
        .as_ref()
        .and_then(|m| m.tool.as_ref())
        .map(|t| t.state)
    {
        Some(ToolState::Running) => RunState::Running,
        Some(ToolState::Completed) => RunState::Completed,
        Some(ToolState::Failed) => RunState::Failed,
        Some(ToolState::Interrupted) => RunState::Interrupted,
        Some(ToolState::Unknown) | None => RunState::Unavailable,
    }
}

#[component]
pub(crate) fn ToolActivity(
    model: Model,
    message: Derived<Option<Message>>,
    controller: ControllerState,
) -> Element {
    let tool = Derived::new(move || message.get().and_then(|m| m.tool));
    let kind = tool
        .get_untracked()
        .map(|t| t.kind)
        .unwrap_or(ToolKind::Other);
    let open = State::new(true);
    let raw = State::new(false);
    let source = State::new(false);
    let activity_state = Derived::new(move || state(&message.get(), &model.snapshot.get()));
    let output = Derived::new(move || message.get().map(|m| m.body).unwrap_or_default());
    let identity = Derived::new(move || message.get().map(|m| m.id).unwrap_or_default());
    view! {
        col #relay.module width:1fr min-width:0px gap:0px {
            button #relay.tree-control @click:{open.set(!open.get_untracked());} width:1fr
                height:{px(36.0)}px justify:between font-color:ink.fg fill:surface.raised
                label:{format!("Toggle {} activity {}", category(kind), identity.get())} {
                row min-width:0px width:1fr align:center gap:{px(9.0)}px {
                    el width:{px(36.0)}px height:fill shrink:0 align:center justify:center
                        fill:{color(swatch(kind))} font-color:ink.fg
                        stroke:(width:{px(1.0)} color:rule.line edges:right) {
                        icon size:{px(18.0)}px
                            {match kind {ToolKind::Terminal=>tool_terminal,ToolKind::Read=>tool_read,ToolKind::Edit=>tool_edit,ToolKind::Search=>tool_search,ToolKind::Web=>tool_web,ToolKind::Agent=>tool_agent,ToolKind::Other=>tool_other}}
                    }
                    row #relay.eyebrow min-width:0px width:1fr height:fill align:center
                        font-color:ink.fg {
                        text {category(kind)}
                    }
                    row width:max-content shrink:0 align:center gap:{px(7.0)}px
                        pad:(right:{px(10.0)}px)
                        label:{format!("{}: {}",category(kind),activity_state.get().label())} {
                        StatusGlyph state:(activity_state)
                        text font-size:{px(12.0)}px
                            font-color:{color(activity_state.get().text_color())}
                            {activity_state.get().label()}
                        icon size:{px(12.0)}px
                            {if open.get() {tree_chevron_down} else {tree_chevron_right}}
                    }
                }
            }
            row height:min-content min-width:0px pad:{px(10.0)}px gap:{px(8.0)}px
                stroke:(width:{px(1.0)} color:rule.hair edges:top) selectable {
                if kind == ToolKind::Terminal {
                    text font-color:{color(accent.focus)} {"$"}
                }
                text font-size:{px(13.0)}px
                    {tool.get().map(|t|if t.target.is_empty() {t.name} else {t.target}).unwrap_or_default()}
            }
            if open.get() {
                col height:min-content min-width:0px gap:0px {
                    if kind == ToolKind::Terminal {
                        TerminalOutput output:(output)
                            running:(Derived::new(move || activity_state.get()==RunState::Running))
                    } else if kind == ToolKind::Edit {
                        FileChanges
                            input:(Derived::new(move || tool.get().map(|t|t.input).unwrap_or_default()))
                            output:(output)
                    } else if kind == ToolKind::Read || kind == ToolKind::Search {
                        IndexedOutput output:(output) kind:(kind)
                    } else {
                        ResultOutput output:(output) kind:(kind)
                    }
                    row height:{px(30.0)}px align:center justify:between
                        stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                        row #relay.caption width:1fr min-width:0px height:fill align:center
                            pad:(horizontal:{px(10.0)}px vertical:0px) {
                            text
                                {tool.get().map(|t|match (t.directory,t.exit_code) {(Some(dir),Some(code))=>format!("{dir} · Exit {code}"),(Some(dir),None)=>dir,(None,Some(code))=>format!("Exit {code}"),_=>t.name}).unwrap_or_default()}
                        }
                        button #relay.header-action @click:{source.set(!source.get_untracked());}
                            font-size:{px(11.0)}px
                            label:{format!("Toggle tool output {}",identity.get())}
                            {if source.get() {"Hide output"} else if kind==ToolKind::Edit {"Output"} else {"Raw output"}}
                        button #relay.header-action @click:{raw.set(!raw.get_untracked());}
                            font-size:{px(11.0)}px
                            label:{format!("Toggle tool input {}",identity.get())}
                            {if raw.get() {"Hide input"} else {"Input"}}
                    }
                    if source.get() {
                        col height:min-content pad:{px(10.0)}px
                            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                            BoundedPanel limit:(Derived::new(||px(240.0))) {
                                BufferText model:(model) controller:(controller)
                                    location:(Location::Source {message:identity.get(),start:0,end:None})
                                    mirror:false
                            }
                        }
                    }
                    if raw.get() {
                        col height:min-content pad:{px(10.0)}px fill:surface.raised
                            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                            BoundedPanel limit:(Derived::new(|| px(200.0))) {
                                col height:min-content selectable font-size:{px(12.0)}px {
                                    text {tool.get().map(|t|t.input).unwrap_or_default()}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TerminalOutput(output: Derived<String>, running: Derived<bool>) -> Element {
    view! {
        col height:min-content min-width:0px pad:{px(10.0)}px fill:surface.base
            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
            BoundedPanel limit:(Derived::new(|| px(240.0))) {
                col height:min-content selectable font-size:{px(12.0)}px {
                    text
                        {if output.get().is_empty() {if running.get() {"Waiting for output".into()} else {"No output".into()}} else {output.get()}}
                }
            }
        }
    }
}

#[component]
fn ResultOutput(output: Derived<String>, kind: ToolKind) -> Element {
    let label = match kind {
        ToolKind::Read => "Contents",
        ToolKind::Search => "Matches",
        ToolKind::Web => "Response",
        ToolKind::Agent => "Result",
        _ => "Output",
    };
    view! {
        col height:min-content gap:{px(6.0)}px pad:{px(10.0)}px
            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
            row #relay.eyebrow height:min-content gap:{px(7.0)}px align:center {
                el width:{px(4.0)}px height:{px(10.0)}px fill:{color(swatch(kind))} {}
                text {label}
            }
            BoundedPanel limit:(Derived::new(|| px(240.0))) {
                col height:min-content selectable font-size:{px(12.0)}px {
                    text
                        {if output.get().is_empty() {"No output yet".into()} else {readable_output(&output.get())}}
                }
            }
        }
    }
}

/// Claude content blocks and MCP responses keep their original JSON on the wire,
/// but their text blocks are readable in the conversation.
fn readable_output(output: &str) -> String {
    let value: serde_json::Value = match serde_json::from_str(output) {
        Ok(value) => value,
        Err(_) => return output.into(),
    };
    if let Some(text) = value.as_str() {
        return text.into();
    }
    let blocks = value.as_array().or_else(|| value["content"].as_array());
    if let Some(blocks) = blocks {
        return blocks
            .iter()
            .map(|b| {
                b["text"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| b.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.into())
}

fn changes(input: &str, output: &str) -> Vec<(usize, (String, String))> {
    let value = serde_json::from_str::<serde_json::Value>(input).unwrap_or_default();
    if let Some(files) = value.as_array() {
        return files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    i,
                    (
                        f["path"].as_str().unwrap_or("File").into(),
                        f["diff"].as_str().unwrap_or("").into(),
                    ),
                )
            })
            .collect();
    }
    let path = value["file_path"]
        .as_str()
        .or_else(|| value["path"].as_str())
        .unwrap_or("Changes");
    let mut diff = String::new();
    if let Some(old) = value["old_string"].as_str() {
        for line in old.lines() {
            diff.push_str(&format!("-{line}\n"));
        }
    }
    if let Some(new) = value["new_string"]
        .as_str()
        .or_else(|| value["content"].as_str())
    {
        for line in new.lines() {
            diff.push_str(&format!("+{line}\n"));
        }
    }
    if diff.is_empty() {
        diff = readable_output(output);
    }
    vec![(0, (path.into(), diff))]
}

#[component]
fn FileChanges(input: Derived<String>, output: Derived<String>) -> Element {
    view! {
        BoundedPanel limit:(Derived::new(|| px(280.0))) {
            col height:min-content min-width:0px {
                for (_, file) in {changes(&input.get(),&output.get())} {
                    let file=State::new(file.clone());
                    col height:min-content gap:0px {
                        if changes(&input.get(),&output.get()).len()>1 {
                            row #relay.caption height:min-content pad:{px(10.0)}px
                                fill:surface.raised
                                stroke:(width:{px(1.0)} color:rule.hair edges:top) selectable {
                                text {file.get().0}
                            }
                        }
                        col height:min-content min-width:0px selectable font-size:{px(12.0)}px {
                            for (_, line) in {file.get().1.lines().take(240).enumerate().map(|(i,l)|(i,l.to_owned())).collect::<Vec<_>>()} {
                                let line=State::new(line.clone());
                                row height:min-content min-width:0px gap:{px(10.0)}px
                                    pad:(horizontal:{px(10.0)}px vertical:{px(2.0)}px)
                                    fill:{if line.get().starts_with('+') {color(tint.mint)} else if line.get().starts_with('-') {color(tint.sand)} else {color(surface.panel)}} {
                                    text font-color:ink.fg {line.get()}
                                }
                            }
                            if file.get().1.lines().count() > 240 {
                                row #relay.caption height:min-content pad:{px(10.0)}px {
                                    text {"Preview limited to 240 lines"}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// File reads use numbered lines; searches use a ruled result list. Keep the
/// original text available even when the bounded excerpt has many tiny lines.
#[component]
fn IndexedOutput(output: Derived<String>, kind: ToolKind) -> Element {
    let text = Derived::new(move || readable_output(&output.get()));
    view! {
        col height:min-content min-width:0px gap:0px
            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
            row #relay.eyebrow height:{px(26.0)}px align:center justify:between
                pad:(horizontal:{px(10.0)}px vertical:0px) fill:surface.raised {
                text {if kind==ToolKind::Read {"Contents"} else {"Results"}}
                text
                    {let count=text.get().lines().count();format!("{count} {}",if count==1 {"line"} else {"lines"})}
            }
            BoundedPanel limit:(Derived::new(|| px(240.0))) {
                col height:min-content min-width:0px font-size:{px(12.0)}px {
                    for (index, line) in {text.get().lines().take(240).enumerate().map(|(i,l)|(i,l.to_owned())).collect::<Vec<_>>()} {
                        let line=State::new(line.clone());
                        let number=*index+1;
                        row height:min-content min-width:0px gap:0px
                            stroke:(width:{px(if kind==ToolKind::Search {1.0} else {0.0})} color:rule.hair edges:bottom) {
                            row width:{px(42.0)}px shrink:0 justify:end
                                pad:(horizontal:{px(8.0)}px vertical:{px(3.0)}px)
                                font-color:ink.muted fill:surface.raised
                                stroke:(width:{px(1.0)} color:rule.hair edges:right) {
                                if kind==ToolKind::Read {
                                    text {number.to_string()}
                                } else {
                                    el width:{px(5.0)}px height:{px(5.0)}px fill:tint.sand {}
                                }
                            }
                            row height:min-content width:1fr min-width:0px selectable
                                pad:(horizontal:{px(10.0)}px vertical:{px(3.0)}px) {
                                text {if line.get().is_empty() {" ".into()} else {line.get()}}
                            }
                        }
                    }
                    if text.get().lines().count()>240 {
                        col height:min-content pad:{px(10.0)}px selectable {
                            text {text.get().lines().skip(240).collect::<Vec<_>>().join("\n")}
                        }
                    }
                    if text.get().is_empty() {
                        row #relay.caption height:min-content pad:{px(10.0)}px {
                            text {"No output yet"}
                        }
                    }
                }
            }
        }
    }
}

/// Old transcripts lack lifecycle metadata. Give them a typed presentation
/// while explicitly leaving their status unknown; never infer success from text.
pub(crate) fn with_legacy_tool(mut message: Message) -> Message {
    if message.tool.is_some()
        || !matches!(message.kind.as_str(), "command_execution" | "file_change")
    {
        return message;
    }
    let (name, kind, input, target, output, code) =
        if let Some(command) = message.body.strip_prefix("Command: ") {
            let mut lines = command.splitn(3, '\n');
            let command = lines.next().unwrap_or("").to_owned();
            let code = lines
                .next()
                .and_then(|s| s.strip_prefix("Exit: "))
                .and_then(|s| s.parse::<i64>().ok());
            (
                "commandExecution".into(),
                ToolKind::Terminal,
                command.clone(),
                command,
                lines.next().unwrap_or("").into(),
                code,
            )
        } else if message.kind == "file_change" {
            (
                "fileChange".into(),
                ToolKind::Edit,
                message.body.clone(),
                "Recorded file changes".into(),
                message.body.clone(),
                None,
            )
        } else if let Some((name, input)) = message.body.split_once('\n')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(input)
            && value.is_object()
        {
            let target = [
                "command",
                "file_path",
                "path",
                "pattern",
                "query",
                "url",
                "description",
            ]
            .into_iter()
            .find_map(|key| value[key].as_str())
            .unwrap_or(name);
            (
                name.into(),
                ToolKind::from_name(name),
                input.into(),
                target.into(),
                String::new(),
                None,
            )
        } else {
            (
                "Recorded tool".into(),
                ToolKind::Other,
                String::new(),
                "Recorded tool output".into(),
                message.body.clone(),
                None,
            )
        };
    message.tool = Some(relay_core::ToolCall {
        run_id: String::new(),
        name,
        kind,
        state: ToolState::Unknown,
        input,
        target,
        directory: None,
        exit_code: code,
    });
    message.body = output;
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_activity_uses_recorded_type_without_inventing_status() {
        let message = Message {
            id: "old".into(),
            session_id: "session".into(),
            author: "Codex".into(),
            kind: "command_execution".into(),
            body: "Command: cargo test\nExit: 0\ntests passed".into(),
            parts: vec![],
            tool: None,
        };
        let shown = with_legacy_tool(message.clone());
        assert_eq!(shown.tool.as_ref().unwrap().kind, ToolKind::Terminal);
        assert_eq!(shown.tool.as_ref().unwrap().state, ToolState::Unknown);
        assert_eq!(shown.tool.as_ref().unwrap().target, "cargo test");
        assert_eq!(shown.body, "tests passed");
        assert_eq!(message.body, "Command: cargo test\nExit: 0\ntests passed");
    }
}
