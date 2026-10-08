use super::*;

#[component]
pub(crate) fn Profiles(model: Model) -> Element {
    let width = State::new(0.0f32);
    let wide = Derived::new(move || width.get() >= px(1040.0));
    let profile_kind = Derived::new(move || {
        String::from(if model.editor.get() == EditTarget::Defaults {
            "ALL"
        } else {
            "DIR"
        })
    });
    let profile_title = Derived::new(move || {
        String::from(if model.editor.get() == EditTarget::Defaults {
            "Project defaults"
        } else {
            "Director profile"
        })
    });
    view! {
        col width:1fr pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) gap:{px(14.0)}px
            @layout:{move |rect: Rect| width.set(rect.size.width)} {
            scroll width:max-content {
                row height:min-content width:max-content gap:0px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    button #relay.tree-control @click:{ model.open_profile(EditTarget::Defaults); }
                        pad:(horizontal:{px(12.0)}px vertical:0px)
                        fill:{color(if model.editor.get() == EditTarget::Defaults {ink.inverse} else {surface.panel})}
                        label:"Project defaults" {
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
                            label:{model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| d.name.clone()).unwrap_or_default()} {
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
                        label:"Create director" {
                        text font-weight:{if model.editor.get() == EditTarget::New {700} else {400}}
                            font-color:{color(if model.editor.get() == EditTarget::New {ink.on_inverse} else {ink.fg})}
                            "+ Director"
                    }
                }
            } as tabs
            { tabs.root().style_dyn(move || Style::stack().width(Dimension::Fill).height(px(42.0)).basis(px(42.0)).shrink(0.0)); }
            scroll {
                col height:min-content gap:{px(20.0)}px {
                    row height:min-content gap:{px(12.0)}px align:center {
                        col width:{px(48.0)}px height:{px(48.0)}px align:center justify:center
                            shrink:0 fill:ink.inverse {
                            text text-wrap:none font-weight:{700} font-color:{color(ink.on_inverse)}
                                font-size:{px(12.0)}px { profile_kind.get() }
                        }
                        col width:1fr height:min-content gap:{px(4.0)}px {
                            text font-size:{px(20.0)}px font-weight:{650}
                                font-family:{FontFamily::SansSerif} { profile_title.get() }
                            text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                                { if model.editor.get() == EditTarget::Defaults { "Directors inherit these values unless they override a field. Applies to the next worker turn." } else { "Overrides apply to this director's next worker turn; omitted fields follow project defaults." } }
                        }
                    }
                    if model.editor.get() != EditTarget::Defaults {
                        input #relay.field label:"Director name" model.editor_name
                    }
                    // One grid for both layouts: the track template changes with the
                    // width, the controls themselves are never rebuilt.
                    grid height:min-content gap:{px(24.0)}px align:start
                        cols:{if wide.get() { GridTracks::new([GridTrack::fr(1.0), px(460.0).into()]) } else { GridTracks::new([GridTrack::fr(1.0)]) }} {
                        col height:min-content min-width:0px label:"Action matrix column" {
                            ActionMatrix model:(model)
                        }
                        col height:min-content min-width:0px {
                            ProfileControls model:(model)
                        }
                    }
                    row height:min-content gap:{px(8.0)}px {
                        button #relay.action @click:{ model.export_toml(); } "Export / edit TOML"
                        button #relay.action
                            @click:{ model.toml.set(model.editor_profile.get_untracked().to_toml()); model.advanced.set(true); }
                            "Show effective profile"
                    }
                    if model.advanced.get() {
                        col height:min-content gap:{px(10.0)}px {
                            text font-size:{px(12.0)}px font-color:ink.muted
                                "Project defaults use a complete profile. Directors use overrides; omitted fields inherit. Copy this text to export."
                            input #relay.area multiline height:{px(240.0)}px label:"Profile TOML"
                                model.toml
                            button #relay.action @click:{ model.import_toml(); }
                                "Import TOML into draft"
                        }
                    }
                }
            }
            row height:min-content gap:{px(12.0)}px align:center shrink:0 pad:(top:{px(10.0)}px)
                stroke:(width:{px(1.0)} color:rule.line edges:top) {
                button #relay.primary @click:{ model.save_profile(); }
                    disabled:{ model.busy.get() || !model.connected.get() } label:"Save profile"
                    "Save profile"
                text font-size:{px(12.0)}px font-color:ink.muted
                    "Explicit overrides survive project default changes"
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
        row height:{px(46.0)}px align:center pad:(horizontal:{px(12.0)}px vertical:0px)
            stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
            text width:1fr text-wrap:none font-size:{px(13.0)}px (task.label())
            StepToggle model:(model) task:(task) completion:false
            StepToggle model:(model) task:(task) completion:true
            SlidingSegments name:(format!("{} permission", task.label()))
                options:(PERMISSIONS.iter().map(|p| p.label().to_string()).collect::<Vec<_>>())
                index:(index) select:(choose) attention-slot:(Some(1)) cell-width:(78.0)
                disabled:(Derived::new(|| false))
        }
    }
}

