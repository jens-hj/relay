//! The gallery's content: every primitive in every state, with the design
//! pack's example data. Shared by the example window and the offscreen
//! screenshot test.

#[path = "d2.rs"]
pub(crate) mod d2;
use mosaic::prelude::*;
use relay_ui::prelude::*;
use relay_ui::tokens::{ModelFamily, Signal, ThemeVariant, ToolKind, ground, px};
use std::rc::Rc;

/// Switches the installed theme variant.
pub type SetTheme = Rc<dyn Fn(ThemeVariant)>;

/// The whole gallery page.
pub fn gallery(set_theme: SetTheme) -> Element {
    view! {
        col fill:ground.page font-color:ground.ink font-family:monospace {
            theme_bar(set_theme.clone())
            scroll width:fill height:1fr {
                col height:min-content gap:{px(28.0)}px
                    pad:(horizontal:{px(36.0)}px vertical:{px(28.0)}px) {
                    col height:min-content gap:{px(6.0)}px {
                        Label text:"RELAY UI · FOUNDATIONS" role:(TextRole::Caption)
                            tone:(Tone::Muted)
                        row height:min-content gap:{px(10.0)}px align:end {
                            SerifTitle text:"The" size:(SerifSize::Large)
                            SerifTitle text:"primitives" size:(SerifSize::Large) italic:true
                                tone:(Tone::Muted)
                        }
                    }
                    type_section()
                    status_section()
                    effort_section()
                    frame_section()
                    dither_section()
                    cells_section()
                    overlay_section()
                    d2::conversation()
                    d2::composer_states()
                    d2::tool_board()
                }
            }
        }
    }
}

fn heading(text: &str) -> Element {
    let text = text.to_uppercase();
    view! {
        col height:min-content gap:{px(10.0)}px {
            Label text:(text) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
            Hairline
        }
    }
}

fn theme_bar(set_theme: SetTheme) -> Element {
    let bar = view! {
        RuledRow {
            Cell {
                Label text:"Theme" role:(TextRole::Label) tone:(Tone::Muted)
            }
        }
    };
    for variant in ThemeVariant::ALL {
        let set = set_theme.clone();
        let press: OnPress = Rc::new(move || set(variant));
        let cell = view! {
            CellButton label:(variant.name()) on-press:(press) rule:(Rule::Left) {
                Label text:(variant.name()) role:(TextRole::Label) weight:500
            }
        };
        bar.adopt(&cell);
    }
    view! {
        col height:{px(44.0)}px shrink:0 fill:ground.panel
            stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
            {
                parent.adopt(&bar);
            }
        }
    }
}

fn row_of(items: Vec<Element>, gap: f32) -> Element {
    let row = view! {
        row height:min-content gap:{px(gap)}px align:center {}
    };
    for item in items {
        row.adopt(&item);
    }
    row
}

fn caption(text: &str) -> Element {
    view! {
        Label text:(text) role:(TextRole::Caption) tone:(Tone::Muted)
    }
}

fn type_section() -> Element {
    let roles = [
        (TextRole::Micro, "micro 9.5"),
        (TextRole::Caption, "caption 10.5"),
        (TextRole::Label, "label 11.5"),
        (TextRole::Body, "body 12.5"),
        (TextRole::BodyLarge, "body-large 13"),
    ]
    .into_iter()
    .map(|(role, name)| {
        view! {
            Label text:(name) role:(role)
        }
    })
    .collect();
    let tones = [
        (Tone::Ink, "ink"),
        (Tone::Muted, "muted"),
        (Tone::Faint, "faint"),
        (Tone::Signal(Signal::Live), "live"),
        (Tone::Signal(Signal::NeedsYou), "needs you"),
        (Tone::Signal(Signal::Failed), "failed"),
        (Tone::Signal(Signal::FullSteam), "full steam"),
    ]
    .into_iter()
    .map(|(tone, name)| {
        view! {
            Label text:(name) tone:(tone) weight:500
        }
    })
    .collect();
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Type")
            row_of(roles, 24.0)
            row_of(tones, 18.0)
            row height:min-content gap:{px(24.0)}px align:end {
                SerifTitle text:"Needs you" size:(SerifSize::Small) italic:true
                    tone:(Tone::Signal(Signal::NeedsYou))
                SerifTitle text:"Agents" size:(SerifSize::Medium)
                SerifTitle text:"API migration" size:(SerifSize::Large)
                row width:max-content height:min-content gap:{px(6.0)}px align:center {
                    Kbd keys:"P"
                    Kbd keys:"⇧S"
                }
            }
            row height:min-content gap:{px(24.0)}px align:center {
                col width:{px(220.0)}px height:min-content gap:{px(6.0)}px
                    stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)})
                    pad:{px(8.0)}px {
                    caption("Ellipsis at 204px")
                    Ellipsis text:"Release 2.4 / API migration / Backfill script / Verify rows"
                        role:(TextRole::Label) tone:(Tone::Muted)
                    Ellipsis text:"Fits in full" role:(TextRole::Label)
                }
                row width:max-content height:{px(120.0)}px gap:{px(18.0)}px align:center {
                    VerticalLabel text:"CODEX"
                    VerticalLabel text:"CLAUDE CODE"
                    caption("Vertical labels, bottom to top")
                }
            }
        }
    }
}

