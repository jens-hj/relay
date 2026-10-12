//! Context: the square-ring gauge in the header and the breakdown panel
//! behind it.

#[cfg(test)]
mod tests;

use crate::marks::{Mark, mark, paint_mark, sized_text};
use crate::prelude::*;
use crate::tokens::{ContextPart, ground, px, thresholds};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{Scene, Shape};
use mosaic::widgets::{OverlayAlign, OverlaySide};
use std::rc::Rc;

/// One supplied part of the context breakdown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextShare {
    pub part: ContextPart,
    pub tokens: u64,
}

/// The context window of one session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextView {
    /// Tokens in the window.
    pub used: u64,
    /// The window size.
    pub window: u64,
    /// Compactions so far.
    pub compactions: u32,
    /// The breakdown; only supplied parts are drawn.
    pub parts: Vec<ContextShare>,
    /// "Sol 6.1"; hidden when empty.
    pub model: String,
    /// The compaction history line ("Compacted twice, last at 182k");
    /// hidden when empty.
    pub history: String,
}

impl ContextView {
    pub fn new(used: u64, window: u64, compactions: u32) -> Self {
        ContextView {
            used,
            window,
            compactions,
            ..Default::default()
        }
    }

    /// Percent of the window used.
    pub fn percent(&self) -> f32 {
        if self.window == 0 {
            0.0
        } else {
            (self.used as f64 / self.window as f64 * 100.0) as f32
        }
    }

    pub fn level(&self) -> thresholds::Level {
        thresholds::level(self.percent())
    }

    /// "free", "compacts soon" or "compacting next turn".
    pub fn status(&self) -> &'static str {
        match self.level() {
            thresholds::Level::Room => "free",
            thresholds::Level::Amber => "compacts soon",
            thresholds::Level::Red => "compacting next turn",
        }
    }

    /// "Context 62 percent used, 124k of 200k, compacted 2 times".
    pub fn label(&self) -> String {
        format!(
            "Context {:.0} percent used, {} of {}, compacted {} times",
            self.percent(),
            tokens(self.used),
            tokens(self.window),
            self.compactions
        )
    }
}

/// The name of a breakdown part.
pub fn part_name(part: ContextPart) -> &'static str {
    match part {
        ContextPart::System => "System + tools",
        ContextPart::FilesRead => "Files read",
        ContextPart::Conversation => "Conversation",
        ContextPart::SubAgentResults => "Sub-agent results",
        ContextPart::Compacted => "Earlier, compacted",
    }
}

/// A token count as "850", "124k" or "1.2M".
pub fn tokens(count: u64) -> String {
    if count >= 1_000_000 {
        let millions = count as f64 / 1_000_000.0;
        if millions >= 10.0 {
            format!("{millions:.0}M")
        } else {
            format!("{millions:.1}M").replace(".0M", "M")
        }
    } else if count >= 1000 {
        format!("{}k", (count as f64 / 1000.0).round() as u64)
    } else {
        count.to_string()
    }
}

/// How the compactions sit in the gauge's gap: one square each up to
/// three, a number past that, nothing when there were none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapMarks {
    None,
    Squares(u32),
    Number(u32),
}

impl GapMarks {
    pub fn of(compactions: u32) -> Self {
        match compactions {
            0 => GapMarks::None,
            1..=3 => GapMarks::Squares(compactions),
            n => GapMarks::Number(n),
        }
    }
}

/// The ring's polyline in a `size`-unit box: from the gap's left edge
/// along the bottom, up the left side, across the top, down the right and
/// back to the gap's right edge — clockwise.
pub fn ring_path(size: f32) -> [(f32, f32); 6] {
    let far = size - 3.0;
    [
        (13.0, far),
        (3.0, far),
        (3.0, 3.0),
        (far, 3.0),
        (far, far),
        (size - 13.0, far),
    ]
}

