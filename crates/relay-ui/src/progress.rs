//! The progress strip: an agent's own estimate of how far its task is, as a
//! bar with the likely range around the estimate. Hovering shows what is
//! completed and what remains; pressing keeps that open.

use crate::prelude::*;
use crate::tokens::{Signal, ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{Scene, Shape};
use mosaic::widgets::{
    OverlayAlign, OverlayPlacement, OverlaySide, TooltipOptions, TooltipTrigger, Visual,
};
use std::time::Duration;

/// An agent's latest progress report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskProgress {
    /// Share of the goal that is done, 0 to 100.
    pub percent: u8,
    /// The likely range around `percent`, 0 to 100.
    pub low: u8,
    pub high: u8,
    /// How sure the agent is of `percent`, 0 to 100.
    pub confidence: u8,
    pub completed: String,
    pub remaining: String,
}

crate::value::live_value!(TaskProgress);

impl TaskProgress {
    /// Whether the estimate has a range around it.
    pub fn uncertain(&self) -> bool {
        self.low < self.high
    }

    /// "60%", or "60% ±15" with the wider side of the range.
    pub fn readout(&self) -> String {
        if self.uncertain() {
            let spread =
                (self.percent.saturating_sub(self.low)).max(self.high.saturating_sub(self.percent));
            format!("{}% ±{spread}", self.percent)
        } else {
            format!("{}%", self.percent)
        }
    }

    /// "Progress: 60 percent, likely 45 to 75 percent".
    pub fn label(&self) -> String {
        if self.uncertain() {
            format!(
                "Progress: {} percent, likely {} to {} percent",
                self.percent, self.low, self.high
            )
        } else {
            format!("Progress: {} percent", self.percent)
        }
    }

    /// "Likely 45–75% · Confidence 70%".
    pub fn range_line(&self) -> String {
        if self.uncertain() {
            format!(
                "Likely {}–{}% · Confidence {}%",
                self.low, self.high, self.confidence
            )
        } else {
            format!("Confidence {}%", self.confidence)
        }
    }
}

/// Height of the strip, of the fill, and of the range bracket around it.
const HEIGHT: f32 = 22.0;
const THICKNESS: f32 = 4.0;
const BAND: f32 = 12.0;
const LINE: f32 = 1.0;

fn rect(scene: &mut Scene, rect: Rect, tint: Color) {
    scene.shape(Shape::new(rect).fill(tint));
}

/// The bar: a rule across the full width, the fill up to the estimate, and
/// an outlined bracket over the likely range with a tick at the estimate.
/// Nothing is painted behind the fill's grain except the rule, so the grain
/// keeps its full contrast; only the part of the range past the estimate,
/// which has no grain, is tinted. The fill is the live colour and flows
/// while the agent works; it is ink at rest.
fn bar(progress: &TaskProgress, working: bool) -> Element {
    let tint = if working {
        Signal::Live.token()
    } else {
        ground.ink
    };
    let fraction = f32::from(progress.percent.min(100)) / 100.0;
    let low = f32::from(progress.low.min(100)) / 100.0;
    let high = f32::from(progress.high.min(100)) / 100.0;
    let uncertain = progress.uncertain();
    let inset = (HEIGHT - THICKNESS) / 2.0;
    let track = view! {
        el width:fill height:fill nohit {}
    };
    track.paint(move |ctx| {
        let unit = ctx.rect.size.height / HEIGHT;
        let r = ctx.rect;
        rect(
            ctx.scene,
            Rect::from_xywh(
                r.origin.x,
                r.origin.y + (HEIGHT - LINE) / 2.0 * unit,
                r.size.width,
                LINE * unit,
            ),
            color(ground.rule),
        );
        // The done part of the rule takes the fill colour, so the filled
        // length reads even where the grain is sparse.
        let done = color(tint);
        rect(
            ctx.scene,
            Rect::from_xywh(
                r.origin.x,
                r.origin.y + (HEIGHT - LINE) / 2.0 * unit,
                r.size.width * fraction,
                LINE * unit,
            ),
            done.alpha(done.a * 0.75),
        );
    });
    let fill = view! {
        el height:fill nohit clip {
            Dither colour:(tint) gradient:(Gradient::Usage) opacity:1.0 seed:23 flowing:(working)
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
        let unit = ctx.rect.size.height / HEIGHT;
        let r = ctx.rect;
        let band_top = r.origin.y + (HEIGHT - BAND) / 2.0 * unit;
        let band = BAND * unit;
        let line = LINE * unit;
        let ink = color(tint);
        let x = r.origin.x + r.size.width * fraction;
        if uncertain {
            let left = r.origin.x + r.size.width * low;
            let right = r.origin.x + r.size.width * high;
            let edge = ink.alpha(ink.a * 0.6);
            // Past the estimate there is no grain to obscure.
            rect(
                ctx.scene,
                Rect::from_xywh(x, band_top, (right - x).max(0.0), band),
                ink.alpha(ink.a * 0.16),
            );
            for y in [band_top, band_top + band - line] {
                rect(
                    ctx.scene,
                    Rect::from_xywh(left, y, right - left, line),
                    edge,
                );
            }
            for cap in [left, (right - line).max(left)] {
                rect(ctx.scene, Rect::from_xywh(cap, band_top, line, band), edge);
            }
        }
        rect(
            ctx.scene,
            Rect::from_xywh((x - 1.5 * unit).max(r.origin.x), band_top, 1.5 * unit, band),
            ink,
        );
    });
    view! {
        stack height:{px(HEIGHT)}px min-width:0px nohit {
            {
                parent.adopt(&track);
                parent.adopt(&lane);
                parent.adopt(&top);
            }
        }
    }
}

