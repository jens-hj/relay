//! Page chrome: the page header with its control cells, module sections
//! and notices.

use crate::marks::Mark;
use crate::prelude::*;
use crate::session_header::{column, header_cell};
use crate::tokens::{Signal, ground, px};
use mosaic::prelude::*;
use std::rc::Rc;

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
    #[prop(optional)] path: Vec<String>,
    /// Height in logical pixels before UI scale.
    #[prop(default = 72.0)]
    height: f32,
    #[prop(optional)] children: Children,
) -> Element {
    let titles = view! {
        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px {}
    };
    if !path.is_empty() {
        titles.adopt(&view! {
            PathText segments:(path) trailing:true
        });
    }
    let named = title.clone();
    titles.adopt(&view! {
        SerifTitle text:(named) size:(SerifSize::Large)
    });
    view! {
        row height:{px(height)}px shrink:0 gap:{px(12.0)}px align:center
            pad:(left:{px(32.0)}px right:0px top:0px bottom:0px) label:{title.get()}
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            {
                parent.adopt(&titles);
            }
            row width:max-content shrink:0 {
                children
            }
        }
    }
}

/// A section of a settings-like page: a head (serif, or mono caps when
/// `caps`) over a hairline, then the content.
#[component]
pub fn Module(
    title: String,
    /// Sets the head in mono caps instead of the serif.
    #[prop(default = false)]
    caps: bool,
    #[prop(optional)] children: Children,
) -> Element {
    let head = if caps {
        let text = title.to_uppercase();
        view! {
            text font-family:monospace font-size:{px(11.0)}px font-weight:500
                letter-spacing:{px(1.1)}px font-color:ground.muted (text)
        }
    } else {
        view! {
            SerifTitle text:(title.clone()) size:(SerifSize::Small)
        }
    };
    view! {
        col height:min-content shrink:0 label:(title) {
            col height:min-content
                pad:(left:{px(18.0)}px right:{px(18.0)}px top:{px(16.0)}px bottom:{px(8.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                {
                    parent.adopt(&head);
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

/// A ruled strip with one line of text and its actions (Retry, Rebase…)
/// as embedded cells at the right.
#[component]
pub fn Notice(
    text: UiText,
    #[prop(default = NoticeTone::Info)] tone: NoticeTone,
    #[prop(optional)] actions: Vec<NoticeAction>,
) -> Element {
    let tint = match tone {
        NoticeTone::Info => Tone::Ink,
        NoticeTone::Failure => Tone::Signal(Signal::Failed),
    };
    let strip = view! {
        row height:{px(40.0)}px shrink:0 align:center label:(text.get_untracked())
            stroke:(width:{px(1.0)} color:ground.hairline edges:(top bottom)) {
            row width:1fr min-width:0px gap:{px(8.0)}px align:center
                pad:(horizontal:{px(18.0)}px vertical:0px) {
                {
                    if tone == NoticeTone::Failure {
                        parent.adopt(&view! {
                            StatusSquare status:(AgentStatus::Failed)
                        });
                    }
                }
                Ellipsis text:(text.clone()) role:(TextRole::Label) tone:(tint)
            }
        }
    };
    for (label, press) in actions {
        strip.adopt(&view! {
            CellButton label:(label.clone()) on-press:(press) rule:(Rule::Left) width:max-content
                shrink:0 {
                Label text:(label) role:(TextRole::Label) weight:500
            }
        });
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
}

impl Command {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Command {
            id: id.into(),
            title: title.into(),
            keys: String::new(),
        }
    }

    pub fn keys(mut self, keys: impl Into<String>) -> Self {
        self.keys = keys.into();
        self
    }
}

crate::value::live_value!(Vec<Command>);

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
                    let row = view! {
                        col height:{px(36.0)}px shrink:0
                            fill:(if selected { ground.selected } else { ground.panel_raised }) {
                            CellButton label:(command.title.clone()) on-press:(press)
                                rule:(Rule::Top) {
                                row height:min-content gap:{px(10.0)}px align:center {
                                    Ellipsis text:(command.title.clone()) role:(TextRole::Label)
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
            font-size:{px(13.0)}px font-color:ground.ink label:"Command"
            placeholder:"Type a command" query
    };
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
            _ => {}
        }
    });
    view! {
        col width:{px(width)}px height:min-content shrink:0 fill:ground.panel-raised
            stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {
            row height:min-content align:center
                stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
                {
                    parent.adopt(&field);
                }
            }
            {
                parent.adopt(&list);
            }
        }
    }
}