/// The first `fraction` of a polyline, by length.
pub fn partial_path(points: &[(f32, f32)], fraction: f32) -> Vec<(f32, f32)> {
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|pair| ((pair[1].0 - pair[0].0).powi(2) + (pair[1].1 - pair[0].1).powi(2)).sqrt())
        .collect();
    let mut left = lengths.iter().sum::<f32>() * fraction.clamp(0.0, 1.0);
    let mut out = vec![points[0]];
    for (index, length) in lengths.iter().enumerate() {
        let (a, b) = (points[index], points[index + 1]);
        if left >= *length {
            out.push(b);
            left -= length;
        } else {
            if left > 0.0 {
                let t = left / length;
                out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
            break;
        }
    }
    out
}

fn paint_ring(scene: &mut Scene, rect: Rect, size: f32, fraction: f32, tint: Color) {
    let unit = rect.size.width / size;
    let ring = ring_path(size);
    paint_legs(scene, rect, unit, &ring, color(ground.rule));
    let used = partial_path(&ring, fraction);
    if used.len() > 1 {
        paint_legs(scene, rect, unit, &used, tint);
    }
}

/// Strokes an axis-aligned polyline 3 units wide as rects: square joins
/// between legs, butt ends at both extremes.
fn paint_legs(scene: &mut Scene, rect: Rect, unit: f32, points: &[(f32, f32)], tint: Color) {
    let half = 1.5;
    let last = points.len().saturating_sub(2);
    for (index, pair) in points.windows(2).enumerate() {
        let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        let horizontal = (ay - by).abs() < f32::EPSILON;
        // Extend into a join, not past a free end.
        let grow_a = if index > 0 { half } else { 0.0 };
        let grow_b = if index < last { half } else { 0.0 };
        let (mut x0, mut x1, mut y0, mut y1) = (x0, x1, y0, y1);
        if horizontal {
            y0 -= half;
            y1 += half;
            if ax <= bx {
                x0 -= grow_a;
                x1 += grow_b;
            } else {
                x0 -= grow_b;
                x1 += grow_a;
            }
        } else {
            x0 -= half;
            x1 += half;
            if ay <= by {
                y0 -= grow_a;
                y1 += grow_b;
            } else {
                y0 -= grow_b;
                y1 += grow_a;
            }
        }
        scene.shape(
            Shape::new(Rect::from_xywh(
                rect.origin.x + x0 * unit,
                rect.origin.y + y0 * unit,
                (x1 - x0) * unit,
                (y1 - y0) * unit,
            ))
            .fill(tint),
        );
    }
}

/// The gauge micrographic alone: a square ring with a gap at the bottom
/// filling clockwise by the share used, the percentage in the centre, and
/// the compactions in the gap.
#[component]
pub fn ContextRing(
    context: ContextView,
    /// Edge length in logical pixels before UI scale: 40 in the header,
    /// 48 in the panel.
    #[prop(default = 40.0)]
    size: f32,
) -> Element {
    let fraction = context.percent() / 100.0;
    let tone = context.level().token();
    let ring = view! {
        el width:fill height:fill nohit {}
    };
    let marks = GapMarks::of(context.compactions);
    ring.paint(move |ctx| {
        paint_ring(ctx.scene, ctx.rect, size, fraction, color(tone));
        if let GapMarks::Squares(count) = marks {
            let unit = ctx.rect.size.width / size;
            let start = size / 2.0 - (count as f32 * 5.0 - 2.0) / 2.0;
            for index in 0..count {
                let x = ctx.rect.origin.x + (start + index as f32 * 5.0) * unit;
                let y = ctx.rect.origin.y + (size - 4.5) * unit;
                ctx.scene.shape(
                    Shape::new(Rect::from_xywh(x, y, 3.0 * unit, 3.0 * unit))
                        .fill(color(ground.ink)),
                );
            }
        }
    });
    let numeral = format!("{:.0}", context.percent().min(999.0));
    let host = view! {
        stack width:{px(size)}px height:{px(size)}px shrink:0 role:image
            label:(format!("{numeral} percent")) {
            col align:center justify:center nohit {
                text font-family:monospace font-size:{px(11.5)}px font-weight:600 font-color:(tone)
                    (numeral.clone())
            }
        }
    };
    host.adopt(&ring);
    if let GapMarks::Number(count) = marks {
        host.adopt(&view! {
            col align:center justify:end nohit {
                text font-family:monospace font-size:{px(7.0)}px font-weight:600
                    line-height:{px(8.0)}px font-color:ground.ink (format!("{count}×"))
            }
        });
    }
    host
}

