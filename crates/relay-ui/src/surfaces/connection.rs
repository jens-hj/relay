//! The embedded sidebar connection instrument. Data and actions belong to the app.
use crate::prelude::*;
use crate::tokens::{ground, px, status};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{LineCap, LineJoin, Shape, Stroke};
use mosaic::widgets::Tick;
use std::{cell::Cell, rc::Rc, time::Duration};

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
            Self::Connected => "Connected",
            Self::Connecting => "Reconnecting",
            Self::Offline => "Offline",
        }
    }
    fn token(self) -> ColorToken {
        match self {
            Self::Connected => ground.ink,
            Self::Connecting => status.needs_you,
            Self::Offline => status.failed,
        }
    }
}
/// Quarters of the ring filled by measured link quality.
pub fn latency_bars(ms: u32) -> u8 {
    match ms {
        0..80 => 4,
        80..150 => 3,
        150..300 => 2,
        _ => 1,
    }
}
/// Nearest-rank p95; no invented sample when the history is empty.
pub fn ping_p95(samples: &[u32]) -> Option<u32> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Some(sorted[(sorted.len() * 95).div_ceil(100) - 1])
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ConnectionInfo {
    pub endpoint: String,
    pub transport: String,
    pub protocol: String,
    pub revision: String,
}
crate::value::live_value!(ConnectionInfo);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionTelemetry {
    pub samples: Vec<u32>,
    pub client_revision: String,
    pub server_revision: String,
    /// The application formats wall time in its local timezone.
    pub offline_since: Option<String>,
    pub retry_in: Option<u32>,
    /// None means compatibility has not been established.
    pub compatible: Option<bool>,
    pub warning: Option<String>,
}
crate::value::live_value!(ConnectionTelemetry);
impl ConnectionTelemetry {
    pub fn summary(&self, state: Connection, latency: Option<u32>) -> String {
        match state {
            Connection::Connected => latency.map(|ms| format!("{ms} ms")).unwrap_or_default(),
            Connection::Connecting => self
                .retry_in
                .map(|seconds| format!("retry in {seconds}s"))
                .unwrap_or_default(),
            Connection::Offline => self
                .offline_since
                .as_ref()
                .map(|time| format!("since {time}"))
                .unwrap_or_default(),
        }
    }
    pub fn label(&self, state: Connection, latency: Option<u32>, open: bool) -> String {
        let summary = self.summary(state, latency);
        format!(
            "Connection: {}{}. {} details",
            state.word(),
            if summary.is_empty() {
                String::new()
            } else {
                format!(", {summary}")
            },
            if open { "Hide" } else { "Show" }
        )
    }
}
fn ping_colour(ms: u32) -> Color {
    match ms {
        0..80 => Color::from_rgb_hex(0x9BD7A9),
        80..150 => color(status.needs_you),
        _ => color(status.failed),
    }
}
fn line(
    scene: &mut mosaic::render::Scene,
    rect: Rect,
    points: &[(f32, f32)],
    width: f32,
    tint: Color,
    units: f32,
) {
    let x_unit = rect.size.width / units;
    let y_unit = rect.size.height / if units == 304.0 { 74.0 } else { units };
    let stroke = width * x_unit.min(y_unit);
    for pair in points.windows(2) {
        let (x0, y0) = (
            rect.origin.x + pair[0].0 * x_unit,
            rect.origin.y + pair[0].1 * y_unit,
        );
        let (x1, y1) = (
            rect.origin.x + pair[1].0 * x_unit,
            rect.origin.y + pair[1].1 * y_unit,
        );
        if x0 == x1 && y0 != y1 {
            scene.shape(
                Shape::new(Rect::from_xywh(
                    x0 - stroke / 2.0,
                    y0.min(y1),
                    stroke,
                    (y1 - y0).abs(),
                ))
                .fill(tint),
            );
        } else if y0 == y1 && x0 != x1 {
            scene.shape(
                Shape::new(Rect::from_xywh(
                    x0.min(x1),
                    y0 - stroke / 2.0,
                    (x1 - x0).abs(),
                    stroke,
                ))
                .fill(tint),
            );
        } else if x0 != x1 || y0 != y1 {
            let centre = rect.center();
            scene.shape(
                Shape::new(rect)
                    .path(
                        vec![
                            Vector2::new(x0 - centre.x, y0 - centre.y),
                            Vector2::new(x1 - centre.x, y1 - centre.y),
                        ],
                        false,
                    )
                    .stroke(
                        Stroke::new(stroke, tint)
                            .join(LineJoin::Miter)
                            .cap(LineCap::Butt),
                    ),
            );
        }
    }
    // Square elbows join orthogonal strokes without a gap at the corner.
    for point in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        scene.shape(
            Shape::new(Rect::from_xywh(
                rect.origin.x + point.0 * x_unit - stroke / 2.0,
                rect.origin.y + point.1 * y_unit - stroke / 2.0,
                stroke,
                stroke,
            ))
            .fill(tint),
        );
    }
}
fn portion(points: &[(f32, f32)], start: f32, end: f32) -> Vec<(f32, f32)> {
    let lengths = points
        .windows(2)
        .map(|p| ((p[1].0 - p[0].0).powi(2) + (p[1].1 - p[0].1).powi(2)).sqrt())
        .collect::<Vec<_>>();
    let total: f32 = lengths.iter().sum();
    let mut offset = 0.0;
    let mut result = Vec::new();
    for (pair, len) in points.windows(2).zip(lengths) {
        let from = (start * total - offset).max(0.0);
        let to = (end * total - offset).min(len);
        if to > from {
            for d in [from, to] {
                result.push((
                    pair[0].0 + (pair[1].0 - pair[0].0) * d / len,
                    pair[0].1 + (pair[1].1 - pair[0].1) * d / len,
                ));
            }
        }
        offset += len;
    }
    result.dedup();
    result
}
#[derive(Clone, Default)]
struct ConnectionMotion {
    ring: Rc<Cell<f32>>,
    socket: Rc<Cell<f32>>,
}
fn phase(
    element: &Element,
    period: f32,
    active: bool,
    elapsed: Rc<Cell<f32>>,
) -> (Rc<Cell<f32>>, Rc<Cell<bool>>) {
    let visible = Rc::new(Cell::new(false));
    if active {
        let (time, seen, target) = (elapsed.clone(), visible.clone(), element.clone());
        element.animate(move |dt| {
            if !seen.replace(false) {
                return Tick::Sleep(Duration::from_millis(250));
            }
            time.set((time.get() + dt.as_secs_f32() / period) % 1.0);
            target.paint_dirty();
            Tick::Sleep(Duration::from_millis(33))
        });
    }
    (elapsed, visible)
}
fn ring(state: Connection, latency: Option<u32>, sending: bool, motion: Rc<Cell<f32>>) -> Element {
    let glyph = view! {
        el width:{px(20.0)}px height:{px(20.0)}px shrink:0 nohit {}
    };
    let (time, visible) = phase(
        &glyph,
        1.6,
        state == Connection::Connecting || (state == Connection::Connected && sending),
        motion,
    );
    glyph.paint(move |ctx| {
        visible.set(true);
        let path = [
            (5.5, 14.75),
            (1.25, 14.75),
            (1.25, 1.25),
            (14.75, 1.25),
            (14.75, 14.75),
            (10.5, 14.75),
        ];
        let tint = color(state.token());
        match state {
            Connection::Connected => {
                line(ctx.scene, ctx.rect, &path, 2.2, color(ground.outline), 16.0);
                if let Some(ms) = latency {
                    line(
                        ctx.scene,
                        ctx.rect,
                        &portion(&path, 0.0, f32::from(latency_bars(ms)) / 4.0),
                        2.2,
                        if sending { tint.alpha(0.35) } else { tint },
                        16.0,
                    );
                }
                if sending {
                    for (start, end) in [
                        (time.get(), (time.get() + 0.18).min(1.0)),
                        (0.0, (time.get() + 0.18 - 1.0).max(0.0)),
                    ] {
                        if end > start {
                            line(
                                ctx.scene,
                                ctx.rect,
                                &portion(&path, start, end),
                                2.2,
                                color(ground.ink),
                                16.0,
                            );
                        }
                    }
                }
            }
            Connection::Offline => {
                line(ctx.scene, ctx.rect, &path, 2.2, tint.alpha(0.35), 16.0);
                line(
                    ctx.scene,
                    ctx.rect,
                    &[(5.0, 11.0), (11.0, 5.0)],
                    1.6,
                    tint,
                    16.0,
                );
            }
            Connection::Connecting => {
                let square = [
                    (1.25, 1.25),
                    (14.75, 1.25),
                    (14.75, 14.75),
                    (1.25, 14.75),
                    (1.25, 1.25),
                ];
                for begin in [time.get(), (time.get() + 0.5) % 1.0] {
                    line(
                        ctx.scene,
                        ctx.rect,
                        &portion(&square, begin, (begin + 0.42).min(1.0)),
                        2.2,
                        tint.alpha(0.55),
                        16.0,
                    );
                    if begin + 0.42 > 1.0 {
                        line(
                            ctx.scene,
                            ctx.rect,
                            &portion(&square, 0.0, begin + 0.42 - 1.0),
                            2.2,
                            tint.alpha(0.55),
                            16.0,
                        );
                    }
                }
            }
        }
        if state != Connection::Offline {
            let u = ctx.rect.size.width / 16.0;
            ctx.scene.shape(
                Shape::new(Rect::from_xywh(
                    ctx.rect.origin.x + 5.5 * u,
                    ctx.rect.origin.y + 5.5 * u,
                    5.0 * u,
                    5.0 * u,
                ))
                .fill(tint),
            );
        }
    });
    glyph
}
fn topology(
    state: Connection,
    info: &ConnectionInfo,
    telemetry: &ConnectionTelemetry,
    motion: Rc<Cell<f32>>,
) -> Element {
    let drawing = view! {
        el width:fill height:fill nohit {}
    };
    let (time, visible) = phase(&drawing, 1.2, state == Connection::Connected, motion);
    drawing.paint(move |ctx| {
        visible.set(true);
        let u = ctx.rect.size.width / 304.0;
        let v = ctx.rect.size.height / 74.0;
        let tint = color(state.token());
        for x in [0.5, 239.5] {
            ctx.scene.shape(
                Shape::new(Rect::from_xywh(
                    ctx.rect.origin.x + x * u,
                    ctx.rect.origin.y + 18.5 * v,
                    64.0 * u,
                    40.0 * v,
                ))
                .stroke(Stroke::new(u, color(ground.outline))),
            );
            let side = if x < 100.0 { x } else { 297.5 };
            ctx.scene.shape(
                Shape::new(Rect::from_xywh(
                    ctx.rect.origin.x + side * u,
                    ctx.rect.origin.y + 18.5 * v,
                    6.0 * u,
                    40.0 * v,
                ))
                .fill(tint),
            );
        }
        line(
            ctx.scene,
            ctx.rect,
            &[(66.0, 25.0), (238.0, 25.0)],
            1.0,
            tint,
            304.0,
        );
        line(
            ctx.scene,
            ctx.rect,
            &[(72.0, 22.0), (66.0, 25.0), (72.0, 28.0)],
            1.0,
            tint,
            304.0,
        );
        line(
            ctx.scene,
            ctx.rect,
            &[(232.0, 22.0), (238.0, 25.0), (232.0, 28.0)],
            1.0,
            tint,
            304.0,
        );
        if state == Connection::Connected {
            let shift = time.get() * 20.0;
            for i in -2..20 {
                let x = 66.0 + i as f32 * 10.0 + shift;
                let start = x.max(66.0);
                let end = (x + 6.0).min(238.0);
                if end > start {
                    line(
                        ctx.scene,
                        ctx.rect,
                        &[(start, 53.0), (end, 53.0)],
                        2.0,
                        tint,
                        304.0,
                    );
                }
            }
        } else {
            line(
                ctx.scene,
                ctx.rect,
                &[(66.0, 53.0), (238.0, 53.0)],
                2.0,
                tint,
                304.0,
            );
        }
        if state == Connection::Offline {
            line(
                ctx.scene,
                ctx.rect,
                &[(132.0, 59.0), (172.0, 19.0)],
                5.0,
                color(ground.panel),
                304.0,
            );
            line(
                ctx.scene,
                ctx.rect,
                &[(132.0, 59.0), (172.0, 19.0)],
                2.0,
                tint,
                304.0,
            );
        }
    });
    let host = view! {
        stack height:{px(74.0)}px label:"Connection topology" {}
    };
    host.adopt(&drawing);
    let client = telemetry.client_revision.clone();
    let server = telemetry.server_revision.clone();
    let protocol = State::new(info.protocol.clone());
    let socket = if state == Connection::Connected {
        "WebSocket · open"
    } else {
        "WebSocket · closed"
    };
    host.adopt(&view! {
        row align:start {
            col width:{px(64.0)}px shrink:0 height:min-content
                pad:(left:{px(14.0)}px right:0px top:{px(23.0)}px bottom:0px) gap:{px(4.0)}px {
                Label text:"Client" role:(TextRole::Caption) weight:600
                Ellipsis text:(client) role:(TextRole::Caption) tone:(Tone::Muted)
            }
            col width:1fr min-width:0px height:min-content
                pad:(left:{px(14.0)}px right:0px top:{px(7.0)}px bottom:0px) gap:0px {
                Label text:"HTTP" role:(TextRole::Caption) tone:(Tone::Muted)
                el height:{px(11.0)}px shrink:0 {}
                if state != Connection::Offline && !protocol.get().is_empty() {
                    row height:{px(16.0)}px justify:center {
                        col width:{px(40.0)}px shrink:0 {
                            CutCorner cut:(Cut::Tiny) edge:(state.token()) {
                                row height:{px(16.0)}px align:center justify:center {
                                    Label text:(protocol.get()) role:(TextRole::Caption) weight:600
                                }
                            }
                        }
                    }
                } else {
                    el height:{px(16.0)}px {}
                }
                el height:{px(10.0)}px shrink:0 {}
                Label text:(socket) role:(TextRole::Caption) tone:(Tone::Muted)
            }
            col width:{px(64.0)}px shrink:0 height:min-content
                pad:(left:{px(7.0)}px right:{px(6.0)}px top:{px(23.0)}px bottom:0px)
                gap:{px(4.0)}px {
                Label text:"Server" role:(TextRole::Caption) weight:600
                Ellipsis text:(server) role:(TextRole::Caption) tone:(Tone::Muted)
            }
        }
    });
    host
}
fn endpoint_text(endpoint: String) -> Element {
    let display = endpoint
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(&endpoint)
        .to_owned();
    let width = State::new(f32::MAX);
    let host = view! {
        row width:1fr min-width:0px height:min-content clip label:(endpoint)
            @layout:{move |rect:Rect|width.set(rect.size.width)} {}
    };
    let shown = Derived::new(move || {
        if let Some(ui) = crate::attached() {
            crate::text::fit_text(
                &mut ui.fonts().borrow_mut(),
                &display,
                &crate::text::mono_style(11.5, Weight::REGULAR),
                width.get(),
            )
        } else {
            display.clone()
        }
    });
    // Span colours are resolved here, so the theme's ink and muted colours
    // are part of the key: a theme switch rebuilds the spans.
    host.switch(
        move || (shown.get(), color(ground.ink), color(ground.muted)),
        move |parent, (value, ink, muted)| {
            let split = value.find('/').unwrap_or(value.len());
            let spans = vec![
                mosaic::widgets::TextSpan::new(&value[..split], TextStyle::inherited().color(*ink)),
                mosaic::widgets::TextSpan::new(
                    &value[split..],
                    TextStyle::inherited().color(*muted),
                ),
            ];
            parent.adopt(&view! {
                text font-size:{px(11.5)}px text-wrap:none (spans)
            });
        },
    );
    host
}

