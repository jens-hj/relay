use super::*;
use crate::controls::{
    ContrastToggle, ContrastToggleProps, ModeSegments, ModeSegmentsProps, ResetSetting,
    ResetSettingProps,
};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Label, LabelProps, Rule, TextRole, Tone, UiText,
};
use relay_ui::surfaces::{
    HarnessCard as HarnessCell, HarnessCardProps as HarnessCellProps, Module, ModuleProps,
    Notice as NoticeStrip, NoticeProps as NoticeStripProps, NoticeTone, OnNumber, Stepper,
    StepperProps, TextInput, TextInputProps,
};

/// A muted caption in a padded cell over a hairline.
fn caption(text: impl Into<UiText>, rule: bool) -> Element {
    toned_caption(text, rule, Tone::Muted)
}

/// A caption in a status tone, for warnings such as a failed refresh.
fn toned_caption(text: impl Into<UiText>, rule: bool, tone: Tone) -> Element {
    let text = text.into();
    view! {
        row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px)
            stroke:(width:{px(if rule {1.0} else {0.0})} color:ground.hairline edges:bottom) {
            Label text:(text) role:(TextRole::Caption) tone:(tone)
        }
    }
}

/// Settings: harnesses, appearance, interface scale and fonts, as modules
/// sharing the page's rules.
#[component]
pub(crate) fn Settings(model: Model) -> Element {
    let percent = Derived::new(move || (model.preferences.get().scale * 100.0).round() as i64);
    let set_percent: OnNumber = std::rc::Rc::new(move |value| {
        let value = (value as f32 / 100.0).clamp(0.8, 2.0);
        if model.preferences.get_untracked().scale != value {
            model.preferences.update(|p| p.scale = value);
        }
    });
    let suspended = Derived::new(move || match model.settings_store.get().persistence {
        crate::settings::Persistence::Suspended { reason } => {
            Some(crate::settings::suspended_notice(&reason))
        }
        _ => None,
    });
    let saving = view! {
        col height:min-content {}
    };
    saving.switch(
        move || (suspended.get(), model.settings_store.get().path),
        move |parent, (reason, path)| {
            let Some(reason) = reason.clone() else {
                return;
            };
            let detail = match path {
                Some(path) => format!("{reason} File: {}", path.display()),
                None => reason,
            };
            let actions = vec![
                (
                    "Back up file and save current settings".to_string(),
                    press(move || crate::settings::recover_by_backup(model)),
                ),
                (
                    "Retry reading settings file".to_string(),
                    press(move || crate::settings::retry_reading(model)),
                ),
            ];
            parent.adopt(&view! {
                col height:min-content label:"Display settings not saved" {
                    NoticeStrip text:"Display settings are not being saved"
                        tone:(NoticeTone::Failure) detail:(Some(UiText::from(detail)))
                        actions:(actions)
                }
            });
        },
    );
    let harnesses = view! {
        Module title:"Harnesses" caps:true
            note:(Some(UiText::from("Installed on the connected server"))) {
            if !model.connected.get() || !model.harness_error.get().is_empty() {
                { parent.adopt(&toned_caption(Derived::new(move || if !model.connected.get() {"Disconnected · last checked status retained".to_string()} else {model.harness_error.get()}), true, Tone::Signal(relay_ui::tokens::Signal::NeedsYou))); }
            }
            HarnessPanel model:(model) harness:(Harness::Codex)
            HarnessPanel model:(model) harness:(Harness::ClaudeCode)
            { parent.adopt(&caption("Existing harness logins and model settings are used.", false)); }
        }
    };
    let root = view! {
        scroll {
            col height:min-content gap:0px {
                if cfg!(target_arch = "wasm32") {
                    row height:{px(40.0)}px
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        row width:max-content {
                            CellButton label:"Sign out of Relay"
                                on-press:(press(move || crate::platform::logout(model)))
                                rule:(Rule::Right) {
                                Label text:"Sign out" role:(TextRole::Label) weight:500
                            }
                        }
                    }
                }
                { parent.adopt(&saving); }
                if model.settings_store.get().backup.is_some() {
                    { parent.adopt(&caption(Derived::new(move || model.settings_store.get().backup.map(|b| format!("Previous settings file kept at {}", b.display())).unwrap_or_default()), true)); }
                }
                { parent.adopt(&harnesses); }
                Module title:"Appearance" caps:true
                    note:(Some(UiText::from("Saved on this machine"))) {
                    AppearanceRow model:(model) title:"Theme"
                        setting:(crate::settings::Setting::Mode)
                    AppearanceRow model:(model) title:"Contrast"
                        setting:(crate::settings::Setting::Contrast)
                }
                Module title:"Interface scale" caps:true
                    note:(Some(UiText::from("Display scaling follows the operating system"))) {
                    row height:{px(40.0)}px gap:0px align:center label:"Interface scale" {
                        row width:{px(150.0)}px shrink:0
                            stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                            Stepper value:(relay_ui::Live::from(percent)) on-change:(set_percent)
                                min:80i64 max:200i64 step:10i64 label:"Interface scale percent"
                                decrease-label:"Decrease interface scale"
                                increase-label:"Increase interface scale" suffix:"%"
                        }
                        row width:1fr {}
                        ResetSetting model:(model) setting:(crate::settings::Setting::Scale)
                            name:("interface scale")
                    }
                }
                Module title:"Fonts" caps:true {
                    { parent.adopt(&caption("Text: Geist Mono · Titles: Instrument Serif", false)); }
                }
                { parent.adopt(&caption("Appearance is saved on this machine. Projects and harness configuration are saved on the server.", false)); }
            }
        }
    };
    root
}

