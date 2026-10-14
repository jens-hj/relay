//! Page chrome: the page header with its control cells, module sections
//! and notices.

use crate::marks::Mark;
use crate::prelude::*;
use crate::session_header::{column, header_cell};
use crate::tokens::{Signal, ground, px};
use mosaic::prelude::*;
use std::rc::Rc;

/// A removal action embedded at the right edge of a resource row.
#[component]
pub fn RemoveCell(
    label: UiText,
    on_press: OnPress,
    #[prop(default = crate::Live::fixed(false))] disabled: crate::Live<bool>,
) -> Element {
    use crate::tokens::{srgb_alpha, status};
    use mosaic::core::theme::color;
    use mosaic::widgets::Visual;
    let body = view! {
        row width:max-content height:min-content gap:{px(8.0)}px align:center {}
    };
    let accessible = label.clone();
    let cell = view! {
        button @click:{on_press()} width:max-content height:fill shrink:0 radius:0px
            pad:(horizontal:{px(14.0)}px vertical:0px) gap:{px(8.0)}px align:center
            label:{accessible.get()} disabled:{disabled.get()} {
            { parent.adopt(&body); }
        }
    };
    let interaction = cell.interaction();
    cell.visual_dyn(move || {
        let red = color(status.failed);
        let amount = if interaction.pressed() { 0.16 } else { 0.08 };
        let fill = if interaction.hovered() || interaction.pressed() {
            red.alpha(srgb_alpha(red, color(ground.page), amount))
        } else {
            Color::TRANSPARENT
        };
        let mut visual = Visual::new()
            .fill(fill)
            .stroke(StrokeSpec::new(px(1.0), color(ground.rule)).edges(StrokeEdges::LEFT));
        if interaction.focus_visible() {
            visual = visual.stroke(StrokeSpec::new(px(1.0), color(ground.ink)).offset(-px(1.0)));
        }
        visual
    });
    let tint = move || {
        color(if interaction.hovered() || interaction.pressed() {
            status.failed
        } else {
            ground.ink
        })
    };
    let icon = crate::icons::icon_mark(crate::icons::mark.remove, 14.0);
    icon.font_color_dyn(tint);
    body.adopt(&icon);
    let word = view! {
        text font-family:monospace font-size:{px(11.5)}px font-weight:500 {label.get()}
    };
    word.font_color_dyn(tint);
    body.adopt(&word);
    cell
}

/// A full-height control cell for a [`PageHeader`]: an optional mark, the
/// label, and an optional key hint.
#[component]
pub fn HeaderCell(
    label: String,
    on_press: OnPress,
    #[prop(optional)] mark: Option<Mark>,
    #[prop(optional)] keys: Option<String>,
) -> Element {
    let mut parts = Vec::new();
    if let Some(each) = mark {
        parts.push(crate::marks::mark(each, 14.0, ground.ink));
    }
    parts.push(crate::marks::sized_text(
        label.clone().into(),
        12.0,
        500,
        ground.ink,
    ));
    if let Some(keys) = keys {
        parts.push(view! {
            Kbd keys:(keys)
        });
    }
    header_cell(label, on_press, false, false, column(parts))
}