#[component]
pub(crate) fn ProfileField(model: Model, title: &'static str, field: &'static str) -> Element {
    view! {
        row height:min-content justify:between align:center gap:{px(8.0)}px {
            col height:min-content gap:{px(4.0)}px {
                text font-size:{px(13.0)}px font-weight:650 font-family:sans-serif (title)
                text font-size:{px(11.0)}px font-color:{color(ink.muted)} { model.origin(field) }
            }
            if model.origin(field) == "Director override" {
                button #relay.action @click:{ model.inherit(field); }
                    label:{ format!("Inherit {field}") } "Inherit"
            }
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
        row height:min-content width:max-content {
            button #relay.tree-control
                @click:{ model.modify_profile(if completion { "completion" } else { "responsibilities" }, |p| {
                let steps = if completion { &mut p.completion } else { &mut p.responsibilities };
                if steps.contains(&task) { steps.retain(|t| t != &task); } else { steps.push(task); }
            }); }
                width:{px(110.0)}px justify:start role:checkbox
                label:{ format!("{} {}", task.label(), if completion { "required for completion" } else { "responsibility" }) } {
                el width:{px(14.0)}px height:{px(14.0)}px
                    fill:{color(if on.get() {ink.fg} else {surface.panel})}
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-0.5)}) {}
            } as toggle
            {let semantic = toggle.clone(); Effect::new(move || { semantic.toggled(on.get()); });}
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
    view! {
        col height:min-content gap:{px(20.0)}px {
            ProfileField model:(model) title:"Agent harness" field:"harness"
            SlidingSegments name:("Agent harness".to_string())
                options:(vec!["Codex".to_string(), "Claude Code".to_string()]) index:(harness_index)
                select:(choose_harness) attention-slot:(None) cell-width:(130.0)
                disabled:(Derived::new(|| false))
            ProfileField model:(model) title:"Execution approval" field:"execution"
            SlidingSegments name:("Execution approval".to_string())
                options:(ApprovalMode::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                index:(approval_index) select:(choose_approval) attention-slot:(None)
                cell-width:(150.0) disabled:(Derived::new(|| false))
            text font-size:{px(12.0)}px font-color:ink.muted
                "Execution mode configures the harness. Action permissions are separate workflow settings."
            ProfileField model:(model) title:"Scope" field:"scope"
            row height:min-content gap:{px(6.0)}px {
                button #relay.action
                    @click:{ model.modify_profile("scope", |p| p.scope = DirectorScope::Project); }
                    fill:{color(if model.editor_profile.get().scope == DirectorScope::Project {ink.inverse} else {surface.panel})}
                    font-color:{color(if model.editor_profile.get().scope == DirectorScope::Project {ink.on_inverse} else {ink.fg})}
                    font-weight:{if model.editor_profile.get().scope == DirectorScope::Project {700} else {400}}
                    "Whole project"
                for (_, issue) in { model.snapshot.get().issues.into_iter().filter(|i| i.project_id == model.project.get() && model.snapshot.get().canonical_issue_id(&i.id) == i.id).map(|i| (i.id.clone(), i)).collect::<Vec<_>>() } {
                    let id = issue.id.clone();
                    let issue_id = State::new(id.clone());
                    let number = issue.reference.as_ref().map(|r|format!("#{}",r.number)).unwrap_or_else(||issue.title.clone());
                    button #relay.action
                        @click:{
                                model.modify_profile("scope", |p| {
                                    let mut ids = match &p.scope { DirectorScope::Issues { issue_ids } => issue_ids.clone(), _ => vec![] };
                                    if model.scope_contains(&ids, &id) { let snapshot = model.snapshot.get_untracked(); ids.retain(|x| snapshot.canonical_issue_id(x) != snapshot.canonical_issue_id(&id)); } else { ids.push(id.clone()); }
                                    p.scope = if ids.is_empty() { DirectorScope::Project } else { DirectorScope::Issues { issue_ids: ids } };
                                });
                            }
                        fill:{color(if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())) {ink.inverse} else {surface.panel})}
                        font-color:{color(if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())) {ink.on_inverse} else {ink.fg})}
                        font-weight:{if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())) {700} else {400}}
                        { format!("{} {}", if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())) { "✓" } else { "+" }, number) }
                }
            }
            text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                {
                        match model.editor_profile.get().scope {
                            DirectorScope::Project => "All project issues".into(),
                            DirectorScope::Issues { issue_ids } => issue_ids.iter()
                                .filter_map(|id| model.snapshot.get().issue(id).ok().cloned())
                                .map(|issue|issue.reference.as_ref().map(|r|format!("#{}",r.number)).unwrap_or_else(||issue.title.clone()))
                                .collect::<Vec<_>>().join(", "),
                        }
                    }
            ProfileField model:(model) title:"Concurrent workers" field:"max_workers"
            row height:min-content gap:{px(10.0)}px align:center {
                button #relay.action
                    @click:{ model.modify_profile("max_workers", |p| p.max_workers = p.max_workers.saturating_sub(1)); }
                    label:"Decrease worker limit" "−"
                text font-size:{px(18.0)}px
                    { format!("{:02}", model.editor_profile.get().max_workers) }
                button #relay.action
                    @click:{ model.modify_profile("max_workers", |p| p.max_workers = (p.max_workers + 1).min(64)); }
                    label:"Increase worker limit" "+"
                SlotMeter running:(Derived::new(move || capacity.get().0))
                    active:(Derived::new(move || capacity.get().1))
                    limit:(Derived::new(move || model.editor_profile.get().max_workers as usize))
                text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                    { if model.editor_profile.get().max_workers == 0 { "0 pauses delegation".to_string() } else if matches!(model.editor.get(), EditTarget::Director(_)) { format!("{} active now", capacity.get().1) } else { "0 pauses delegation".to_string() } }
            }
        }
    }
}

