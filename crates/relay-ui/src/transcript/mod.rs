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
    #[prop(optional)] harness: Option<Harness>,
    #[prop(optional)] children: Children,
) -> Element {
    view! {
        row height:min-content align:start min-width:0px {
            row width:{px(128.0)}px shrink:0 height:min-content gap:{px(7.0)}px align:center
                pad:(top:{px(3.0)}px) font-color:ground.muted {
                if harness.is_some() {
                    HarnessGlyph harness:(harness.unwrap()) size:12.0
                }
                Ellipsis text:(speaker) role:(TextRole::Label) tone:(Tone::Muted) weight:500
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
        col height:min-content min-width:0px pad:(vertical:{px(14.0)}px horizontal:{px(16.0)}px)
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
/// Compatible tree anatomy until D1's AgentTreeRow is integrated.
#[component]
pub fn SpawnChildRow(child: SpawnChild) -> Element {
    let agent_status = child.status;
    let reach = child.effort.map(|e| live_reach(e.level())).unwrap_or(1.0);
    let row = view! {
        row height:{px(34.0)}px align:center gap:{px(8.0)}px
            pad:(horizontal:{px(14.0)}px vertical:0px) font-color:ground.muted {
            StatusSquare status:(child.status)
        }
    };
    if let Some(h) = child.harness {
        row.adopt(&view! {
            HarnessGlyph harness:(h) size:12.0
        });
    }
    row.adopt(&view! {
        col width:1fr min-width:0px height:min-content {
            Ellipsis text:(child.name) weight:500
                tone:(match child.status { AgentStatus::Done => Tone::Muted, AgentStatus::Waiting => Tone::Signal(Signal::NeedsYou), _ => Tone::Ink })
        }
    });
    if let Some(m) = child.model {
        row.adopt(&view! {
            Label text:(m.short_name()) role:(TextRole::Caption) tone:(Tone::Muted)
        });
        if let Some(e) = child.effort {
            row.adopt(&view! {
                EffortTrack levels:(e.levels) current:(e.selected()) has-max:(e.has_max)
                    colour:(m.hue.token()) size:(TrackSize::Compact)
            });
        }
    }
    view! {
        stack height:{px(34.0)}px clip {
            if matches!(agent_status, AgentStatus::Working | AgentStatus::Waiting) {
                Wash
                    signal:(if agent_status == AgentStatus::Waiting { Signal::NeedsYou } else { Signal::Live })
                    reach:(reach)
            }
            { parent.adopt(&row); }
        }
    }
}
#[component]
pub fn SpawnBlock(agents: Live<Vec<SpawnChild>>, on_open_tree: OnPress) -> Element {
    let count = agents.clone();
    view! {
        col height:min-content stroke:(width:{px(1.0)} color:ground.hairline) {
            row height:{px(36.0)}px align:center pad:(horizontal:{px(14.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Label text:(Derived::new(move || format!("Spawned {} agents", count.get().len())))
                    role:(TextRole::Label) weight:500
                el width:1fr height:1px {}
                LinkButton text:"Open in tree" on-press:(on_open_tree)
            }
            for (key, child) in { agents.get().into_iter().enumerate().map(|(i, c)| ((i, format!("{c:?}")), c)) } {
                { let _ = key; parent.adopt(&view! { SpawnChildRow child:(child.clone()) }); }
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
                              { if let Some(cmd) = vm.command { parent.adopt(&view! { CodeBlock lines:(vec![CodeLine { text:cmd, ..Default::default() }]) }); } }
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
#[component]
pub fn ThinkingLine(#[prop(default = Live::fixed(1u8))] effort: Live<u8>) -> Element {
    view! {
        stack height:{px(40.0)}px clip {
            Wash reach:(Derived::new(move || live_reach(effort.get()))) over:(ground.page)
            row align:center gap:{px(10.0)}px pad:(horizontal:{px(14.0)}px vertical:0px) {
                StatusSquare status:(AgentStatus::Working)
                Label text:"Thinking" role:(TextRole::Label) weight:500
            }
        }
    }
}
