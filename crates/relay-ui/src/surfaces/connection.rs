//! The connection cell at the sidebar foot. It uses ground colours only
//! (red when offline): violet and amber mean live work and "needs you".

use crate::marks::{Mark, paint_mark};
use crate::prelude::*;
use crate::tokens::{Signal, ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{Shape, Stroke};
use std::rc::Rc;

/// The state of the connection to the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Connection {
    Connected,
    Connecting,
    Offline,
}

crate::value::live_value!(Connection);
crate::value::live_value!(Option<u32>);

impl Connection {
    pub fn word(self) -> &'static str {
        match self {
            Connection::Connected => "Connected",
            Connection::Connecting => "Connecting",
            Connection::Offline => "Offline",
        }
    }
}

/// Lit latency bars out of four: 4 under 50 ms, 3 under 150, 2 under 400,
/// otherwise 1.
pub fn latency_bars(milliseconds: u32) -> u8 {
    match milliseconds {
        0..50 => 4,
        50..150 => 3,
        150..400 => 2,
        _ => 1,
    }
}

/// Details for the connection popover; empty fields are hidden.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ConnectionInfo {
    pub endpoint: String,
    pub transport: String,
    pub protocol: String,
    pub revision: String,
}

fn paint_state(scene: &mut mosaic::render::Scene, rect: Rect, state: Connection) {
    let unit = rect.size.width / 12.0;
    match state {
        Connection::Connected => paint_mark(scene, rect, Mark::Check, color(ground.ink)),
        Connection::Connecting => {
            let square = Rect::from_xywh(
                rect.origin.x + 1.5 * unit,
                rect.origin.y + 1.5 * unit,
                9.0 * unit,
                9.0 * unit,
            );
            scene.shape(Shape::new(square).stroke(Stroke::new(unit, color(ground.muted))));
        }
        Connection::Offline => {
            let square = Rect::from_xywh(
                rect.origin.x + unit,
                rect.origin.y + unit,
                10.0 * unit,
                10.0 * unit,
            );
            scene.shape(Shape::new(square).fill(color(Signal::Failed.token())));
            let inner = Rect::from_xywh(
                rect.origin.x + 3.0 * unit,
                rect.origin.y + 3.0 * unit,
                6.0 * unit,
                6.0 * unit,
            );
            paint_mark(scene, inner, Mark::Cross, color(ground.inverse));
        }
    }
}

/// The sidebar-foot connection cell: a state glyph (an ink check when
/// connected, a still hollow square while connecting, a red square with a
/// cross when offline), the state, latency micro-bars, and a popover with
/// the endpoint, transport, protocol and revision.
#[component]
pub fn ConnectionStatus(
    state: Live<Connection>,
    /// Round-trip latency; bars hidden without it.
    #[prop(default = Live::fixed(None))]
    latency: Live<Option<u32>>,
    #[prop(optional)] info: ConnectionInfo,
) -> Element {
    let open = State::new(false);
    let toggle = move || open.set(!open.get_untracked());
    let glyph = view! {
        el width:{px(12.0)}px height:{px(12.0)}px shrink:0 nohit {}
    };
    {
        let (target, watched) = (glyph.clone(), state.clone());
        Effect::new(move || {
            let _ = watched.get();
            target.paint_dirty();
        });
        let state = state.clone();
        glyph.paint(move |ctx| paint_state(ctx.scene, ctx.rect, state.get_untracked()));
    }
    let bars = view! {
        el width:{px(15.0)}px height:{px(10.0)}px shrink:0 nohit {}
    };
    {
        let (target, watched) = (bars.clone(), latency.clone());
        Effect::new(move || {
            let _ = watched.get();
            target.paint_dirty();
        });
        let latency = latency.clone();
        bars.paint(move |ctx| {
            let Some(ms) = latency.get_untracked() else {
                return;
            };
            let lit = latency_bars(ms);
            let unit = ctx.rect.size.height / 10.0;
            for index in 0..4u8 {
                let height = (4.0 + 2.0 * index as f32) * unit;
                let bar = Rect::from_xywh(
                    ctx.rect.origin.x + index as f32 * 4.0 * unit,
                    ctx.rect.origin.y + ctx.rect.size.height - height,
                    2.0 * unit,
                    height,
                );
                let tint = if index < lit {
                    ground.ink
                } else {
                    ground.track
                };
                ctx.scene.shape(Shape::new(bar).fill(color(tint)));
            }
        });
    }
    let (word, tone, described) = (state.clone(), state.clone(), state.clone());
    let delay = latency.clone();
    let shown = latency.clone();
    let details = info.clone();
    let summary = state.clone();
    let content: PopoverContent = Rc::new(move || {
        let rows = view! {
            col width:{px(280.0)}px height:min-content
                pad:(horizontal:{px(16.0)}px vertical:{px(12.0)}px) gap:{px(8.0)}px {
                SerifTitle text:"Connection" size:(SerifSize::Medium)
            }
        };
        let mut fields = vec![("State", summary.get_untracked().word().to_string())];
        for (name, value) in [
            ("Endpoint", &details.endpoint),
            ("Transport", &details.transport),
            ("Protocol", &details.protocol),
            ("Revision", &details.revision),
        ] {
            if !value.is_empty() {
                fields.push((name, value.clone()));
            }
        }
        if let Some(ms) = shown.get_untracked() {
            fields.push(("Latency", format!("{ms} ms")));
        }
        for (name, value) in fields {
            rows.adopt(&view! {
                row height:min-content gap:{px(12.0)}px align:center {
                    row width:{px(80.0)}px height:min-content shrink:0 {
                        Label text:(name) role:(TextRole::Label) tone:(Tone::Muted)
                    }
                    Ellipsis text:(value) role:(TextRole::Label)
                }
            });
        }
        rows
    });
    view! {
        stack height:{px(40.0)}px shrink:0 {
            button @click:{toggle()} width:fill height:fill radius:0px
                pad:(horizontal:{px(18.0)}px vertical:0px) gap:{px(10.0)}px align:center
                fill:(Color::TRANSPARENT)
                label:{
                    let word = described.get().word();
                    match delay.get() {
                        Some(ms) => format!("{word}, {ms} ms"),
                        None => word.to_string(),
                    }
                }
                stroke:(width:{px(1.0)} color:ground.rule edges:top) hover { fill:ground.hover }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                {
                    parent.adopt(&glyph);
                }
                row width:1fr min-width:0px height:min-content {
                    text font-family:monospace font-size:{px(11.5)}px font-weight:500
                        font-color:{
                            color(if tone.get() == Connection::Offline {
                                Signal::Failed.token()
                            } else {
                                ground.ink
                            })
                        }
                        {word.get().word()}
                }
                text font-family:monospace font-size:{px(10.5)}px font-color:ground.muted shrink:0
                    {latency.get().map(|ms| format!("{ms} ms")).unwrap_or_default()}
                {
                    parent.adopt(&bars);
                }
            }
            Popover open:(open) label:"Connection" content:(content)
                side:(mosaic::widgets::OverlaySide::Top)
        }
    }
}
