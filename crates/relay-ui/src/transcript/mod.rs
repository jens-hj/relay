//! Reading column and transcript blocks; rich text belongs to the consumer.
use crate::prelude::*;
use crate::tokens::{Signal, ground, px, status};
use mosaic::prelude::*;

#[component]
pub fn TranscriptColumn(#[prop(optional)] children: Children) -> Element {
    view! {
        row height:min-content justify:center {
            col max-width:{px(848.0)}px height:min-content gap:{px(28.0)}px {
                children
            }
        }
    }
}
#[component]
pub fn MessageRow(
    speaker: UiText,
    /// Relationship or role, shown above the speaker's name when present.
    #[prop(default = Live::fixed(String::new()))]
    speaker_role: UiText,
    /// The speaking agent's harness glyph.
    #[prop(optional)]
    harness: Option<Harness>,
    /// A speaker mark for speakers that are not agents (the application
    /// itself, a system); used when there is no harness.
    #[prop(optional)]
    icon: Option<SvgToken>,
    /// Opens the speaker, such as the agent session that sent a message.
    #[prop(optional)]
    on_speaker: Option<OnPress>,
    /// Whether the speaker still has a navigation target.
    #[prop(default = Live::fixed(true))]
    speaker_link_enabled: Live<bool>,
    #[prop(optional)] children: Children,
) -> Element {
    let name = view! {
        col width:1fr min-width:0px height:min-content {}
    };
    let has_link = on_speaker.is_some();
    name.switch(move || has_link && speaker_link_enabled.get(), move |parent, linked| {
        let speaker = speaker.clone();
        let role = speaker_role.clone();
        let role_text = role.clone();
        let show_role = role.clone();
        let show_role = Derived::new(move || !show_role.get().is_empty());
        let content = view! {
            col width:1fr min-width:0px height:min-content gap:{px(2.0)}px {
                if show_role.get() {
                    Ellipsis text:(role_text.clone()) role:(TextRole::Caption) tone:(Tone::Faint)
                }
                Ellipsis text:(speaker.clone()) role:(TextRole::Label) tone:(Tone::Faint)
            }
        };
        if *linked {
            let press = on_speaker.clone();
            parent.adopt(&view! {
                button @click:{if let Some(press) = &press { press(); }} width:1fr min-width:0px
                    height:min-content radius:0px pad:0px fill:(Color::TRANSPARENT)
                    label:{let role = role.get(); if role.is_empty() { speaker.get() } else { format!("{role} · {}", speaker.get()) }}
                    hover { fill:ground.hover } pressed { fill:ground.pressed }
                    focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(2.0)}) } {
                    { parent.adopt(&content); }
                }
            });
        } else {
            parent.adopt(&content);
        }
    });
    view! {
        row height:min-content align:start min-width:0px {
            row width:{px(128.0)}px shrink:0 height:min-content gap:{px(7.0)}px align:center
                pad:(top:{px(3.0)}px right:{px(16.0)}px) font-color:ground.muted {
                if harness.is_some() {
                    HarnessGlyph harness:(harness.unwrap()) size:12.0
                } else if icon.is_some() {
                    { parent.adopt(&crate::icons::icon_mark(icon.unwrap(), 12.0)); }
                }
                { parent.adopt(&name); }
            }
            col width:1fr height:min-content min-width:0px gap:{px(12.0)}px font-size:{px(13.0)}px
                line-height:1.7 font-color:ground.ink {
                children
            }
        }
    }
}
#[component]
pub fn UserMessage(#[prop(optional)] children: Children) -> Element {
    view! {
        col height:min-content min-width:0px
            pad:(top:{px(10.0)}px bottom:{px(6.0)}px left:{px(12.0)}px right:{px(12.0)}px)
            fill:ground.panel-raised stroke:(width:{px(1.0)} color:ground.hairline)
            font-size:{px(13.0)}px line-height:1.7 {
            children
        }
    }
}
#[component]
pub fn AgentText(#[prop(optional)] children: Children) -> Element {
    view! {
        col height:min-content min-width:0px gap:{px(12.0)}px font-size:{px(13.0)}px line-height:1.7
            font-color:ground.ink {
            children
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnChild {
    pub name: String,
    pub status: AgentStatus,
    pub harness: Option<Harness>,
    pub model: Option<ModelIdentity>,
    pub effort: Option<Effort>,
}
#[component]
pub fn SpawnBlock(agents: Live<Vec<SpawnChild>>, on_open_tree: OnPress) -> Element {
    let count = agents.clone();
    view! {
        col height:min-content stroke:(width:{px(1.0)} color:ground.hairline) {
            row height:{px(36.0)}px align:center
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                row width:1fr min-width:0px align:center
                    pad:(horizontal:{px(14.0)}px vertical:0px) {
                    Label
                        text:(Derived::new(move || format!("Spawned {} agents", count.get().len())))
                        role:(TextRole::Label) weight:500
                }
                CellButton label:"Open in tree" on-press:(on_open_tree) rule:(Rule::Left)
                    width:max-content shrink:0 {
                    Label text:"Open in tree" role:(TextRole::Label) weight:500
                }
            }
            for (key, child) in { agents.get().into_iter().enumerate().map(|(i, c)| ((i, format!("{c:?}")), c)) } {
                {
                    let line = crate::tree::AgentLine {
                        archived: false,
                        can_delete: false,
                        can_spawn: false,
                        id: format!("spawn-{key:?}"), name: child.name.clone(), status: child.status,
                        harness: child.harness.unwrap_or(Harness::Codex), model: child.model.clone(),
                        effort: child.effort, parent: None, guides: vec![], child_count: 0,
                        expandable: false, expanded: false, selected: false, below: Default::default(),
                    };
                    parent.adopt(&view! { AgentTreeRow line:(line) show-harness:(child.harness.is_some()) over:(ground.page) });
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalView {
    pub requester: Option<String>,
    pub path: Option<String>,
    pub heading: String,
    pub command: Option<String>,
    pub why: Option<String>,
    pub touches: Option<String>,
    pub blocks: Option<String>,
}
crate::value::live_value!(ApprovalView);
#[component]
pub fn ApprovalBlock(
    data: Live<ApprovalView>,
    on_approve: OnPress,
    on_reject: OnPress,
    on_review_diff: OnPress,
) -> Element {
    let host = view! {
        col height:min-content {}
    };
    host.switch(move || data.get(), move |parent, vm| { let vm = vm.clone(); let (approve, reject, review) = (on_approve.clone(), on_reject.clone(), on_review_diff.clone());
              parent.adopt(&view! {
                  CutCorner cut:(Cut::Block) edge:(status.needs_you) edge-opacity:0.32
                      fill:(ground.page) {
                      stack height:min-content clip {
                          Wash signal:(Signal::NeedsYou) opacity:0.22 over:(ground.page)
                          col height:min-content gap:{px(12.0)}px pad:{px(18.0)}px {
                              row height:min-content gap:{px(8.0)}px align:center {
                                  StatusSquare status:(AgentStatus::Waiting)
                                  Label text:(vm.requester.clone().unwrap_or_default())
                                      role:(TextRole::Label) tone:(Tone::Signal(Signal::NeedsYou))
                              }
                              { if let Some(path) = vm.path { parent.adopt(&view! { Ellipsis text:(path) role:(TextRole::Label) tone:(Tone::Muted) }); } }
                              Label text:(vm.heading) role:(TextRole::BodyLarge) weight:600
                              { if let Some(cmd) = vm.command { parent.adopt(&view! {
                                  col height:min-content min-width:0px
                                      pad:(vertical:{px(6.0)}px horizontal:{px(12.0)}px)
                                      fill:ground.code stroke:(width:{px(1.0)} color:ground.hairline) {
                                      text label:"Approval arguments" height:min-content min-width:0px font-family:monospace
                                          font-size:{px(12.0)}px line-height:1.55 {cmd.clone()}
                                  }
                              }); } }
                              { if let Some(why) = vm.why { parent.adopt(&view! { text font-size:{px(12.0)}px line-height:1.55 {why.clone()} }); } }
                              { if let Some(touches) = vm.touches { parent.adopt(&view! { KeyValue entries:(vec![("touches".into(), touches)]) }); } }
                              row width:max-content height:min-content gap:{px(10.0)}px {
                                  PrimaryButton text:"Approve" on-press:(approve)
                                  SecondaryButton text:"Reject" on-press:(reject)
                                  LinkButton text:"Review diff" on-press:(review)
                              }
                              { if let Some(blocks) = vm.blocks { parent.adopt(&view! { Label text:(format!("blocks: {blocks}")) role:(TextRole::Label) tone:(Tone::Muted) }); } }
                          }
                      }
                  }
              });
    });
    host
}
/// Normalize and stop before handing text to shaping/accessibility. Even a
/// single enormous word has a bounded cost and a Unicode-safe excerpt.
pub(crate) fn update_excerpt(text: &str, limit: usize) -> (String, bool) {
    let mut result = String::new();
    let mut count = 0;
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = !result.is_empty();
            continue;
        }
        if space && count < limit {
            result.push(' ');
            count += 1;
        }
        if count == limit {
            return (result, true);
        }
        result.push(ch);
        count += 1;
        space = false;
    }
    (result, false)
}

#[component]
pub fn ThinkingLine(
    #[prop(default = Live::fixed(1u8))] effort: Live<u8>,
    #[prop(default = Live::fixed(ModelHue::Neutral))] hue: Live<ModelHue>,
    #[prop(default = Live::fixed(String::new()))] summary: Live<String>,
    #[prop(default = Live::fixed(String::new()))] run_key: Live<String>,
    #[prop(default = std::rc::Rc::new(|| {}) as OnPress)] on_toggle: OnPress,
) -> Element {
    let expanded = State::new(false);
    let previous_run = State::new(None::<String>);
    Effect::new(move || {
        let key = run_key.get();
        if previous_run.get_untracked().as_ref() != Some(&key) {
            previous_run.set(Some(key));
            expanded.set(false);
        }
    });
    let preview_summary = summary.clone();
    let preview = Derived::new(move || {
        let (mut text, truncated) = update_excerpt(&preview_summary.get(), 240);
        if text.is_empty() {
            "No update yet".to_owned()
        } else {
            if truncated {
                text.push('…');
            }
            text
        }
    });
    let detail_summary = summary.clone();
    let detail = Derived::new(move || {
        let (mut excerpt, truncated) = update_excerpt(&detail_summary.get(), 600);
        if truncated {
            excerpt.push_str("… (Full update in the transcript.)");
        }
        excerpt
    });
    let host = view! {
        col height:min-content min-width:0px {}
    };
    host.switch(
        move || hue.get(),
        move |parent, hue| {
            let colour = hue.token();
            let effort = effort.clone();
            let summary = summary.clone();
            let on_toggle = on_toggle.clone();
            let toggle: OnPress = std::rc::Rc::new(move || {
                on_toggle();
                expanded.set(!expanded.get_untracked());
            });
            let has_summary = Derived::new(move || !summary.get().trim().is_empty());
            parent.adopt(&view! {
                col height:min-content min-width:0px {
                    stack height:{px(40.0)}px clip {
                        Dither colour:(colour) gradient:(crate::dither::Gradient::Horizontal)
                            reach:(Derived::new(move || live_reach(effort.get()))) opacity:0.3
                            seed:7 over:(ground.page)
                        row align:center min-width:0px gap:{px(10.0)}px
                            pad:(horizontal:{px(14.0)}px vertical:0px) {
                            StatusSquare status:(AgentStatus::Working) working-colour:(colour)
                            Label text:"Working" role:(TextRole::Label) weight:500
                            Ellipsis text:(preview) role:(TextRole::Label) tone:(Tone::Muted)
                            if has_summary.get() {
                                LinkButton
                                    text:(Derived::new(move || if expanded.get() { "Hide".to_owned() } else { "Peek".to_owned() }))
                                    on-press:(toggle.clone())
                            }
                        }
                    }
                    if expanded.get() && has_summary.get() {
                        col height:min-content pad:(horizontal:{px(14.0)}px vertical:{px(8.0)}px)
                            gap:{px(6.0)}px {
                            Label text:"Latest Update" role:(TextRole::Caption) tone:(Tone::Muted)
                            text font-size:{px(12.0)}px line-height:1.5 {detail.get()}
                        }
                    }
                }
            });
        },
    );
    host
}