fn panel(
    state: Connection,
    latency: Option<u32>,
    info: &ConnectionInfo,
    telemetry: &ConnectionTelemetry,
    copy: Option<OnPress>,
    motion: &ConnectionMotion,
) -> Element {
    let glyph = ring(state, latency, false, motion.ring.clone());
    let drawing = topology(state, info, telemetry, motion.socket.clone());
    let info = info.clone();
    let telemetry = telemetry.clone();
    let description = format!("{} · {} · {}", info.transport, info.protocol, info.revision);
    let samples = telemetry.samples.clone();
    let bars = view! {
        el width:1fr min-width:0px height:{px(22.0)}px label:"Recent connection pings" {}
    };
    bars.paint(move |ctx| {
        let step = ctx.rect.size.width / 20.0;
        for (i, ms) in samples
            .iter()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            let height = (4.0 + *ms as f32 * 0.086).min(22.0) * ctx.rect.size.height / 22.0;
            ctx.scene.shape(
                Shape::new(Rect::from_xywh(
                    ctx.rect.origin.x + i as f32 * step,
                    ctx.rect.origin.y + ctx.rect.size.height - height,
                    step * 6.8 / 8.8,
                    height,
                ))
                .fill(ping_colour(*ms).alpha(if i + 1 == samples.len().min(20) {
                    1.0
                } else {
                    0.55
                })),
            );
        }
    });
    let current = if state == Connection::Connected {
        latency
            .map(|ms| format!("{ms} ms"))
            .unwrap_or_else(|| "no pings".into())
    } else {
        "no pings".into()
    };
    let secondary = if state == Connection::Connected {
        ping_p95(&telemetry.samples)
            .map(|ms| format!("p95 {ms}"))
            .unwrap_or_default()
    } else {
        telemetry
            .samples
            .last()
            .map(|ms| format!("last {ms} ms"))
            .unwrap_or_default()
    };
    let compatibility = telemetry
        .warning
        .clone()
        .unwrap_or_else(|| match telemetry.compatible {
            Some(true) => "client and server revisions compatible".into(),
            Some(false) => "client and server revisions incompatible".into(),
            None => "compatibility not established".into(),
        });
    let compatibility_tone = match telemetry.compatible {
        Some(false) => Tone::Colour(status.failed),
        None => Tone::Colour(status.needs_you),
        Some(true) => Tone::Muted,
    };
    let endpoint = info.endpoint.clone();
    let host = view! {
        col height:min-content shrink:0 label:"Connection details"
            stroke:(width:{px(1.0)} color:ground.rule edges:top) {
            col height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(16.0)}px)
                gap:{px(14.0)}px {
                row height:min-content align:center gap:{px(8.0)}px {
                    SerifTitle text:"Connection" size:(SerifSize::Large)
                    el width:1fr {}
                    {parent.adopt(&glyph);}
                    Label text:(state.word()) role:(TextRole::Label)
                        tone:(Tone::Colour(state.token())) weight:600
                }
                {parent.adopt(&drawing);}
                row height:min-content align:end gap:{px(12.0)}px {
                    col width:{px(52.0)}px shrink:0 {
                        Label text:"latency" role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                    {parent.adopt(&bars);}
                    col width:max-content shrink:0 height:min-content align:end gap:{px(3.0)}px {
                        text font-size:{px(12.0)}px font-weight:600
                            font-color:{if state==Connection::Connected {latency.map(ping_colour).unwrap_or(color(status.failed))} else {color(status.failed)}}
                            {current.clone()}
                        Label text:(secondary) role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
                row height:min-content align:center gap:{px(6.0)}px {
                    row width:{px(12.0)}px height:{px(12.0)}px shrink:0 align:center justify:center
                        font-color:(compatibility_tone.token()) {
                        {
                            if telemetry.compatible == Some(true) {
                                parent.adopt(&crate::icons::icon_mark(crate::icons::mark.check, 12.0));
                            } else {
                                parent.adopt(&view! {
                                    Label text:"!" role:(TextRole::Caption) tone:(compatibility_tone)
                                        weight:600
                                });
                            }
                        }
                    }
                    text font-size:{px(10.5)}px
                        font-color:(match telemetry.compatible {Some(false)=>status.failed,None=>status.needs_you,_=>ground.muted})
                        {compatibility.clone()}
                }
            }
            row height:{px(40.0)}px align:center
                stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                row width:1fr min-width:0px align:center gap:{px(10.0)}px
                    pad:(left:{px(18.0)}px right:{px(8.0)}px) {
                    row width:{px(14.0)}px height:{px(14.0)}px shrink:0 font-color:ground.muted {
                        {parent.adopt(&crate::icons::icon_mark(crate::icons::mark.link, 14.0));}
                    }
                    endpoint_text(endpoint.clone())
                }
                row width:{px(40.0)}px shrink:0 {
                    {parent.adopt(&copy_cell(copy.clone()));}
                }
            }
        }
    };
    host.description(description);
    host
}
#[component]
pub fn ConnectionStatus(
    state: Live<Connection>,
    #[prop(default = Live::fixed(false))] sending: Live<bool>,
    #[prop(default=Live::fixed(None))] latency: Live<Option<u32>>,
    #[prop(default=Live::fixed(ConnectionInfo::default()))] info: Live<ConnectionInfo>,
    #[prop(default=Live::fixed(ConnectionTelemetry::default()))] telemetry: Live<
        ConnectionTelemetry,
    >,
    #[prop(optional)] on_copy: Option<OnPress>,
    #[prop(default = false)] initially_open: bool,
) -> Element {
    let open = State::new(initially_open);
    let host = view! {
        col height:min-content shrink:0 {}
    };
    let details = view! {
        col height:min-content shrink:0 {}
    };
    let (panel_state, panel_latency, panel_info, panel_telemetry) = (
        state.clone(),
        latency.clone(),
        info.clone(),
        telemetry.clone(),
    );
    let panel_motion = ConnectionMotion::default();
    details.switch(
        move || {
            let mut telemetry = panel_telemetry.get();
            telemetry.retry_in = None;
            telemetry.offline_since = None;
            (
                panel_state.get(),
                panel_latency.get(),
                panel_info.get(),
                telemetry,
                open.get(),
            )
        },
        move |parent, value| {
            let (state, latency, info, telemetry, expanded) = value.clone();
            if expanded {
                parent.adopt(&panel(
                    state,
                    latency,
                    &info,
                    &telemetry,
                    on_copy.clone(),
                    &panel_motion,
                ));
            }
        },
    );
    host.adopt(&details);
    let glyph = view! {
        col width:{px(20.0)}px height:{px(20.0)}px shrink:0 {}
    };
    let (glyph_state, glyph_latency, glyph_sending) =
        (state.clone(), latency.clone(), sending.clone());
    let banner_motion = Rc::new(Cell::new(0.0));
    glyph.switch(
        move || (glyph_state.get(), glyph_latency.get(), glyph_sending.get()),
        move |parent, (state, latency, sending)| {
            parent.adopt(&ring(*state, *latency, *sending, banner_motion.clone()));
        },
    );
    let word_state = state.clone();
    let word = Derived::new(move || word_state.get().word().to_owned());
    let tone_state = state.clone();
    let tone = Derived::new(move || tone_state.get().token());
    let (summary_state, summary_latency, summary_telemetry) =
        (state.clone(), latency.clone(), telemetry.clone());
    let summary_sending = sending.clone();
    let summary = Derived::new(move || {
        if summary_state.get() == Connection::Connected && summary_sending.get() {
            return "sending…".into();
        }
        summary_telemetry
            .get()
            .summary(summary_state.get(), summary_latency.get())
    });
    let label_sending = sending.clone();
    let label = Derived::new(move || {
        if state.get() == Connection::Connected && label_sending.get() {
            return format!(
                "Connection: Connected, sending…. {} details",
                if open.get() { "Hide" } else { "Show" }
            );
        }
        telemetry
            .get()
            .label(state.get(), latency.get(), open.get())
    });
    host.adopt(&view! {
        button @click:{open.set(!open.get_untracked())} width:fill height:{px(44.0)}px shrink:0
            radius:0px pad:(horizontal:{px(18.0)}px vertical:0px) gap:{px(10.0)}px align:center
            fill:(Color::TRANSPARENT) label:{label.get()}
            stroke:(width:{px(1.0)} color:ground.rule edges:top) hover { fill:ground.hover }
            focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            {parent.adopt(&glyph);}
            text font-size:{px(12.0)}px font-weight:600 font-color:{color(tone.get())} {word.get()}
            el width:1fr {}
            text font-size:{px(12.0)}px font-color:ground.muted {summary.get()}
        }
    });
    host
}

