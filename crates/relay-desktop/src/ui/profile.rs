use super::*;
use crate::window_chrome::{WindowControlsSpace, WindowControlsSpaceProps};
use relay_ui::overlay::{Popover, PopoverContent, PopoverProps, Tooltip, TooltipProps};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Ellipsis, EllipsisProps, HarnessGlyph, HarnessGlyphProps, Label,
    LabelProps, PrimaryButton, PrimaryButtonProps, Rule, SerifSize, SerifTitle, SerifTitleProps,
    TextRole, Tone,
};
use relay_ui::surfaces::{
    FlagColumn, HarnessChoices, HarnessChoicesProps, HarnessOption, Module, ModuleProps, OnFlag,
    OnPermission, PermissionMatrix, PermissionMatrixProps, PermissionRow, ReadoutCell,
    ReadoutCellProps, Stepper, StepperProps, TextInput, TextInputProps,
};
use relay_ui::tokens::Signal;

/// Profile fields an agent can override, each as a whole.
const OVERRIDE_FIELDS: usize = 9;

#[component]
fn AgentProfileSelector(model: Model) -> Element {
    let open = State::new(false);
    let content: PopoverContent = std::rc::Rc::new(move || {
        view! {
            col width:{px(240.0)}px height:min-content {
                for (_, id) in { ["director", "worker"].into_iter().map(|id| (id.to_string(), id.to_string())) } {
                    let id = id.clone();
                    let title = if id == "director" { "Director" } else { "Worker" };
                    row height:{px(40.0)}px {
                        CellButton label:(format!("Use {title} profile")) start:true
                            on-press:(press(move || { model.select_agent_template(id.clone()); open.set(false); })) {
                            Label text:(title) role:(TextRole::Label)
                        }
                    }
                }
                for (_, template) in {model.snapshot.get().agent_templates.into_iter().filter(|p| p.project_id == model.project.get()).map(|p| (p.id.clone(), p)).collect::<Vec<_>>()} {
                    let template = template.clone();
                    let id = template.id.clone();
                    row height:{px(40.0)}px {
                        CellButton label:(format!("Use {} profile", template.name)) start:true
                            on-press:(press(move || { model.select_agent_template(id.clone()); open.set(false); })) {
                            Ellipsis text:(template.name.clone()) role:(TextRole::Label)
                        }
                    }
                }
            }
        }
    });
    let trigger = view! {
        button @click:{open.set(!open.get_untracked())} label:"Select agent profile"
            width:max-content height:min-content min-width:0px align:center gap:{px(8.0)}px pad:0px
            radius:0px fill:(Color::TRANSPARENT) stroke:(width:0px color:{Color::TRANSPARENT})
            hover { fill:ground.hover } focused { fill:ground.hover } {
            row width:max-content min-width:0px height:min-content clip {
                SerifTitle text:(Derived::new(move || model.agent_template_name()))
                    size:(SerifSize::Large)
            }
            icon size:{px(12.0)}px tree-chevron-down
        }
    };
    let handle = trigger.clone();
    view! {
        stack width:max-content min-width:0px height:min-content {
            { parent.adopt(&trigger); }
            Popover open:(open) label:"Agent profiles" content:(content) trigger:(Some(handle))
        }
    }
}