/// The compaction mark followed by its count.
fn compaction_count(count: u32, tone: ColorToken, size: f32) -> Element {
    view! {
        row width:max-content height:min-content gap:{px(4.0)}px align:center shrink:0 {
            {
                parent.adopt(&mark(Mark::Compaction, size, tone));
            }
            text font-family:monospace font-size:{px(10.5)}px font-color:(tone) (count.to_string())
        }
    }
}

/// The header gauge: the ring, "124k / 200k", and a status line ("free",
/// "compacts soon", "compacting next turn") with the compaction count.
/// Pressing it opens the [`ContextPanel`].
#[component]
pub fn ContextGauge(
    context: ContextView,
    /// Embedded in the panel footer when given.
    #[prop(optional)]
    on_compact: Option<OnPress>,
) -> Element {
    let open = State::new(false);
    let toggle = move || open.set(!open.get_untracked());
    let tone = match context.level() {
        thresholds::Level::Room => ground.muted,
        level => level.token(),
    };
    let panel_view = context.clone();
    let content: PopoverContent = Rc::new(move || {
        let compact = on_compact.clone();
        view! {
            ContextPanel context:(panel_view.clone()) on-compact:(compact)
        }
    });
    let used = tokens(context.used);
    let window = format!(" / {}", tokens(context.window));
    view! {
        stack width:max-content height:min-content shrink:0 {
            button @click:{toggle()} width:max-content height:{px(48.0)}px radius:0px
                pad:(left:{px(6.0)}px right:{px(12.0)}px top:0px bottom:0px) gap:{px(10.0)}px
                align:center fill:(Color::TRANSPARENT) label:(context.label())
                hover { fill:ground.hover } pressed { fill:ground.pressed }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                ContextRing context:(context.clone())
                col width:max-content height:min-content gap:{px(5.0)}px {
                    row width:max-content height:min-content {
                        text font-family:monospace font-size:{px(12.0)}px font-weight:600
                            font-color:ground.ink (used)
                        text font-family:monospace font-size:{px(12.0)}px font-color:ground.muted
                            (window)
                    }
                    row width:max-content height:min-content gap:{px(6.0)}px align:center {
                        text font-family:monospace font-size:{px(10.5)}px font-color:(tone)
                            (context.status())
                        text font-family:monospace font-size:{px(10.5)}px
                            font-color:ground.rule-strong "·"
                        {
                            parent.adopt(&compaction_count(context.compactions, tone, 10.0));
                        }
                    }
                }
            }
            Popover open:(open) label:"Context" content:(content) align:(OverlayAlign::End)
                side:(OverlaySide::Bottom)
        }
    }
}

