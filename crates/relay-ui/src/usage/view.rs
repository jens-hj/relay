//! Usage components: the dithered bars, the forecast bar, the resting row,
//! the opened detail and the sidebar foot that holds them.

use super::forecast::{Forecast, Verdict};
use super::{HarnessUsage, OnApply, UsageInsight, UsageWindow};
use crate::marks::sized_text;
use crate::model::Harness;
use crate::prelude::*;
use crate::tokens::{Signal, ground, px, thresholds};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{Scene, Shape};
use std::rc::Rc;

/// The flat colour of a level: ink, amber from 80%, red from 95%.
fn level_token(used: f32) -> ColorToken {
    thresholds::level(used).token()
}

/// The fill of a bar: the harness colour while there is room, then the
/// amber and red levels.
fn fill_token(used: f32, harness: Option<Harness>) -> ColorToken {
    match (thresholds::level(used), harness) {
        (thresholds::Level::Room, Some(harness)) => harness.tint(),
        (level, _) => level.token(),
    }
}

fn rect(scene: &mut Scene, rect: Rect, tint: Color) {
    scene.shape(Shape::new(rect).fill(tint));
}

/// A dithered fill `used` percent long and `thickness` high, centred in a
/// `height`-high box, over a hairline track. `overlay` paints on top with
/// the box and the track's top edge.
fn dithered_bar(
    used: f32,
    tint: ColorToken,
    thickness: f32,
    height: f32,
    label: String,
    overlay: impl Fn(&mut Scene, Rect, f32) + 'static,
) -> Element {
    let inset = (height - thickness) / 2.0;
    let fraction = (used / 100.0).clamp(0.0, 1.0);
    let track = view! {
        el width:fill height:fill nohit {}
    };
    track.paint(move |ctx| {
        let unit = ctx.rect.size.height / height;
        let r = ctx.rect;
        rect(
            ctx.scene,
            Rect::from_xywh(
                r.origin.x,
                r.origin.y + inset * unit,
                r.size.width,
                thickness * unit,
            ),
            color(ground.hairline),
        );
    });
    let fill = view! {
        el height:fill nohit clip {
            Dither colour:(tint) gradient:(Gradient::Usage) opacity:1.0 seed:19 flowing:true
        }
    };
    fill.style_dyn(move || {
        Style::default()
            .size(Dimension::Percent(fraction), Dimension::Fill)
            .shrink(0.0)
    });
    let strip = view! {
        row nohit {}
    };
    strip.adopt(&fill);
    let lane = view! {
        col pad:(top:{px(inset)}px bottom:{px(inset)}px) nohit {}
    };
    lane.adopt(&strip);
    let top = view! {
        el width:fill height:fill nohit {}
    };
    top.paint(move |ctx| {
        let unit = ctx.rect.size.height / height;
        let r = ctx.rect;
        let x = r.origin.x + r.size.width * fraction;
        if fraction > 0.0 {
            let end = Rect::from_xywh(
                (x - 1.5 * unit).max(r.origin.x),
                r.origin.y + inset * unit,
                1.5 * unit,
                thickness * unit,
            );
            rect(ctx.scene, end, color(tint));
        }
        overlay(ctx.scene, r, unit);
    });
    view! {
        stack height:{px(height)}px shrink:0 role:image label:(label) {
            {
                parent.adopt(&track);
                parent.adopt(&lane);
                parent.adopt(&top);
            }
        }
    }
}

fn bar_label(window: &UsageWindow) -> String {
    match window.elapsed_fraction() {
        Some(elapsed) => format!(
            "{:.0} percent used, {:.0} percent of the window elapsed",
            window.used,
            elapsed * 100.0
        ),
        None => format!("{:.0} percent used", window.used),
    }
}

