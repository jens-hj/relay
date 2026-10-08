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
            scroll {
                col height:min-content gap:{px(24.0)}px
                    pad:(left:{gutter.get()}px right:{gutter.get()}px top:{px(28.0)}px bottom:{px(24.0)}px) {
                    ProfileHeader model:(model)
                    Inheritance model:(model)
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
                row #relay.module height:{px(50.0)}px align:center gap:0px pad:0px
                    label:"Save bar" {
                    row #relay.caption width:1fr min-width:0px height:min-content clip
                        pad:(horizontal:{px(14.0)}px vertical:0px) {
                        text text-wrap:none "Explicit overrides survive project default changes"
                    }
                    button #relay.primary @click:{ model.save_profile(); } height:fill
                        pad:(horizontal:{px(16.0)}px vertical:0px)
                        stroke:(width:{px(1.0)} color:rule.line edges:left)
                        disabled:{ model.busy.get() || !model.connected.get() } label:"Save profile"
                        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-2.0)}) }
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
    let width = State::new(0.0f32);
    let readings = Derived::new(move || width.get() >= px(760.0));
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
        row #relay.module height:{px(74.0)}px shrink:0 label:"Profile header"
            @layout:{move |rect:Rect| width.set(rect.size.width)} {
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
                if defaults.get() {
                    stack #relay.fade-label #relay.title height:min-content font-size:{px(22.0)}px {
                        row #relay.fade-line {
                            text width:max-content shrink:0 text-wrap:none {title.get()}
                        }
                    }
                } else {
                    input width:fill height:{px(32.0)}px pad:0px radius:0px
                        fill:(Color::TRANSPARENT)
                        stroke:(width:{px(1.0)} color:(Color::TRANSPARENT)) font-family:sans-serif
                        font-size:{px(22.0)}px font-weight:600 label:"Director name"
                        placeholder:"Director name"
                        focused { stroke:(width:{px(1.0)} color:accent.focus edges:bottom) }
                        model.editor_name
                }
            }
            if defaults.get() && readings.get() {
                HeaderCell key:("Directors".to_string()) value:(directors)
            }
            if director.get().is_some() && readings.get() {
                HeaderCell key:("Overrides".to_string())
                    value:(Derived::new(move || format!("{:02} / {:02}", overridden_fields(&model.editor_overrides.get()).len(), OVERRIDE_FIELDS)))
                HeaderCell key:("Workers".to_string())
                    value:(Derived::new(move || format!("{} / {} active", capacity.get().1, capacity.get().2)))
            }
            if readings.get() {
                HeaderCell key:("Profile fields".to_string())
                    value:(Derived::new(move || if model.editor_profile.get().validate().is_ok() {"Valid".to_string()} else {"Invalid".to_string()})) {
                    tooltip #relay.tooltip summary:"Profile field validation" {
                        text {model.editor_profile.get().validate().err().unwrap_or_else(|| "Local profile field validation passes. The server rechecks scope and policy when saving.".to_string())}
                    }
                }
            }
            row width:max-content stroke:(width:{px(1.0)} color:rule.line edges:left) {
                CommandPaletteButton model:(model)
                    compact:(Derived::new(move || width.get() < px(400.0)))
            }
        }
    }
}

/// Where the edited values come from: project defaults, then the director's
/// overrides, applied when the next worker turn starts.
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
        grid height:min-content gap:{px(12.0)}px shrink:0 label:"Inheritance"
            @layout:{move |rect:Rect| width.set(rect.size.width)}
            cols:{if horizontal.get() {GridTracks::new([GridTrack::fr(1.0),GridTrack::fr(1.0),GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0)])}} {
            ChainStep step:"01 · Project defaults" value:(base) current:(defaults)
            ChainStep step:"02 · Director overrides" value:(director)
                current:(Derived::new(move || !defaults.get()))
            ChainStep step:"03 · Next worker turn"
                value:(Derived::new(|| "Applies when the next worker turn starts".to_string()))
                current:(Derived::new(|| false))
        }
    }
}