fn status_section() -> Element {
    let squares = AgentStatus::ALL
        .into_iter()
        .map(|status| {
            view! {
                row width:max-content height:min-content gap:{px(6.0)}px align:center {
                    StatusSquare status:(status)
                    Label text:(status.word()) role:(TextRole::Label) tone:(Tone::Muted)
                }
            }
        })
        .collect();
    let glyphs = Harness::ALL
        .into_iter()
        .map(|harness| {
            view! {
                row width:max-content height:min-content gap:{px(7.0)}px align:center {
                    HarnessGlyph harness:(harness)
                    Label text:(harness.name()) role:(TextRole::Label)
                }
            }
        })
        .collect();
    let large = Harness::ALL
        .into_iter()
        .map(|harness| {
            view! {
                HarnessGlyph harness:(harness) size:24.0
            }
        })
        .collect();
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Status squares and harness glyphs")
            row_of(squares, 22.0)
            row_of(glyphs, 22.0)
            row_of(large, 16.0)
        }
    }
}

fn ledger(family: ModelFamily, levels: u8, current: u8, has_max: bool) -> Element {
    let name = if has_max && current >= levels {
        "max".to_string()
    } else {
        ["minimal", "low", "medium", "high", "xhigh"][current as usize].to_string()
    };
    let total = levels + u8::from(has_max);
    let step = format!("{:02}/{:02}", current + 1, total);
    let title = format!(
        "{} · {}",
        family.name(),
        if has_max { "max-capable" } else { "5-step" }
    );
    let value = State::new(current);
    let select: relay_ui::effort::OnEffort = Rc::new(move |index| value.set(index));
    view! {
        col width:max-content height:min-content gap:{px(8.0)}px {
            caption(&title)
            col width:{px(176.0)}px height:min-content gap:{px(8.0)}px pad:{px(9.0)}px
                fill:ground.panel stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {
                row height:{px(22.0)}px align:center justify:between
                    pad:(horizontal:{px(8.0)}px vertical:0px)
                    stroke:(width:{px(1.0)} color:ground.rule-strong offset:{px(-1.0)}) {
                    Label text:"EFFORT" role:(TextRole::Micro) tone:(Tone::Muted) weight:500
                    row width:max-content height:min-content gap:{px(6.0)}px align:end {
                        Label text:(name) role:(TextRole::Label) weight:600
                        Label text:(step) role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
                EffortTrack levels:(levels) current:(value) has-max:(has_max)
                    colour:(family.token()) on-select:(Some(select))
            }
        }
    }
}

fn effort_section() -> Element {
    let ledgers = vec![
        ledger(ModelFamily::Sol, 5, 4, false),
        ledger(ModelFamily::Opus, 5, 3, true),
        ledger(ModelFamily::Opus, 5, 4, true),
        ledger(ModelFamily::Opus, 5, 5, true),
    ];
    let compact = ModelFamily::ALL
        .into_iter()
        .enumerate()
        .map(|(index, family)| {
            let has_max = index == 1;
            view! {
                row width:max-content height:min-content gap:{px(8.0)}px align:center {
                    Label text:(format!("{} 5.5", family.name())) role:(TextRole::Label)
                        tone:(Tone::Muted)
                    EffortTrack levels:5u8 current:(index as u8 + 1) has-max:(has_max)
                        colour:(family.token()) size:(TrackSize::Compact)
                }
            }
        })
        .collect();
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Effort tracks")
            row_of(ledgers, 24.0)
            row height:min-content gap:{px(24.0)}px align:center {
                col width:max-content height:min-content gap:{px(8.0)}px {
                    caption("Vertical")
                    row width:max-content height:min-content gap:{px(16.0)}px {
                        col width:max-content height:{px(170.0)}px {
                            EffortTrack levels:5u8 current:4u8 colour:(ModelFamily::Sol.token())
                                orientation:(Orientation::Vertical)
                        }
                        col width:max-content height:{px(170.0)}px {
                            EffortTrack levels:5u8 current:5u8 has-max:true
                                colour:(ModelFamily::Opus.token())
                                orientation:(Orientation::Vertical)
                        }
                    }
                }
                col width:max-content height:min-content gap:{px(10.0)}px {
                    caption("Compact (tree rows)")
                    row_of(compact, 20.0)
                }
            }
        }
    }
}

