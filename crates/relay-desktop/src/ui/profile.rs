use super::*;

/// The seven profile fields a director can override, each as a whole.
const OVERRIDE_FIELDS: usize = 7;

pub(crate) fn overridden_fields(overrides: &ProfileOverrides) -> Vec<&'static str> {
    [
        ("Harness", overrides.harness.is_some()),
        ("Execution", overrides.execution.is_some()),
        ("Scope", overrides.scope.is_some()),
        ("Responsibilities", overrides.responsibilities.is_some()),
        ("Completion", overrides.completion.is_some()),
        ("Workers", overrides.max_workers.is_some()),
        ("Permissions", overrides.permissions.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, set)| set.then_some(name))
    .collect()
}

/// A one-line summary of a profile: harness, scope and worker limit.
fn profile_summary(profile: &DirectorProfile, snapshot: &Snapshot) -> String {
    let harness = match profile.harness {
        Harness::Codex => "Codex",
        Harness::ClaudeCode => "Claude Code",
    };
    let scope = match &profile.scope {
        DirectorScope::Project => "Whole project".to_string(),
        DirectorScope::Issues { .. } => scope_label(&profile.scope, snapshot)
            .trim_start_matches("Selected issues: ")
            .to_string(),
    };
    let workers = profile.max_workers;
    format!(
        "{harness} · {scope} · {workers} worker{}",
        if workers == 1 { "" } else { "s" }
    )
}