#[component]
pub(crate) fn ActionMatrix(model: Model) -> Element {
    view! {
        col height:min-content gap:0px stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
            label:"Action matrix" {
            grid cols:(1fr 1fr 1fr) height:min-content gap:{px(12.0)}px pad:{px(12.0)}px
                stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                ProfileField model:(model) title:"Responsibilities" field:"responsibilities"
                ProfileField model:(model) title:"Required for completion" field:"completion"
                ProfileField model:(model) title:"Action permissions" field:"permissions"
            }
            row height:{px(30.0)}px align:center pad:(horizontal:{px(12.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                text width:1fr font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
                    letter-spacing:{px(0.6)}px "Action"
                text width:{px(110.0)}px font-size:{px(11.0)}px font-color:ink.muted
                    text-transform:uppercase letter-spacing:{px(0.6)}px "Responsible"
                text width:{px(110.0)}px font-size:{px(11.0)}px font-color:ink.muted
                    text-transform:uppercase letter-spacing:{px(0.6)}px "Required"
                text width:{px(234.0)}px font-size:{px(11.0)}px font-color:ink.muted
                    text-transform:uppercase letter-spacing:{px(0.6)}px "Permission"
            }
            for task in Task::ALL {
                ActionRow model:(model) task:(task)
            }
            text font-size:{px(12.0)}px font-color:ink.muted pad:{px(12.0)}px
                "Implement: Deny blocks worker turns; Ask requires approval each turn. Other actions are workflow settings."
        }
    }
}
pub(crate) const PERMISSIONS: [Permission; 3] =
    [Permission::Deny, Permission::Ask, Permission::Allow];