fn frame_section() -> Element {
    let cuts = [
        (Cut::Small, "small 10"),
        (Cut::Card, "card 12"),
        (Cut::Block, "block 14"),
    ]
    .into_iter()
    .map(|(cut, name)| {
        view! {
            col width:{px(150.0)}px height:min-content {
                CutCorner cut:(cut) {
                    col height:min-content pad:{px(14.0)}px {
                        Label text:(name) role:(TextRole::Label) tone:(Tone::Muted)
                    }
                }
            }
        }
    })
    .collect();
    let approve: OnPress = Rc::new(|| {});
    let review: OnPress = Rc::new(|| {});
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Cut corners")
            row_of(cuts, 18.0)
            col width:{px(356.0)}px height:min-content {
                CutCorner cut:(Cut::Card) edge:(Signal::NeedsYou.token()) edge-opacity:0.32 {
                    stack height:min-content {
                        Wash signal:(Signal::NeedsYou) opacity:0.3
                        col height:min-content gap:{px(8.0)}px pad:{px(15.0)}px {
                            row height:min-content gap:{px(8.0)}px align:center {
                                StatusSquare status:(AgentStatus::Waiting)
                                HarnessGlyph harness:(Harness::ClaudeCode) size:12.0
                                Label text:"Write migrations" weight:600
                                el width:1fr height:1px {}
                                Label text:"Sonnet 5.5" role:(TextRole::Caption) tone:(Tone::Muted)
                            }
                            Ellipsis text:"Release 2.4 / API migration" role:(TextRole::Label)
                                tone:(Tone::Muted)
                            row height:min-content gap:{px(14.0)}px align:center {
                                PrimaryButton text:"Approve" on-press:(approve)
                                LinkButton text:"Review diff" on-press:(review)
                            }
                        }
                    }
                }
            }
        }
    }
}

fn wash_row(level: u8) -> Element {
    view! {
        stack width:{px(380.0)}px height:{px(34.0)}px fill:ground.panel {
            Wash reach:(live_reach(level))
            row gap:{px(8.0)}px align:center pad:(horizontal:{px(12.0)}px vertical:0px) {
                StatusSquare status:(AgentStatus::Working)
                HarnessGlyph harness:(Harness::Codex) size:12.0
                Label text:(format!("Effort level {level}")) weight:500
                el width:1fr height:1px {}
                Label text:(format!("reach {:.0}%", live_reach(level) * 100.0))
                    role:(TextRole::Caption) tone:(Tone::Muted)
            }
        }
    }
}

fn usage_bar(percent: f32, family: ColorToken) -> Element {
    view! {
        row width:max-content height:min-content gap:{px(8.0)}px align:center {
            Label text:"5h" role:(TextRole::Micro) tone:(Tone::Faint)
            stack width:{px(262.0)}px height:{px(4.0)}px fill:ground.hairline {
                el width:{px(262.0 * percent / 100.0)}px height:fill {
                    Dither colour:(family) gradient:(Gradient::Usage) opacity:1.0 seed:9
                }
            }
            Label text:(format!("{percent:.0}%")) role:(TextRole::Caption) weight:600
        }
    }
}

fn dither_section() -> Element {
    let rows: Vec<Element> = (1..=6).map(wash_row).collect();
    let column = view! {
        col width:max-content height:min-content gap:{px(1.0)}px {}
    };
    for row in rows {
        column.adopt(&row);
    }
    let glows = ModelFamily::ALL
        .into_iter()
        .zip(["6.1", "5.5", "5.5", "5.5"])
        .map(|(family, version)| {
            view! {
                stack width:{px(143.0)}px height:{px(40.0)}px fill:ground.panel
                    stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) clip {
                    Dither colour:(family.token()) gradient:(Gradient::Corner) reach:0.43f32
                        opacity:1.0 seed:3 breath:(Some(Breath::GLOW))
                    row gap:{px(7.0)}px align:center pad:(horizontal:{px(12.0)}px vertical:0px) {
                        SerifTitle text:(family.name()) size:(SerifSize::Large)
                        Label text:(version) weight:600
                    }
                }
            }
        })
        .collect();
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Dither")
            row height:min-content gap:{px(28.0)}px align:start {
                col width:max-content height:min-content gap:{px(8.0)}px {
                    caption("Live rows: reach = 22% + 13% per effort level")
                    {
                        parent.adopt(&column);
                    }
                }
                col width:max-content height:min-content gap:{px(8.0)}px {
                    caption("Session header wash (0.14)")
                    stack width:{px(420.0)}px height:{px(88.0)}px fill:ground.panel {
                        Wash reach:0.87f32 opacity:0.14
                        col gap:{px(6.0)}px justify:center
                            pad:(horizontal:{px(24.0)}px vertical:0px) {
                            Label text:"Release 2.4 /" role:(TextRole::Label) tone:(Tone::Muted)
                            SerifTitle text:"API migration" size:(SerifSize::Large)
                        }
                    }
                    caption("Usage fill, densest at the value")
                    usage_bar(62.0, ground.ink)
                    usage_bar(84.0, Signal::NeedsYou.token())
                    usage_bar(97.0, Signal::Failed.token())
                    caption("Model glow, breathing")
                    row_of(glows, 10.0)
                }
            }
        }
    }
}

