use crate::projects::{
    ActionCell, ActionCellProps, Caption, CaptionProps, InputCell, InputCellProps,
};
use crate::ui::press;
use crate::{
    buffer,
    model::{Model, Saved},
};
use mosaic::prelude::*;
use relay_core::*;
use relay_ui::prelude::{
    CellButton, CellButtonProps, Label, LabelProps, Rule, TextRole, Tone, UiText,
};
use relay_ui::tokens::{ground, px};

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
    pub description: String,
    pub skill: Option<SkillReference>,
    pub command: Option<HarnessCommand>,
}
#[derive(Clone, Debug)]
pub struct Completion {
    pub part: String,
    pub start: usize,
    pub end: usize,
    pub choices: Vec<Choice>,
    pub index: usize,
}

pub fn refresh(model: Model, force: bool) {
    if let Some(sender) = model.buffer_requests.get_untracked() {
        let _ = sender.send(crate::buffer_network::Request::Catalog {
            session: model.session.get_untracked(),
            force,
        });
    }
}

pub fn bind_discovery(model: Model) {
    let catalog_key = State::new(String::new());
    Effect::new(move || {
        let snapshot = model.snapshot.get();
        let id = model.session.get();
        let connected = model.connected.get();
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id && s.worker.is_some());
        let key = format!(
            "{connected}:{id}:{:?}:{:?}:{:?}",
            session.map(|s| (&s.workspaces, &s.connection_ids)),
            snapshot.installations,
            id.strip_prefix("director-draft-")
                .and_then(|id| snapshot.directors.iter().find(|d| d.id == id))
                .and_then(|d| snapshot.effective_profile(d).ok())
        );
        if catalog_key.get_untracked() != key {
            catalog_key.set(key);
            model.completion.set(None);
            model.command_flow.set(String::new());
            if connected && (session.is_some() || id.starts_with("director-draft-")) {
                refresh(model, false);
            }
        }
    });
    // Tree rows show effort from any loaded catalog of their harness, so
    // load one catalog per harness the selected project runs.
    let requested = State::new(std::collections::BTreeSet::<String>::new());
    Effect::new(move || {
        let snapshot = model.snapshot.get();
        let project = model.project.get();
        let catalogs = model.catalogs.get();
        if !model.connected.get() {
            return;
        }
        let Some(sender) = model.buffer_requests.get_untracked() else {
            return;
        };
        for harness in [Harness::Codex, Harness::ClaudeCode] {
            if catalogs
                .values()
                .any(|c| c.as_ref().is_ok_and(|c| c.harness == harness))
            {
                continue;
            }
            let Some(session) = snapshot.sessions.iter().find(|s| {
                s.project_id == project && s.worker.as_ref().is_some_and(|w| w.harness == harness)
            }) else {
                continue;
            };
            if requested.get_untracked().contains(&session.id) {
                continue;
            }
            requested.update(|ids| {
                ids.insert(session.id.clone());
            });
            let _ = sender.send(crate::buffer_network::Request::Catalog {
                session: session.id.clone(),
                force: false,
            });
        }
    });
}