/// A resting usage bar: a 4px dithered fill densest at its crisp end, in
/// the harness colour (ink without one), amber from 80% or red from 95%,
/// with a thin grey tick for how much of the window has passed.
#[component]
pub fn UsageBar(window: UsageWindow, #[prop(optional)] harness: Option<Harness>) -> Element {
    let elapsed = window.elapsed_fraction();
    dithered_bar(
        window.used,
        fill_token(window.used, harness),
        4.0,
        10.0,
        bar_label(&window),
        move |scene, r, unit| {
            if let Some(elapsed) = elapsed {
                let x = r.origin.x + r.size.width * elapsed;
                rect(
                    scene,
                    Rect::from_xywh(x, r.origin.y, unit, r.size.height),
                    color(ground.muted),
                );
            }
        },
    )
}

/// The opened forecast bar: usage so far, a dashed projection to the
/// value at reset ending in a hollow square when it lands under 100%, or a
/// red arrow at the end when it overruns, and the elapsed tick.
#[component]
pub fn ForecastBar(window: UsageWindow, #[prop(optional)] harness: Option<Harness>) -> Element {
    let forecast = window.forecast();
    let elapsed = window.elapsed_fraction();
    let label = match &forecast {
        Some(forecast) => format!(
            "{:.0} percent used, projected {:.0} percent by reset",
            window.used, forecast.projected
        ),
        None => bar_label(&window),
    };
    let tint = fill_token(window.used, harness);
    dithered_bar(
        window.used,
        tint,
        6.0,
        16.0,
        label,
        move |scene, r, unit| {
            if let Some(forecast) = forecast {
                paint_projection(scene, r, unit, &forecast);
            }
            if let Some(elapsed) = elapsed {
                let x = r.origin.x + r.size.width * elapsed;
                rect(
                    scene,
                    Rect::from_xywh(x, r.origin.y, unit, r.size.height),
                    color(ground.muted),
                );
            }
        },
    )
}

/// The dashed line (2 on, 3 off) from just past the value to the
/// projection, with its end mark.
fn paint_projection(scene: &mut Scene, r: Rect, unit: f32, forecast: &Forecast) {
    let overrun = forecast.projected > 100.0;
    let tint = color(if overrun {
        Signal::Failed.token()
    } else {
        ground.muted
    });
    let width = r.size.width;
    let mid = r.origin.y + r.size.height / 2.0;
    let start = width * (forecast.used / 100.0).clamp(0.0, 1.0) + 2.0 * unit;
    let end = if overrun {
        width - 3.0 * unit
    } else {
        width * (forecast.projected / 100.0).clamp(0.0, 1.0) - 3.0 * unit
    };
    let mut x = start;
    while x < end {
        let dash = (2.0 * unit).min(end - x);
        rect(
            scene,
            Rect::from_xywh(r.origin.x + x, mid - unit / 2.0, dash, unit),
            tint,
        );
        x += 5.0 * unit;
    }
    if overrun {
        let tip = r.origin.x + width;
        let bounds = Rect::from_xywh(tip - 7.0 * unit, mid - 5.0 * unit, 7.0 * unit, 10.0 * unit);
        let points = vec![
            Vector2::new(-3.5 * unit, -5.0 * unit),
            Vector2::new(3.5 * unit, 0.0),
            Vector2::new(-3.5 * unit, 5.0 * unit),
        ];
        scene.shape(Shape::new(bounds).path(points, true).fill(tint));
    } else {
        let centre = r.origin.x + width * (forecast.projected / 100.0).clamp(0.0, 1.0);
        let square = Rect::from_xywh(
            centre - 2.5 * unit,
            mid - 2.5 * unit,
            5.0 * unit,
            5.0 * unit,
        );
        scene.shape(Shape::new(square).stroke(mosaic::render::Stroke::new(unit, tint).offset(0.0)));
    }
}

fn percent(used: f32) -> Element {
    let tone = level_token(used);
    view! {
        row width:{px(30.0)}px height:min-content shrink:0 justify:end {
            text font-family:monospace font-size:{px(10.5)}px font-weight:600 font-color:(tone)
                (format!("{used:.0}%"))
        }
    }
}

