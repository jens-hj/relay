use super::*;
use crate::controls::{
    ContrastToggle, ContrastToggleProps, ModeSegments, ModeSegmentsProps, ResetSetting,
    ResetSettingProps,
};
use relay_ui::prelude::{CellButton, CellButtonProps, Label, LabelProps, TextRole, Tone, UiText};
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
        Module title:"Harnesses" icon:(Some(relay_ui::icons::harness.codex))
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
                { parent.adopt(&saving); }
                if model.settings_store.get().backup.is_some() {
                    { parent.adopt(&caption(Derived::new(move || model.settings_store.get().backup.map(|b| format!("Previous settings file kept at {}", b.display())).unwrap_or_default()), true)); }
                }
                { parent.adopt(&harnesses); }
                Module title:"Claude Window Start" icon:(Some(relay_ui::icons::harness.claude_code))
                    note:(Some(UiText::from("Saved on the connected server"))) {
                    WindowStartSetting model:(model)
                }
                Module title:"Appearance" icon:(Some(relay_ui::icons::section.appearance))
                    note:(Some(UiText::from("Saved on this machine"))) {
                    AppearanceRow model:(model) title:"Theme"
                        setting:(crate::settings::Setting::Mode)
                    AppearanceRow model:(model) title:"Contrast"
                        setting:(crate::settings::Setting::Contrast)
                }
                Module title:"Interface Scale" icon:(Some(relay_ui::icons::call.search))
                    note:(Some(UiText::from("Display scaling follows the operating system"))) {
                    row height:{px(40.0)}px gap:0px align:center label:"Interface Scale" {
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
                Module title:"Fonts" icon:(Some(relay_ui::icons::section.fonts)) {
                    { parent.adopt(&caption("Text: Geist Mono · Titles: Instrument Serif", false)); }
                }
                { parent.adopt(&caption("Appearance is saved on this machine. Projects and harness configuration are saved on the server.", false)); }
            }
        }
    };
    root
}

/// "2d 4h", "3h 05m" or "12m".
fn span(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{}m", minutes.max(1))
    }
}

/// What to do about a failure, when there is something to do.
fn failure_hint(reason: WindowStartFailure) -> &'static str {
    match reason {
        WindowStartFailure::SignedOut => {
            " Sign in to Claude Code on the server, then turn this off and on."
        }
        WindowStartFailure::NotSubscription => {
            " Sign in with a Claude subscription on the server, then turn this off and on."
        }
        WindowStartFailure::ManagedHooks => {
            " Administrator policy can run hooks, which this message must not. Set \"disableAllHooks\": true in the managed settings on the server, then turn this off and on."
        }
        WindowStartFailure::ModelUnavailable => {
            " Relay only uses a canonical Haiku model and does not substitute another."
        }
        WindowStartFailure::WeeklyLimit => " It waits for the weekly limit to reset.",
        _ => "",
    }
}

/// The last message and the next one due, in words. `now` is Unix seconds.
pub(crate) fn window_start_status(start: &WindowStart, now: u64) -> String {
    let ago = start
        .last_attempt_at
        .map(|at| format!(" {} ago", span(now.saturating_sub(at))))
        .unwrap_or_default();
    let mut text = match &start.last_outcome {
        None => "No message sent yet.".to_string(),
        Some(WindowStartOutcome::Sent { model }) => format!("Last message sent to {model}{ago}."),
        Some(WindowStartOutcome::NotNeeded) => {
            "Claude had other work after the window ended, so no message was sent.".to_string()
        }
        Some(WindowStartOutcome::Failed { reason, attempts }) => format!(
            "Last message failed{ago}: {} (attempt {attempts}).{}",
            reason.label(),
            failure_hint(*reason)
        ),
        Some(WindowStartOutcome::Unconfirmed { reason }) => format!(
            "Last message may have been delivered{ago}: {}. Nothing more is sent for this window until Claude reports a new one, or you turn this off and on.",
            reason.label()
        ),
    };
    if start.enabled {
        text.push_str(&match start.next_due_at {
            Some(at) if at > now => format!(" Next message in {}.", span(at - now)),
            Some(_) => " Next message is due now.".to_string(),
            None => " No message is scheduled until Claude reports a window end.".to_string(),
        });
    }
    text
}