#[component]
fn AgentSetup(model: Model) -> Element {
    let parent_title = Derived::new(move || {
        let id = model.editor_parent.get();
        model
            .snapshot
            .get()
            .sessions
            .into_iter()
            .find(|s| Some(&s.id) == id.as_ref())
            .map(|s| s.title)
            .unwrap_or_default()
    });
    view! {
        col height:min-content {
            TextInput value:(model.editor_name) label:"Custom name (optional)"
                placeholder:"Automatic from first message" framed:false
            if model.editor_parent.get().is_some() {
                row height:min-content pad:{px(14.0)}px {
                    Label text:(Derived::new(move || format!("Subagent of {}", parent_title.get())))
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        }
    }
}

#[component]
fn ProfileChangeActions(model: Model) -> Element {
    let open = State::new(false);
    let content: PopoverContent = std::rc::Rc::new(move || {
        view! {
            col width:{px(260.0)}px height:min-content {
                TextInput value:(model.editor_template_name) label:"Profile name"
                    placeholder:"Profile name" framed:false
                row height:{px(40.0)}px {
                    CellButton label:"Save new agent profile"
                        on-press:(press(move || { model.save_agent_template(); open.set(false); })) {
                        Label text:"Save profile" role:(TextRole::Label)
                    }
                }
            }
        }
    });
    view! {
        row width:max-content shrink:0 {
            row width:{px(40.0)}px {
                Tooltip text:"Override for this agent" width:(Dimension::Fill)
                    height:(Dimension::Fill) {
                    CellButton label:"Override for this agent" rule:(Rule::Left)
                        horizontal-inset:0.0 selected:(model.editor_override_accepted)
                        on-press:(press(move || model.editor_override_accepted.set(true))) {
                        { parent.adopt(&relay_ui::marks::mark(relay_ui::marks::Mark::Check, 12.0, ground.ink)); }
                    }
                }
            }
            stack width:{px(40.0)}px {
                Tooltip text:"Save as new profile" width:(Dimension::Fill)
                    height:(Dimension::Fill) {
                    CellButton label:"Save as new profile" rule:(Rule::Left) horizontal-inset:0.0
                        on-press:(press(move || open.set(!open.get_untracked()))) {
                        icon size:{px(14.0)}px plus-icon
                    }
                }
                Popover open:(open) label:"Save agent profile" content:(content)
                    align:(OverlayAlign::End)
            }
            row width:{px(40.0)}px {
                Tooltip text:"Discard profile changes" width:(Dimension::Fill)
                    height:(Dimension::Fill) {
                    CellButton label:"Discard profile changes" rule:(Rule::Left)
                        horizontal-inset:0.0
                        on-press:(press(move || model.discard_profile_changes())) {
                        icon size:{px(14.0)}px reset-icon
                    }
                }
            }
        }
    }
}

pub(crate) fn overridden_fields(overrides: &ProfileOverrides) -> Vec<&'static str> {
    [
        ("Harness", overrides.harness.is_some()),
        ("Model", overrides.model.is_some()),
        ("Execution", overrides.execution.is_some()),
        ("Network", overrides.network_access.is_some()),
        ("Scope", overrides.scope.is_some()),
        ("Responsibilities", overrides.responsibilities.is_some()),
        ("Completion", overrides.completion.is_some()),
        ("Workers", overrides.max_workers.is_some()),
        ("Worker Model", overrides.worker_models.is_some()),
        ("Permissions", overrides.permissions.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, set)| set.then_some(name))
    .collect()
}

/// A one-line summary of a profile: harness, scope and worker limit.
fn profile_summary(profile: &AgentProfile, snapshot: &Snapshot) -> String {
    let harness = match profile.harness {
        Harness::Codex => "Codex",
        Harness::ClaudeCode => "Claude Code",
    };
    let scope = match &profile.scope {
        AgentScope::Project => "Whole project".to_string(),
        AgentScope::Issues { .. } => scope_label(&profile.scope, snapshot)
            .trim_start_matches("Selected issues: ")
            .to_string(),
    };
    let workers = profile.max_workers;
    format!(
        "{harness} · {scope} · {workers} worker{}",
        if workers == 1 { "" } else { "s" }
    )
}

/// The profile editor: its header, where its values come from, then the
/// action matrix and TOML beside (or above) the field modules, all sharing
/// their rules, and the save bar.
#[component]
pub(crate) fn Profiles(model: Model) -> Element {
    let width = State::new(0.0f32);
    let wide = Derived::new(move || width.get() >= px(960.0));
    let save_disabled = Derived::new(move || {
        model.busy.get()
            || (model.editor.get() == EditTarget::New
                && model.editor_profile.get().model.is_empty())
            || !model.connected.get()
            || (model.editor.get() == EditTarget::New
                && model.editor_profile.get() != model.editor_base.get()
                && !model.editor_override_accepted.get())
    });
    view! {
        col width:1fr gap:0px @layout:{move |rect: Rect| width.set(rect.size.width)} {
            ProfileHeader model:(model)
            Notice model:(model)
            scroll {
                col height:min-content gap:0px {
                    if model.editor.get() != EditTarget::Defaults {
                        AgentSetup model:(model)
                    }
                    Inheritance model:(model)
                    // One grid for both layouts: the track template changes with the
                    // width, the controls themselves are never rebuilt.
                    stack height:min-content min-width:0px {
                        grid height:min-content gap:0px align:start
                            cols:{if wide.get() { GridTracks::new([GridTrack::fr(1.0), GridTrack::fr(1.0)]) } else { GridTracks::new([GridTrack::fr(1.0)]) }} {
                            col height:min-content min-width:0px gap:0px
                                label:"Action matrix column" {
                                ActionMatrix model:(model)
                                ProfileToml model:(model)
                            }
                            col height:min-content min-width:0px {
                                ProfileControls model:(model)
                            }
                        }
                        if wide.get() {
                            grid nohit gap:0px rows:{GridTracks::new([GridTrack::fr(1.0)])}
                                cols:{GridTracks::new([GridTrack::fr(1.0), px(1.0).into(), GridTrack::fr(1.0)])} {
                                el nohit {}
                                el width:{px(1.0)}px height:fill fill:ground.rule-strong nohit
                                    label:"Profile column divider" {}
                                el nohit {}
                            }
                        }
                    }
                }
            }
            row height:{px(50.0)}px shrink:0 align:center gap:0px label:"Save bar"
                stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                row width:1fr min-width:0px height:min-content
                    pad:(horizontal:{px(18.0)}px vertical:0px) {
                    Ellipsis
                        text:(Derived::new(move || if model.editor.get() == EditTarget::New {
                        if model.editor_override_accepted.get() { "Profile changes apply only to this agent".into() } else { "Choose a profile and configure this agent".into() }
                    } else { "Explicit overrides survive project default changes".into() }))
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
                if model.editor.get() == EditTarget::New && model.editor_profile.get() != model.editor_base.get() {
                    ProfileChangeActions model:(model)
                }
                row width:max-content shrink:0 {
                    PrimaryButton
                        text:(Derived::new(move || if model.editor.get() == EditTarget::New { "Create agent".into() } else { "Save profile".into() }))
                        on-press:(press(move || model.save_profile())) disabled:(save_disabled)
                        fill:true
                }
            }
        }
    }
}

/// The profile's 74px identifier header: its mark, kind and name, and real
/// readings for the profile being edited.
#[component]
fn ProfileHeader(model: Model) -> Element {
    let width = State::new(0.0f32);
    let readings = Derived::new(move || width.get() >= px(856.0));
    let defaults = Derived::new(move || model.editor.get() == EditTarget::Defaults);
    let director = Derived::new(move || match model.editor.get() {
        EditTarget::Config(id) => Some(id),
        _ => None,
    });
    let kind = Derived::new(move || {
        String::from(match model.editor.get() {
            EditTarget::Defaults => "Project Defaults",
            EditTarget::Config(_) => "Agent Profile",
            EditTarget::New => "New Agent",
        })
    });
    let title = Derived::new(move || {
        let name = model.editor_name.get();
        if defaults.get() {
            model
                .snapshot
                .get()
                .projects
                .iter()
                .find(|p| p.id == model.project.get())
                .map(|p| p.name.clone())
                .unwrap_or_default()
        } else if name.trim().is_empty() {
            kind.get()
        } else {
            name
        }
    });
    let capacity = Derived::new(move || {
        director
            .get()
            .map(|id| crate::sidebar::director_capacity(&model.snapshot.get(), &id))
            .unwrap_or_default()
    });
    let agent_configs = Derived::new(move || {
        let count = model
            .snapshot
            .get()
            .agent_configs
            .iter()
            .filter(|d| d.project_id == model.project.get())
            .count();
        format!("{count:02}")
    });
    let validity = Derived::new(move || {
        model
            .editor_profile
            .get()
            .validate()
            .err()
            .unwrap_or_else(|| {
                "Local profile field validation passes. The server rechecks scope and policy when saving."
                    .to_string()
            })
    });
    view! {
        row height:{px(74.0)}px shrink:0 label:"Profile header"
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
            @pointer:{move |event, ctx| crate::window_chrome::drag_header(model, event, ctx)}
            @layout:{move |rect:Rect| width.set(rect.size.width)} {
            NavigationButton model:(model)
            stack width:{px(if width.get() < px(600.0) {36.0} else {74.0})}px shrink:0 align:center
                justify:center fill:ground.selected {
                HarnessGlyph
                    harness:(relay_ui::Live::from(Derived::new(move || crate::adapt::harness(model.editor_profile.get().harness))))
                    size:18.0
            }
            col width:1fr min-width:0px justify:center gap:{px(4.0)}px
                pad:(horizontal:{px(if width.get() < px(600.0) {8.0} else {18.0})}px vertical:0px) {
                Label text:(kind) role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
                if model.editor.get() == EditTarget::New {
                    row height:min-content min-width:0px gap:{px(8.0)}px align:center {
                        SerifTitle text:"New" size:(SerifSize::Large)
                        AgentProfileSelector model:(model)
                    }
                } else if defaults.get() {
                    row height:min-content min-width:0px clip {
                        SerifTitle text:(title) size:(SerifSize::Large)
                    }
                } else {
                    input width:fill height:{px(32.0)}px pad:0px radius:0px
                        fill:(Color::TRANSPARENT)
                        stroke:(width:{px(1.0)} color:(Color::TRANSPARENT)) font-family:serif
                        font-size:{px(26.0)}px font-color:ground.ink label:"Agent name"
                        placeholder:"Agent name"
                        focused { stroke:(width:{px(1.0)} color:ground.ink edges:bottom) }
                        model.editor_name
                }
            }
            if defaults.get() && readings.get() {
                ReadoutCell key:"Directors" value:(agent_configs)
            }
            if director.get().is_some() && readings.get() {
                ReadoutCell key:"Overrides"
                    value:(Derived::new(move || format!("{:02} / {:02}", overridden_fields(&model.editor_overrides.get()).len(), OVERRIDE_FIELDS)))
                ReadoutCell key:"Workers"
                    value:(Derived::new(move || format!("{} / {} active", capacity.get().1, capacity.get().2)))
            }
            if readings.get() {
                ReadoutCell key:"Profile fields"
                    value:(Derived::new(move || if model.editor_profile.get().validate().is_ok() {"Valid".to_string()} else {"Invalid".to_string()}))
                    tone:(relay_ui::Live::from(Derived::new(move || if model.editor_profile.get().validate().is_ok() {Tone::Signal(Signal::FullSteam)} else {Tone::Signal(Signal::Failed)}))) {
                    Tooltip text:(validity) {
                        Label text:"?" role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
            }
            CommandPaletteButton model:(model)
                compact:(Derived::new(move || width.get() < px(600.0)))
            WindowControlsSpace
        }
    }
}

/// Where the edited values come from: project defaults, then the director's
/// overrides, applied when the next worker turn starts. One ruled strip of
/// three cells.
#[component]
fn Inheritance(model: Model) -> Element {
    let width = State::new(0.0f32);
    let horizontal = Derived::new(move || width.get() >= px(780.0));
    let defaults = Derived::new(move || model.editor.get() == EditTarget::Defaults);
    let base = Derived::new(move || {
        let profile = if defaults.get() {
            model.editor_profile.get()
        } else {
            model.editor_base.get()
        };
        profile_summary(&profile, &model.snapshot.get())
    });
    let director = Derived::new(move || {
        if defaults.get() {
            return "Each agent may override fields".to_string();
        }
        let fields = overridden_fields(&model.editor_overrides.get());
        if fields.is_empty() {
            "No overrides".to_string()
        } else {
            fields.join(" · ")
        }
    });
    view! {
        grid height:min-content gap:0px shrink:0 label:"Inheritance"
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
            @layout:{move |rect:Rect| width.set(rect.size.width)}
            cols:{if horizontal.get() {GridTracks::new([GridTrack::fr(1.0),GridTrack::fr(1.0),GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0)])}} {
            ChainStep
                step:(Derived::new(move || if model.editor.get() == EditTarget::New { format!("01 · {} profile", model.agent_template_name()) } else { "01 · Project defaults".into() }))
                value:(base) current:(defaults) last:(Derived::new(|| false))
                horizontal:(horizontal)
            ChainStep step:"02 · Agent overrides" value:(director)
                current:(Derived::new(move || !defaults.get())) last:(Derived::new(|| false))
                horizontal:(horizontal)
            ChainStep step:"03 · Next agent turn"
                value:(Derived::new(|| "Applies when the next agent turn starts".to_string()))
                current:(Derived::new(|| false)) last:(Derived::new(|| true))
                horizontal:(horizontal)
        }
    }
}

#[component]
fn ChainStep(
    step: relay_ui::UiText,
    value: Derived<String>,
    current: Derived<bool>,
    last: Derived<bool>,
    horizontal: Derived<bool>,
) -> Element {
    view! {
        col width:1fr min-width:0px height:{px(54.0)}px shrink:0 justify:center gap:{px(3.0)}px
            pad:(horizontal:{px(18.0)}px vertical:0px)
            fill:{color(if current.get() {ground.selected} else {ground.panel})}
            stroke:(width:{px(if last.get() {0.0} else {1.0})} color:ground.hairline edges:if horizontal.get() {StrokeEdges::RIGHT} else {StrokeEdges::BOTTOM}) {
            Label text:(step) role:(TextRole::Caption) caps:true weight:500 tone:(Tone::Muted)
            row height:min-content min-width:0px {
                Ellipsis text:(value) role:(TextRole::Body)
            }
        }
    }
}

/// Where a profile field's value comes from, and Inherit when this director
/// overrides it, as head cells of its module.
fn field_origin(
    model: Model,
    title: &'static str,
    field: &'static str,
    extra: Option<Element>,
) -> Element {
    let overridden = Derived::new(move || model.origin(field) == "Agent override");
    let detail = Derived::new(move || {
        format!(
            "{title}: {}. Overrides replace this whole field.",
            model.origin(field)
        )
    });
    let end = view! {
        row width:max-content shrink:0 gap:0px align:center {
            row width:max-content height:min-content pad:(horizontal:{px(12.0)}px vertical:0px) {
                Tooltip text:(detail) {
                    OriginWord overridden:(overridden)
                }
            }
            if overridden.get() {
                row width:max-content shrink:0 {
                    CellButton label:(format!("Inherit {field}"))
                        on-press:(press(move || model.inherit(field))) rule:(Rule::Left) {
                        Label text:"Inherit" role:(TextRole::Label) weight:500
                    }
                }
            }
        }
    };
    if let Some(extra) = extra {
        let cell = view! {
            row width:max-content shrink:0
                stroke:(width:{px(1.0)} color:ground.hairline edges:left) {}
        };
        cell.adopt(&extra);
        end.adopt(&cell);
    }
    end
}

/// A profile field's module: caps head, origin and Inherit at its right.
fn field_module(
    model: Model,
    title: &'static str,
    field: &'static str,
    extra: Option<Element>,
    body: Element,
) -> Element {
    let end = field_origin(model, title, field, extra);
    let module = view! {
        Module title:(title.to_string())
            icon:(Some(match field { "harness" => relay_ui::icons::harness.codex, "execution" => relay_ui::icons::call.approval, "scope" => relay_ui::icons::section.scope, "max_workers" | "worker_models" => relay_ui::icons::call.spawn, _ => relay_ui::icons::section.permissions }))
            head-end:(Some(end)) head-label:(Some(format!("{title} field header")))
    };
    module.adopt(&body);
    module
}

/// The core permission of a relay-ui choice and back.
fn core_permission(choice: relay_ui::surfaces::Permission) -> Permission {
    match choice {
        relay_ui::surfaces::Permission::Allow => Permission::Allow,
        relay_ui::surfaces::Permission::Ask => Permission::Ask,
        relay_ui::surfaces::Permission::Deny => Permission::Deny,
    }
}

fn ui_permission(permission: Permission) -> relay_ui::surfaces::Permission {
    match permission {
        Permission::Allow => relay_ui::surfaces::Permission::Allow,
        Permission::Ask => relay_ui::surfaces::Permission::Ask,
        Permission::Deny => relay_ui::surfaces::Permission::Deny,
    }
}

/// The action matrix module: who owns each action, whether completion needs
/// it, and its permission, on relay-ui's permission matrix.
#[component]
pub(crate) fn ActionMatrix(model: Model) -> Element {
    let origins = view! {
        row height:min-content gap:0px align:center pad:(left:{px(18.0)}px right:0px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            row width:1fr min-width:0px height:min-content {
                Label text:"Responsibility · completion · permission" role:(TextRole::Caption)
                    tone:(Tone::Muted)
            }
        }
    };
    for (title, field, short) in [
        ("Responsibilities", "responsibilities", "Resp."),
        ("Completion", "completion", "Done"),
        ("Permissions", "permissions", "Permission"),
    ] {
        origins.adopt(&field_cell(model, title, field, short));
    }
    let entries = Derived::new(move || {
        let profile = model.editor_profile.get();
        Task::ALL
            .iter()
            .map(|task| PermissionRow {
                action: task.label().to_string(),
                choice: ui_permission(
                    profile
                        .permissions
                        .get(task)
                        .copied()
                        .unwrap_or(Permission::Deny),
                ),
                pending: false,
                name: format!("{} permission", task.label()),
                flags: vec![
                    profile.responsibilities.contains(task),
                    profile.completion.contains(task),
                ],
            })
            .collect::<Vec<_>>()
    });
    let change: OnPermission = std::rc::Rc::new(move |row, choice| {
        let task = Task::ALL[row];
        model.modify_profile("permissions", |p| {
            p.permissions.insert(task, core_permission(choice));
        });
    });
    let flag: OnFlag = std::rc::Rc::new(move |row, column, on| {
        let task = Task::ALL[row];
        let field = if column == 0 {
            "responsibilities"
        } else {
            "completion"
        };
        model.modify_profile(field, |p| {
            let steps = if column == 0 {
                &mut p.responsibilities
            } else {
                &mut p.completion
            };
            steps.retain(|t| t != &task);
            if on {
                steps.push(task);
            }
        });
    });
    let columns = vec![
        FlagColumn {
            head: "Resp.".into(),
            name: "responsibility".into(),
        },
        FlagColumn {
            head: "Done".into(),
            name: "required for completion".into(),
        },
    ];
    let choices = PERMISSIONS
        .iter()
        .map(|p| (ui_permission(*p), p.label().to_string()))
        .collect::<Vec<_>>();
    let matrix = view! {
        PermissionMatrix entries:(relay_ui::Live::from(entries)) on-change:(change)
            flag-columns:(columns) on-flag:(Some(flag)) choices:(choices) label:"Actions"
    };
    view! {
        col height:min-content min-width:0px label:"Action matrix" {
            Module title:"Actions" icon:(Some(relay_ui::icons::section.permissions)) {
                { parent.adopt(&origins); parent.adopt(&matrix); }
                row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                    Label
                        text:"Implement: Deny blocks worker turns; Ask requires approval each turn. Other actions are workflow settings."
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        }
    }
}

/// The origin of one whole matrix field, with Inherit when overridden.
fn field_cell(
    model: Model,
    title: &'static str,
    field: &'static str,
    short: &'static str,
) -> Element {
    let overridden = Derived::new(move || model.origin(field) == "Agent override");
    let detail = Derived::new(move || {
        format!(
            "{title}: {}. Overrides replace this whole field.",
            model.origin(field)
        )
    });
    view! {
        row width:max-content height:{px(40.0)}px shrink:0 align:center gap:0px
            stroke:(width:{px(1.0)} color:ground.hairline edges:left)
            label:(format!("{title} origin")) {
            col width:max-content height:min-content gap:{px(2.0)}px
                pad:(horizontal:{px(12.0)}px vertical:0px) {
                Label text:(short) role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
                Tooltip text:(detail) {
                    OriginWord overridden:(overridden)
                }
            }
            if overridden.get() {
                row width:{px(32.0)}px shrink:0 {
                    CellButton label:(format!("Inherit {field}"))
                        on-press:(press(move || model.inherit(field))) rule:(Rule::Left)
                        horizontal-inset:(0.0) {
                        Label text:"×" role:(TextRole::Label)
                    }
                }
            }
        }
    }
}

/// The profile as TOML: export, the effective profile, and import.
#[component]
fn ProfileToml(model: Model) -> Element {
    let toggle = view! {
        row width:max-content shrink:0 {
            CellButton label:"Toggle profile TOML" rule:(Rule::Left)
                on-press:(press(move || {
                    if !model.advanced.get_untracked() && model.toml.get_untracked().is_empty() {
                        model.toml.set(model.editor_profile.get_untracked().to_toml());
                    }
                    model.advanced.update(|open| *open = !*open);
                })) {
                Label
                    text:(Derived::new(move || if model.advanced.get() {"Hide".to_string()} else {"Edit".to_string()}))
                    role:(TextRole::Label) weight:500
            }
        }
    };
    view! {
        Module title:"Effective Profile · TOML" icon:(Some(relay_ui::icons::call.read))
            head-end:(Some(toggle)) {
            if model.advanced.get() {
                col height:min-content gap:0px {
                    row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px)
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        Label
                            text:"Project defaults use a complete profile. Agents use overrides; omitted fields inherit. Copy this text to export."
                            role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                    row height:min-content
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        TextInput value:(model.toml) label:"Profile TOML" multiline:true
                            height:(Some(240.0)) framed:false
                    }
                    grid height:min-content gap:0px
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(150.0).into(), GridTrack::fr(1.0)))} {
                        row height:{px(36.0)}px {
                            CellButton label:"Export / edit TOML"
                                on-press:(press(move || model.export_toml())) {
                                Label text:"Export / edit TOML" role:(TextRole::Label)
                            }
                        }
                        row height:{px(36.0)}px {
                            CellButton label:"Show effective profile" rule:(Rule::Left)
                                on-press:(press(move || model.toml.set(model.editor_profile.get_untracked().to_toml()))) {
                                Label text:"Show effective profile" role:(TextRole::Label)
                            }
                        }
                        row height:{px(36.0)}px {
                            CellButton label:"Import TOML into draft" rule:(Rule::Left)
                                on-press:(press(move || model.import_toml())) {
                                Label text:"Import TOML into draft" role:(TextRole::Label)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The field modules: harness, execution approval, scope and the worker
/// limit.
#[component]
pub(crate) fn ProfileControls(model: Model) -> Element {
    let approval_index = Derived::new(move || {
        let mode = model.editor_profile.get().execution.approval;
        ApprovalMode::ALL
            .iter()
            .position(|m| *m == mode)
            .unwrap_or(0)
    });
    let choose_approval: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot: usize| {
        model.modify_profile("execution", |p| {
            p.execution.approval = ApprovalMode::ALL[slot]
        });
    });
    let capacity = Derived::new(move || match model.editor.get() {
        EditTarget::Config(id) => crate::sidebar::director_capacity(&model.snapshot.get(), &id),
        _ => (0, 0, 0),
    });
    let whole_project =
        Derived::new(move || model.editor_profile.get().scope == AgentScope::Project);
    let harness = field_module(
        model,
        "Agent Harness",
        "harness",
        None,
        view! {
            col height:min-content {
                HarnessPicker model:(model)
            }
        },
    );
    let model_selection = field_module(
        model,
        "Agent Model",
        "model",
        None,
        view! {
            ModelPicker model:(model)
        },
    );
    let approval = field_module(
        model,
        "Execution Approval",
        "execution",
        None,
        view! {
            col height:min-content gap:0px {
                row height:{px(48.0)}px
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    Segments name:("Execution Approval".to_string())
                        options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                        icons:(crate::controls::approval_icons())
                        tones:(crate::controls::approval_tones())
                        selected:(relay_ui::Live::from(approval_index)) on-select:(choose_approval)
                }
                row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                    Label
                        text:"Execution mode configures the harness. Action permissions are separate workflow settings."
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        },
    );
    let network_index =
        Derived::new(move || usize::from(model.editor_profile.get().network_access));
    let choose_network: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot| {
        model.modify_profile("network_access", |p| p.network_access = slot == 1);
    });
    let network = field_module(
        model,
        "Network Access",
        "network_access",
        None,
        view! {
            col height:min-content gap:0px {
                row height:{px(48.0)}px {
                    Segments name:("Network Access".to_string())
                        options:(vec!["Off".to_string(), "On".to_string()])
                        selected:(relay_ui::Live::from(network_index)) on-select:(choose_network)
                }
                row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                    Label
                        text:"Codex uses this setting with the workspace filesystem sandbox. Unrestricted Access always allows network. Claude Code does not support this setting. Changes apply to the next turn."
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        },
    );
    let whole = view! {
        CellButton label:"Scope: Whole project" selected:(whole_project)
            on-press:(press(move || model.modify_profile("scope", |p| p.scope = AgentScope::Project))) {
            Label text:"Whole project" role:(TextRole::Label) weight:500
        }
    };
    whole.role(mosaic::widgets::Role::Checkbox);
    let semantic = whole.clone();
    let effect = Effect::new(move || {
        semantic.toggled(whole_project.get());
    });
    whole.__hot_on_remove(move || effect.dispose());
    let scope = field_module(
        model,
        "Scope",
        "scope",
        Some(whole),
        view! {
            grid height:min-content gap:0px
                cols:{GridTracks::auto_fit(GridTrack::minmax(px(200.0).into(), GridTrack::fr(1.0)))} {
                for (_, issue) in { model.snapshot.get().issues.into_iter().filter(|i| i.project_id == model.project.get() && model.snapshot.get().canonical_issue_id(&i.id) == i.id).map(|i| (i.id.clone(), i)).collect::<Vec<_>>() } {
                    ScopeChip model:(model) issue:(issue.clone())
                }
            }
        },
    );
    let limit = Derived::new(move || i64::from(model.editor_profile.get().max_workers));
    let set_limit: relay_ui::surfaces::OnNumber = std::rc::Rc::new(move |value| {
        model.modify_profile("max_workers", |p| p.max_workers = value.clamp(0, 64) as u16);
    });
    let workers = field_module(
        model,
        "Concurrent Workers",
        "max_workers",
        None,
        view! {
            row height:{px(40.0)}px gap:0px align:center {
                row width:{px(150.0)}px shrink:0
                    stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                    Stepper value:(relay_ui::Live::from(limit)) on-change:(set_limit) min:0i64
                        max:64i64 label:"Worker limit" decrease-label:"Decrease worker limit"
                        increase-label:"Increase worker limit"
                }
                row width:1fr min-width:0px height:min-content
                    pad:(horizontal:{px(14.0)}px vertical:0px) {
                    Ellipsis
                        text:(Derived::new(move || {
                            let limit = model.editor_profile.get().max_workers;
                            if limit == 0 {
                                "0 pauses delegation · range 0–64".to_string()
                            } else if matches!(model.editor.get(), EditTarget::Config(_)) {
                                format!("{} active · limit {limit} · range 0–64", capacity.get().1)
                            } else {
                                format!("Limit {limit} per agent tree · range 0–64")
                            }
                        }))
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        },
    );
    let worker_model = field_module(
        model,
        "Worker Model",
        "worker_models",
        None,
        view! {
            WorkerModelControls model:(model)
        },
    );
    view! {
        col height:min-content gap:0px {
            { for module in [&harness, &model_selection, &approval, &network, &scope, &workers, &worker_model] { parent.adopt(module); } }
        }
    }
}

const WORKER_MODES: [&str; 2] = ["Delegator Chooses", "Fixed"];

/// Effort caps offered when no other cap is set, lowest first.
const EFFORT_CAPS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

fn effort_title(effort: &str) -> String {
    match effort {
        "xhigh" => "XHigh".into(),
        _ => {
            let mut chars = effort.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

/// The profile catalog of one harness: loading (none), failed, or loaded.
fn profile_catalog(model: Model, harness: Harness) -> Option<Result<HarnessCatalog, String>> {
    let key = format!("profile:{}:{harness:?}", model.project.get());
    model.catalogs.get().get(&key).cloned()
}

fn catalog_note(result: &Option<Result<HarnessCatalog, String>>) -> String {
    match result {
        None => "Loading models…".into(),
        Some(Err(error)) => error.clone(),
        Some(Ok(_)) => "No models available".into(),
    }
}

/// A checkbox cell, selected while `on`.
fn checkbox_cell(
    label: String,
    title: &'static str,
    on: Derived<bool>,
    toggle: impl Fn() + 'static,
) -> Element {
    let cell = view! {
        CellButton label:(label) selected:(on) on-press:(press(toggle)) start:true {
            Label text:(title) role:(TextRole::Label) weight:500
        }
    };
    cell.role(mosaic::widgets::Role::Checkbox);
    let semantic = cell.clone();
    let effect = Effect::new(move || {
        semantic.toggled(on.get());
    });
    cell.__hot_on_remove(move || effect.dispose());
    cell
}

/// The Worker Model module: the mode, then either the fixed harness, model
/// and effort, or the allowed models, effort cap and selection considerations.
#[component]
fn WorkerModelControls(model: Model) -> Element {
    // Model choices span every harness, so both catalogs are requested.
    let requested = State::new(std::collections::BTreeSet::<String>::new());
    Effect::new(move || {
        let project = model.project.get();
        for harness in Harness::ALL {
            let key = format!("profile:{project}:{harness:?}");
            if model.connected.get()
                && !model.catalogs.get().contains_key(&key)
                && !requested.get().contains(&key)
                && let Some(sender) = model.buffer_requests.get()
                && sender
                    .send(crate::buffer_network::Request::ProfileCatalog {
                        project: project.clone(),
                        harness,
                    })
                    .is_ok()
            {
                requested.update(|keys| {
                    keys.insert(key);
                });
            }
        }
    });
    let fixed = Derived::new(move || model.editor_profile.get().worker_models.is_fixed());
    let mode_index = Derived::new(move || usize::from(fixed.get()));
    let choose_mode: relay_ui::surfaces::OnIndex = std::rc::Rc::new(move |slot| {
        model.modify_profile("worker_models", |p| {
            if (slot == 1) != p.worker_models.is_fixed() {
                p.worker_models = if slot == 1 {
                    WorkerModelPolicy::Fixed {
                        harness: p.harness,
                        model: p.model.clone(),
                        effort: None,
                    }
                } else {
                    WorkerModelPolicy::default()
                };
            }
        });
    });
    view! {
        col height:min-content gap:0px {
            row height:{px(48.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Segments name:("Worker Model Mode".to_string())
                    options:(WORKER_MODES.iter().map(|m| m.to_string()).collect::<Vec<_>>())
                    selected:(relay_ui::Live::from(mode_index)) on-select:(choose_mode)
            }
            if fixed.get() {
                FixedWorkerModel model:(model)
            } else {
                ChosenWorkerModels model:(model)
            }
        }
    }
}

/// Fixed mode: the one harness, model and effort every worker uses.
#[component]
fn FixedWorkerModel(model: Model) -> Element {
    let fixed = Derived::new(move || match model.editor_profile.get().worker_models {
        WorkerModelPolicy::Fixed {
            harness,
            model,
            effort,
        } => Some((harness, model, effort)),
        _ => None,
    });
    let harness_index =
        Derived::new(move || usize::from(fixed.get().is_some_and(|f| f.0 == Harness::ClaudeCode)));
    let choose_harness: relay_ui::surfaces::OnIndex = std::rc::Rc::new(move |slot| {
        let chosen = Harness::ALL[slot];
        model.modify_profile("worker_models", |p| {
            if let WorkerModelPolicy::Fixed {
                harness,
                model,
                effort,
            } = &mut p.worker_models
                && *harness != chosen
            {
                *harness = chosen;
                model.clear();
                *effort = None;
            }
        });
    });
    let open = State::new(false);
    let content: PopoverContent = std::rc::Rc::new(move || {
        let host = view! {
            col width:{px(320.0)}px height:min-content {}
        };
        host.switch(
            move || fixed.get().and_then(|f| profile_catalog(model, f.0)),
            move |parent, result| {
                let models = result
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|c| c.models.clone())
                    .unwrap_or_default();
                if models.is_empty() {
                    let text = catalog_note(result);
                    parent.adopt(&view! {
                        row height:min-content pad:{px(14.0)}px {
                            Label text:(text.clone()) role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    });
                }
                for choice in models {
                    let id = choice.id.clone();
                    let selected_id = id.clone();
                    let label = format!("Use worker model {id}");
                    let selected: relay_ui::Live<bool> = Derived::new(move || {
                        fixed.get().is_some_and(|f| f.1 == selected_id)
                    })
                    .into();
                    parent.adopt(&view! {
                        row height:{px(40.0)}px {
                            CellButton label:(label.clone()) start:true selected:(selected)
                                on-press:(press(move || {
                                    model.modify_profile("worker_models", |p| {
                                        if let WorkerModelPolicy::Fixed { model, effort, .. } = &mut p.worker_models {
                                            *model = id.clone();
                                            *effort = None;
                                        }
                                    });
                                    open.set(false);
                                })) {
                                Ellipsis text:(choice.name.clone()) role:(TextRole::Label)
                            }
                        }
                    });
                }
            },
        );
        host
    });
    let name = Derived::new(move || {
        let Some((harness, id, _)) = fixed.get() else {
            return String::new();
        };
        profile_catalog(model, harness)
            .and_then(Result::ok)
            .and_then(|c| c.models.into_iter().find(|m| m.id == id))
            .map(|m| m.name)
            .unwrap_or_else(|| {
                if id.is_empty() {
                    "Choose Worker Model".into()
                } else {
                    id
                }
            })
    });
    let effort = view! {
        row height:{px(48.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:top) {}
    };
    effort.switch(
        move || {
            let Some((harness, id, effort)) = fixed.get() else {
                return vec![];
            };
            let mut efforts = profile_catalog(model, harness)
                .and_then(Result::ok)
                .and_then(|c| c.models.into_iter().find(|m| m.id == id))
                .map(|m| m.efforts)
                .unwrap_or_default();
            if let Some(effort) = effort
                && !efforts.contains(&effort)
            {
                efforts.push(effort);
            }
            efforts
        },
        move |parent, efforts| {
            let options = std::iter::once("Model Default".to_string())
                .chain(efforts.iter().map(|e| effort_title(e)))
                .collect::<Vec<_>>();
            let listed = efforts.clone();
            let selected = Derived::new(move || match model.editor_profile.get().worker_models {
                WorkerModelPolicy::Fixed {
                    effort: Some(effort),
                    ..
                } => listed
                    .iter()
                    .position(|e| *e == effort)
                    .map_or(0, |i| i + 1),
                _ => 0,
            });
            let choices = efforts.clone();
            let choose: relay_ui::surfaces::OnIndex = std::rc::Rc::new(move |slot| {
                let chosen = slot.checked_sub(1).map(|i| choices[i].clone());
                model.modify_profile("worker_models", |p| {
                    if let WorkerModelPolicy::Fixed { effort, .. } = &mut p.worker_models {
                        *effort = chosen;
                    }
                });
            });
            parent.adopt(&view! {
                Segments name:("Fixed Worker Effort".to_string()) options:(options)
                    selected:(relay_ui::Live::from(selected)) on-select:(choose)
            });
        },
    );
    view! {
        col height:min-content gap:0px {
            row height:{px(48.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Segments name:("Fixed Worker Harness".to_string())
                    options:(Harness::ALL.iter().map(|h| h.label().to_string()).collect::<Vec<_>>())
                    selected:(relay_ui::Live::from(harness_index)) on-select:(choose_harness)
            }
            stack height:{px(44.0)}px {
                CellButton label:"Select fixed worker model" start:true
                    on-press:(press(move || open.set(!open.get_untracked()))) {
                    Label text:(name) role:(TextRole::Label)
                    icon size:{px(12.0)}px tree-chevron-down
                }
                Popover open:(open) label:"Worker Models" content:(content)
            }
            { parent.adopt(&effort); }
            row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                Label
                    text:"Every worker uses this harness, model and effort. Relay rejects spawn requests for anything else."
                    role:(TextRole::Caption) tone:(Tone::Muted)
            }
        }
    }
}

/// Delegator Chooses mode: the allowed models across harnesses, the effort
/// cap and the selection considerations.
#[component]
fn ChosenWorkerModels(model: Model) -> Element {
    let policy = Derived::new(move || model.editor_profile.get().worker_models);
    let allowed = Derived::new(move || match policy.get() {
        WorkerModelPolicy::DelegatorChooses { allowed_models, .. } => allowed_models,
        _ => vec![],
    });
    // Every catalog model, then allowed models no loaded catalog lists.
    let choices = Derived::new(move || {
        let mut choices: Vec<(ModelChoice, String)> = vec![];
        for harness in Harness::ALL {
            if let Some(Ok(catalog)) = profile_catalog(model, harness) {
                for entry in catalog.models {
                    choices.push((
                        ModelChoice {
                            harness,
                            model: entry.id,
                        },
                        entry.name,
                    ));
                }
            }
        }
        for choice in allowed.get() {
            if !choices.iter().any(|(c, _)| *c == choice) {
                let name = choice.model.clone();
                choices.push((choice, name));
            }
        }
        choices
    });
    let summary = Derived::new(move || {
        let count = allowed.get().len();
        if count == 0 {
            "No models selected: any model of a connected harness is allowed".to_string()
        } else {
            format!("{count} allowed model{}", if count == 1 { "" } else { "s" })
        }
    });
    let loading = Derived::new(move || {
        let missing = Harness::ALL
            .iter()
            .filter(|h| !matches!(profile_catalog(model, **h), Some(Ok(_))))
            .map(|h| {
                format!(
                    "{}: {}",
                    h.label(),
                    catalog_note(&profile_catalog(model, *h))
                )
            })
            .collect::<Vec<_>>();
        missing.join(" · ")
    });
    let cap = view! {
        row height:{px(48.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:(top bottom)) {}
    };
    cap.switch(
        move || match policy.get() {
            WorkerModelPolicy::DelegatorChooses {
                max_effort: Some(cap),
                ..
            } if !EFFORT_CAPS.contains(&cap.as_str()) => Some(cap),
            _ => None,
        },
        move |parent, extra| {
            let mut levels: Vec<String> = EFFORT_CAPS.iter().map(|e| e.to_string()).collect();
            if let Some(extra) = extra {
                levels.push(extra.clone());
                levels.sort_by_key(|e| effort_rank(e));
            }
            let options = std::iter::once("No Cap".to_string())
                .chain(levels.iter().map(|e| effort_title(e)))
                .collect::<Vec<_>>();
            let listed = levels.clone();
            let selected = Derived::new(move || match policy.get() {
                WorkerModelPolicy::DelegatorChooses {
                    max_effort: Some(cap),
                    ..
                } => listed.iter().position(|e| *e == cap).map_or(0, |i| i + 1),
                _ => 0,
            });
            let choose: relay_ui::surfaces::OnIndex = std::rc::Rc::new(move |slot| {
                let chosen = slot.checked_sub(1).map(|i| levels[i].clone());
                model.modify_profile("worker_models", |p| {
                    if let WorkerModelPolicy::DelegatorChooses { max_effort, .. } =
                        &mut p.worker_models
                    {
                        *max_effort = chosen;
                    }
                });
            });
            parent.adopt(&view! {
                Segments name:("Maximum Worker Effort".to_string()) options:(options)
                    selected:(relay_ui::Live::from(selected)) on-select:(choose)
            });
        },
    );
    let consideration = move |usage: bool| {
        Derived::new(move || match policy.get() {
            WorkerModelPolicy::DelegatorChooses {
                model_capability,
                available_usage,
                ..
            } => {
                if usage {
                    available_usage
                } else {
                    model_capability
                }
            }
            _ => false,
        })
    };
    let toggle = move |usage: bool| {
        move || {
            model.modify_profile("worker_models", |p| {
                if let WorkerModelPolicy::DelegatorChooses {
                    model_capability,
                    available_usage,
                    ..
                } = &mut p.worker_models
                {
                    let flag = if usage {
                        available_usage
                    } else {
                        model_capability
                    };
                    *flag = !*flag;
                }
            });
        }
    };
    let capability = checkbox_cell(
        "Consider model capability".into(),
        "Model Capability",
        consideration(false),
        toggle(false),
    );
    let usage = checkbox_cell(
        "Consider available usage".into(),
        "Available Usage",
        consideration(true),
        toggle(true),
    );
    let usage_cell = view! {
        row height:{px(40.0)}px min-width:0px
            stroke:(width:{px(1.0)} color:ground.hairline edges:left) {}
    };
    usage_cell.adopt(&usage);
    view! {
        col height:min-content gap:0px {
            row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Ellipsis text:(summary) role:(TextRole::Caption) tone:(Tone::Muted)
            }
            grid height:min-content gap:0px
                cols:{GridTracks::auto_fit(GridTrack::minmax(px(200.0).into(), GridTrack::fr(1.0)))} {
                for (_, entry) in { choices.get().into_iter().map(|(c, name)| (format!("{:?}:{}", c.harness, c.model), (c, name))).collect::<Vec<_>>() } {
                    AllowedModelChip model:(model) choice:(entry.0.clone()) name:(entry.1.clone())
                }
            }
            if !loading.get().is_empty() {
                row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px) {
                    Ellipsis text:(loading) role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
            { parent.adopt(&cap); }
            grid height:min-content gap:0px
                cols:{GridTracks::new([GridTrack::fr(1.0), GridTrack::fr(1.0)])} {
                row height:{px(40.0)}px min-width:0px {
                    { parent.adopt(&capability); }
                }
                { parent.adopt(&usage_cell); }
            }
            row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                Label
                    text:"Model Capability matches model strength and effort to task difficulty. Available Usage prefers harnesses and models with more remaining usage. Relay rejects models outside the list and efforts above the cap."
                    role:(TextRole::Caption) tone:(Tone::Muted)
            }
        }
    }
}

/// One harness model the delegator may choose: a checkbox cell with the
/// harness and model name, selected while the model is allowed.
#[component]
fn AllowedModelChip(model: Model, choice: ModelChoice, name: String) -> Element {
    let allowed_choice = choice.clone();
    let on = Derived::new(move || {
        matches!(
            model.editor_profile.get().worker_models,
            WorkerModelPolicy::DelegatorChooses { ref allowed_models, .. } if allowed_models.contains(&allowed_choice)
        )
    });
    let toggled = choice.clone();
    let chip = view! {
        CellButton label:(format!("Allow worker model {}", choice.label())) selected:(on) start:true
            on-press:(press(move || {
                model.modify_profile("worker_models", |p| {
                    if let WorkerModelPolicy::DelegatorChooses { allowed_models, .. } = &mut p.worker_models {
                        match allowed_models.iter().position(|c| *c == toggled) {
                            Some(index) => {
                                allowed_models.remove(index);
                            }
                            None => allowed_models.push(toggled.clone()),
                        }
                    }
                });
            })) {
            Label text:(choice.harness.label()) role:(TextRole::Caption) weight:600
            row width:1fr min-width:0px height:min-content {
                Ellipsis text:(name.clone()) role:(TextRole::Label)
            }
        }
    };
    chip.role(mosaic::widgets::Role::Checkbox);
    let semantic = chip.clone();
    let effect = Effect::new(move || {
        semantic.toggled(on.get());
    });
    chip.__hot_on_remove(move || effect.dispose());
    let cell = view! {
        row height:{px(34.0)}px min-width:0px
            stroke:(width:{px(1.0)} color:ground.hairline edges:(right bottom)) {}
    };
    cell.adopt(&chip);
    cell
}

#[component]
fn ModelPicker(model: Model) -> Element {
    let requested = State::new(std::collections::BTreeSet::<String>::new());
    Effect::new(move || {
        let project = model.project.get();
        let harness = model.editor_profile.get().harness;
        let key = format!("profile:{project}:{harness:?}");
        if model.connected.get()
            && !model.catalogs.get().contains_key(&key)
            && !requested.get().contains(&key)
            && let Some(sender) = model.buffer_requests.get()
            && sender
                .send(crate::buffer_network::Request::ProfileCatalog { project, harness })
                .is_ok()
        {
            requested.update(|keys| {
                keys.insert(key);
            });
        }
    });
    let open = State::new(false);
    let content: PopoverContent = std::rc::Rc::new(move || {
        let host = view! {
            col width:{px(320.0)}px height:min-content {}
        };
        host.switch(move || {
            let profile = model.editor_profile.get();
            let key = format!("profile:{}:{:?}", model.project.get(), profile.harness);
            model.catalogs.get().get(&key).cloned()
        }, move |parent, result| {
            if let Some(Ok(catalog)) = result.as_ref().filter(|result| result.as_ref().is_ok_and(|catalog| !catalog.models.is_empty())) {
                for choice in &catalog.models {
                    let id = choice.id.clone();
                    let selected_id = id.clone();
                    let title = choice.name.clone();
                    let label = format!("Use profile model {id}");
                    let selected: relay_ui::Live<bool> = Derived::new(move || model.editor_profile.get().model == selected_id).into();
                    parent.adopt(&view! {
                        row height:{px(40.0)}px {
                            CellButton label:(label.clone()) start:true selected:(selected)
                                on-press:(press(move || { model.modify_profile("model", |p| p.model = id.clone()); open.set(false); })) {
                                Ellipsis text:(title.clone()) role:(TextRole::Label)
                            }
                        }
                    });
                }
            } else {
                let text = match result { Some(Err(error)) => error.clone(), Some(Ok(_)) => "No models available".into(), None => "Loading models…".into() };
                parent.adopt(&view! {
                    col height:min-content {
                        row height:min-content pad:{px(14.0)}px {
                            Label text:(text.clone()) role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                        row height:{px(40.0)}px {
                            CellButton label:"Retry profile models"
                                on-press:(press(move || {
                                let harness = model.editor_profile.get_untracked().harness;
                                let project = model.project.get_untracked();
                                if let Some(sender) = model.buffer_requests.get_untracked() { let _ = sender.send(crate::buffer_network::Request::ProfileCatalog { project, harness }); }
                            })) {
                                Label text:"Retry" role:(TextRole::Label)
                            }
                        }
                    }
                });
            }
        });
        host
    });
    let name = Derived::new(move || {
        let profile = model.editor_profile.get();
        let key = format!("profile:{}:{:?}", model.project.get(), profile.harness);
        model
            .catalogs
            .get()
            .get(&key)
            .and_then(|c| c.as_ref().ok())
            .and_then(|c| c.models.iter().find(|m| m.id == profile.model))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| {
                if profile.model.is_empty() {
                    "Choose Model".into()
                } else {
                    profile.model
                }
            })
    });
    view! {
        stack height:{px(44.0)}px {
            CellButton label:"Select profile model" start:true
                on-press:(press(move || open.set(!open.get_untracked()))) {
                Label text:(name) role:(TextRole::Label)
                icon size:{px(12.0)}px tree-chevron-down
            }
            Popover open:(open) label:"Profile Models" content:(content)
        }
    }
}

/// The harness choice on relay-ui's harness cards, each with the connected
/// server's last availability check.
#[component]
fn HarnessPicker(model: Model) -> Element {
    let state_of = move |harness: Harness| {
        if !model.connected.get() {
            return "disconnected".to_string();
        }
        if !model.harness_error.get().is_empty() {
            return "stale".to_string();
        }
        model
            .harnesses
            .get()
            .into_iter()
            .find(|s| s.harness == harness)
            .map(|s| s.state)
            .unwrap_or_default()
    };
    let caption = |state: &str| {
        match state {
            "ready" => "Installed · signed in",
            "signed_out" => "Installed · sign in",
            "missing" => "Not installed",
            "incompatible" => "Incompatible version",
            "unknown" | "error" | "failed" => "Check failed",
            "disconnected" => "Disconnected",
            "stale" => "Refresh failed",
            "" => "Checking",
            _ => "Check unavailable",
        }
        .to_string()
    };
    let selected = Derived::new(move || {
        usize::from(model.editor_profile.get().harness == Harness::ClaudeCode)
    });
    let choose: relay_ui::surfaces::OnIndex = std::rc::Rc::new(move |slot| {
        model.modify_profile("harness", |p| {
            p.harness = if slot == 0 {
                Harness::Codex
            } else {
                Harness::ClaudeCode
            }
        });
    });
    let host = view! {
        col height:min-content label:"Harness choices" {}
    };
    host.switch(
        move || [Harness::Codex, Harness::ClaudeCode].map(|harness| caption(&state_of(harness))),
        move |parent, captions| {
            let options = [Harness::Codex, Harness::ClaudeCode]
                .into_iter()
                .zip(captions.iter())
                .map(|(harness, caption)| {
                    let name = if harness == Harness::Codex {
                        "Codex"
                    } else {
                        "Claude Code"
                    };
                    let state = state_of(harness);
                    HarnessOption {
                        harness: crate::adapt::harness(harness),
                        label: format!("Agent harness: {name}"),
                        description: caption.clone(),
                        status: harness_state(&state).map(|s| (s, caption.clone())),
                    }
                })
                .collect::<Vec<_>>();
            parent.adopt(&view! {
                HarnessChoices options:(options) selected:(relay_ui::Live::from(selected))
                    on-select:(choose.clone()) label:"Agent Harness"
            });
        },
    );
    host
}

/// One issue in the director's scope: a checkbox cell with its number and
/// title, selected while the issue is in scope.
#[component]
fn ScopeChip(model: Model, issue: Issue) -> Element {
    let id = issue.id.clone();
    let issue_id = id.clone();
    let fallback = issue.clone();
    let current = Derived::new(move || {
        model
            .snapshot
            .get()
            .issue(&issue_id)
            .cloned()
            .unwrap_or_else(|_| fallback.clone())
    });
    let number = Derived::new(move || current.get().reference.map(|r| format!("#{}", r.number)));
    let title = Derived::new(move || current.get().title);
    let chip_label =
        Derived::new(move || format!("Scope {}", number.get().unwrap_or_else(|| title.get())));
    let scope_id = id.clone();
    let on = Derived::new(
        move || matches!(&model.editor_profile.get().scope, AgentScope::Issues { issue_ids } if model.scope_contains(issue_ids, &scope_id)),
    );
    let toggle = press(move || {
        model.modify_profile("scope", |p| {
            let mut ids = match &p.scope {
                AgentScope::Issues { issue_ids } => issue_ids.clone(),
                _ => vec![],
            };
            if model.scope_contains(&ids, &id) {
                let snapshot = model.snapshot.get_untracked();
                ids.retain(|x| snapshot.canonical_issue_id(x) != snapshot.canonical_issue_id(&id));
            } else {
                ids.push(id.clone());
            }
            p.scope = if ids.is_empty() {
                AgentScope::Project
            } else {
                AgentScope::Issues { issue_ids: ids }
            };
        });
    });
    let chip = view! {
        CellButton label:(chip_label) on-press:(toggle) selected:(on) start:true {
            if number.get().is_some() {
                Label text:(Derived::new(move || number.get().unwrap_or_default()))
                    role:(TextRole::Caption) weight:600
            }
            row width:1fr min-width:0px height:min-content {
                Ellipsis text:(title) role:(TextRole::Label)
            }
        }
    };
    chip.role(mosaic::widgets::Role::Checkbox);
    let semantic = chip.clone();
    let effect = Effect::new(move || {
        semantic.toggled(on.get());
    });
    chip.__hot_on_remove(move || effect.dispose());
    let cell = view! {
        row height:{px(34.0)}px min-width:0px
            stroke:(width:{px(1.0)} color:ground.hairline edges:(right bottom)) {}
    };
    cell.adopt(&chip);
    cell
}

pub(crate) const PERMISSIONS: [Permission; 3] =
    [Permission::Deny, Permission::Ask, Permission::Allow];

/// Where a field's value comes from: "Override" in amber, because the
/// director's own value is the user's hand (as Ask and the primary action
/// are), or "Default" muted when it is inherited.
#[component]
fn OriginWord(overridden: Derived<bool>) -> Element {
    view! {
        row width:max-content height:min-content {
            if overridden.get() {
                Label text:"Override" role:(TextRole::Caption) weight:500
                    tone:(Tone::Signal(Signal::NeedsYou))
            } else {
                Label text:"Default" role:(TextRole::Caption) tone:(Tone::Muted)
            }
        }
    }
}