fn bar_line(caption: &str, window: &UsageWindow, harness: Harness) -> Element {
    let caption = caption.to_string();
    view! {
        row height:min-content gap:{px(7.0)}px align:center {
            text font-family:monospace font-size:{px(9.5)}px font-color:ground.faint shrink:0
                (caption)
            UsageBar window:(window.clone()) harness:(Some(harness))
            {
                parent.adopt(&percent(window.used));
            }
        }
    }
}

/// One harness at rest: its glyph (name on hover), then the 5h and wk
/// bars with their percentages. Pressing it opens the detail.
#[component]
pub fn UsageRow(
    usage: HarnessUsage,
    #[prop(default = false)] open: bool,
    on_press: OnPress,
) -> Element {
    let label = format!(
        "{}. {}",
        usage.summary(),
        if open { "Hide details" } else { "Show details" }
    );
    let name = usage.harness.name().to_string();
    let bars = view! {
        col width:1fr min-width:0px height:min-content gap:{px(3.0)}px {}
    };
    if let Some(session) = &usage.session {
        bars.adopt(&bar_line("5h", session, usage.harness));
    }
    if let Some(week) = &usage.week {
        bars.adopt(&bar_line("wk", week, usage.harness));
    }
    if usage.session.is_none() && usage.week.is_none() {
        bars.adopt(&view! {
            Label text:"No usage data" role:(TextRole::Label) tone:(Tone::Muted)
        });
    }
    view! {
        Tooltip text:(name) width:(Dimension::Fill) {
            button @click:{on_press()} width:fill height:min-content radius:0px
                pad:(horizontal:{px(18.0)}px vertical:{px(5.0)}px) gap:{px(10.0)}px align:center
                fill:(Color::TRANSPARENT) font-color:ground.ink label:(label)
                hover { fill:ground.hover } pressed { fill:ground.pressed }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                row width:{px(16.0)}px height:min-content shrink:0 justify:center {
                    HarnessGlyph harness:(usage.harness)
                }
                {
                    parent.adopt(&bars);
                }
            }
        }
    }
}

fn line(left: Element, right: Option<Element>) -> Element {
    let row = view! {
        row height:min-content gap:{px(10.0)}px align:center {}
    };
    row.adopt(&left);
    if let Some(right) = right {
        row.adopt(&right);
    }
    row
}

fn grow(text: String, tone: ColorToken, weight: u16) -> Element {
    view! {
        row width:1fr min-width:0px height:min-content {
            {
                parent.adopt(&crate::marks::hugging_ellipsis(
                    text.clone(),
                    TextRole::Label,
                    Tone::Colour(tone),
                    Weight(weight),
                ));
            }
        }
    }
}

fn window_block(title: &str, window: &UsageWindow, harness: Harness) -> Element {
    let block = view! {
        col height:min-content gap:{px(6.0)}px pad:(top:{px(8.0)}px) {}
    };
    let resets = (!window.resets.is_empty())
        .then(|| sized_text(window.resets.clone().into(), 12.0, 400, ground.ink));
    block.adopt(&line(grow(title.to_string(), ground.muted, 400), resets));
    block.adopt(&view! {
        ForecastBar window:(window.clone()) harness:(Some(harness))
    });
    if let Some(forecast) = window.forecast() {
        let tone = match forecast.verdict {
            Verdict::RunsOutEarly => Signal::Failed.token(),
            Verdict::OnPace => ground.ink,
            Verdict::FullSteam => Signal::FullSteam.token(),
        };
        let (unit, suffix) = window.rate_unit();
        let rate = forecast.rate_per(unit);
        let rate = if suffix == "%/h" {
            format!("{rate:.0}{suffix}")
        } else {
            format!("{rate:.1}{suffix}")
        };
        block.adopt(&line(
            grow(forecast.verdict.text().to_string(), tone, 600),
            Some(sized_text(rate.into(), 12.0, 400, ground.muted)),
        ));
        block.adopt(&view! {
            Ellipsis text:(forecast.detail()) role:(TextRole::Label) tone:(Tone::Muted)
        });
    }
    block
}

