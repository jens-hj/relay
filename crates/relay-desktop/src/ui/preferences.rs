use super::*;
use crate::controls::{ResetSetting, ResetSettingProps};

#[component]
pub(crate) fn Settings(model: Model) -> Element {
    let scale = State::new(model.preferences.get_untracked().scale * 100.0);
    let update_scale = Effect::new(move || {
        let value = (scale.get() / 100.0).clamp(0.8, 2.0);
        if model.preferences.get_untracked().scale != value {
            model.preferences.update(|p| p.scale = value);
        }
    });
    let sync_scale = Effect::new(move || {
        let value = model.preferences.get().scale * 100.0;
        if (scale.get_untracked() - value).abs() > 0.01 {
            scale.set(value);
        }
    });
    let root = view! {
        scroll {
            col height:min-content pad:{px(28.0)}px gap:{px(24.0)}px {
                // Notices carry their own spacing, so none leaves an empty gap.
                col height:min-content gap:0px {
                if matches!(model.settings_store.get().persistence, crate::settings::Persistence::Suspended { .. }) {
                    col height:min-content pad:(bottom:{px(24.0)}px) {
                    col height:min-content max-width:{px(760.0)}px gap:{px(10.0)}px pad:{px(14.0)}px
                        fill:attention.fill stroke:(width:{px(4.0)} color:attention.text edges:left)
                        label:"Display settings not saved" {
                        row #relay.title height:min-content font-size:{px(15.0)}px
                            font-color:attention.on {
                            text "Display settings are not being saved"
                        }
                        row height:min-content font-size:{px(12.0)}px font-color:attention.on {
                            text
                                {match model.settings_store.get().persistence {crate::settings::Persistence::Suspended { reason } => crate::settings::suspended_notice(&reason), _ => String::new()}}
                        }
                        row height:min-content font-size:{px(12.0)}px font-color:attention.on {
                            text
                                {model.settings_store.get().path.map(|p| format!("File: {}", p.display())).unwrap_or_default()}
                        }
                        row height:min-content gap:{px(8.0)}px {
                            button #relay.action @click:{crate::settings::recover_by_backup(model);}
                                label:"Back up file and save current settings" "Back up and save"
                            button #relay.action @click:{crate::settings::retry_reading(model);}
                                label:"Retry reading settings file" "Retry reading"
                        }
                    }
                    }
                }
                if model.settings_store.get().backup.is_some() {
                    row #relay.caption height:min-content max-width:{px(760.0)}px {
                        text
                            {model.settings_store.get().backup.map(|b| format!("Previous settings file kept at {}", b.display())).unwrap_or_default()}
                    }
                }
                }
                col #relay.module max-width:{px(760.0)}px label:"Harnesses" {
                    SettingsHead title:"Harnesses" note:"Installed on the connected server"
                    col height:min-content gap:0px {
                        if !model.connected.get() || !model.harness_error.get().is_empty() {
                            row #relay.caption height:min-content
                                pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px)
                                stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                                text
                                    {if !model.connected.get() {"Disconnected · last checked status retained".to_string()} else {model.harness_error.get()}}
                            }
                        }
                        for (_, harness) in [("codex",Harness::Codex),("claude",Harness::ClaudeCode)] {
                            col height:min-content
                                pad:(horizontal:{px(12.0)}px vertical:{px(10.0)}px)
                                stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                                HarnessCard model:(model) harness:(harness)
                            }
                        }
                        row #relay.caption height:min-content
                            pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px) {
                            text "Existing harness logins and model settings are used."
                        }
                    }
                }
                col #relay.module max-width:{px(760.0)}px label:"Appearance" {
                    SettingsHead title:"Appearance" note:"Saved on this machine"
                    col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                        row #relay.eyebrow height:min-content {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Theme"
                        }
                        row height:min-content align:center gap:{px(8.0)}px {
                            el width:1fr min-width:0px height:{px(34.0)}px { scroll width:max-content { AppearanceSegments model:(model) field:(0usize) } }
                            ResetSetting model:(model) setting:(crate::settings::Setting::Mode) name:("theme")
                        }
                        row #relay.eyebrow height:min-content pad:(top:{px(4.0)}px) {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Light palette"
                        }
                        row height:min-content align:center gap:{px(8.0)}px {
                            el width:1fr min-width:0px height:{px(34.0)}px { scroll width:max-content { AppearanceSegments model:(model) field:(1usize) } }
                            ResetSetting model:(model) setting:(crate::settings::Setting::LightPalette) name:("light palette")
                        }
                        row #relay.eyebrow height:min-content pad:(top:{px(4.0)}px) {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Dark palette"
                        }
                        row height:min-content align:center gap:{px(8.0)}px {
                            el width:1fr min-width:0px height:{px(34.0)}px { scroll width:max-content { AppearanceSegments model:(model) field:(2usize) } }
                            ResetSetting model:(model) setting:(crate::settings::Setting::DarkPalette) name:("dark palette")
                        }
                    }
                }
                col #relay.module max-width:{px(760.0)}px label:"Interface scale" {
                    SettingsHead title:"Interface scale"
                        note:"Display scaling follows the operating system"
                    col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                        row height:min-content gap:{px(8.0)}px align:center {
                            stepper #relay.scale-stepper min:80 max:200 step:10 label:"Interface scale percent" scale as scale_control
                            {scale_control.decrement().label("Decrease interface scale");scale_control.increment().label("Increase interface scale");}
                            row #relay.caption height:min-content width:max-content {text "%"}
                            ResetSetting model:(model) setting:(crate::settings::Setting::Scale) name:("interface scale")
                        }
                    }
                }
                col #relay.module max-width:{px(760.0)}px label:"Fonts" {
                    SettingsHead title:"Fonts" note:""
                    row #relay.value height:min-content pad:{px(12.0)}px {
                        text "Titles: Reddit Sans · Text: Zed Mono"
                    }
                }
                row #relay.caption height:min-content max-width:{px(760.0)}px {
                    text
                        "Appearance is saved on this machine. Projects and harness configuration are saved on the server."
                }
            }
        }
    };
    root.__hot_on_remove(move || {
        update_scale.dispose();
        sync_scale.dispose();
    });
    root
}