/// Whether the server sends one tiny Claude message after each reported
/// five-hour window ends. Off until the owner turns it on; it spends
/// subscription usage, so the cost is stated beside the control.
#[component]
fn WindowStartSetting(model: Model) -> Element {
    let selected =
        Derived::new(move || usize::from(model.snapshot.get().claude_window_start.enabled));
    let choose: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot: usize| {
        model.action(Command::SetClaudeWindowStart { enabled: slot == 1 })
    });
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    let status = Derived::new(move || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        window_start_status(&model.snapshot.get().claude_window_start, now)
    });
    view! {
        col height:min-content gap:0px {
            row height:{px(40.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Segments name:("Claude window start".to_string())
                    options:(vec!["Off".to_string(), "On".to_string()])
                    tones:(vec![ground.ink, relay_ui::tokens::Signal::NeedsYou.token()])
                    selected:(relay_ui::Live::from(selected)) on-select:(choose)
                    disabled:(relay_ui::Live::from(blocked))
            }
            Caption
                text:"When Claude reports that a five-hour window has ended, the server sends one tiny message (Reply OK.) through its Claude Code subscription sign-in, using a canonical Haiku model, unless other Claude work has already started a window. Nothing is sent until Claude has reported a window end. It refuses to send unless Claude Code is verifiably signed in with a subscription, not an API key, and where administrator-managed hooks could run. Each message uses a little of your Claude usage. This reduces idle gaps; it does not add usage or change weekly limits, and Anthropic does not guarantee when a window starts. Use it on one Relay server per Claude account: servers do not coordinate with each other."
            { parent.adopt(&caption(status, false)); }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_start_status_names_the_last_outcome_and_the_next_message() {
        let now = 1_000_000;
        let mut start = WindowStart::default();
        assert_eq!(window_start_status(&start, now), "No message sent yet.");
        start.set_enabled(true, now);
        assert_eq!(
            window_start_status(&start, now),
            "No message sent yet. No message is scheduled until Claude reports a window end."
        );
        start.last_attempt_at = Some(now - 3 * 3600 - 300);
        start.last_outcome = Some(WindowStartOutcome::Sent {
            model: "Haiku 4.5".into(),
        });
        start.next_due_at = Some(now + 4 * 3600);
        assert_eq!(
            window_start_status(&start, now),
            "Last message sent to Haiku 4.5 3h 05m ago. Next message in 4h 00m."
        );
        start.last_outcome = Some(WindowStartOutcome::Failed {
            reason: WindowStartFailure::SignedOut,
            attempts: 1,
        });
        start.next_due_at = None;
        let text = window_start_status(&start, now);
        assert!(
            text.contains("Claude Code is signed out (attempt 1)"),
            "{text}"
        );
        assert!(
            text.contains("Sign in to Claude Code on the server"),
            "{text}"
        );
        // Turned off, the last outcome stays and nothing is scheduled.
        start.set_enabled(false, now);
        let text = window_start_status(&start, now);
        assert!(
            !text.contains("Next message") && !text.contains("scheduled"),
            "{text}"
        );
        // A message that may have been delivered says so and is not retried.
        start.last_outcome = Some(WindowStartOutcome::Unconfirmed {
            reason: WindowStartFailure::TimedOut,
        });
        let text = window_start_status(&start, now);
        assert!(text.contains("may have been delivered"), "{text}");
        assert!(
            text.contains("Nothing more is sent for this window"),
            "{text}"
        );
        start.last_outcome = Some(WindowStartOutcome::Failed {
            reason: WindowStartFailure::NotSubscription,
            attempts: 1,
        });
        let text = window_start_status(&start, now);
        assert!(text.contains("not signed in with a subscription"), "{text}");
    }
}
