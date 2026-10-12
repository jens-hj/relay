//! Board surfaces: columns, issue cards, the issue inspector frame, the
//! permission matrix and harness cards.

use super::form::{OnIndex, Segments, SegmentsProps};
use crate::marks::{Mark, mark, sized_text};
use crate::prelude::*;
use crate::tokens::{Signal, ground, px};
use mosaic::prelude::*;
use std::rc::Rc;

/// A board column: a 40px head with the name and count, and its cards
/// (children) divided by hairlines, with a full-height rule on the right.
#[component]
pub fn BoardColumn(
    name: String,
    count: Live<usize>,
    /// Width in logical pixels before UI scale.
    #[prop(default = 300.0)]
    width: f32,
    #[prop(optional)] children: Children,
) -> Element {
    view! {
        col width:{px(width)}px shrink:0 label:(name.clone())
            stroke:(width:{px(1.0)} color:ground.rule edges:right) {
            row height:{px(40.0)}px shrink:0 gap:{px(8.0)}px align:center
                pad:(horizontal:{px(14.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Ellipsis text:(name.clone()) role:(TextRole::Label) weight:(Weight::SEMIBOLD)
                text font-family:monospace font-size:{px(11.0)}px font-color:ground.muted shrink:0
                    {count.get().to_string()}
            }
            scroll {
                children
            }
        }
    }
}

/// What an issue card shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct IssueView {
    /// "#31".
    pub identifier: String,
    pub title: String,
    /// The linked agent's state; no square without one.
    pub status: Option<AgentStatus>,
    pub labels: Vec<String>,
    pub selected: bool,
}

/// One issue on a board: status square, identifier chip and title on one
/// line, labels as faint chips beneath; a hairline under it rather than a
/// frame around it.
#[component]
pub fn IssueCard(issue: IssueView, #[prop(optional)] on_open: Option<OnPress>) -> Element {
    let press = on_open.unwrap_or_else(|| Rc::new(|| {}));
    let labels = view! {
        row height:min-content gap:{px(6.0)}px align:center clip {}
    };
    for name in &issue.labels {
        let name = name.clone();
        labels.adopt(&view! {
            row width:max-content height:min-content shrink:0
                pad:(horizontal:{px(6.0)}px vertical:{px(3.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
                text font-family:monospace font-size:{px(10.5)}px font-color:ground.faint (name)
            }
        });
    }
    let head = view! {
        row height:min-content gap:{px(8.0)}px align:center {}
    };
    if let Some(status) = issue.status {
        head.adopt(&view! {
            StatusSquare status:(status)
        });
    }
    if !issue.identifier.is_empty() {
        head.adopt(&sized_text(
            issue.identifier.clone().into(),
            11.0,
            500,
            ground.muted,
        ));
    }
    head.adopt(&view! {
        Ellipsis text:(issue.title.clone()) role:(TextRole::Label) weight:(Weight::MEDIUM)
    });
    let card = view! {
        button @click:{press()} width:fill height:min-content radius:0px
            pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px) label:(issue.title.clone())
            fill:{if issue.selected {
                mosaic::core::theme::color(ground.selected)
            } else {
                Color::TRANSPARENT
            }}
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) hover { fill:ground.hover }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            col height:min-content gap:{px(8.0)}px {
                {
                    parent.adopt(&head);
                    if !issue.labels.is_empty() {
                        parent.adopt(&labels);
                    }
                }
            }
        }
    };
    card
}

