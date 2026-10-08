use super::*;

#[component]
pub(crate) fn Settings(model: Model) -> Element {
    let scale = State::new(model.preferences.get_untracked().scale * 100.0);
    Effect::new(move || {
        let value = (scale.get() / 100.0).clamp(0.8, 2.0);
        if model.preferences.get_untracked().scale != value {
            model.preferences.update(|p| p.scale = value);
        }
    });
    Effect::new(move || {
        let value = model.preferences.get().scale * 100.0;
        if (scale.get_untracked() - value).abs() > 0.01 {
            scale.set(value);
        }
    });
    view! {
        scroll {
            col height:min-content pad:{px(28.0)}px gap:{px(24.0)}px {
                if matches!(model.settings_store.get().persistence, crate::settings::Persistence::Suspended { .. }) {
                    col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px fill:attention.fill
                        stroke:(width:{px(4.0)} color:attention.text edges:left)
                        label:"Display settings not saved" {
                        text font-family:sans-serif font-size:{px(15.0)}px font-weight:650
                            font-color:attention.on "Display settings are not being saved"
                        text font-size:{px(12.0)}px font-color:{color(attention.on)}
                            {match model.settings_store.get().persistence {crate::settings::Persistence::Suspended { reason } => crate::settings::suspended_notice(&reason), _ => String::new()}}
                        text font-size:{px(12.0)}px font-color:{color(attention.on)}
                            {model.settings_store.get().path.map(|p| format!("File: {}", p.display())).unwrap_or_default()}
                        row height:min-content gap:{px(8.0)}px {
                            button #relay.action @click:{crate::settings::recover_by_backup(model);}
                                label:"Back up file and save current settings" "Back up and save"
                            button #relay.action @click:{crate::settings::retry_reading(model);}
                                label:"Retry reading settings file" "Retry reading"
                        }
                    }
                }
                if model.settings_store.get().backup.is_some() {
                    text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                        {model.settings_store.get().backup.map(|b| format!("Previous settings file kept at {}", b.display())).unwrap_or_default()}
                }
                col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px max-width:{px(760.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Harnesses"
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "Installed on the connected server. Existing harness logins and model settings are used."
                    if !model.connected.get() {
                        text font-size:{px(12.0)}px font-color:ink.muted
                            "Disconnected · last checked status retained"
                    }
                    if !model.harness_error.get().is_empty() {
                        text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                            {model.harness_error.get()}
                    }
                    for (_, harness) in [("codex",Harness::Codex),("claude",Harness::ClaudeCode)] {
                        HarnessCard model:(model) harness:(harness)
                    }
                }
                col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px max-width:{px(760.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Appearance"
                    AppearanceSegments model:(model) field:(0usize)
                    text font-family:sans-serif font-size:{px(14.0)}px "Light palette"
                    AppearanceSegments model:(model) field:(1usize)
                    text font-family:sans-serif font-size:{px(14.0)}px "Dark palette"
                    AppearanceSegments model:(model) field:(2usize)
                }
                col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px max-width:{px(760.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650
                        "Interface scale"
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "Display scaling follows your operating system. Adjust the interface size here."
                    row height:min-content gap:{px(8.0)}px align:center {
                        stepper #relay.scale-stepper min:80 max:200 step:10 label:"Interface scale percent" scale as scale_control
                        {scale_control.decrement().label("Decrease interface scale");scale_control.increment().label("Increase interface scale");}
                        text "%"
                    }
                    button #relay.action @click:{model.preferences.update(|p|p.scale=1.0);}
                        label:"Reset interface scale" "Reset"
                }
                col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px max-width:{px(760.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Sidebar"
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "Drag its right edge to resize."
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px {
                        button #relay.action
                            @click:{model.preferences.update(|p| p.sidebar_width = (p.sidebar_width - 20.0).max(160.0));}
                            width:fill label:"Narrower sidebar" "Narrower"
                        button #relay.action
                            @click:{model.preferences.update(|p| p.sidebar_width = (p.sidebar_width + 20.0).min(360.0));}
                            width:fill label:"Wider sidebar" "Wider"
                        button #relay.action
                            @click:{model.preferences.update(|p| p.sidebar_width = 220.0);}
                            width:fill label:"Reset sidebar width" "Reset"
                    }
                }
                col height:min-content gap:{px(10.0)}px pad:{px(14.0)}px max-width:{px(760.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Fonts"
                    text font-size:{px(12.0)}px "Titles: Reddit Sans · Text: Zed Mono"
                }
                text font-size:{px(12.0)}px font-color:ink.muted
                    "Appearance is saved on this machine. Projects and harness configuration are saved on the server."
            }
        }
    }
}

#[component]
pub(crate) fn HarnessCard(model: Model, harness: Harness) -> Element {
    let harness_status = Derived::new(move || {
        model
            .harnesses
            .get()
            .into_iter()
            .find(|s| s.harness == harness)
    });
    let advanced = State::new(false);
    let executable = State::new(String::new());
    let name = match harness {
        Harness::Codex => "Codex",
        Harness::ClaudeCode => "Claude Code",
    };
    let width = State::new(0.0f32);
    view! {
        col height:min-content max-width:{px(720.0)}px gap:{px(8.0)}px
            @layout:{move |rect:Rect|width.set(rect.size.width)} {
            if width.get() < px(520.0) {
                col height:min-content gap:{px(6.0)}px {
                    HarnessSummary model:(model) harness:(harness)
                    HarnessActions model:(model) harness:(harness) advanced:(advanced)
                        executable:(executable)
                }
            } else {
                row height:min-content align:center gap:{px(12.0)}px {
                    HarnessSummary model:(model) harness:(harness)
                    HarnessActions model:(model) harness:(harness) advanced:(advanced)
                        executable:(executable)
                }
            }
            if advanced.get() {
                col height:min-content gap:{px(8.0)}px {
                    text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                        {harness_status.get().map(|s|s.detail).unwrap_or_default()}
                    text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                        {harness_status.get().map(|s|format!("{} · {}",s.version.unwrap_or_else(||"Version unavailable".into()),sync_label(Some(s.checked_at)).replacen("Last synced", "Checked", 1))).unwrap_or_default()}
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "Automatic · Ask · Unrestricted Access. Mode availability also depends on the harness account and managed settings."
                    input #relay.field label:{format!("{name} executable on server")} executable
                    button #relay.action
                        @click:{model.submit(Command::ConfigureHarness{harness,executable:executable.get_untracked().trim().into()},model.snapshot.get_untracked().revision,crate::model::Saved::Action);}
                        disabled:{!model.connected.get() || model.busy.get()}
                        label:{format!("Save {name} executable and check status")} "Save and check"
                }
            }
        }
    }
}

#[component]
pub(crate) fn HarnessSummary(model: Model, harness: Harness) -> Element {
    let harness_status = Derived::new(move || {
        model
            .harnesses
            .get()
            .into_iter()
            .find(|s| s.harness == harness)
    });
    let state = Derived::new(move || {
        if !model.connected.get() || !model.harness_error.get().is_empty() {
            String::new()
        } else {
            harness_status.get().map(|s| s.state).unwrap_or_default()
        }
    });
    view! {
        row height:min-content width:1fr align:center gap:{px(8.0)}px {
            icon size:{px(20.0)}px shrink:0
                {if harness==Harness::Codex{harness_codex}else{harness_claude}}
            text font-family:{FontFamily::SansSerif} font-size:{px(14.0)}px font-weight:{650}
                {if harness==Harness::Codex{"Codex"}else{"Claude Code"}}
            icon size:{px(14.0)}px shrink:0
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>status.success,"signed_out"|"incompatible"=>status.warning,"unknown"|"error"|"failed"=>status.danger,_=>ink.muted})}
                {match state.get().as_str(){"ready"=>harness_ready,"signed_out"|"incompatible"=>harness_warning,"unknown"|"error"|"failed"=>harness_failed,_=>harness_neutral}}
            text font-size:{px(12.0)}px
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>status.success,"signed_out"|"incompatible"=>status.warning,"unknown"|"error"|"failed"=>status.danger,_=>ink.muted})}
                {if !model.connected.get(){"Disconnected".into()}else if !model.harness_error.get().is_empty(){format!("Refresh failed · last check: {}",harness_status.get().map(|s|match s.state.as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible",_=>"Check failed"}).unwrap_or("Unavailable"))}else{match state.get().as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible","unknown"|"error"|"failed"=>"Check failed",""=>"Checking",_=>"Check unavailable"}.into()}}
        }
    }
}
#[component]
pub(crate) fn HarnessActions(
    model: Model,
    harness: Harness,
    advanced: State<bool>,
    executable: State<String>,
) -> Element {
    let name = if harness == Harness::Codex {
        "Codex"
    } else {
        "Claude Code"
    };
    view! {
        row height:min-content width:min-content gap:{px(6.0)}px {
            button #relay.action
                @click:{if let Some(sender)=model.harness_refresh.get_untracked(){let _=sender.send(());}}
                disabled:{!model.connected.get()} label:{format!("Refresh {name} status")} "Refresh"
            button #relay.action
                @click:{if let Some(found)=model.harnesses.get_untracked().iter().find(|s|s.harness==harness){executable.set(found.executable.clone());}advanced.set(!advanced.get_untracked());}
                label:{format!("{name} executable and status details")} "Details"
        }
    }
}