#[component]
pub(crate) fn Profiles(model: Model) -> Element {
    let width = State::new(0.0f32);
    let wide = Derived::new(move || width.get() >= px(1040.0));
    let gutter = Derived::new(move || px(if wide.get() { 40.0 } else { 20.0 }));
    view! {
        col width:1fr gap:0px @layout:{move |rect: Rect| width.set(rect.size.width)} {
            row height:min-content shrink:0
                pad:(left:{gutter.get()}px right:{gutter.get()}px top:{px(16.0)}px) {
                scroll width:max-content {
                    row height:min-content width:max-content gap:0px
                        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                        button #relay.tree-control
                            @click:{ model.open_profile(EditTarget::Defaults); }
                            pad:(horizontal:{px(12.0)}px vertical:0px)
                            fill:{color(if model.editor.get() == EditTarget::Defaults {ink.inverse} else {surface.panel})}
                            label:"Project defaults"
                            hover {
                                fill:{color(if model.editor.get() == EditTarget::Defaults {ink.inverse} else {surface.raised})}
                            }
                            pressed {
                                fill:{color(if model.editor.get() == EditTarget::Defaults {ink.inverse} else {surface.raised})}
                            } {
                            text
                                font-weight:{if model.editor.get() == EditTarget::Defaults {700} else {400}}
                                font-color:{color(if model.editor.get() == EditTarget::Defaults {ink.on_inverse} else {ink.fg})}
                                "Project defaults"
                        }
                        for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                            let id = State::new(director.id.clone());
                            button #relay.tree-control
                                @click:{ model.open_profile(EditTarget::Director(id.get_untracked())); }
                                pad:(horizontal:{px(12.0)}px vertical:0px)
                                stroke:(width:{px(1.0)} color:rule.hair edges:left)
                                fill:{color(if model.editor.get() == EditTarget::Director(id.get()) {ink.inverse} else {surface.panel})}
                                label:{model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| d.name.clone()).unwrap_or_default()}
                                hover {
                                    fill:{color(if model.editor.get() == EditTarget::Director(id.get()) {ink.inverse} else {surface.raised})}
                                }
                                pressed {
                                    fill:{color(if model.editor.get() == EditTarget::Director(id.get()) {ink.inverse} else {surface.raised})}
                                } {
                                text
                                    font-weight:{if model.editor.get() == EditTarget::Director(id.get()) {700} else {400}}
                                    font-color:{color(if model.editor.get() == EditTarget::Director(id.get()) {ink.on_inverse} else {ink.fg})}
                                    {model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| d.name.clone()).unwrap_or_default()}
                            }
                        }
                        button #relay.tree-control @click:{ model.open_profile(EditTarget::New); }
                            pad:(horizontal:{px(12.0)}px vertical:0px)
                            stroke:(width:{px(1.0)} color:rule.hair edges:left)
                            fill:{color(if model.editor.get() == EditTarget::New {ink.inverse} else {surface.panel})}
                            label:"Create director"
                            hover {
                                fill:{color(if model.editor.get() == EditTarget::New {ink.inverse} else {surface.raised})}
                            }
                            pressed {
                                fill:{color(if model.editor.get() == EditTarget::New {ink.inverse} else {surface.raised})}
                            } {
                            text
                                font-weight:{if model.editor.get() == EditTarget::New {700} else {400}}
                                font-color:{color(if model.editor.get() == EditTarget::New {ink.on_inverse} else {ink.fg})}
                                "+ Director"
                        }
                    }
                } as tabs
                { tabs.root().style_dyn(move || Style::stack().width(Dimension::Fill).height(px(42.0)).basis(px(42.0)).shrink(0.0)); }
            }
            scroll {
                col height:min-content gap:{px(24.0)}px
                    pad:(left:{gutter.get()}px right:{gutter.get()}px top:{px(20.0)}px bottom:{px(24.0)}px) {
                    ProfileHeader model:(model)
                    // The name module brings its own spacing, so the defaults
                    // page has no empty gap where it would be.
                    col height:min-content gap:0px {
                        Inheritance model:(model)
                        if model.editor.get() != EditTarget::Defaults {
                            col height:min-content pad:(top:{px(24.0)}px) {
                                col #relay.module {
                                    row #relay.module-head {
                                        row #relay.eyebrow height:min-content {
                                            text text-transform:uppercase letter-spacing:{px(0.6)}px
                                                "Name"
                                        }
                                    }
                                    col height:min-content pad:{px(12.0)}px {
                                        input #relay.field label:"Director name" model.editor_name
                                    }
                                }
                            }
                        }
                    }
                    // One grid for both layouts: the track template changes with the
                    // width, the controls themselves are never rebuilt.
                    grid height:min-content gap:{px(24.0)}px align:start
                        cols:{if wide.get() { GridTracks::new([GridTrack::fr(1.0), GridTrack::fr(1.0)]) } else { GridTracks::new([GridTrack::fr(1.0)]) }} {
                        col height:min-content min-width:0px gap:{px(24.0)}px
                            label:"Action matrix column" {
                            ActionMatrix model:(model)
                            ProfileToml model:(model)
                        }
                        col height:min-content min-width:0px {
                            ProfileControls model:(model)
                        }
                    }
                }
            }
            row height:min-content shrink:0
                pad:(left:{gutter.get()}px right:{gutter.get()}px bottom:{px(16.0)}px) {
                row #relay.module height:{px(50.0)}px align:center gap:{px(12.0)}px
                    pad:(left:{px(14.0)}px right:{px(8.0)}px) label:"Save bar" {
                    row #relay.caption width:1fr min-width:0px height:min-content clip {
                        text text-wrap:none "Explicit overrides survive project default changes"
                    }
                    button #relay.primary @click:{ model.save_profile(); }
                        disabled:{ model.busy.get() || !model.connected.get() } label:"Save profile"
                        "Save profile"
                }
            }
        }
    }
}

/// The profile's 74px identifier header: its mark, kind and name, and real
/// readings for the profile being edited.
#[component]
fn ProfileHeader(model: Model) -> Element {
    let defaults = Derived::new(move || model.editor.get() == EditTarget::Defaults);
    let director = Derived::new(move || match model.editor.get() {
        EditTarget::Director(id) => Some(id),
        _ => None,
    });
    let kind = Derived::new(move || {
        String::from(match model.editor.get() {
            EditTarget::Defaults => "Project defaults",
            EditTarget::Director(_) => "Director profile",
            EditTarget::New => "New director",
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
            .map(|id| crate::labels::director_capacity(&model.snapshot.get(), &id))
            .unwrap_or_default()
    });
    let directors = Derived::new(move || {
        let count = model
            .snapshot
            .get()
            .directors
            .iter()
            .filter(|d| d.project_id == model.project.get())
            .count();
        format!("{count:02}")
    });
    view! {
        row #relay.module height:{px(74.0)}px shrink:0 label:"Profile header" {
            stack width:{px(74.0)}px shrink:0 align:center justify:center fill:ink.inverse {
                DirectorMark size:(22.0) active:(Derived::new(move || capacity.get().1 > 0))
                    inverse:(Derived::new(|| true)) defaults:(defaults)
            }
            col width:1fr min-width:0px justify:center gap:{px(4.0)}px
                pad:(horizontal:{px(16.0)}px vertical:0px) {
                row #relay.eyebrow height:min-content {
                    text text-transform:{TextTransform::Uppercase} letter-spacing:{px(0.6)}px
                        {kind.get()}
                }
                stack #relay.fade-label #relay.title height:min-content font-size:{px(22.0)}px {
                    row #relay.fade-line {
                        text width:max-content shrink:0 text-wrap:none {title.get()}
                    }
                }
            }
            if defaults.get() {
                HeaderCell key:("Directors".to_string()) value:(directors)
            }
            if director.get().is_some() {
                HeaderCell key:("Overrides".to_string())
                    value:(Derived::new(move || format!("{:02} / {:02}", overridden_fields(&model.editor_overrides.get()).len(), OVERRIDE_FIELDS)))
                HeaderCell key:("Workers".to_string())
                    value:(Derived::new(move || format!("{} / {} active", capacity.get().1, capacity.get().2)))
            }
        }
    }
}