/// A page header: the path in muted mono over the serif title, and its
/// control cells (children, usually [`HeaderCell`]s) flush right at full
/// height.
#[component]
pub fn PageHeader(
    title: UiText,
    /// Optional mark beside the page title.
    #[prop(optional)]
    icon: Option<SvgToken>,
    #[prop(optional)] path: Vec<String>,
    /// A short muted line above the title, for what kind of page it is.
    #[prop(optional)]
    eyebrow: Option<UiText>,
    /// Height in logical pixels before UI scale.
    #[prop(default = 72.0)]
    height: f32,
    /// Keeps `height` as given rather than scaling it, for strips level
    /// with native window chrome.
    #[prop(default = true)]
    scaled: bool,
    #[prop(default = SerifSize::Large)] title_size: SerifSize,
    /// Accessible name; the title by default.
    #[prop(optional)]
    label: Option<String>,
    /// A cell before the titles, such as a navigation toggle.
    #[prop(optional)]
    leading: Option<Element>,
    #[prop(optional)] children: Children,
) -> Element {
    // With a leading cell the titles keep the page's 32px start: the
    // cell, the 12px gap and 20px more.
    let inset = if leading.is_some() { 20.0 } else { 0.0 };
    let titles = view! {
        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px pad:(left:{px(inset)}px) {}
    };
    if let Some(eyebrow) = eyebrow {
        titles.adopt(&view! {
            Label text:(eyebrow) role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
        });
    }
    if !path.is_empty() {
        titles.adopt(&view! {
            PathText segments:(path) trailing:true
        });
    }
    let named = title.clone();
    let title_row = view! {
        row height:min-content min-width:0px gap:{px(10.0)}px align:center {}
    };
    if let Some(icon) = icon {
        title_row.adopt(&crate::icons::icon_mark(icon, 20.0));
    }
    title_row.adopt(&view! {
        SerifTitle text:(named) size:(title_size)
    });
    titles.adopt(&title_row);
    let has_leading = leading.is_some();
    view! {
        row height:{if scaled { px(height) } else { height }}px shrink:0 gap:{px(12.0)}px
            align:center
            pad:(left:{px(if has_leading { 0.0 } else { 32.0 })}px right:0px top:0px bottom:0px)
            label:{label.clone().unwrap_or_else(|| title.get())}
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            {
                if let Some(leading) = &leading {
                    parent.adopt(leading);
                }
                parent.adopt(&titles);
            }
            row width:max-content shrink:0 {
                children
            }
        }
    }
}

/// A named page section: an icon and serif title on a raised heading band.
#[component]
pub fn Module(
    title: String,
    /// A fitting icon for the named section.
    #[prop(optional)]
    icon: Option<SvgToken>,
    /// A muted note at the head's right end.
    #[prop(optional)]
    note: Option<UiText>,
    /// Controls at the head's right end, after the note.
    #[prop(optional)]
    head_end: Option<Element>,
    /// Accessible name of the head.
    #[prop(optional)]
    head_label: Option<String>,
    #[prop(optional)] children: Children,
) -> Element {
    let head = view! {
        SerifTitle text:(title.clone()) size:(SerifSize::Small)
    };
    let has_end = head_end.is_some();
    view! {
        col height:min-content shrink:0 label:(title)
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            el width:fill height:{px(1.0)}px shrink:0 fill:ground.rule-strong nohit {}
            row height:{px(42.0)}px shrink:0 fill:ground.panel-raised gap:{px(12.0)}px align:center
                pad:(left:{px(18.0)}px right:{px(if has_end { 0.0 } else { 18.0 })}px top:0px bottom:0px)
                label:(head_label.clone().unwrap_or_default())
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                row width:max-content height:min-content gap:{px(8.0)}px align:center {
                    {
                        if let Some(icon) = icon {
                            parent.adopt(&crate::icons::icon_mark(icon, 16.0));
                        }
                        parent.adopt(&head);
                    }
                }
                row width:1fr min-width:0px height:min-content justify:end {
                    {
                        if let Some(note) = &note {
                            parent.adopt(&view! {
                                Ellipsis text:(note.clone()) role:(TextRole::Label) tone:(Tone::Muted)
                            });
                        }
                    }
                }
                {
                    if let Some(end) = &head_end {
                        parent.adopt(end);
                    }
                }
            }
            col height:min-content {
                children
            }
        }
    }
}

/// What a notice reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NoticeTone {
    /// Plain information, in ink.
    #[default]
    Info,
    /// A failure, in red.
    Failure,
}

/// An action embedded in a notice: its label and handler.
pub type NoticeAction = (String, OnPress);