/// A settings module head: its caps name and a short note on the right.
#[component]
fn SettingsHead(title: &'static str, note: &'static str) -> Element {
    view! {
        row #relay.module-head gap:{px(8.0)}px {
            row #relay.eyebrow height:min-content width:max-content {
                text text-transform:uppercase letter-spacing:{px(0.6)}px (title)
            }
            row #relay.caption height:min-content width:1fr min-width:0px justify:end clip {
                text text-wrap:none (note)
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
                    row #relay.caption height:min-content {
                        text {harness_status.get().map(|s|s.detail).unwrap_or_default()}
                    }
                    row #relay.caption height:min-content {
                        text
                            {harness_status.get().map(|s|format!("{} · {}",s.version.unwrap_or_else(||"Version unavailable".into()),sync_label(Some(s.checked_at)).replacen("Last synced", "Checked", 1))).unwrap_or_default()}
                    }
                    row #relay.caption height:min-content {
                        text
                            "Automatic · Ask · Unrestricted Access. Mode availability also depends on the harness account and managed settings."
                    }
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
            row #relay.title height:min-content width:max-content font-size:{px(14.0)}px {
                text (if harness == Harness::Codex { "Codex" } else { "Claude Code" })
            }
            icon size:{px(14.0)}px shrink:0
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>status.success,"signed_out"|"incompatible"=>status.warning,"unknown"|"error"|"failed"=>status.danger,_=>ink.muted})}
                {match state.get().as_str(){"ready"=>harness_ready,"signed_out"|"incompatible"=>harness_warning,"unknown"|"error"|"failed"=>harness_failed,_=>harness_neutral}}
            row height:min-content width:max-content font-size:{px(12.0)}px
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>status.success,"signed_out"|"incompatible"=>status.warning,"unknown"|"error"|"failed"=>status.danger,_=>ink.muted})} {
                text
                    {if !model.connected.get(){"Disconnected".into()}else if !model.harness_error.get().is_empty(){format!("Refresh failed · last check: {}",harness_status.get().map(|s|match s.state.as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible",_=>"Check failed"}).unwrap_or("Unavailable"))}else{match state.get().as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible","unknown"|"error"|"failed"=>"Check failed",""=>"Checking",_=>"Check unavailable"}.into()}}
            }
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