/// Where the edited values come from: project defaults, then the director's
/// overrides, applied when the next worker turn starts.
#[component]
fn Inheritance(model: Model) -> Element {
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
            return "Each director may override fields".to_string();
        }
        let fields = overridden_fields(&model.editor_overrides.get());
        if fields.is_empty() {
            "No overrides".to_string()
        } else {
            fields.join(" · ")
        }
    });
    view! {
        row height:{px(54.0)}px shrink:0 align:center label:"Inheritance" {
            ChainStep step:"01 · Project defaults" value:(base) current:(defaults)
            el width:{px(40.0)}px height:{px(1.0)}px shrink:0 fill:rule.line {}
            ChainStep step:"02 · Director overrides" value:(director)
                current:(Derived::new(move || !defaults.get()))
            el width:{px(40.0)}px height:{px(1.0)}px shrink:0 fill:rule.line {}
            ChainStep step:"03 · Next worker turn"
                value:(Derived::new(|| "Applies when the next worker turn starts".to_string()))
                current:(Derived::new(|| false))
        }
    }
}

#[component]
fn ChainStep(step: &'static str, value: Derived<String>, current: Derived<bool>) -> Element {
    view! {
        col width:1fr min-width:0px justify:center gap:{px(3.0)}px
            pad:(horizontal:{px(14.0)}px vertical:0px)
            fill:{color(if current.get() {ink.inverse} else {surface.panel})}
            stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
            row height:min-content font-size:{px(11.0)}px letter-spacing:{px(0.6)}px
                font-color:{color(if current.get() {ink.on_inverse} else {ink.muted})}
                font-weight:{if current.get() {700} else {400}} {
                text text-transform:uppercase (step)
            }
            stack #relay.fade-label height:min-content font-size:{px(12.5)}px
                font-color:{color(if current.get() {ink.on_inverse} else {ink.fg})}
                font-weight:{if current.get() {700} else {400}} {
                row #relay.fade-line {
                    text width:max-content shrink:0 text-wrap:none {value.get()}
                }
            }
        }
    }
}

/// A module head for one profile field: its name, where its value comes
/// from, and Inherit when this director overrides it. Extra controls sit
/// between the name and the origin.
#[component]
fn FieldHead(
    model: Model,
    title: &'static str,
    field: &'static str,
    #[prop(optional)] children: Children,
) -> Element {
    view! {
        row #relay.module-head gap:{px(8.0)}px {
            row #relay.eyebrow height:min-content width:1fr min-width:0px {
                text text-transform:uppercase letter-spacing:{px(0.6)}px (title)
            }
            children
            row #relay.caption height:min-content width:max-content {
                text { model.origin(field) }
            }
            if model.origin(field) == "Director override" {
                button #relay.action @click:{ model.inherit(field); }
                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                    label:{ format!("Inherit {field}") } "Inherit"
            }
        }
    }
}