/// A ruled notice: its text (wrapping), an optional muted detail line,
/// and its actions (Retry, Rebase…) as embedded cells in a row beneath
/// that wraps when the pane is narrow.
#[component]
pub fn Notice(
    text: UiText,
    #[prop(default = NoticeTone::Info)] tone: NoticeTone,
    #[prop(optional)] actions: Vec<NoticeAction>,
    /// A second, muted line, such as the request being retried.
    #[prop(optional)]
    detail: Option<UiText>,
) -> Element {
    let tint = match tone {
        NoticeTone::Info => Tone::Ink,
        NoticeTone::Failure => Tone::Signal(Signal::Failed),
    };
    let body = view! {
        col height:min-content gap:{px(4.0)}px
            pad:(horizontal:{px(18.0)}px vertical:{px(11.0)}px) {}
    };
    let line = view! {
        row height:min-content gap:{px(8.0)}px align:center {}
    };
    if tone == NoticeTone::Failure {
        line.adopt(&view! {
            StatusSquare status:(AgentStatus::Failed)
        });
    }
    line.adopt(&view! {
        Label text:(text.clone()) role:(TextRole::Label) tone:(tint)
    });
    body.adopt(&line);
    if let Some(detail) = detail {
        body.adopt(&view! {
            Label text:(detail) role:(TextRole::Caption) tone:(Tone::Muted)
        });
    }
    let strip = view! {
        col height:min-content shrink:0 label:"Notice"
            stroke:(width:{px(1.0)} color:ground.hairline edges:(top bottom)) {}
    };
    strip.adopt(&body);
    if !actions.is_empty() {
        let cells = view! {
            grid height:min-content gap:0px
                cols:{GridTracks::auto_fit(GridTrack::minmax(px(150.0).into(), GridTrack::fr(1.0)))}
                stroke:(width:{px(1.0)} color:ground.hairline edges:top) {}
        };
        for (index, (label, press)) in actions.into_iter().enumerate() {
            let rule = if index == 0 { Rule::None } else { Rule::Left };
            cells.adopt(&view! {
                col height:{px(34.0)}px {
                    CellButton label:(label.clone()) on-press:(press) rule:(rule) {
                        Label text:(label) role:(TextRole::Label) weight:500
                    }
                }
            });
        }
        strip.adopt(&cells);
    }
    strip
}

/// One entry of a [`CommandPalette`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Command {
    pub id: String,
    pub title: String,
    /// Key hint ("⇧S"); hidden when empty.
    pub keys: String,
    /// The kind ("Navigate", "Create"), read as the row's description.
    pub detail: String,
    /// The kind's icon, at the row's left.
    pub icon: Option<SvgToken>,
}

impl Command {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Command {
            id: id.into(),
            title: title.into(),
            keys: String::new(),
            detail: String::new(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: SvgToken) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn keys(mut self, keys: impl Into<String>) -> Self {
        self.keys = keys.into();
        self
    }
}

crate::value::live_value!(Vec<Command>);
crate::value::live_value!(Tone);

/// The commands whose titles contain every word of `query`, in order of
/// the list (case-insensitive).
pub fn filter_commands(commands: &[Command], query: &str) -> Vec<Command> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    commands
        .iter()
        .filter(|command| {
            let title = command.title.to_lowercase();
            words.iter().all(|word| title.contains(word))
        })
        .cloned()
        .collect()
}

/// Called with a command's id.
pub type OnCommand = Rc<dyn Fn(String)>;

