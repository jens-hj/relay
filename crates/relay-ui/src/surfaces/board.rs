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
    name: UiText,
    count: Live<usize>,
    /// Width in logical pixels before UI scale.
    #[prop(default = 300.0)]
    width: f32,
    /// Shares the board's width with its sibling columns instead.
    #[prop(default = false)]
    fill: bool,
    /// Scrolls its cards; off when the whole board scrolls (stacked).
    #[prop(default = true)]
    scrolls: bool,
    /// Accessible name of the head.
    #[prop(optional)]
    head_label: Option<String>,
    #[prop(optional)] children: Children,
) -> Element {
    let head = view! {
        row height:{px(40.0)}px shrink:0 gap:{px(8.0)}px align:center
            pad:(horizontal:{px(14.0)}px vertical:0px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            Ellipsis text:(name.clone()) role:(TextRole::Label) weight:(Weight::SEMIBOLD)
            text font-family:monospace font-size:{px(11.0)}px font-color:ground.muted shrink:0
                {format!("{:02}", count.get())}
        }
    };
    if let Some(label) = head_label {
        head.label(label);
    }
    let cards = if scrolls {
        view! {
            scroll {
                col height:min-content {
                    children
                }
            }
        }
    } else {
        view! {
            col height:min-content {
                children
            }
        }
    };
    view! {
        col width:{if fill { Dimension::Fr(1.0) } else { Dimension::Px(px(width)) }} min-width:0px
            height:{if scrolls { Dimension::Fill } else { Dimension::MinContent }} shrink:0
            label:{name.get()} stroke:(width:{px(1.0)} color:ground.rule edges:right) {
            {
                parent.adopt(&head);
                parent.adopt(&cards);
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
    /// A clipped excerpt of the body; hidden when empty.
    pub preview: String,
    /// A muted note at the foot, such as the linked session count.
    pub meta: String,
    /// Accessible name and description; the title when empty.
    pub label: String,
    pub description: String,
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
    let foot = (!issue.preview.is_empty() || !issue.meta.is_empty()).then(|| {
        let block = view! {
            col height:min-content gap:{px(6.0)}px {}
        };
        if !issue.preview.is_empty() {
            let preview = issue.preview.clone();
            block.adopt(&view! {
                col height:min-content max-height:{px(34.0)}px clip label:"Preview"
                    mask:linear(to:bottom stops:(0%:#FFFFFF 62%:#FFFFFF 100%:#FFFFFF00)) {
                    text font-family:monospace font-size:{px(11.0)}px line-height:{px(17.0)}px
                        font-color:ground.muted (preview)
                }
            });
        }
        if !issue.meta.is_empty() {
            let meta = issue.meta.clone();
            block.adopt(&view! {
                Label text:(meta) role:(TextRole::Caption) tone:(Tone::Muted)
            });
        }
        block
    });
    let name = if issue.label.is_empty() {
        issue.title.clone()
    } else {
        issue.label.clone()
    };
    let about = issue.description.clone();
    let card = view! {
        button @click:{press()} width:fill height:min-content radius:0px
            pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px) label:(name) description:(about)
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
                    if let Some(foot) = &foot {
                        parent.adopt(foot);
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
    /// Takes the whole width instead (narrow windows).
    #[prop(default = false)]
    fill: bool,
    #[prop(default = "Close".to_string())] close_label: String,
    /// Accessible name of the head.
    #[prop(optional)]
    head_label: Option<String>,
    /// Under the title in the head, such as label chips.
    #[prop(optional)]
    head_extra: Option<Element>,
    /// After the Close cell, at the head's right end.
    #[prop(optional)]
    head_end: Option<Element>,
    /// Fixed under the scrolling body, such as the inspector's actions.
    #[prop(optional)]
    footer: Option<Element>,
    #[prop(optional)] children: Children,
) -> Element {
    let head = view! {
        row height:min-content gap:0px align:start
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    if let Some(label) = head_label {
        head.label(label);
    }
    let named = title.clone();
    let titles = view! {
        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px
            pad:(left:{px(18.0)}px right:{px(12.0)}px top:{px(14.0)}px bottom:{px(14.0)}px) {
            text font-family:monospace font-size:{px(11.0)}px font-weight:500
                font-color:ground.muted (identifier.clone())
            SerifTitle text:(named) size:(SerifSize::Medium)
        }
    };
    if let Some(extra) = head_extra {
        titles.adopt(&extra);
    }
    head.adopt(&titles);
    if let Some(close) = on_close {
        head.adopt(&view! {
            row width:{px(44.0)}px height:{px(44.0)}px shrink:0 {
                CellButton label:(close_label) on-press:(close) rule:(Rule::Left) {
                    {
                        parent.adopt(&mark(Mark::Cross, 12.0, ground.ink));
                    }
                }
            }
        });
    }
    if let Some(end) = head_end {
        head.adopt(&end);
    }
    let frame = view! {
        col width:{if fill { Dimension::Fill } else { Dimension::Px(px(width)) }} shrink:0
            fill:ground.panel label:{title.get()}
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
    };
    if let Some(footer) = footer {
        frame.adopt(&footer);
    }
    frame
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
    /// Names the row's choice for assistive technology ("Plan permission");
    /// the action when empty.
    pub name: String,
    /// One on/off value per [`FlagColumn`] of the matrix.
    pub flags: Vec<bool>,
}

/// An on/off column of a [`PermissionMatrix`], before the choice.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlagColumn {
    /// The short head ("Resp.").
    pub head: String,
    /// What a cell turns on, after the action in its label
    /// ("responsibility" → "Plan responsibility").
    pub name: String,
}

/// Called with a row index, a flag column index and the new value.
pub type OnFlag = Rc<dyn Fn(usize, usize, bool)>;

/// Called with a row index and its new choice.
pub type OnPermission = Rc<dyn Fn(usize, Permission)>;

/// Capabilities against allow / ask / deny segments, one row each, with
/// optional on/off columns before the choice (a head row names them).
#[component]
pub fn PermissionMatrix(
    entries: Vec<PermissionRow>,
    on_change: OnPermission,
    #[prop(optional)] flag_columns: Vec<FlagColumn>,
    #[prop(optional)] on_flag: Option<OnFlag>,
    #[prop(default = "Permissions".to_string())] label: String,
) -> Element {
    let host = view! {
        col height:min-content role:list label:(label) {}
    };
    if !flag_columns.is_empty() {
        let head = view! {
            row height:{px(30.0)}px shrink:0 align:center pad:(left:{px(14.0)}px right:0px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                row width:1fr min-width:0px height:min-content {
                    Label text:"Action" role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                        caps:true
                }
            }
        };
        for column in &flag_columns {
            let name = column.head.clone();
            head.adopt(&view! {
                row width:{px(60.0)}px height:min-content shrink:0 justify:center {
                    Label text:(name) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                        caps:true
                }
            });
        }
        head.adopt(&view! {
            row width:{px(172.0)}px height:min-content shrink:0 pad:(left:{px(12.0)}px) {
                Label text:"Permission" role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                    caps:true
            }
        });
        host.adopt(&head);
    }
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
        let group = if row.name.is_empty() {
            row.action.clone()
        } else {
            row.name.clone()
        };
        let line = view! {
            row height:{px(44.0)}px shrink:0 gap:0px align:center pad:(left:{px(14.0)}px right:0px)
                label:(row.action.clone())
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                row width:1fr min-width:0px height:min-content {
                    Ellipsis text:(row.action.clone()) role:(TextRole::Label)
                }
            }
        };
        for (column, header) in flag_columns.iter().enumerate() {
            let on = row.flags.get(column).copied().unwrap_or(false);
            let flip = on_flag.clone();
            let press: OnPress = Rc::new(move || {
                if let Some(flip) = &flip {
                    flip(index, column, !on);
                }
            });
            let cell = view! {
                row width:{px(60.0)}px height:fill shrink:0 {
                    CellButton label:(format!("{} {}", row.action, header.name)) on-press:(press)
                        rule:(Rule::Left) {
                        el width:{px(12.0)}px height:{px(12.0)}px
                            fill:{mosaic::core::theme::color(if on { ground.ink } else { ground.panel })}
                            stroke:(width:{px(1.0)} color:{mosaic::core::theme::color(if on { ground.ink } else { ground.rule })} offset:{px(-0.5)}) {}
                    }
                }
            };
            line.adopt(&cell);
        }
        line.adopt(&view! {
            row width:max-content height:fill shrink:0 align:center pad:(horizontal:{px(12.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                Segments options:(options) selected:(selected) on-select:(pick) chosen-tone:(tone)
                    name:(group)
            }
        });
        host.adopt(&line);
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
    /// A state square and word under the name, such as an availability
    /// check; hidden without it.
    #[prop(optional)]
    status: Option<(AgentStatus, String)>,
    /// The current choice among several cards.
    #[prop(default = false)]
    selected: bool,
    /// Accessible name; the harness name by default.
    #[prop(optional)]
    label: Option<String>,
    #[prop(optional)] description: Option<UiText>,
    /// Action cells at the right (label, handler), outside the card's
    /// own control.
    #[prop(optional)]
    actions: Vec<(String, OnPress)>,
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
    if let Some((state, word)) = status {
        let tone = match state {
            AgentStatus::Failed => Tone::Signal(Signal::Failed),
            AgentStatus::Waiting => Tone::Signal(Signal::NeedsYou),
            _ => Tone::Muted,
        };
        details.adopt(&view! {
            row width:max-content height:min-content gap:{px(6.0)}px align:center {
                StatusSquare status:(state)
                Label text:(word) role:(TextRole::Caption) tone:(tone)
            }
        });
    }
    let named = label.unwrap_or_else(|| name.clone());
    let about = description.clone();
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
        button @click:{press()} width:fill height:min-content min-height:{px(56.0)}px radius:0px
            pad:(horizontal:{px(14.0)}px vertical:{px(10.0)}px) gap:{px(12.0)}px align:center
            fill:{if selected { mosaic::core::theme::color(ground.selected) } else { Color::TRANSPARENT }}
            label:(named) description:{about.as_ref().map(|d| d.get()).unwrap_or_default()}
            hover { fill:ground.hover }
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
    let host = view! {
        row height:min-content shrink:0 stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    host.adopt(&card);
    for (label, press) in actions {
        host.adopt(&view! {
            row width:max-content height:fill shrink:0 {
                CellButton label:(label.clone()) on-press:(press) rule:(Rule::Left) {
                    Label text:(label) role:(TextRole::Label) weight:500
                }
            }
        });
    }
    host
}