#[component]
fn ChainStep(step: &'static str, value: Derived<String>, current: Derived<bool>) -> Element {
    view! {
        col width:1fr min-width:0px height:{px(54.0)}px shrink:0 justify:center gap:{px(3.0)}px
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
    let width = State::new(0.0f32);
    let wide = Derived::new(move || width.get() >= px(480.0));
    view! {
        grid #relay.module-head @layout:{move |rect:Rect| width.set(rect.size.width)}
            label:{format!("{title} field header")} content-align:(x:start y:center)
            items-align:(x:start y:center)
            height:{if wide.get() {px(30.0).into()} else {Dimension::MinContent}} pad:0px gap:0px
            rows:{if wide.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::MinContent, GridTrack::MinContent, GridTrack::MinContent])}}
            cols:{if wide.get() {GridTracks::new([GridTrack::fr(1.0), GridTrack::MaxContent, GridTrack::MaxContent])} else {GridTracks::new([GridTrack::fr(1.0)])}} {
            row #relay.eyebrow height:{if wide.get() {Dimension::Fill} else {Dimension::MinContent}}
                min-height:{px(30.0)}px min-width:0px align:center
                pad:(horizontal:{px(12.0)}px vertical:0px)
                stroke:(width:{px(if wide.get() {0.0} else {1.0})} color:rule.hair edges:bottom) {
                text text-transform:uppercase letter-spacing:{px(0.6)}px (title)
            }
            row height:{if wide.get() {Dimension::Fill} else {Dimension::MinContent}}
                width:{if wide.get() {Dimension::MaxContent} else {Dimension::Fill}} align:center
                stroke:(width:{px(1.0)} color:rule.hair edges:if wide.get() {StrokeEdges::LEFT} else {StrokeEdges::BOTTOM}) {
                children
            }
            row height:{if wide.get() {Dimension::Fill} else {Dimension::MinContent}}
                min-height:{px(30.0)}px
                width:{if wide.get() {Dimension::MaxContent} else {Dimension::Fill}} align:center
                gap:0px
                stroke:(width:{px(if wide.get() {1.0} else {0.0})} color:rule.hair edges:left) {
                row #relay.caption height:min-content
                    width:{if wide.get() {Dimension::MaxContent} else {Dimension::Fill}}
                    min-width:0px pad:(horizontal:{px(12.0)}px vertical:0px) {
                    text
                        { if wide.get() {model.origin(field)} else if model.origin(field) == "Director override" {"Override"} else {"Default"} }
                }
                if model.origin(field) == "Director override" {
                    button #relay.header-action @click:{ model.inherit(field); }
                        min-height:{px(30.0)}px stroke:(width:{px(1.0)} color:rule.hair edges:left)
                        label:{ format!("Inherit {field}") } "Inherit"
                }
                tooltip #relay.tooltip summary:"Field origin" {
                    text {format!("{title}: {}. Overrides replace this whole field.", model.origin(field))}
                }
            }
        }
    }
}