pub fn complete(model: Model, part: &str, text: &str, caret: usize) {
    let catalog = model.catalogs.get_untracked();
    let Some(Ok(catalog)) = catalog.get(&model.session.get_untracked()) else {
        model.completion.set(None);
        return;
    };
    let Some(prefix) = text.get(..caret) else {
        return;
    };
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let token = &prefix[start..];
    let mut choices = vec![];
    if let Some(query) = token.strip_prefix('$') {
        // Reuse the prose parser so code, escaped and quoted references do not open completion.
        let valid = if query.is_empty() {
            skill_tokens(&format!("{prefix}x"))
                .iter()
                .any(|(i, _, _)| *i == start)
        } else {
            skill_tokens(prefix).iter().any(|(i, _, _)| *i == start)
        };
        if valid {
            for skill in catalog
                .skills
                .iter()
                .filter(|s| s.enabled && s.name.to_lowercase().contains(&query.to_lowercase()))
            {
                choices.push(Choice {
                    label: format!("${}", skill.name),
                    description: skill.description.clone(),
                    skill: Some(SkillReference {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                    }),
                    command: None,
                });
            }
        }
    } else if let Some(query) = token.strip_prefix('/')
        && start == 0
        && buffer::parts(model, &model.session.get_untracked())
            .first()
            .is_none_or(|p| p.id == part)
    {
        for command in catalog
            .commands
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&query.to_lowercase()))
        {
            choices.push(Choice {
                label: format!("/{}", command.name),
                description: command.reason.clone().unwrap_or_else(|| {
                    format!("{} {}", command.description, command.argument_hint)
                }),
                skill: None,
                command: Some(command.clone()),
            });
        }
    }
    choices.truncate(4);
    if choices.is_empty() {
        model.completion.set(None);
    } else {
        model.completion.set(Some(Completion {
            part: part.into(),
            start,
            end: caret,
            choices,
            index: 0,
        }));
    }
}

pub fn discovery_feedback(model: Model) -> Option<String> {
    let id = model.session.get();
    let text = plain_text(&buffer::parts(model, &id));
    let prose = if text.ends_with('$') {
        format!("{text}x")
    } else {
        text.clone()
    };
    if !text.trim_start().starts_with('/') && skill_tokens(&prose).is_empty() {
        return None;
    }
    let catalog = model.catalogs.get();
    if catalog.get(&id).is_some_and(Result::is_ok) {
        return None;
    }
    if model
        .snapshot
        .get()
        .sessions
        .iter()
        .any(|s| s.id == id && (s.fixture || s.worker.is_none()))
    {
        return Some("This transcript has no agent harness. Open a live conversation to use commands and skills.".into());
    }
    match catalog.get(&id) {
        None => Some("Loading commands and skills…".into()),
        Some(Err(error)) => Some(format!("Cannot load commands and skills: {error}")),
        Some(Ok(_)) => None,
    }
}

pub fn open(model: Model, name: &str) {
    let parts = buffer::parts(model, &model.session.get_untracked());
    let text = plain_text(&parts);
    if parts
        .iter()
        .all(|p| matches!(p.kind, PartKind::Text { .. }))
        && text
            .trim()
            .strip_prefix('/')
            .is_some_and(|rest| rest.split_whitespace().next() == Some(name))
    {
        buffer::edit(model, &model.session.get_untracked(), vec![]);
    }
    model.completion.set(None);
    model.command_argument.set(String::new());
    model.command_model.set(String::new());
    let name = canonical_command(name);
    model.command_flow.set(name.into());
    if matches!(name, "effort" | "fast") {
        let snapshot = model.snapshot.get_untracked();
        if let Some(session) = snapshot
            .sessions
            .iter()
            .find(|s| s.id == model.session.get_untracked())
        {
            let id = session
                .selection
                .model
                .as_ref()
                .or_else(|| session.worker.as_ref().and_then(|w| w.model.as_ref()));
            if let Some(id) = id
                && model
                    .catalogs
                    .get_untracked()
                    .get(&session.id)
                    .and_then(|c| c.as_ref().ok())
                    .is_some_and(|c| c.models.iter().any(|m| &m.id == id))
            {
                model.command_model.set(id.clone());
            }
        }
    }
}