/// The origin of one whole matrix field, with Inherit when overridden.
#[component]
fn FieldOrigin(model: Model, title: &'static str, field: &'static str) -> Element {
    view! {
        row width:1fr min-width:0px align:center gap:{px(8.0)}px
            pad:(horizontal:{px(12.0)}px vertical:{px(6.0)}px)
            stroke:(width:{px(1.0)} color:rule.hair edges:right) {
            col width:1fr min-width:0px height:min-content gap:{px(2.0)}px {
                row #relay.eyebrow height:min-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px (title)
                }
                row #relay.caption height:min-content {
                    text { model.origin(field) }
                }
            }
            if model.origin(field) == "Director override" {
                button #relay.action @click:{ model.inherit(field); }
                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                    label:{ format!("Inherit {field}") } "Inherit"
            }
        }
    }
}

#[component]
pub(crate) fn ActionRow(model: Model, task: Task) -> Element {
    let index = Derived::new(move || {
        let permission = model
            .editor_profile
            .get()
            .permissions
            .get(&task)
            .copied()
            .unwrap_or(Permission::Deny);
        PERMISSIONS
            .iter()
            .position(|p| *p == permission)
            .unwrap_or(0)
    });
    let choose: crate::labels::Select = std::rc::Rc::new(move |slot: usize| {
        model.modify_profile("permissions", |p| {
            p.permissions.insert(task, PERMISSIONS[slot]);
        });
    });
    view! {
        row height:{px(40.0)}px align:center pad:(left:{px(12.0)}px right:{px(3.0)}px)
            stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
            row width:1fr min-width:0px height:min-content font-size:{px(13.0)}px clip {
                text text-wrap:none (task.label())
            }
            StepToggle model:(model) task:(task) completion:false
            StepToggle model:(model) task:(task) completion:true
            SlidingSegments name:(format!("{} permission", task.label()))
                options:(PERMISSIONS.iter().map(|p| p.label().to_string()).collect::<Vec<_>>())
                index:(index) select:(choose) attention-slot:(Some(1)) cell-width:(72.0)
                disabled:(Derived::new(|| false))
        }
    }
}

#[component]
pub(crate) fn StepToggle(model: Model, task: Task, completion: bool) -> Element {
    let on = Derived::new(move || {
        let profile = model.editor_profile.get();
        if completion {
            profile.completion.contains(&task)
        } else {
            profile.responsibilities.contains(&task)
        }
    });
    view! {
        row width:max-content stroke:(width:{px(1.0)} color:rule.hair edges:left) {
            button #relay.tree-control
                @click:{ model.modify_profile(if completion { "completion" } else { "responsibilities" }, |p| {
                let steps = if completion { &mut p.completion } else { &mut p.responsibilities };
                if steps.contains(&task) { steps.retain(|t| t != &task); } else { steps.push(task); }
            }); }
                width:{px(96.0)}px height:fill justify:center role:checkbox
                label:{ format!("{} {}", task.label(), if completion { "required for completion" } else { "responsibility" }) } {
                el width:{px(12.0)}px height:{px(12.0)}px
                    fill:{color(if on.get() {ink.fg} else {surface.panel})}
                    stroke:(width:{px(1.0)} color:{color(if on.get() {ink.fg} else {rule.line})} offset:{px(-0.5)}) {}
            } as toggle
            {let semantic = toggle.clone(); Effect::new(move || { semantic.toggled(on.get()); });}
        }
    }
}

/// The action matrix module: the three whole-field origins, then one row per
/// action with its responsibility, completion step and permission.
#[component]
pub(crate) fn ActionMatrix(model: Model) -> Element {
    view! {
        col #relay.module label:"Action matrix" {
            row #relay.module-head {
                row #relay.eyebrow height:min-content width:max-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Actions"
                }
                row #relay.caption height:min-content width:max-content {
                    text "Responsibility · completion · permission"
                }
            }
            row height:min-content stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                FieldOrigin model:(model) title:"Responsibilities" field:"responsibilities"
                FieldOrigin model:(model) title:"Completion" field:"completion"
                FieldOrigin model:(model) title:"Permissions" field:"permissions"
            }
            row #relay.eyebrow height:{px(26.0)}px align:center
                pad:(left:{px(12.0)}px right:{px(3.0)}px)
                stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                row width:1fr height:min-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Action"
                }
                row width:{px(96.0)}px height:min-content justify:center {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Responsible"
                }
                row width:{px(96.0)}px height:min-content justify:center {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Required"
                }
                row width:{px(216.0)}px height:min-content justify:center {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Permission"
                }
            }
            for task in Task::ALL {
                ActionRow model:(model) task:(task)
            }
            row #relay.caption height:min-content pad:{px(12.0)}px {
                text
                    "Implement: Deny blocks worker turns; Ask requires approval each turn. Other actions are workflow settings."
            }
        }
    }
}