/// An appearance setting: its caps name, its control and its reset cell.
#[component]
fn AppearanceRow(model: Model, title: &'static str, setting: crate::settings::Setting) -> Element {
    let contrast = matches!(setting, crate::settings::Setting::Contrast);
    view! {
        row height:{px(40.0)}px gap:0px align:center
            stroke:(width:{px(if contrast {0.0} else {1.0})} color:ground.hairline edges:bottom) {
            row width:{px(120.0)}px shrink:0 align:center pad:(horizontal:{px(18.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                Label text:(title) role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
            }
            row width:1fr min-width:0px align:center {
                if contrast {
                    ContrastToggle model:(model)
                } else {
                    ModeSegments model:(model)
                }
            }
            ResetSetting model:(model) setting:(setting) name:(title.to_ascii_lowercase())
        }
    }
}

/// The availability of a harness on the connected server, as a status and
/// its word.
fn harness_reading(model: Model, harness: Harness) -> (Option<AgentStatus>, String, String) {
    let found = model
        .harnesses
        .get()
        .into_iter()
        .find(|s| s.harness == harness);
    let state = if !model.connected.get() || !model.harness_error.get().is_empty() {
        String::new()
    } else {
        found.as_ref().map(|s| s.state.clone()).unwrap_or_default()
    };
    let word = if !model.connected.get() {
        "Disconnected".to_string()
    } else if !model.harness_error.get().is_empty() {
        format!(
            "Refresh failed · last check: {}",
            found
                .as_ref()
                .map(|s| match s.state.as_str() {
                    "ready" => "Ready",
                    "signed_out" => "Sign in",
                    "missing" => "Missing",
                    "incompatible" => "Incompatible",
                    _ => "Check failed",
                })
                .unwrap_or("Unavailable")
        )
    } else {
        match state.as_str() {
            "ready" => "Ready",
            "signed_out" => "Sign in",
            "missing" => "Missing",
            "incompatible" => "Incompatible",
            "unknown" | "error" | "failed" => "Check failed",
            "" => "Checking",
            _ => "Check unavailable",
        }
        .to_string()
    };
    let version = found
        .and_then(|s| s.version)
        .filter(|_| model.connected.get())
        .unwrap_or_default();
    // A failed refresh is retryable: it needs you, like a failed check.
    let status = if model.connected.get() && !model.harness_error.get().is_empty() {
        Some(AgentStatus::Waiting)
    } else {
        harness_state(&state)
    };
    (status, word, version)
}

/// One harness on relay-ui's harness card, with Refresh and Details cells
/// and the executable setting beneath when Details is open.
#[component]
pub(crate) fn HarnessPanel(model: Model, harness: Harness) -> Element {
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
    let card = view! {
        col height:min-content {}
    };
    card.switch(
        move || harness_reading(model, harness),
        move |parent, (state, word, version)| {
            let actions = vec![
                (
                    "Refresh".to_string(),
                    press(move || {
                        if model.connected.get_untracked()
                            && let Some(sender) = model.harness_refresh.get_untracked()
                        {
                            let _ = sender.send(());
                        }
                    }),
                ),
                (
                    "Details".to_string(),
                    press(move || {
                        if let Some(found) = model
                            .harnesses
                            .get_untracked()
                            .iter()
                            .find(|s| s.harness == harness)
                        {
                            executable.set(found.executable.clone());
                        }
                        advanced.set(!advanced.get_untracked());
                    }),
                ),
            ];
            let labels = vec![
                format!("Refresh {name} status"),
                format!("{name} executable and status details"),
            ];
            parent.adopt(&view! {
                HarnessCell harness:(crate::adapt::harness(harness)) version:(version.clone())
                    status:(state.map(|s| (s, word.clone()))) actions:(actions)
                    action-labels:(labels) description:(Some(UiText::from(word.clone())))
            });
        },
    );
    let save_disabled = Derived::new(move || !model.connected.get() || model.busy.get());
    view! {
        col height:min-content gap:0px {
            { parent.adopt(&card); }
            if advanced.get() {
                col height:min-content gap:0px
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    { parent.adopt(&caption(Derived::new(move || harness_status.get().map(|s| s.detail).unwrap_or_default()), false)); }
                    { parent.adopt(&caption(Derived::new(move || harness_status.get().map(|s| format!("{} · {}", s.version.unwrap_or_else(|| "Version unavailable".into()), sync_label(Some(s.checked_at)).replacen("Last synced", "Checked", 1))).unwrap_or_default()), false)); }
                    { parent.adopt(&caption("Automatic · Ask · Unrestricted Access. Mode availability also depends on the harness account and managed settings.", true)); }
                    row height:min-content
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        TextInput value:(executable) label:(format!("{name} executable on server"))
                            placeholder:"Executable path" framed:false
                    }
                    row height:{px(36.0)}px {
                        CellButton label:(format!("Save {name} executable and check status"))
                            start:true disabled:(save_disabled) horizontal-inset:(18.0)
                            on-press:(press(move || model.submit(Command::ConfigureHarness { harness, executable: executable.get_untracked().trim().into() }, model.snapshot.get_untracked().revision, crate::model::Saved::Action))) {
                            Label text:"Save and check" role:(TextRole::Label) weight:500
                        }
                    }
                }
            }
        }
    }
}
