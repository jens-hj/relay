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
    // A press that travels (a swipe across the board) does not open the card.
    let grab: Rc<std::cell::Cell<Option<Vector2>>> = Rc::default();
    let swiped = Rc::new(std::cell::Cell::new(false));
    let tapped = swiped.clone();
    let card = view! {
        button @click:{if !tapped.replace(false) { press() }} width:fill height:min-content
            radius:0px pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px) label:(name)
            description:(about)
            fill:{if issue.selected {
                mosaic::core::theme::color(ground.selected)
            } else {
                Color::TRANSPARENT
            }}
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) hover { fill:ground.hover }
            pressed { fill:ground.pressed }
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
    card.on_pointer(move |event, _| match event.kind {
        PointerEventKind::Down(_) => {
            grab.set(Some(event.position));
            swiped.set(false);
        }
        PointerEventKind::Move => {
            if let Some(start) = grab.get() {
                let travel = event.position - start;
                if travel.x * travel.x + travel.y * travel.y > 64.0 {
                    swiped.set(true);
                }
            }
        }
        _ => {}
    });
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
    /// Before the titles in the head, such as a navigation toggle.
    #[prop(optional)]
    head_start: Option<Element>,
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
    if let Some(start) = &head_start {
        head.adopt(start);
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

    /// The chosen colour: Allow lets work run at full steam, Ask waits on
    /// you, Deny blocks it like a failure.
    pub fn token(self) -> ColorToken {
        match self {
            Self::Allow => Signal::FullSteam.token(),
            Self::Ask => Signal::NeedsYou.token(),
            Self::Deny => Signal::Failed.token(),
        }
    }
}

/// One capability in a [`PermissionMatrix`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PermissionRow {
    /// "Write to the database".
    pub action: String,
    pub choice: Permission,
    /// Whether a request under this rule is waiting for you right now; the
    /// action then reads amber.
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

crate::value::live_value!(Vec<PermissionRow>);