/// The range, then what is completed and what remains.
fn summary(progress: &TaskProgress) -> Element {
    let range = progress.range_line();
    let completed = progress.completed.clone();
    let remaining = progress.remaining.clone();
    view! {
        col height:min-content min-width:0px gap:{px(9.0)}px {
            text font-family:monospace font-size:{px(11.5)}px font-color:ground.muted (range)
            col height:min-content min-width:0px gap:{px(3.0)}px {
                Label text:"Completed" role:(TextRole::Caption) tone:(Tone::Muted)
                text font-size:{px(12.0)}px line-height:1.5 (completed)
            }
            col height:min-content min-width:0px gap:{px(3.0)}px {
                Label text:"Remaining" role:(TextRole::Caption) tone:(Tone::Muted)
                text font-size:{px(12.0)}px line-height:1.5 (remaining)
            }
        }
    }
}

/// The strip. It sits above the message field as the line under the history.
/// Hovering or focusing it shows the summary above it without moving the
/// page; pressing it sets `open`, which keeps the summary in the page, above
/// the bar, until it is pressed again.
#[component]
pub fn ProgressStrip(
    progress: Live<TaskProgress>,
    #[prop(default = Live::fixed(false))] working: Live<bool>,
    /// Whether the summary stays open. Owned by the caller so that it
    /// outlives the strip.
    open: State<bool>,
) -> Element {
    let host = view! {
        col height:min-content min-width:0px shrink:0 {}
    };
    // These outlive each rebuilt strip. Only this effect reads `open`, so no
    // effect of a replaced strip runs again after its button is gone.
    let over = State::new(false);
    let peek = State::new(false);
    Effect::new(move || peek.set(over.get() && !open.get()));
    // Counts rebuilt strips. A replaced button's hover effect can be woken once
    // more by a change in how focus is shown; it must then stop before it
    // reads the button it belonged to, which is gone.
    let generation = std::rc::Rc::new(std::cell::Cell::new(0u64));
    {
        let generation = generation.clone();
        mosaic::core::reactive::on_cleanup(move || generation.set(u64::MAX));
    }
    host.switch(
        move || (progress.get(), working.get()),
        move |parent, value| {
            let (progress, working) = value.clone();
            let mine = generation.get().wrapping_add(1);
            generation.set(mine);
            let toggle = move || open.set(!open.get_untracked());
            let readout = progress.readout();
            let button = view! {
                button @click:{toggle()} width:fill height:{px(HEIGHT)}px radius:0px
                    pad:(left:0px right:{px(14.0)}px top:0px bottom:0px) gap:{px(10.0)}px
                    align:center fill:(Color::TRANSPARENT) label:(progress.label())
                    hover { fill:ground.hover } pressed { fill:ground.pressed }
                    focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                    {
                        parent.adopt(&bar(&progress, working));
                    }
                    text font-family:monospace font-size:{px(11.5)}px font-color:ground.muted
                        (readout)
                }
            };
            over.set(false);
            {
                let interaction = button.interaction();
                // Keyboard focus only: a bar focused by a press must not keep
                // its summary up after the pointer leaves.
                let generation = generation.clone();
                Effect::new(move || {
                    if generation.get() != mine {
                        return;
                    }
                    over.set(interaction.hovered() || interaction.focus_visible());
                });
            }
            let options = TooltipOptions::default()
                .trigger(TooltipTrigger::Manual)
                .open(peek)
                .show_delay(Duration::ZERO)
                .hide_delay(Duration::ZERO)
                .placement(
                    OverlayPlacement::default()
                        .side(OverlaySide::Top)
                        .align(OverlayAlign::Start)
                        .gap(0.0),
                );
            let shown = progress.clone();
            mosaic::widgets::tooltip(&button, progress.label(), options, move |root| {
                root.hit_testable(false);
                root.style(
                    Style::column()
                        .width(px(460.0))
                        .height(Dimension::MinContent)
                        .shrink(0.0)
                        .padding(Edges::symmetric(px(14.0), px(11.0))),
                );
                root.visual_dyn(move || {
                    Visual::new().fill(color(ground.panel_raised)).stroke(
                        StrokeSpec::new(px(1.0), color(ground.rule_strong)).offset(-px(1.0)),
                    )
                });
                root.adopt(&summary(&shown));
            });
            let pinned = progress.clone();
            parent.adopt(&view! {
                col height:min-content min-width:0px {
                    if open.get() {
                        col height:min-content min-width:0px label:"Progress summary"
                            pad:(horizontal:{px(14.0)}px vertical:{px(11.0)}px)
                            stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                            {
                                parent.adopt(&summary(&pinned));
                            }
                        }
                    }
                }
            });
            parent.adopt(&button);
        },
    );
    host
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mount;

    fn report(percent: u8, low: u8, high: u8) -> TaskProgress {
        TaskProgress {
            percent,
            low,
            high,
            confidence: 70,
            completed: "The tool and the setting exist.".into(),
            remaining: "The bar is not drawn yet.".into(),
        }
    }

    #[test]
    fn readout_label_and_range_line_state_the_estimate_and_its_range() {
        let sure = report(40, 40, 40);
        assert!(!sure.uncertain());
        assert_eq!(sure.readout(), "40%");
        assert_eq!(sure.label(), "Progress: 40 percent");
        assert_eq!(sure.range_line(), "Confidence 70%");
        let unsure = report(60, 45, 75);
        assert_eq!(unsure.readout(), "60% ±15");
        assert_eq!(
            unsure.label(),
            "Progress: 60 percent, likely 45 to 75 percent"
        );
        assert_eq!(unsure.range_line(), "Likely 45–75% · Confidence 70%");
        // A range cut off at an end reports its wider side.
        assert_eq!(report(95, 70, 100).readout(), "95% ±25");
    }

    #[test]
    fn hover_shows_the_summary_and_a_press_keeps_it_until_pressed_again() {
        let open = State::new(false);
        let mounted = mount(Size::new(800.0, 400.0), move || {
            view! {
                col width:{560.0}px {
                    ProgressStrip progress:(report(60, 45, 75)) open:(open)
                }
            }
        });
        let label = "Progress: 60 percent, likely 45 to 75 percent";
        let strip = mounted.rect(label);
        assert_eq!(strip.size.width, 560.0);
        assert_eq!(strip.size.height, HEIGHT);
        assert!(mounted.has("60% ±15"));
        assert!(!mounted.has("The tool and the setting exist."));

        mounted.hover(strip.center());
        assert!(
            mounted.has("The tool and the setting exist."),
            "{:?}",
            mounted.labels()
        );
        assert!(mounted.has("The bar is not drawn yet."));
        assert!(mounted.has("Likely 45–75% · Confidence 70%"));
        assert!(!mounted.has("Progress summary"));

        mounted.click(strip.center());
        assert!(open.get_untracked());
        assert!(mounted.has("Progress summary"));
        // Pinned, the summary is in the page once, not also in the overlay.
        assert_eq!(mounted.count("The tool and the setting exist."), 1);
        mounted.hover(Vector2::new(700.0, 380.0));
        assert!(mounted.has("The tool and the setting exist."));

        // The pinned summary is above the bar, so the bar has moved down.
        let moved = mounted.rect(label);
        assert!(mounted.rect("Progress summary").max_y() <= moved.origin.y);
        mounted.click(moved.center());
        assert!(!open.get_untracked());
        assert!(!mounted.has("Progress summary"));
        mounted.hover(Vector2::new(700.0, 380.0));
        assert!(!mounted.has("The tool and the setting exist."));
    }

    #[test]
    fn a_new_report_redraws_the_strip_and_keeps_the_summary_open() {
        let open = State::new(true);
        let progress = State::new(report(20, 0, 60));
        let mounted = mount(Size::new(800.0, 400.0), move || {
            view! {
                col width:{560.0}px {
                    ProgressStrip progress:(Live::from(progress)) working:(true) open:(open)
                }
            }
        });
        assert!(mounted.has("Progress: 20 percent, likely 0 to 60 percent"));
        assert!(mounted.has("Progress summary"));
        let mut next = report(100, 100, 100);
        next.remaining = "Nothing is left.".into();
        progress.set(next);
        mounted.settle();
        assert!(mounted.has("Progress: 100 percent"));
        assert!(mounted.has("100%"));
        assert!(mounted.has("Nothing is left."));
        assert!(!mounted.has("The bar is not drawn yet."));
        assert!(mounted.has("Progress summary"));
        // The redrawn strip still answers presses and hover.
        mounted.click_label("Progress: 100 percent");
        assert!(!open.get_untracked());
        assert!(!mounted.has("Progress summary"));
        mounted.hover(mounted.rect("Progress: 100 percent").center());
        assert!(mounted.has("Nothing is left."));
        mounted.click_label("Progress: 100 percent");
        assert!(mounted.has("Progress summary"));
        assert_eq!(mounted.count("Nothing is left."), 1);
    }
}