fn cells_section() -> Element {
    let noop: OnPress = Rc::new(|| {});
    let (pause, stop, add, compact) = (noop.clone(), noop.clone(), noop.clone(), noop.clone());
    let tools = ToolKind::ALL
        .into_iter()
        .map(|tool| {
            view! {
                row width:max-content height:min-content gap:{px(6.0)}px align:center {
                    el width:{px(9.0)}px height:{px(9.0)}px fill:(tool.token()) {}
                    Label text:(tool.name()) role:(TextRole::Label)
                        tone:(Tone::Colour(tool.token()))
                }
            }
        })
        .collect();
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Cells, rules and controls")
            row width:max-content height:{px(88.0)}px fill:ground.panel
                stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {
                RuledRow width:max-content {
                    Cell {
                        Label text:"Working" role:(TextRole::Label) weight:500
                        row width:max-content height:min-content gap:{px(4.0)}px align:center {
                            StatusSquare status:(AgentStatus::Done)
                            StatusSquare status:(AgentStatus::Waiting)
                            StatusSquare status:(AgentStatus::Working)
                            StatusSquare status:(AgentStatus::Queued)
                            Label text:"4 below" role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    }
                    CellButton label:"Pause" on-press:(pause) {
                        col width:max-content height:min-content gap:{px(8.0)}px {
                            Label text:"Pause" role:(TextRole::Label) weight:500
                            Kbd keys:"P"
                        }
                    }
                    CellButton label:"Stop" on-press:(stop) {
                        col width:max-content height:min-content gap:{px(8.0)}px {
                            Label text:"Stop" role:(TextRole::Label) weight:500
                            Kbd keys:"⇧S"
                        }
                    }
                }
            }
            row height:min-content gap:{px(14.0)}px align:center {
                IconCell label:"New agent" on-press:(add) {
                    Label text:"+" role:(TextRole::BodyLarge)
                }
                Chip text:"depth 6"
                Chip text:"pending" tone:(Some(Signal::NeedsYou.token()))
                SecondaryButton text:"Review diff" on-press:(compact)
            }
            row_of(tools, 14.0)
        }
    }
}

fn overlay_section() -> Element {
    let open = State::new(false);
    let toggle: OnPress = Rc::new(move || open.set(!open.get_untracked()));
    let content: PopoverContent = Rc::new(|| {
        view! {
            col width:{px(260.0)}px height:min-content gap:{px(10.0)}px pad:{px(16.0)}px {
                SerifTitle text:"Context"
                Label text:"124k of 200k used · Sol 6.1" role:(TextRole::Label) tone:(Tone::Muted)
                Hairline
                Label text:"Escape or a click outside closes this." role:(TextRole::Label)
            }
        }
    });
    view! {
        col height:min-content gap:{px(14.0)}px {
            heading("Overlays")
            row height:min-content gap:{px(24.0)}px align:center {
                stack width:{px(140.0)}px height:{px(48.0)}px
                    stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) {
                    CellButton label:"Context" on-press:(toggle) {
                        Label text:"Context" role:(TextRole::Label) weight:500
                    }
                    Popover open:(open) label:"Context" content:(content)
                }
                Tooltip text:"Waiting for approval · Write migrations" {
                    row width:max-content height:min-content gap:{px(6.0)}px align:center {
                        StatusSquare status:(AgentStatus::Waiting)
                        Label text:"Hover for a tooltip" role:(TextRole::Label) tone:(Tone::Muted)
                    }
                }
            }
            el width:fill height:{px(160.0)}px {}
        }
    }
}