/// How long the copy cell shows its confirming check before turning back.
pub const COPIED_FOR: Duration = Duration::from_millis(1600);

/// The endpoint row's copy cell. Pressing it copies, then the copy icon
/// crossfades to a green check over a green-tinted cell for
/// [`COPIED_FOR`], and back.
fn copy_cell(copy: Option<OnPress>) -> Element {
    let copied = State::new(false);
    let slot: Rc<std::cell::RefCell<Option<Element>>> = Rc::default();
    let driver = slot.clone();
    let press = move || {
        if let Some(copy) = &copy {
            copy();
        }
        copied.set(true);
        if let Some(cell) = driver.borrow().as_ref() {
            let mut left = COPIED_FOR;
            cell.animate(move |dt| {
                left = left.saturating_sub(dt);
                if left.is_zero() {
                    copied.set(false);
                    Tick::Done
                } else {
                    Tick::Sleep(left)
                }
            });
        }
    };
    let green = status.full_steam;
    let cell = view! {
        button @click:{press()} width:fill height:fill radius:0px pad:0px align:center
            justify:center label:{if copied.get() { "Endpoint copied" } else { "Copy endpoint" }}
            fill:{if copied.get() {
                let tint = color(green);
                tint.alpha(crate::tokens::srgb_alpha(tint, color(ground.panel), 0.14))
            } else {
                Color::TRANSPARENT
            }}
            stroke:(width:{px(1.0)} color:ground.rule edges:left) hover { fill:ground.hover }
            focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            stack width:{px(14.0)}px height:{px(14.0)}px shrink:0 nohit {
                row font-color:ground.ink opacity:{if copied.get() { 0.0 } else { 1.0 }}
                    transition:(opacity:ease(150ms)) {
                    {parent.adopt(&crate::icons::icon_mark(crate::icons::mark.copy, 14.0));}
                }
                row font-color:(green) opacity:{if copied.get() { 1.0 } else { 0.0 }}
                    transition:(opacity:ease(150ms)) {
                    {parent.adopt(&crate::icons::icon_mark(crate::icons::mark.check, 14.0));}
                }
            }
        }
    };
    *slot.borrow_mut() = Some(cell.clone());
    cell
}