fn mix_block(insight: &UsageInsight) -> Option<Element> {
    if insight.model_mix.is_empty() {
        return None;
    }
    let bar = view! {
        row height:{px(4.0)}px shrink:0 gap:{px(2.0)}px role:image label:"Model mix" {}
    };
    let legend = view! {
        row height:min-content gap:{px(14.0)}px align:center {}
    };
    for share in &insight.model_mix {
        let tone = share.hue.token();
        let segment = view! {
            el height:fill fill:(tone) {}
        };
        let weight = share.share.max(0.0);
        segment.style_dyn(move || {
            Style::default()
                .size(Dimension::Fr(weight), Dimension::Fill)
                .min_width(0.0)
        });
        bar.adopt(&segment);
        let text = format!("{} {:.0}%", share.name, share.share);
        legend.adopt(&view! {
            row width:max-content height:min-content gap:{px(5.0)}px align:center shrink:0 {
                el width:{px(6.0)}px height:{px(6.0)}px shrink:0 fill:(tone) {}
                text font-family:monospace font-size:{px(12.0)}px font-color:ground.ink (text)
            }
        });
    }
    Some(view! {
        col height:min-content gap:{px(6.0)}px pad:(top:{px(8.0)}px) {
            Label text:"Model mix this week" role:(TextRole::Label) tone:(Tone::Muted)
            {
                parent.adopt(&bar);
                parent.adopt(&legend);
            }
        }
    })
}

fn heaviest_block(insight: &UsageInsight) -> Option<Element> {
    if insight.heaviest.is_empty() {
        return None;
    }
    let block = view! {
        col height:min-content gap:{px(2.0)}px pad:(top:{px(8.0)}px) {
            col height:min-content pad:(bottom:{px(4.0)}px) {
                Label text:"Heaviest agents this week" role:(TextRole::Label) tone:(Tone::Muted)
            }
        }
    };
    for (name, share) in &insight.heaviest {
        let share = format!("{share:.0}%");
        block.adopt(&view! {
            row height:{px(20.0)}px shrink:0 gap:{px(10.0)}px align:center {
                Ellipsis text:(name.clone()) role:(TextRole::Label)
                {
                    parent.adopt(&sized_text(share.clone().into(), 12.0, 400, ground.muted));
                }
            }
        });
    }
    Some(block)
}

fn stats_block(insight: &UsageInsight) -> Option<Element> {
    if insight.stats.is_empty() {
        return None;
    }
    let grid = view! {
        grid cols:4 height:min-content gap:{px(6.0)}px {}
    };
    for (key, value) in &insight.stats {
        let (key, value) = (key.clone(), value.clone());
        grid.adopt(&view! {
            Label text:(key) role:(TextRole::Label) tone:(Tone::Muted)
        });
        grid.adopt(&view! {
            Label text:(value) role:(TextRole::Label)
        });
    }
    Some(grid)
}