/// Typed UI commands take the same path as selected suggestions.
pub fn intercept(model: Model) -> bool {
    let parts = buffer::parts(model, &model.session.get_untracked());
    let text = plain_text(&parts);
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let (name, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let catalogs = model.catalogs.get_untracked();
    let Some(Ok(catalog)) = catalogs.get(&model.session.get_untracked()) else {
        model
            .notice
            .set("Wait for commands and skills to load, or retry discovery".into());
        return true;
    };
    let Some(command) = catalog.commands.iter().find(|c| c.name == name) else {
        model.notice.set(format!("Unknown command /{name}"));
        return true;
    };
    if command.dispatch == CommandDispatch::Unavailable {
        model.notice.set(command.reason.clone().unwrap_or_default());
        return true;
    }
    if command.dispatch == CommandDispatch::Flow && !matches!(name, "compact" | "review") {
        if parts
            .iter()
            .any(|p| !matches!(p.kind, PartKind::Text { .. }))
        {
            model
                .notice
                .set("Send this command separately from skills, replies, and attachments".into());
            return true;
        }
        let native = canonical_command(name);
        if !arg.trim().is_empty() && !matches!(native, "model" | "effort" | "fast" | "resume") {
            model.notice.set(format!(
                "Open /{name} without arguments to use its controls"
            ));
            return true;
        }
        if native == "model"
            && !arg.trim().is_empty()
            && !catalog.models.iter().any(|m| m.id == arg.trim())
        {
            model
                .notice
                .set("Choose an available model from /model".into());
            return true;
        }
        if !arg.trim().is_empty() && matches!(native, "effort" | "fast") {
            let snapshot = model.snapshot.get_untracked();
            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == model.session.get_untracked());
            let current = session.map(|s| s.selection.clone()).unwrap_or_default();
            let id = current.model.as_ref().or_else(|| {
                session
                    .and_then(|s| s.worker.as_ref())
                    .and_then(|w| w.model.as_ref())
            });
            let Some(chosen) = catalog.models.iter().find(|m| Some(&m.id) == id) else {
                model.notice.set("Choose a session model first".into());
                return true;
            };
            let effort = if native == "effort" {
                if matches!(arg.trim(), "auto" | "default") {
                    None
                } else if chosen.efforts.iter().any(|e| e == arg.trim()) {
                    Some(arg.trim().into())
                } else {
                    model
                        .notice
                        .set("Effort is not supported by this model".into());
                    return true;
                }
            } else {
                current
                    .effort
                    .clone()
                    .or_else(|| chosen.default_effort.clone())
            };
            let fast = if native == "fast" {
                match arg.trim() {
                    "on" if chosen.fast => true,
                    "off" => false,
                    _ => {
                        model.notice.set(
                            "Use /fast on or /fast off with a model that supports fast mode".into(),
                        );
                        return true;
                    }
                }
            } else {
                current.fast
            };
            open(model, name);
            selection(model, chosen.id.clone(), effort, fast);
            return true;
        }
        if native == "resume" && !arg.trim().is_empty() {
            let snapshot = model.snapshot.get_untracked();
            let target = snapshot
                .sessions
                .iter()
                .find(|s| s.id == model.session.get_untracked())
                .and_then(|s| s.conversations.iter().find(|c| c.id == arg.trim()));
            if target.is_none() {
                model
                    .notice
                    .set("Choose a retained conversation from /resume".into());
                return true;
            }
            open(model, name);
            model.submit(
                Command::ConversationCommand {
                    session_id: model.session.get_untracked(),
                    name: "resume".into(),
                    argument: arg.trim().into(),
                },
                snapshot.revision,
                Saved::Action,
            );
            model.command_flow.set(String::new());
            return true;
        }
        open(model, name);
        model.command_argument.set(arg.trim().into());
        if name == "model" && !arg.trim().is_empty() {
            model.command_model.set(arg.trim().into());
        }
        return true;
    }
    false
}

fn selection(model: Model, model_id: String, effort: Option<String>, fast: bool) {
    let current = model
        .snapshot
        .get_untracked()
        .sessions
        .iter()
        .find(|s| s.id == model.session.get_untracked())
        .map(|s| s.selection.clone())
        .unwrap_or_default();
    let fast = if model.command_flow.get_untracked() == "effort" {
        current.fast
    } else {
        fast
    };
    model.submit(
        Command::SetHarnessSelection {
            session_id: model.session.get_untracked(),
            selection: HarnessSelection {
                model: Some(model_id),
                effort,
                fast,
                plan: current.plan,
            },
        },
        model.snapshot.get_untracked().revision,
        Saved::Action,
    );
    model.command_flow.set(String::new());
}