/// The issue inspector frame: 392px with a rule on its left, a head with
/// the identifier, the serif title and a Close cell, and the body
/// (children) scrolling below.
#[component]
pub fn IssueInspector(
    identifier: String,
    title: UiText,
    #[prop(optional)] on_close: Option<OnPress>,
    #[prop(default = 392.0)] width: f32,
    #[prop(optional)] children: Children,
) -> Element {
    let head = view! {
        row height:min-content gap:{px(12.0)}px align:start
            pad:(left:{px(18.0)}px right:{px(12.0)}px top:{px(14.0)}px bottom:{px(14.0)}px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    let named = title.clone();
    head.adopt(&view! {
        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px {
            text font-family:monospace font-size:{px(11.0)}px font-weight:500
                font-color:ground.muted (identifier.clone())
            SerifTitle text:(named) size:(SerifSize::Medium)
        }
    });
    if let Some(close) = on_close {
        head.adopt(&view! {
            IconCell label:"Close" on-press:(close) {
                {
                    parent.adopt(&mark(Mark::Cross, 12.0, ground.ink));
                }
            }
        });
    }
    view! {
        col width:{px(width)}px shrink:0 fill:ground.panel label:{title.get()}
            stroke:(width:{px(1.0)} color:ground.rule edges:left) {
            {
                parent.adopt(&head);
            }
            col height:1fr min-height:0px {
                scroll {
                    children
                }
            }
        }
    }
}

/// A permission choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}

impl Permission {
    pub const ALL: [Self; 3] = [Self::Allow, Self::Ask, Self::Deny];

    pub fn word(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

/// One capability in a [`PermissionMatrix`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PermissionRow {
    /// "Write to the database".
    pub action: String,
    pub choice: Permission,
    /// Whether a request under this rule is waiting for you right now; only
    /// then is "ask" amber.
    pub pending: bool,
}

/// Called with a row index and its new choice.
pub type OnPermission = Rc<dyn Fn(usize, Permission)>;

/// Capabilities against allow / ask / deny segments, one row each.
#[component]
pub fn PermissionMatrix(entries: Vec<PermissionRow>, on_change: OnPermission) -> Element {
    let host = view! {
        col height:min-content role:list label:"Permissions" {}
    };
    for (index, row) in entries.into_iter().enumerate() {
        let change = on_change.clone();
        let pick: OnIndex = Rc::new(move |choice| change(index, Permission::ALL[choice]));
        let selected = Permission::ALL
            .iter()
            .position(|each| *each == row.choice)
            .unwrap_or(0);
        let tone = (row.pending && row.choice == Permission::Ask).then(|| Signal::NeedsYou.token());
        let options: Vec<String> = Permission::ALL
            .iter()
            .map(|each| each.word().to_string())
            .collect();
        host.adopt(&view! {
            row height:{px(44.0)}px shrink:0 gap:{px(12.0)}px align:center
                pad:(horizontal:{px(14.0)}px vertical:0px) label:(row.action.clone())
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Ellipsis text:(row.action.clone()) role:(TextRole::Label)
                Segments options:(options) selected:(selected) on-select:(pick) chosen-tone:(tone)
            }
        });
    }
    host
}

/// Whether a harness is signed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HarnessAuth {
    SignedIn,
    SignedOut,
    /// Not checked yet; nothing is shown.
    Unknown,
}

/// A connected harness: glyph, name, version, and whether it is signed in.
#[component]
pub fn HarnessCard(
    harness: Harness,
    /// Hidden when empty.
    #[prop(optional)]
    version: String,
    #[prop(default = HarnessAuth::Unknown)] auth: HarnessAuth,
    #[prop(optional)] on_press: Option<OnPress>,
) -> Element {
    let name = harness.name().to_string();
    let press = on_press.unwrap_or_else(|| Rc::new(|| {}));
    let details = view! {
        col width:1fr min-width:0px height:min-content gap:{px(4.0)}px {
            text font-family:monospace font-size:{px(12.5)}px font-weight:600 (name.clone())
        }
    };
    if !version.is_empty() {
        details.adopt(&sized_text(version.into(), 11.0, 400, ground.muted));
    }
    let state = match auth {
        HarnessAuth::SignedIn => Some(view! {
            row width:max-content height:min-content gap:{px(6.0)}px align:center {
                {
                    parent.adopt(&mark(Mark::Check, 12.0, ground.ink));
                }
                Label text:"Signed in" role:(TextRole::Label)
            }
        }),
        HarnessAuth::SignedOut => Some(view! {
            row width:max-content height:min-content gap:{px(6.0)}px align:center {
                StatusSquare status:(AgentStatus::Queued)
                Label text:"Signed out" role:(TextRole::Label) tone:(Tone::Muted)
            }
        }),
        HarnessAuth::Unknown => None,
    };
    let card = view! {
        button @click:{press()} width:fill height:{px(56.0)}px radius:0px
            pad:(horizontal:{px(14.0)}px vertical:0px) gap:{px(12.0)}px align:center
            fill:(Color::TRANSPARENT) label:(name)
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) hover { fill:ground.hover }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            HarnessGlyph harness:(harness) size:20.0
            {
                parent.adopt(&details);
                if let Some(state) = &state {
                    parent.adopt(state);
                }
            }
        }
    };
    card
}