/// A command palette: a query field over a ruled list of matching
/// commands with their key hints. Up and Down move the highlight, Enter
/// or a click runs a command.
#[component]
pub fn CommandPalette(
    commands: Live<Vec<Command>>,
    query: State<String>,
    on_run: OnCommand,
    /// Width in logical pixels before UI scale.
    #[prop(default = 560.0)]
    width: f32,
    /// A head with this title and, given `on_close`, an Esc cell.
    #[prop(optional)]
    title: Option<String>,
    #[prop(optional)] on_close: Option<OnPress>,
    #[prop(default = "Close".to_string())] close_label: String,
    /// Accessible name and hint of the query field.
    #[prop(default = "Command".to_string())]
    field_label: String,
    #[prop(default = "Type a command".to_string())] placeholder: String,
    /// The list scrolls past this height (logical pixels before UI scale).
    #[prop(default = Live::fixed(420.0))]
    max_height: Live<f32>,
    /// A foot with the match count and the keys.
    #[prop(default = false)]
    footer: bool,
    /// Accessible name of the panel.
    #[prop(optional)]
    label: Option<String>,
    /// Focuses the query field when built.
    #[prop(default = false)]
    autofocus: bool,
) -> Element {
    let highlighted = State::new(0usize);
    let shown = {
        let commands = commands.clone();
        Derived::new(move || filter_commands(&commands.get(), &query.get()))
    };
    Effect::new(move || {
        let _ = query.get();
        highlighted.set(0);
    });
    let list = view! {
        col height:min-content role:list label:"Commands" {}
    };
    {
        let run = on_run.clone();
        list.switch(
            move || (shown.get(), highlighted.get()),
            move |parent, (commands, highlight)| {
                if commands.is_empty() {
                    parent.adopt(&view! {
                        row height:{px(36.0)}px pad:(horizontal:{px(16.0)}px vertical:0px)
                            align:center {
                            Label text:"No matching commands" role:(TextRole::Label)
                                tone:(Tone::Muted)
                        }
                    });
                }
                for (index, command) in commands.iter().cloned().enumerate() {
                    let run = run.clone();
                    let id = command.id.clone();
                    let press: OnPress = Rc::new(move || run(id.clone()));
                    let selected = index == *highlight;
                    let keys = command.keys.clone();
                    let detail = command.detail.clone();
                    let about = (!detail.is_empty()).then(|| UiText::from(detail.clone()));
                    let glyph = view! {
                        row width:{px(16.0)}px height:{px(16.0)}px shrink:0
                            font-color:(if selected { ground.ink } else { ground.muted }) {}
                    };
                    if let Some(token) = command.icon {
                        glyph.adopt(&crate::icons::icon_mark(token, 16.0));
                    }
                    // The icon stands for the category column: an image named by it.
                    if !detail.is_empty() {
                        glyph.role(mosaic::widgets::Role::Image);
                        glyph.label(detail.clone());
                    }
                    let row = view! {
                        col height:{px(36.0)}px shrink:0
                            fill:(if selected { ground.selected } else { ground.panel_raised }) {
                            CellButton label:(command.title.clone()) on-press:(press)
                                rule:(Rule::Top) start:true description:(about) {
                                row height:min-content gap:{px(12.0)}px align:center {
                                    {
                                        if command.icon.is_some() || !detail.is_empty() {
                                            parent.adopt(&glyph);
                                        }
                                    }
                                    row width:1fr min-width:0px height:min-content {
                                        Ellipsis text:(command.title.clone()) role:(TextRole::Label)
                                    }
                                    {
                                        if !keys.is_empty() {
                                            parent.adopt(&view! {
                                                Kbd keys:(keys.clone())
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    };
                    parent.adopt(&row);
                }
            },
        );
    }
    let field = view! {
        input width:fill height:{px(44.0)}px radius:0px pad:(horizontal:{px(16.0)}px vertical:0px)
            fill:(Color::TRANSPARENT) stroke:(width:0px color:ground.rule) font-family:monospace
            font-size:{px(13.0)}px font-color:ground.ink label:(field_label)
            placeholder:(placeholder) focused { fill:ground.hover } query
    };
    let escape = on_close.clone();
    field.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let count = shown.get_untracked().len();
        match event.key {
            Key::ArrowDown if count > 0 => {
                highlighted.set((highlighted.get_untracked() + 1).min(count - 1));
                ctx.stop_propagation();
            }
            Key::ArrowUp => {
                highlighted.set(highlighted.get_untracked().saturating_sub(1));
                ctx.stop_propagation();
            }
            Key::Enter => {
                if let Some(command) = shown.get_untracked().get(highlighted.get_untracked()) {
                    on_run(command.id.clone());
                }
                ctx.stop_propagation();
            }
            Key::Escape => {
                if let Some(close) = &escape {
                    close();
                    ctx.stop_propagation();
                }
            }
            _ => {}
        }
    });
    let panel = view! {
        col width:{px(width)}px max-width:100% height:min-content shrink:0 fill:ground.panel-raised
            stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {}
    };
    if let Some(label) = label {
        panel.label(label);
    }
    if let Some(title) = title {
        let head = view! {
            row height:{px(40.0)}px shrink:0 align:center
                stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
                row width:1fr min-width:0px height:min-content
                    pad:(horizontal:{px(16.0)}px vertical:0px) {
                    SerifTitle text:(title.clone()) size:(SerifSize::Small)
                }
            }
        };
        if let Some(close) = on_close.clone() {
            head.adopt(&view! {
                row width:{px(52.0)}px shrink:0 {
                    CellButton label:(close_label.clone()) on-press:(close) rule:(Rule::Left) {
                        Kbd keys:"Esc"
                    }
                }
            });
        }
        panel.adopt(&head);
    }
    panel.adopt(&view! {
        row height:min-content align:center
            stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
            {
                parent.adopt(&field);
            }
        }
    });
    // The list keeps its own height up to `max-height`, then scrolls.
    let extent = State::new(0.0f32);
    list.on_layout(move |rect| extent.set(rect.size.height));
    let scroller = view! {
        col height:min-content shrink:0 {
            scroll {
                { parent.adopt(&list); }
            } as results
            {
                results.root().label("Command results");
                results.root().style_dyn(move || {
                    Style::stack()
                        .width(Dimension::Fill)
                        .height(extent.get().min(px(max_height.get())))
                        .basis(Dimension::Auto)
                        .grow(0.0)
                        .shrink(0.0)
                });
            }
        }
    };
    panel.adopt(&scroller);
    if autofocus {
        field.focus();
    }
    if footer {
        panel.adopt(&view! {
            row height:{px(30.0)}px shrink:0 align:center justify:between
                pad:(horizontal:{px(16.0)}px vertical:0px)
                stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                Label
                    text:(Derived::new(move || {
                    let n = shown.get().len();
                    format!("{n} {}", if n == 1 { "command" } else { "commands" })
                }))
                    role:(TextRole::Caption) tone:(Tone::Muted)
                Label text:"↑ ↓ move · Enter run · Esc close" role:(TextRole::Caption)
                    tone:(Tone::Muted)
            }
        });
    }
    panel
}

/// A header readout: a muted key above its value, as one ruled cell of a
/// [`PageHeader`]; children (inline controls) follow the value.
#[component]
pub fn ReadoutCell(
    key: String,
    value: UiText,
    /// Places the label beside its value for short summary strips.
    #[prop(default = false)]
    inline: bool,
    /// The value's colour, for readings that carry a signal; ink by default.
    #[prop(default = Live::fixed(Tone::Ink))]
    tone: Live<Tone>,
    #[prop(optional)] children: Children,
) -> Element {
    if inline {
        return view! {
            row width:max-content max-width:{px(320.0)}px min-width:0px shrink:0 align:center
                gap:{px(8.0)}px pad:(horizontal:{px(14.0)}px vertical:{px(4.0)}px)
                label:(key.clone()) stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                Label text:(key) role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
                row width:1fr min-width:0px height:min-content clip {
                    text text-wrap:none font-family:monospace
                        font-size:{px(TextRole::Body.size())}px font-weight:500
                        font-color:{mosaic::core::theme::color(tone.get().token())} {value.get()}
                }
                children
            }
        };
    }
    view! {
        col width:max-content max-width:{px(320.0)}px min-width:0px shrink:0 justify:center
            gap:{px(4.0)}px pad:(horizontal:{px(14.0)}px vertical:0px) label:(key.clone())
            stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
            Label text:(key.clone()) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                caps:true
            row height:min-content gap:{px(8.0)}px align:center {
                row width:max-content min-width:0px height:min-content clip {
                    text text-wrap:none font-family:monospace
                        font-size:{px(TextRole::Body.size())}px font-weight:500
                        font-color:{mosaic::core::theme::color(tone.get().token())} {value.get()}
                }
                children
            }
        }
    }
}