/// Capabilities against allow / ask / deny segments, one row each, with
/// optional on/off columns before the choice (a head row names them).
#[component]
pub fn PermissionMatrix(
    /// The rows; a reactive value updates choices and flags in place, so
    /// keyboard focus stays on the control that changed them.
    entries: Live<Vec<PermissionRow>>,
    on_change: OnPermission,
    #[prop(optional)] flag_columns: Vec<FlagColumn>,
    #[prop(optional)] on_flag: Option<OnFlag>,
    #[prop(default = "Permissions".to_string())] label: String,
    /// The segments in order with their words; allow, ask, deny by default.
    #[prop(optional)]
    choices: Vec<(Permission, String)>,
) -> Element {
    let choices: Vec<(Permission, String)> = if choices.is_empty() {
        Permission::ALL
            .iter()
            .map(|each| (*each, each.word().to_string()))
            .collect()
    } else {
        choices
    };
    let order: Rc<Vec<Permission>> = Rc::new(choices.iter().map(|(each, _)| *each).collect());
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
            row width:{px(172.0)}px height:min-content shrink:0 justify:center {
                Label text:"Permission" role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                    caps:true
            }
        });
        host.adopt(&head);
    }
    for (index, row) in entries.get_untracked().into_iter().enumerate() {
        let change = on_change.clone();
        let order = order.clone();
        let pick: OnIndex = Rc::new(move |choice| change(index, order[choice]));
        let rows = entries.clone();
        let words: Vec<Permission> = choices.iter().map(|(each, _)| *each).collect();
        let selected = Live::from(Derived::new(move || {
            let choice = rows.get().get(index).map(|row| row.choice);
            words
                .iter()
                .position(|each| Some(*each) == choice)
                .unwrap_or(0)
        }));
        let tones: Vec<ColorToken> = choices.iter().map(|(each, _)| each.token()).collect();
        let action_tone = if row.pending {
            Tone::Signal(Signal::NeedsYou)
        } else {
            Tone::Ink
        };
        let options: Vec<String> = choices.iter().map(|(_, word)| word.clone()).collect();
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
                    Ellipsis text:(row.action.clone()) role:(TextRole::Label) tone:(action_tone)
                }
            }
        };
        for (column, header) in flag_columns.iter().enumerate() {
            let rows = entries.clone();
            let on = Derived::new(move || {
                rows.get()
                    .get(index)
                    .and_then(|row| row.flags.get(column).copied())
                    .unwrap_or(false)
            });
            let flip = on_flag.clone();
            let press: OnPress = Rc::new(move || {
                if let Some(flip) = &flip {
                    flip(index, column, !on.get_untracked());
                }
            });
            let toggle = view! {
                CellButton label:(format!("{} {}", row.action, header.name)) on-press:(press)
                    rule:(Rule::Left) {
                    el width:{px(12.0)}px height:{px(12.0)}px
                        fill:{mosaic::core::theme::color(if on.get() { ground.ink } else { ground.panel })}
                        stroke:(width:{px(1.0)} color:{mosaic::core::theme::color(if on.get() { ground.ink } else { ground.rule })} offset:{px(-0.5)}) {}
                }
            };
            toggle.role(mosaic::widgets::Role::Checkbox);
            let semantic = toggle.clone();
            let effect = Effect::new(move || {
                semantic.toggled(on.get());
            });
            toggle.__hot_on_remove(move || effect.dispose());
            let cell = view! {
                row width:{px(60.0)}px shrink:0 {}
            };
            cell.adopt(&toggle);
            line.adopt(&cell);
        }
        line.adopt(&view! {
            row width:{px(172.0)}px shrink:0
                stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                Segments options:(options) selected:(selected) on-select:(pick) tones:(tones)
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

/// What one harness card shows.
#[derive(Clone)]
struct CardFace {
    harness: Harness,
    version: String,
    auth: HarnessAuth,
    status: Option<(AgentStatus, String)>,
    selected: Live<bool>,
    label: String,
    description: Option<UiText>,
}

/// The pressable card: glyph, name, version, status and sign-in state.
fn card_button(face: CardFace, press: OnPress) -> Element {
    let name = face.harness.name().to_string();
    let details = view! {
        col width:1fr min-width:0px height:min-content gap:{px(4.0)}px {
            text font-family:monospace font-size:{px(12.5)}px font-weight:600 (name.clone())
        }
    };
    if !face.version.is_empty() {
        details.adopt(&sized_text(face.version.into(), 11.0, 400, ground.muted));
    }
    // Availability reads ok / needs attention / failed: a full-steam check,
    // the amber waiting square or the red failed square, the word in the
    // same colour. Anything else (checking, queued) stays muted.
    if let Some((state, word)) = face.status {
        let tone = match state {
            AgentStatus::Failed => Tone::Signal(Signal::Failed),
            AgentStatus::Waiting => Tone::Signal(Signal::NeedsYou),
            AgentStatus::Done => Tone::Signal(Signal::FullSteam),
            _ => Tone::Muted,
        };
        let line = view! {
            row width:max-content height:min-content gap:{px(6.0)}px align:center {}
        };
        if state == AgentStatus::Done {
            line.adopt(&mark(Mark::Check, 10.0, Signal::FullSteam.token()));
        } else {
            line.adopt(&view! {
                StatusSquare status:(state)
            });
        }
        line.adopt(&view! {
            Label text:(word) role:(TextRole::Caption) tone:(tone)
        });
        details.adopt(&line);
    }
    let state = match face.auth {
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
    let about = face.description.clone();
    let selected = face.selected.clone();
    let glyph_selected = face.selected.clone();
    let harness = face.harness;
    view! {
        button @click:{press()} width:fill height:fill min-height:{px(56.0)}px radius:0px
            pad:(horizontal:{px(14.0)}px vertical:{px(10.0)}px) gap:{px(12.0)}px align:center
            fill:{if selected.get() { mosaic::core::theme::color(ground.selected) } else { Color::TRANSPARENT }}
            label:(face.label) description:{about.as_ref().map(|d| d.get()).unwrap_or_default()}
            hover { fill:ground.hover } pressed { fill:ground.pressed }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            row width:max-content height:min-content shrink:0
                font-color:{mosaic::core::theme::color(if glyph_selected.get() { ground.ink } else { ground.muted })} {
                HarnessGlyph harness:(harness) size:20.0
            }
            {
                parent.adopt(&details);
                if let Some(state) = &state {
                    parent.adopt(state);
                }
            }
        }
    }
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
    #[prop(default = Live::fixed(false))]
    selected: Live<bool>,
    /// Accessible name; the harness name by default.
    #[prop(optional)]
    label: Option<String>,
    #[prop(optional)] description: Option<UiText>,
    /// Action cells at the right (label, handler), outside the card's
    /// own control.
    #[prop(optional)]
    actions: Vec<(String, OnPress)>,
    /// Accessible names of the actions in order; their text by default.
    #[prop(optional)]
    action_labels: Vec<String>,
) -> Element {
    let face = CardFace {
        harness,
        version,
        auth,
        status,
        selected,
        label: label.unwrap_or_else(|| harness.name().to_string()),
        description,
    };
    let card = card_button(face, on_press.unwrap_or_else(|| Rc::new(|| {})));
    let host = view! {
        row height:min-content shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    host.adopt(&card);
    for (index, (text, press)) in actions.into_iter().enumerate() {
        let name = action_labels
            .get(index)
            .cloned()
            .unwrap_or_else(|| text.clone());
        host.adopt(&view! {
            row width:max-content shrink:0 {
                CellButton label:(name) on-press:(press) rule:(Rule::Left) {
                    Label text:(text) role:(TextRole::Label) weight:500
                }
            }
        });
    }
    host
}

/// One card of a [`HarnessChoices`] group.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HarnessOption {
    pub harness: Harness,
    /// Accessible name; the harness name when empty.
    pub label: String,
    /// Accessible description, such as the availability word.
    pub description: String,
    pub status: Option<(AgentStatus, String)>,
}

/// A single choice among harness cards: radio semantics, cells side by
/// side when there is room (stacked otherwise) divided by hairlines, and
/// arrow, Home and End keys moving the choice and focus.
#[component]
pub fn HarnessChoices(
    options: Vec<HarnessOption>,
    selected: Live<usize>,
    on_select: OnIndex,
    /// Accessible name of the group.
    #[prop(default = "Harness".to_string())]
    label: String,
    /// Narrowest card before the cards stack, in logical pixels.
    #[prop(default = 180.0)]
    min_width: f32,
) -> Element {
    let count = options.len();
    // Side by side the cards divide the whole width equally (one track per
    // card, so no empty trailing cell); narrower than `min_width` each,
    // they stack.
    let width = State::new(f32::MAX);
    let side = Derived::new(move || width.get() >= px(min_width) * count.max(1) as f32);
    let grid = view! {
        grid height:min-content gap:0px label:(label) role:group
            @layout:{move |rect: Rect| width.set(rect.size.width)}
            cols:{if side.get() { GridTracks::new(vec![GridTrack::fr(1.0); count.max(1)]) } else { GridTracks::new([GridTrack::fr(1.0)]) }} {}
    };
    let cards: Rc<std::cell::RefCell<Vec<Element>>> = Rc::default();
    for (index, option) in options.into_iter().enumerate() {
        let current = selected.clone();
        let chosen = Live::from(Derived::new(move || current.get() == index));
        let face = CardFace {
            harness: option.harness,
            version: String::new(),
            auth: HarnessAuth::Unknown,
            status: option.status,
            selected: chosen.clone(),
            label: if option.label.is_empty() {
                option.harness.name().to_string()
            } else {
                option.label
            },
            description: (!option.description.is_empty()).then(|| option.description.into()),
        };
        let pick = on_select.clone();
        let card = card_button(face, Rc::new(move || pick(index)));
        card.role(mosaic::widgets::Role::Radio);
        let semantic = card.clone();
        let effect = Effect::new(move || {
            semantic.toggled(chosen.get());
        });
        card.__hot_on_remove(move || effect.dispose());
        let pick = on_select.clone();
        let focus = cards.clone();
        let current = selected.clone();
        card.on_key(move |event, ctx| {
            if !matches!(event.kind, KeyEventKind::Down { .. }) || count == 0 {
                return;
            }
            let now = current.get_untracked();
            let next = match event.key {
                Key::ArrowRight | Key::ArrowDown => (now + 1) % count,
                Key::ArrowLeft | Key::ArrowUp => (now + count - 1) % count,
                Key::Home => 0,
                Key::End => count - 1,
                _ => return,
            };
            pick(next);
            let target = focus.borrow().get(next).cloned();
            if let Some(target) = target {
                target.focus();
            }
            ctx.stop_propagation();
        });
        cards.borrow_mut().push(card.clone());
        let cell = view! {
            row min-width:0px
                stroke:(width:{px(1.0)} color:ground.hairline edges:if side.get() && index > 0 { StrokeEdges::LEFT | StrokeEdges::BOTTOM } else { StrokeEdges::BOTTOM }) {}
        };
        cell.adopt(&card);
        grid.adopt(&cell);
    }
    grid
}