/// The breakdown: a larger gauge with the serif "Context", the use line,
/// a bar and a row per supplied part (in greys) plus free space, and a
/// ruled footer with the compaction history and a Compact now cell.
#[component]
pub fn ContextPanel(
    context: ContextView,
    #[prop(optional)] on_compact: Option<OnPress>,
) -> Element {
    let mut summary = format!(
        "{} of {} used",
        tokens(context.used),
        tokens(context.window)
    );
    if !context.model.is_empty() {
        summary.push_str(" · ");
        summary.push_str(&context.model);
    }
    let body = view! {
        col height:min-content gap:{px(14.0)}px pad:{px(16.0)}px {
            row height:min-content gap:{px(12.0)}px align:center {
                ContextRing context:(context.clone()) size:48.0
                col width:1fr min-width:0px height:min-content gap:{px(6.0)}px {
                    text font-family:serif font-size:{px(22.0)}px line-height:{px(22.0)}px "Context"
                    Ellipsis text:(summary) role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
        }
    };
    let parts: Vec<ContextShare> = context
        .parts
        .iter()
        .copied()
        .filter(|share| share.tokens > 0)
        .collect();
    if !parts.is_empty() {
        let free = context
            .window
            .saturating_sub(parts.iter().map(|share| share.tokens).sum());
        let bar = view! {
            row height:{px(10.0)}px shrink:0 gap:{px(2.0)}px role:image label:"Context breakdown" {}
        };
        let rows = view! {
            col height:min-content {}
        };
        let grown = |weight: u64, element: &Element| {
            let weight = weight as f32;
            element.restyle_dyn(move |style| style.width(Dimension::Fr(weight)).min_width(0.0));
        };
        for share in &parts {
            let token = share.part.token();
            let segment = view! {
                el height:fill fill:(token) {}
            };
            grown(share.tokens, &segment);
            bar.adopt(&segment);
            rows.adopt(&breakdown_row(
                part_name(share.part),
                share.tokens,
                Some(token),
                ground.ink,
            ));
        }
        if free > 0 {
            let segment = view! {
                el height:fill stroke:(width:{px(1.0)} color:ground.outline offset:{px(-1.0)}) {}
            };
            grown(free, &segment);
            bar.adopt(&segment);
        }
        rows.adopt(&breakdown_row("Free", free, None, ground.muted));
        body.adopt(&bar);
        body.adopt(&rows);
    }
    let panel = view! {
        col width:{px(320.0)}px height:min-content {}
    };
    panel.adopt(&body);
    if !context.history.is_empty() || on_compact.is_some() {
        let footer = view! {
            RuledRow width:fill
        };
        let history = context.history.clone();
        footer.adopt(&view! {
            row width:1fr min-width:0px gap:{px(8.0)}px align:center
                pad:(horizontal:{px(16.0)}px vertical:0px) {
                {
                    if !history.is_empty() {
                        parent.adopt(&mark(Mark::Compaction, 12.0, ground.muted));
                        parent.adopt(&view! {
                            Ellipsis text:(history.clone()) role:(TextRole::Label) tone:(Tone::Muted)
                        });
                    }
                }
            }
        });
        if let Some(compact) = on_compact {
            footer.adopt(&view! {
                CellButton label:"Compact now" on-press:(compact) rule:(Rule::Left)
                    width:max-content shrink:0 {
                    row width:max-content height:min-content gap:{px(8.0)}px align:center {
                        {
                            parent.adopt(&mark(Mark::Compaction, 12.0, ground.ink));
                        }
                        Label text:"Compact now" role:(TextRole::Label) weight:600
                    }
                }
            });
        }
        panel.adopt(&view! {
            col height:{px(40.0)}px shrink:0 stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                {
                    parent.adopt(&footer);
                }
            }
        });
    }
    panel
}

fn breakdown_row(name: &str, count: u64, swatch: Option<ColorToken>, tone: ColorToken) -> Element {
    let swatch = match swatch {
        Some(token) => view! {
            el width:{px(9.0)}px height:{px(9.0)}px shrink:0 fill:(token) {}
        },
        None => view! {
            el width:{px(9.0)}px height:{px(9.0)}px shrink:0
                stroke:(width:{px(1.0)} color:ground.rule-strong offset:{px(-1.0)}) {}
        },
    };
    let name = name.to_string();
    view! {
        row height:{px(24.0)}px shrink:0 gap:{px(10.0)}px align:center {
            {
                parent.adopt(&swatch);
            }
            row width:1fr min-width:0px height:min-content {
                text font-family:monospace font-size:{px(12.0)}px font-color:(tone) (name.clone())
            }
            {
                parent.adopt(&sized_text(tokens(count).into(), 12.0, 400, tone));
            }
        }
    }
}

/// Paints the compaction mark (a rule with arrows pressing onto it from
/// above and below) into `rect`.
pub fn paint_compaction(scene: &mut Scene, rect: Rect, tint: Color) {
    paint_mark(scene, rect, Mark::Compaction, tint);
}