/// The origin of one whole matrix field, with Inherit when overridden.
#[component]
fn FieldOrigin(model: Model, title: &'static str, field: &'static str) -> Element {
    let short = match field {
        "responsibilities" => "Resp.",
        "completion" => "Done",
        _ => "Permission",
    };
    view! {
        row width:1fr min-width:0px align:center gap:{px(6.0)}px
            pad:(horizontal:{px(10.0)}px vertical:{px(6.0)}px)
            stroke:(width:{px(1.0)} color:rule.hair edges:right) label:{format!("{title} origin")} {
            col width:1fr min-width:0px height:min-content gap:{px(2.0)}px {
                row #relay.eyebrow height:min-content {
                    text (short)
                }
                row #relay.caption height:min-content {
                    text
                        {if model.origin(field) == "Director override" {"Override"} else {"Default"}}
                }
            }
            if model.origin(field) == "Director override" {
                button #relay.action @click:{model.inherit(field);} width:{px(20.0)}px
                    height:{px(20.0)}px pad:0px justify:center label:{format!("Inherit {field}")}
                    "×"
            }
            tooltip #relay.tooltip summary:"Field origin" {
                text {format!("{title}: {}. Overrides replace this whole field.", model.origin(field))}
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
        row height:{px(34.0)}px align:center pad:(left:{px(12.0)}px right:{px(3.0)}px)
            stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
            row width:1fr min-width:0px height:min-content font-size:{px(13.0)}px clip {
                text text-wrap:none (task.label())
            }
            StepToggle model:(model) task:(task) completion:false
            StepToggle model:(model) task:(task) completion:true
            SlidingSegments name:(format!("{} permission", task.label()))
                options:(PERMISSIONS.iter().map(|p| p.label().to_string()).collect::<Vec<_>>())
                index:(index) select:(choose) attention-slot:(Some(1)) cell-width:(60.0)
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
                width:{px(60.0)}px height:fill justify:center role:checkbox
                label:{ format!("{} {}", task.label(), if completion { "required for completion" } else { "responsibility" }) } {
                el width:{px(12.0)}px height:{px(12.0)}px
                    fill:{color(if on.get() {ink.fg} else {surface.panel})}
                    stroke:(width:{px(1.0)} color:{color(if on.get() {ink.fg} else {rule.line})} offset:{px(-0.5)}) {}
            } as toggle
            {
                let semantic = toggle.clone();
                let effect = Effect::new(move || { semantic.toggled(on.get()); });
                // Component effects otherwise outlive this removed subtree.
                toggle.__hot_on_remove(move || effect.dispose());
            }
        }
    }
}

/// The action matrix module: the three whole-field origins, then one row per
/// action with its responsibility, completion step and permission.
#[component]
pub(crate) fn ActionMatrix(model: Model) -> Element {
    let width = State::new(0.0f32);
    let extent = State::new(px(400.0));
    let root = view! {
        col height:min-content min-width:0px {
            scroll {
                col #relay.module label:"Action matrix"
                    @layout:{move |rect:Rect| extent.set(rect.size.height)} {
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
                        row width:{px(60.0)}px height:min-content justify:center {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Resp."
                        }
                        row width:{px(60.0)}px height:min-content justify:center {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Done"
                        }
                        row width:{px(180.0)}px height:min-content justify:center {
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
            } as horizontal
            {
                horizontal.root().on_layout(move |rect| width.set(rect.size.width));
                horizontal.content().style_dyn(move || Style::column()
                    .width(width.get().max(px(432.0))).height(Dimension::MinContent).shrink(0.0));
                horizontal.root().style_dyn(move || Style::stack()
                    .width(Dimension::Fill).height(extent.get()).basis(Dimension::Auto).grow(0.0).shrink(0.0));
            }
        }
    };
    root
}

/// The profile as TOML: export, the effective profile, and import.
#[component]
fn ProfileToml(model: Model) -> Element {
    view! {
        col #relay.module {
            row #relay.module-head gap:0px pad:0px {
                row #relay.eyebrow height:min-content width:1fr
                    pad:(horizontal:{px(12.0)}px vertical:0px) {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px
                        "Effective profile · TOML"
                }
                button #relay.header-action
                    @click:{
                        if !model.advanced.get_untracked() && model.toml.get_untracked().is_empty() {
                            model.toml.set(model.editor_profile.get_untracked().to_toml());
                        }
                        model.advanced.update(|open| *open = !*open);
                    }
                    stroke:(width:{px(1.0)} color:rule.line edges:left) label:"Toggle profile TOML"
                    {if model.advanced.get() {"Hide"} else {"Edit"}}
            }
            if model.advanced.get() {
                col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                    row #relay.caption height:min-content {
                        text
                            "Project defaults use a complete profile. Directors use overrides; omitted fields inherit. Copy this text to export."
                    }
                    button #relay.action @click:{ model.export_toml(); } "Export / edit TOML"
                    button #relay.action
                        @click:{ model.toml.set(model.editor_profile.get_untracked().to_toml()); }
                        "Show effective profile"
                    input #relay.area multiline height:{px(240.0)}px label:"Profile TOML" model.toml
                    button #relay.action @click:{ model.import_toml(); } "Import TOML into draft"
                }
            }
        }
    }
}