/// The profile as TOML: export, the effective profile, and import.
#[component]
fn ProfileToml(model: Model) -> Element {
    view! {
        col #relay.module {
            row #relay.module-head gap:{px(6.0)}px {
                row #relay.eyebrow height:min-content width:1fr {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px "Profile TOML"
                }
                button #relay.action @click:{ model.export_toml(); }
                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px) "Export / edit TOML"
                button #relay.action
                    @click:{ model.toml.set(model.editor_profile.get_untracked().to_toml()); model.advanced.set(true); }
                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px) "Show effective profile"
            }
            if model.advanced.get() {
                col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                    row #relay.caption height:min-content {
                        text
                            "Project defaults use a complete profile. Directors use overrides; omitted fields inherit. Copy this text to export."
                    }
                    input #relay.area multiline height:{px(240.0)}px label:"Profile TOML" model.toml
                    button #relay.action @click:{ model.import_toml(); } "Import TOML into draft"
                }
            }
        }
    }
}

#[component]
pub(crate) fn ProfileControls(model: Model) -> Element {
    let harness_index = Derived::new(move || {
        usize::from(model.editor_profile.get().harness == Harness::ClaudeCode)
    });
    let choose_harness: crate::labels::Select = std::rc::Rc::new(move |slot: usize| {
        model.modify_profile("harness", |p| {
            p.harness = if slot == 1 {
                Harness::ClaudeCode
            } else {
                Harness::Codex
            }
        });
    });
    let approval_index = Derived::new(move || {
        let mode = model.editor_profile.get().execution.approval;
        ApprovalMode::ALL
            .iter()
            .position(|m| *m == mode)
            .unwrap_or(0)
    });
    let choose_approval: crate::labels::Select = std::rc::Rc::new(move |slot: usize| {
        model.modify_profile("execution", |p| {
            p.execution.approval = ApprovalMode::ALL[slot]
        });
    });
    let capacity = Derived::new(move || match model.editor.get() {
        EditTarget::Director(id) => crate::labels::director_capacity(&model.snapshot.get(), &id),
        _ => (0, 0, 0),
    });
    let whole_project =
        Derived::new(move || model.editor_profile.get().scope == DirectorScope::Project);
    view! {
        col height:min-content gap:{px(24.0)}px {
            col #relay.module {
                FieldHead model:(model) title:"Agent harness" field:"harness"
                col height:min-content pad:{px(12.0)}px {
                    SlidingSegments name:("Agent harness".to_string())
                        options:(vec!["Codex".to_string(), "Claude Code".to_string()])
                        index:(harness_index) select:(choose_harness) attention-slot:(None)
                        cell-width:(130.0) disabled:(Derived::new(|| false))
                }
            }
            col #relay.module {
                FieldHead model:(model) title:"Execution approval" field:"execution"
                col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                    SlidingSegments name:("Execution approval".to_string())
                        options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                        index:(approval_index) select:(choose_approval) attention-slot:(None)
                        cell-width:(130.0) disabled:(Derived::new(|| false))
                    row #relay.caption height:min-content {
                        text
                            "Execution mode configures the harness. Action permissions are separate workflow settings."
                    }
                }
            }
            col #relay.module {
                FieldHead model:(model) title:"Scope" field:"scope" {
                    button #relay.action
                        @click:{ model.modify_profile("scope", |p| p.scope = DirectorScope::Project); }
                        pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px) role:checkbox
                        fill:{color(if whole_project.get() {ink.inverse} else {surface.panel})}
                        hover {
                            fill:{color(if whole_project.get() {ink.inverse} else {surface.raised})}
                        }
                        pressed {
                            fill:{color(if whole_project.get() {ink.inverse} else {surface.raised})}
                        } {
                        row height:min-content width:max-content
                            font-color:{color(if whole_project.get() {ink.on_inverse} else {ink.fg})}
                            font-weight:{if whole_project.get() {700} else {400}} {
                            text "Whole project"
                        }
                    }
                }
                grid height:min-content gap:{px(6.0)}px pad:{px(12.0)}px
                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(200.0).into(), GridTrack::fr(1.0)))} {
                    for (_, issue) in { model.snapshot.get().issues.into_iter().filter(|i| i.project_id == model.project.get() && model.snapshot.get().canonical_issue_id(&i.id) == i.id).map(|i| (i.id.clone(), i)).collect::<Vec<_>>() } {
                        ScopeChip model:(model) issue:(issue.clone())
                    }
                }
            }
            col #relay.module {
                FieldHead model:(model) title:"Concurrent workers" field:"max_workers" {
                    row height:min-content width:max-content align:center {
                        button #relay.action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = p.max_workers.saturating_sub(1)); }
                            pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                            label:"Decrease worker limit" "−"
                        row #relay.value height:min-content width:{px(40.0)}px justify:center {
                            text { format!("{:02}", model.editor_profile.get().max_workers) }
                        }
                        button #relay.action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = (p.max_workers + 1).min(64)); }
                            pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                            label:"Increase worker limit" "+"
                    }
                }
                row height:min-content gap:{px(10.0)}px align:center pad:{px(12.0)}px {
                    SlotMeter running:(Derived::new(move || capacity.get().0))
                        active:(Derived::new(move || capacity.get().1))
                        limit:(Derived::new(move || model.editor_profile.get().max_workers as usize))
                    row #relay.caption height:min-content width:1fr min-width:0px {
                        text
                            {
                            let limit = model.editor_profile.get().max_workers;
                            if limit == 0 {
                                "0 pauses delegation · range 0–64".to_string()
                            } else if matches!(model.editor.get(), EditTarget::Director(_)) {
                                format!("{} active · limit {limit} · range 0–64", capacity.get().1)
                            } else {
                                format!("Limit {limit} per director · range 0–64")
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One issue in the director's scope: its number block, inverse while
/// selected, and title.
#[component]
fn ScopeChip(model: Model, issue: Issue) -> Element {
    let id = issue.id.clone();
    let issue_id = State::new(id.clone());
    let number = issue.reference.as_ref().map(|r| format!("#{}", r.number));
    let has_number = number.is_some();
    let chip_label = format!(
        "Scope {}",
        number.clone().unwrap_or_else(|| issue.title.clone())
    );
    let number = State::new(number.unwrap_or_default());
    let title = State::new(issue.title.clone());
    let on = Derived::new(
        move || matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())),
    );
    view! {
        row height:min-content min-width:0px {
            button #relay.action
                @click:{
                model.modify_profile("scope", |p| {
                    let mut ids = match &p.scope { DirectorScope::Issues { issue_ids } => issue_ids.clone(), _ => vec![] };
                    if model.scope_contains(&ids, &id) { let snapshot = model.snapshot.get_untracked(); ids.retain(|x| snapshot.canonical_issue_id(x) != snapshot.canonical_issue_id(&id)); } else { ids.push(id.clone()); }
                    p.scope = if ids.is_empty() { DirectorScope::Project } else { DirectorScope::Issues { issue_ids: ids } };
                });
            }
                width:fill height:{px(24.0)}px pad:0px gap:0px justify:start role:checkbox
                label:(chip_label.clone())
                stroke:(width:{px(1.0)} color:{color(if on.get() {rule.line} else {rule.hair})} offset:{px(-1.0)}) {
                if has_number {
                    row width:max-content align:center shrink:0
                        pad:(horizontal:{px(6.0)}px vertical:0px) font-size:{px(10.0)}px
                        font-weight:700 fill:{color(if on.get() {ink.inverse} else {surface.panel})}
                        font-color:{color(if on.get() {ink.on_inverse} else {ink.muted})}
                        stroke:(width:{px(1.0)} color:{color(if on.get() {rule.line} else {rule.hair})} edges:right) {
                        text text-wrap:none {number.get()}
                    }
                }
                stack #relay.fade-label pad:(horizontal:{px(8.0)}px vertical:0px)
                    font-size:{px(12.0)}px
                    font-color:{color(if on.get() {ink.fg} else {ink.muted})} {
                    row #relay.fade-line {
                        text width:max-content shrink:0 text-wrap:none {title.get()}
                    }
                }
            } as chip
            {let semantic = chip.clone(); Effect::new(move || { semantic.toggled(on.get()); });}
        }
    }
}

pub(crate) const PERMISSIONS: [Permission; 3] =
    [Permission::Deny, Permission::Ask, Permission::Allow];