/// The opened detail of one harness: forecast blocks for the session and
/// the week, then the model mix, heaviest agents, quick stats and the
/// suggestion footer with an embedded Apply cell. Parts the caller does
/// not supply are hidden.
#[component]
pub fn UsageDetail(usage: HarnessUsage, #[prop(optional)] on_apply: Option<OnApply>) -> Element {
    let bottom_padding = if usage.insight.suggestion.is_none() {
        px(14.0)
    } else {
        px(0.0)
    };
    let body = view! {
        col height:min-content gap:{px(14.0)}px pad:(horizontal:{px(18.0)}px vertical:0px) {}
    };
    if let Some(session) = &usage.session {
        body.adopt(&window_block("Session · 5h", session, usage.harness));
    }
    if let Some(week) = &usage.week {
        body.adopt(&window_block("Week", week, usage.harness));
    }
    for part in [
        mix_block(&usage.insight),
        heaviest_block(&usage.insight),
        stats_block(&usage.insight),
    ]
    .into_iter()
    .flatten()
    {
        body.adopt(&part);
    }
    let detail = view! {
        col height:min-content shrink:0 gap:{px(14.0)}px
            pad:(top:{px(6.0)}px bottom:{bottom_padding}px) fill:ground.code
            stroke:(width:{px(1.0)} color:ground.hairline edges:(top bottom)) {}
    };
    detail.adopt(&body);
    if let Some(suggestion) = usage.insight.suggestion.clone() {
        let harness = usage.harness;
        let apply: OnPress = Rc::new(move || {
            if let Some(apply) = &on_apply {
                apply(harness);
            }
        });
        let text = format!("→ {}", suggestion.text);
        detail.adopt(&view! {
            col height:{px(42.0)}px shrink:0 stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                RuledRow {
                    row width:1fr min-width:0px gap:{px(6.0)}px align:center
                        pad:(horizontal:{px(18.0)}px vertical:0px) clip {
                        {
                            parent.adopt(&crate::marks::hugging_ellipsis(
                                suggestion.subject.clone(),
                                TextRole::Label,
                                Tone::Ink,
                                Weight::SEMIBOLD,
                            ));
                        }
                        Ellipsis text:(text) role:(TextRole::Label)
                    }
                    CellButton label:"Apply" on-press:(apply) rule:(Rule::Left) width:max-content
                        shrink:0 {
                        Label text:"Apply" role:(TextRole::Label) weight:600
                    }
                }
            }
        });
    }
    detail
}

/// The sidebar foot: serif "Usage", the harness count, and one row per
/// harness; pressing a row opens its detail below it (one at a time).
#[component]
pub fn UsageFoot(
    harnesses: Live<Vec<HarnessUsage>>,
    /// The harness opened initially.
    #[prop(optional)]
    open: Option<Harness>,
    #[prop(optional)] on_apply: Option<OnApply>,
) -> Element {
    let opened = State::new(open);
    let rows = view! {
        col height:min-content {}
    };
    let count = harnesses.clone();
    let list = harnesses.clone();
    rows.switch(
        move || (list.get(), opened.get()),
        move |parent, (usages, opened_now)| {
            for usage in usages {
                let harness = usage.harness;
                let is_open = *opened_now == Some(harness);
                let press: OnPress = Rc::new(move || {
                    opened.set(if is_open { None } else { Some(harness) });
                });
                parent.adopt(&view! {
                    UsageRow usage:(usage.clone()) open:(is_open) on-press:(press)
                });
                if is_open {
                    let apply = on_apply.clone();
                    parent.adopt(&view! {
                        UsageDetail usage:(usage.clone()) on-apply:(apply)
                    });
                }
            }
        },
    );
    view! {
        col height:min-content shrink:0 fill:ground.panel label:"Usage foot" {
            el width:fill height:{px(1.0)}px shrink:0 fill:ground.rule label:"Usage boundary"
                nohit {}
            row height:min-content gap:{px(8.0)}px align:end
                pad:(left:{px(18.0)}px right:{px(18.0)}px top:{px(12.0)}px bottom:{px(6.0)}px) {
                SerifTitle text:"Usage" size:(SerifSize::Small) italic:true
                el width:1fr height:{px(1.0)}px {}
                text font-family:monospace font-size:{px(11.0)}px font-color:ground.muted shrink:0
                    {
                        let n = count.get().len();
                        format!("{n} {}", if n == 1 { "harness" } else { "harnesses" })
                    }
            }
            {
                parent.adopt(&rows);
            }
        }
    }
}