#[component]
pub(crate) fn ProfileControls(model: Model) -> Element {
    let harness_width = State::new(0.0f32);
    let approval_width = State::new(0.0f32);
    let harness_keys: std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<usize, Element>>> =
        Default::default();
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
                grid height:min-content gap:0px label:"Harness choices"
                    @layout:{move |rect:Rect| harness_width.set(rect.size.width)}
                    cols:{if harness_width.get() >= px(360.0) {GridTracks::new([GridTrack::fr(1.0),GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0)])}} {
                    HarnessChoice model:(model) harness:(Harness::Codex) keys:(harness_keys.clone())
                    HarnessChoice model:(model) harness:(Harness::ClaudeCode)
                        keys:(harness_keys.clone())
                }
            }

            col #relay.module {
                FieldHead model:(model) title:"Execution approval" field:"execution"
                col height:min-content gap:0px {
                    row height:{px(34.0)}px
                        @layout:{move |rect:Rect| approval_width.set(rect.size.width)} {
                        scroll width:fill {
                            SlidingSegments name:("Execution approval".to_string())
                                options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                                index:(approval_index) select:(choose_approval)
                                attention-slot:(None) cell-width:(130.0)
                                disabled:(Derived::new(|| false)) fill-width:true
                        } as approval_scroll
                        { approval_scroll.content().style_dyn(move || Style::column().width(approval_width.get().max(px(390.0))).height(px(34.0)).shrink(0.0)); }
                    }
                    row #relay.caption height:min-content pad:{px(12.0)}px {
                        text
                            "Execution mode configures the harness. Action permissions are separate workflow settings."
                    }
                }
            }
            col #relay.module {
                FieldHead model:(model) title:"Scope" field:"scope" {
                    button #relay.header-action
                        @click:{ model.modify_profile("scope", |p| p.scope = DirectorScope::Project); }
                        min-height:{px(30.0)}px label:"Scope: Whole project" role:checkbox
                        fill:{color(if whole_project.get() {ink.inverse} else {surface.panel})}
                        hover {
                            fill:{color(if whole_project.get() {ink.inverse_hover} else {surface.raised})}
                        }
                        pressed {
                            fill:{color(if whole_project.get() {ink.inverse_pressed} else {surface.raised})}
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
                    row min-height:{px(30.0)}px width:max-content align:center {
                        button #relay.header-action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = p.max_workers.saturating_sub(1)); }
                            width:{px(30.0)}px pad:0px stroke:(width:0px)
                            label:"Decrease worker limit" "−"
                        row #relay.value height:fill width:{px(40.0)}px align:center justify:center
                            stroke:(width:{px(1.0)} color:rule.hair edges:left) {
                            text { format!("{:02}", model.editor_profile.get().max_workers) }
                        }
                        button #relay.header-action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = (p.max_workers + 1).min(64)); }
                            width:{px(30.0)}px pad:0px
                            stroke:(width:{px(1.0)} color:rule.hair edges:left)
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

/// A harness choice reports the actual connected server's last check. An
/// inverse selected cell retains the same glyph shape with readable ink.
#[component]
fn HarnessChoice(
    model: Model,
    harness: Harness,
    keys: std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<usize, Element>>>,
) -> Element {
    let name = if harness == Harness::Codex {
        "Codex"
    } else {
        "Claude Code"
    };
    let selected = Derived::new(move || model.editor_profile.get().harness == harness);
    let state = Derived::new(move || {
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
    });
    let caption = Derived::new(move || {
        match state.get().as_str() {
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
    });
    let tone = move || {
        color(if selected.get() {
            ink.on_inverse
        } else {
            match state.get().as_str() {
                "ready" => status.success,
                "signed_out" | "incompatible" => status.warning,
                "unknown" | "error" | "failed" => status.danger,
                _ => ink.muted,
            }
        })
    };
    let button = view! {
        button #relay.tree-control
            @click:{model.modify_profile("harness", |p| p.harness = harness);} width:1fr
            min-width:0px height:min-content pad:0px role:radio
            label:{format!("Agent harness: {name}")} description:{caption.get()}
            fill:{color(if selected.get() {ink.inverse} else {surface.panel})}
            stroke:(width:{px(1.0)} color:rule.hair offset:{px(-1.0)})
            hover { fill:{color(if selected.get() {ink.inverse_hover} else {surface.raised})} }
            pressed { fill:{color(if selected.get() {ink.inverse_pressed} else {surface.raised})} } {
            col height:min-content min-height:{px(76.0)}px justify:center gap:{px(8.0)}px
                pad:{px(12.0)}px {
                row height:min-content gap:{px(8.0)}px align:center
                    font-color:{color(if selected.get() {ink.on_inverse} else {ink.fg})} {
                    icon size:{px(18.0)}px shrink:0
                        {if harness == Harness::Codex {harness_codex} else {harness_claude}}
                    row height:min-content font-family:sans-serif font-size:{px(14.0)}px
                        font-weight:600 {
                        text (name)
                    }
                }
                row height:min-content gap:{px(6.0)}px align:center font-color:{tone()}
                    font-size:{px(11.0)}px {
                    icon size:{px(11.0)}px shrink:0
                        {match state.get().as_str(){"ready" => harness_ready, "signed_out" | "incompatible" => harness_warning, "unknown" | "error" | "failed" => harness_failed, _ => harness_neutral}}
                    text {caption.get()}
                }
                tooltip #relay.tooltip summary:"Harness availability" {
                    text {model.harnesses.get().iter().find(|s| s.harness == harness).map(|s| s.detail.clone()).unwrap_or_else(|| caption.get())}
                }
            }
        }
    };
    let semantic = button.clone();
    let effect = Effect::new(move || {
        semantic.toggled(selected.get());
    });
    button.__hot_on_remove(move || effect.dispose());
    let slot = usize::from(harness == Harness::ClaudeCode);
    keys.borrow_mut().insert(slot, button.clone());
    let cleanup = keys.clone();
    on_cleanup(move || {
        cleanup.borrow_mut().remove(&slot);
    });
    button.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let current =
            usize::from(model.editor_profile.get_untracked().harness == Harness::ClaudeCode);
        let next = match event.key {
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => 1 - current,
            Key::Home => 0,
            Key::End => 1,
            _ => return,
        };
        model.modify_profile("harness", |p| {
            p.harness = if next == 0 {
                Harness::Codex
            } else {
                Harness::ClaudeCode
            }
        });
        let target = keys.borrow().get(&next).cloned();
        if let Some(target) = target {
            target.focus();
        }
        ctx.stop_propagation();
    });
    button
}

/// One issue in the director's scope: its number block, inverse while
/// selected, and title.
#[component]
fn ScopeChip(model: Model, issue: Issue) -> Element {
    let id = issue.id.clone();
    let issue_id = State::new(id.clone());
    let fallback = issue.clone();
    let current = Derived::new(move || {
        model
            .snapshot
            .get()
            .issue(&issue_id.get())
            .cloned()
            .unwrap_or_else(|_| fallback.clone())
    });
    let number = Derived::new(move || current.get().reference.map(|r| format!("#{}", r.number)));
    let title = Derived::new(move || current.get().title);
    let chip_label =
        Derived::new(move || format!("Scope {}", number.get().unwrap_or_else(|| title.get())));
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
                label:{chip_label.get()}
                stroke:(width:{px(1.0)} color:{color(if on.get() {rule.line} else {rule.hair})} offset:{px(-1.0)}) {
                if number.get().is_some() {
                    row width:max-content align:center shrink:0
                        pad:(horizontal:{px(6.0)}px vertical:0px) font-size:{px(10.0)}px
                        font-weight:700 fill:{color(if on.get() {ink.inverse} else {surface.panel})}
                        font-color:{color(if on.get() {ink.on_inverse} else {ink.muted})}
                        stroke:(width:{px(1.0)} color:{color(if on.get() {rule.line} else {rule.hair})} edges:right) {
                        text text-wrap:none {number.get().unwrap_or_default()}
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
            {
                let semantic = chip.clone();
                let effect = Effect::new(move || { semantic.toggled(on.get()); });
                chip.__hot_on_remove(move || effect.dispose());
            }
        }
    }
}

pub(crate) const PERMISSIONS: [Permission; 3] =
    [Permission::Deny, Permission::Ask, Permission::Allow];