pub(crate) fn run(model: Model, name: &str, arg: &str) {
    if has_content(&buffer::parts(model, &model.session.get_untracked())) {
        model
            .notice
            .set("Send or clear the current draft before running this command".into());
        return;
    }
    let text = format!("/{name}{}{}", if arg.is_empty() { "" } else { " " }, arg);
    buffer::edit(
        model,
        &model.session.get_untracked(),
        vec![Part::text(text)],
    );
    model.command_flow.set(String::new());
    buffer::send(model);
}

#[component]
pub fn CommandPanel(model: Model) -> Element {
    let catalog = Derived::new(move || model.catalogs.get().get(&model.session.get()).cloned());
    let current = Derived::new(move || catalog.get().and_then(Result::ok).unwrap_or_default());
    let session = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .iter()
            .find(|s| s.id == model.session.get())
            .cloned()
    });
    let flow = Derived::new(move || model.command_flow.get());
    let argument = model.command_argument;
    let some = |text: String| Some(UiText::from(text));
    let disconnected = Derived::new(move || !model.connected.get());
    let head = view! {
        row height:{px(40.0)}px align:center gap:0px
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            row width:1fr min-width:0px height:min-content
                pad:(horizontal:{px(18.0)}px vertical:0px) {
                Label text:(Derived::new(move || format!("/{}", flow.get()))) role:(TextRole::Label)
                    weight:600
            }
            row width:max-content shrink:0 {
                CellButton label:"Refresh harness commands" rule:(Rule::Left)
                    disabled:(disconnected) on-press:(press(move || refresh(model, true))) {
                    Label text:"Refresh" role:(TextRole::Label) weight:500
                }
            }
            row width:max-content shrink:0 {
                CellButton label:"Close commands" rule:(Rule::Left)
                    on-press:(press(move || model.command_flow.set(String::new()))) {
                    Label text:"Close" role:(TextRole::Label) weight:500
                }
            }
        }
    };
    view! {
        col height:min-content stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
            { parent.adopt(&head); }
            if catalog.get().is_some_and(|c|c.is_err()) {
                Caption
                    text:(Derived::new(move || catalog.get().and_then(Result::err).unwrap_or_default()))
            }
            if catalog.get().is_none() {
                Caption text:"Loading harness commands…"
            }
            if catalog.get().is_some_and(|c|c.is_ok()) {
                if matches!(flow.get().as_str(),"help"|"skills") {
                    if flow.get()=="help" {
                        for (_, command) in {current.get().commands.clone().into_iter().map(|c|(c.name.clone(),c)).collect::<Vec<_>>()} {
                            let command=State::new(command.clone());
                            ActionCell label:(format!("Command /{}", command.get_untracked().name))
                                text:(some(format!("/{} {} · {}", command.get_untracked().name, command.get_untracked().argument_hint, command.get_untracked().reason.unwrap_or(command.get_untracked().description))))
                                on-press:(press(move || { let c = command.get_untracked(); if c.dispatch == CommandDispatch::Unavailable { model.notice.set(c.reason.unwrap_or_default()); } else { open(model, &c.name); } }))
                        }
                    }
                    for (_, skill) in {current.get().skills.clone().into_iter().map(|s|(format!("{}:{}",s.id,s.name),s)).collect::<Vec<_>>()} {
                        let skill=State::new(skill.clone());
                        ActionCell label:(format!("Select skill {}", skill.get_untracked().name))
                            text:(some(format!("${} · {}", skill.get_untracked().name, skill.get_untracked().description)))
                            disabled:(Derived::new(move || !skill.get().enabled))
                            on-press:(press(move || { let s = skill.get_untracked(); let mut parts = buffer::parts(model, &model.session.get_untracked()); parts.push(Part { id: uuid::Uuid::new_v4().to_string(), kind: PartKind::Skill { skill: SkillReference { id: s.id, name: s.name } } }); parts.push(Part::text(" ")); buffer::edit(model, &model.session.get_untracked(), parts); model.command_flow.set(String::new()); }))
                    }
                } else if matches!(flow.get().as_str(),"model"|"effort"|"fast") {
                    if model.command_model.get().is_empty() {
                        for (_, choice) in {current.get().models.clone().into_iter().map(|m|(m.id.clone(),m)).collect::<Vec<_>>()} {
                            let choice=State::new(choice.clone());
                            ActionCell
                                label:(format!("Choose model {}", choice.get_untracked().name))
                                text:(some(choice.get_untracked().name))
                                on-press:(press(move || model.command_model.set(choice.get_untracked().id)))
                        }
                        ActionCell label:"Use harness model defaults"
                            text:(some("Use harness defaults".into()))
                            on-press:(press(move || { model.submit(Command::SetHarnessSelection { session_id: model.session.get_untracked(), selection: HarnessSelection::default() }, model.snapshot.get_untracked().revision, Saved::Action); model.command_flow.set(String::new()); }))
                    } else if current.get().models.iter().any(|m|m.id==model.command_model.get()) {
                        let chosen=State::new(current.get().models.iter().find(|m|m.id==model.command_model.get_untracked()).cloned().unwrap());
                        Caption
                            text:(format!("{} · applies to the next turn", chosen.get_untracked().name))
                        for (_, effort) in {chosen.get().efforts.clone().into_iter().map(|e|(e.clone(),e)).collect::<Vec<_>>()} {
                            let effort=State::new(effort.clone());
                            ActionCell label:(format!("Choose effort {}", effort.get_untracked()))
                                text:(some(effort.get_untracked()))
                                on-press:(press(move || selection(model, chosen.get_untracked().id, Some(effort.get_untracked()), false)))
                        }
                        ActionCell label:"Use default effort" text:(some("Default effort".into()))
                            on-press:(press(move || selection(model, chosen.get_untracked().id, None, false)))
                        if chosen.get().fast {
                            ActionCell label:"Enable fast mode" text:(some("Fast mode".into()))
                                on-press:(press(move || selection(model, chosen.get_untracked().id, chosen.get_untracked().default_effort, true)))
                        }
                        ActionCell label:"Choose another model" text:(some("Back".into()))
                            on-press:(press(move || model.command_model.set(String::new())))
                    } else {
                        Caption text:"Model is unavailable. Refresh or choose another model."
                    }
                } else if flow.get()=="plan" {
                    Caption text:"Planning mode applies to the next turn."
                    ActionCell label:"Toggle planning mode"
                        text:(Some(UiText::from(Derived::new(move || if session.get().is_some_and(|s| s.selection.plan) { "Leave planning mode".to_string() } else { "Enter planning mode".to_string() }))))
                        on-press:(press(move || { let mut selection = session.get_untracked().map(|s| s.selection).unwrap_or_default(); selection.plan = !selection.plan; model.submit(Command::SetHarnessSelection { session_id: model.session.get_untracked(), selection }, model.snapshot.get_untracked().revision, Saved::Action); model.command_flow.set(String::new()); }))
                } else if flow.get()=="permissions" {
                    for (index,mode) in ApprovalMode::ALL.into_iter().enumerate() {
                        ActionCell label:(mode.label().to_string())
                            on-press:(press(move || { model.submit(Command::SetWorkerExecution { session_id: model.session.get_untracked(), execution: Some(ExecutionSettings { approval: ApprovalMode::ALL[index] }) }, model.snapshot.get_untracked().revision, Saved::Action); model.command_flow.set(String::new()); }))
                    }
                } else if matches!(flow.get().as_str(),"new"|"clear"|"resume"|"fork") {
                    if flow.get()=="resume" {
                        for (_, conversation) in {session.get().map(|s|s.conversations).unwrap_or_default().into_iter().map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                            let conversation=State::new(conversation.clone());
                            ActionCell label:(conversation.get_untracked().title)
                                on-press:(press(move || { model.submit(Command::ConversationCommand { session_id: model.session.get_untracked(), name: "resume".into(), argument: conversation.get_untracked().id }, model.snapshot.get_untracked().revision, Saved::Action); model.command_flow.set(String::new()); }))
                        }
                    } else {
                        Caption
                            text:(Derived::new(move || if flow.get()=="fork" {"Branch history into independent repository worktrees. Connected ordinary directories remain shared.".to_string()} else {"Start with empty context and retain this conversation for resume.".to_string()}))
                        ActionCell label:"Confirm conversation action" text:(some("Confirm".into()))
                            disabled:(Derived::new(move || !model.connected.get() || model.busy.get()))
                            on-press:(press(move || { model.submit(Command::ConversationCommand { session_id: model.session.get_untracked(), name: flow.get_untracked(), argument: String::new() }, model.snapshot.get_untracked().revision, Saved::Action); model.command_flow.set(String::new()); }))
                    }
                } else if matches!(flow.get().as_str(),"status"|"context"|"usage"|"diff") {
                    for (_, group) in {session.get().map(|s|crate::conversation::provenance(&model.snapshot.get(),&s,flow.get()=="diff")).unwrap_or_default().into_iter().map(|g|(g.key.clone(),g)).collect::<Vec<_>>()} {
                        let group=State::new(group.clone());
                        Caption tone:(Tone::Ink)
                            text:(Derived::new(move || format!("{}\n{}{}",group.get().title,group.get().rows.iter().map(|(k,v)|format!("{k}: {v}")).collect::<Vec<_>>().join("\n"),if flow.get()=="diff"{group.get().changes.map(|c|format!("\n{c}")).unwrap_or_default()}else{String::new()})))
                    }
                } else if flow.get()=="mcp" {
                    if current.get().mcp.is_empty() {
                        Caption text:"No MCP servers reported by the harness."
                    }
                    for (_,server) in current.get().mcp.into_iter().enumerate() {
                        Caption text:(server.clone()) tone:(Tone::Ink)
                    }
                } else if flow.get()=="review" && current.get().harness==Harness::Codex {
                    if model.command_model.get().is_empty() {
                        ActionCell label:"Review uncommitted changes"
                            text:(some("Uncommitted changes".into()))
                            on-press:(press(move || run(model, "review", "")))
                        ActionCell label:"Choose a base branch for review"
                            text:(some("Base branch".into()))
                            on-press:(press(move || model.command_model.set("branch".into())))
                        ActionCell label:"Choose a commit for review" text:(some("Commit".into()))
                            on-press:(press(move || model.command_model.set("commit".into())))
                        ActionCell label:"Enter review instructions"
                            text:(some("Custom instructions".into()))
                            on-press:(press(move || model.command_model.set("custom".into())))
                    } else {
                        InputCell value:(argument) label:"Review target"
                        ActionCell label:"Start review" text:(some("Review".into()))
                            disabled:(Derived::new(move || argument.get().trim().is_empty()))
                            on-press:(press(move || { let target = model.command_model.get_untracked(); let arg = argument.get_untracked(); run(model, "review", &if target == "custom" { arg } else { format!("{target} {arg}") }); }))
                        ActionCell label:"Choose another review target" text:(some("Back".into()))
                            on-press:(press(move || model.command_model.set(String::new())))
                    }
                } else {
                    if flow.get()!="compact" || current.get().harness==Harness::ClaudeCode {
                        InputCell value:(argument) label:"Command arguments"
                    }
                    ActionCell label:"Run harness command" text:(some("Run".into()))
                        on-press:(press(move || run(model, &flow.get_untracked(), &argument.get_untracked())))
                }
                for (_,warning) in current.get().warnings.into_iter().enumerate() {
                    Caption text:(warning.clone())
                }
            }
        }
    }
}
