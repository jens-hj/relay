//! The effort step track: squares that grow per level on a hairline, the
//! current step framed, and a diamond per top step (max, ultra, …) only for
//! models that offer them.

use crate::Live;
use crate::tokens::{ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{Scene, Shape, Stroke};
use std::rc::Rc;

/// Which way the track runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Left to right.
    #[default]
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// The ledger track or the compact tree-row track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TrackSize {
    /// Growing squares on a hairline, 158 × 20 at rest.
    #[default]
    Ledger,
    /// 3px squares at a 5px pitch, for tree rows.
    Compact,
}

/// Default ordinary step names, lowest first.
pub const EFFORT_NAMES: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];

/// Names of the top steps after the ordinary ones, lowest first. Each is
/// drawn as a diamond.
pub const TIER_NAMES: [&str; 2] = ["max", "ultra"];

/// The shape of one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkShape {
    Square,
    Diamond,
}

/// One step of a ledger track, in design units (a 20-unit-high track).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackMark {
    /// Centre along the track.
    pub at: f32,
    /// Side of a square, or diagonal of a diamond, before the 1-unit stroke.
    pub size: f32,
    pub shape: MarkShape,
    /// Filled in the model colour (at or below the current step).
    pub filled: bool,
    /// Wears the frame.
    pub current: bool,
}

/// Inset of the first and last step centres from the track ends.
const END_INSET: f32 = 12.0;
/// The smallest step's side, and how much each level adds.
const FIRST_SIDE: f32 = 3.0;
const SIDE_STEP: f32 = 2.0;
/// Half-diagonal of a top-step diamond.
const DIAMOND_HALF: f32 = 6.8;
/// How far the frame sits outside the current step.
const FRAME_GAP: f32 = 2.5;

/// Lays out a ledger track `length` units long: `levels` growing squares,
/// then `tiers` diamonds. `current` is the zero-based step across both.
pub fn ledger_marks(levels: u8, current: u8, tiers: u8, length: f32) -> Vec<TrackMark> {
    let count = levels as usize + tiers as usize;
    let span = (length - 2.0 * END_INSET).max(0.0);
    let pitch = if count > 1 {
        span / (count - 1) as f32
    } else {
        0.0
    };
    (0..count)
        .map(|index| {
            let diamond = index >= levels as usize;
            TrackMark {
                at: END_INSET + pitch * index as f32,
                size: if diamond {
                    DIAMOND_HALF * 2.0
                } else {
                    FIRST_SIDE + SIDE_STEP * index as f32
                },
                shape: if diamond {
                    MarkShape::Diamond
                } else {
                    MarkShape::Square
                },
                filled: index <= current as usize,
                current: index == current as usize,
            }
        })
        .collect()
}

/// The accessible name of a track position.
pub fn effort_label(levels: u8, current: u8, tiers: u8) -> String {
    if tiers > 0 && current >= levels {
        let name = TIER_NAMES
            .get(usize::from(current - levels))
            .copied()
            .unwrap_or("top");
        format!("Effort {name}")
    } else {
        format!("Effort {} of {}", current as u32 + 1, levels)
    }
}

/// Called with the chosen zero-based step.
pub type OnEffort = Rc<dyn Fn(u8)>;

/// An effort step track in a model's colour. Ledger tracks grow per level
/// and frame the current step; one diamond per top step (max, ultra, …)
/// follows only when `tiers` is above zero, and fills and takes the frame
/// when selected. Compact tracks show 3px squares for tree rows. With
/// `on-select`, each step is a button.
#[component]
pub fn EffortTrack(
    /// Number of ordinary levels (five for most models).
    levels: Live<u8>,
    /// The zero-based current step across ordinary and top steps.
    current: Live<u8>,
    /// How many top steps the model offers after its ordinary levels.
    #[prop(default = Live::fixed(0))]
    tiers: Live<u8>,
    /// The model's colour token.
    colour: ColorToken,
    /// Horizontal (default) or vertical.
    #[prop(default = Orientation::Horizontal)]
    orientation: Orientation,
    /// Ledger (default) or compact.
    #[prop(default = TrackSize::Ledger)]
    size: TrackSize,
    /// Step names for the buttons; defaults to minimal … xhigh, then max.
    #[prop(optional)]
    names: Vec<String>,
    /// Makes each step a button that reports its zero-based index. The
    /// buttons are built for the step count at construction.
    #[prop(optional)]
    on_select: Option<OnEffort>,
) -> Element {
    let vertical = orientation == Orientation::Vertical;
    let compact = size == TrackSize::Compact;
    let (lv, cur, max) = (levels.clone(), current.clone(), tiers.clone());
    let length = Derived::new(move || {
        let count = lv.get() as f32 + max.get() as f32;
        if compact {
            px((count - 1.0).max(0.0) * 5.0 + 3.0)
        } else {
            px(158.0)
        }
    });
    let thickness = move || if compact { px(3.0) } else { px(20.0) };
    let (lv, max) = (levels.clone(), tiers.clone());
    let track = view! {
        stack shrink:0 role:image width:{if vertical { thickness() } else { length.get() }}px
            height:{if vertical { length.get() } else { thickness() }}px
            label:{effort_label(lv.get(), cur.get(), max.get())} {}
    };
    {
        let target = track.clone();
        let (a, b, c) = (levels.clone(), current.clone(), tiers.clone());
        Effect::new(move || {
            let _ = (a.get(), b.get(), c.get());
            target.paint_dirty();
        });
    }
    {
        let (levels, current, tiers) = (levels.clone(), current.clone(), tiers.clone());
        track.paint(move |ctx| {
            let (n, c, m) = (
                levels.get_untracked(),
                current.get_untracked(),
                tiers.get_untracked(),
            );
            if compact {
                paint_compact(ctx.scene, ctx.rect, n, c, m, colour, vertical);
            } else {
                paint_ledger(ctx.scene, ctx.rect, n, c, m, colour, vertical);
            }
        });
    }
    if let Some(on_select) = on_select {
        let buttons = view! {
            row {}
        };
        if vertical {
            buttons.style(Style::column().size(Dimension::Fill, Dimension::Fill));
        }
        let count = levels.get_untracked() as usize + usize::from(tiers.get_untracked());
        for index in 0..count {
            let name = names.get(index).cloned().unwrap_or_else(|| {
                let levels = levels.get_untracked() as usize;
                if index >= levels {
                    TIER_NAMES
                        .get(index - levels)
                        .copied()
                        .unwrap_or("top")
                        .to_string()
                } else {
                    EFFORT_NAMES
                        .get(index)
                        .copied()
                        .unwrap_or("level")
                        .to_string()
                }
            });
            let select = on_select.clone();
            let step = index as u8;
            let button = view! {
                button @click:{select(step)} width:1fr height:1fr radius:0px pad:0px
                    fill:(Color::TRANSPARENT) label:{format!("Effort {name}")}
                    hover { fill:(Color::TRANSPARENT) } pressed { fill:(Color::TRANSPARENT) }
                    focused { stroke:(width:1px color:ground.ink offset:-1px) } {
                    el width:fill height:fill {}
                }
            };
            buttons.adopt(&button);
        }
        track.adopt(&buttons);
    }
    track
}

/// Paints a ledger track into `rect`, scaled from its 20-unit thickness.
pub fn paint_ledger(
    scene: &mut Scene,
    rect: Rect,
    levels: u8,
    current: u8,
    tiers: u8,
    colour: ColorToken,
    vertical: bool,
) {
    let (length, thick) = if vertical {
        (rect.size.height, rect.size.width)
    } else {
        (rect.size.width, rect.size.height)
    };
    let unit = thick / 20.0;
    if unit <= 0.0 {
        return;
    }
    let tint = color(colour);
    let empty_fill = color(ground.panel);
    let empty_edge = color(ground.rule_strong);
    let base = color(ground.outline);
    let marks = ledger_marks(levels, current, tiers, length / unit);
    let point = |along: f32, across: f32| {
        if vertical {
            Vector2::new(rect.origin.x + across * unit, rect.origin.y + along * unit)
        } else {
            Vector2::new(rect.origin.x + along * unit, rect.origin.y + across * unit)
        }
    };
    let segment = |from: f32, to: f32, width: f32| {
        let (a, b) = (point(from, 10.0), point(to, 10.0));
        let w = width * unit;
        if vertical {
            Rect::from_xywh(a.x - w / 2.0, a.y, w, b.y - a.y)
        } else {
            Rect::from_xywh(a.x, a.y - w / 2.0, b.x - a.x, w)
        }
    };
    if let (Some(first), Some(last)) = (marks.first(), marks.last()) {
        scene.shape(Shape::new(segment(first.at, last.at, 1.0)).fill(base));
        if let Some(reached) = marks.iter().rev().find(|mark| mark.filled) {
            scene.shape(Shape::new(segment(first.at, reached.at, 2.0)).fill(tint));
        }
    }
    for mark in &marks {
        let centre = point(mark.at, 10.0);
        let (fill, edge) = if mark.filled {
            (tint, tint)
        } else {
            (empty_fill, empty_edge)
        };
        match mark.shape {
            MarkShape::Square => {
                let side = mark.size * unit;
                let square =
                    Rect::from_xywh(centre.x - side / 2.0, centre.y - side / 2.0, side, side);
                scene.shape(
                    Shape::new(square)
                        .fill(fill)
                        .stroke(Stroke::new(unit, edge)),
                );
                if mark.current {
                    let side = side + 2.0 * FRAME_GAP * unit;
                    let frame =
                        Rect::from_xywh(centre.x - side / 2.0, centre.y - side / 2.0, side, side);
                    scene.shape(
                        Shape::new(frame).stroke(Stroke::new(0.8 * unit, tint.alpha(tint.a * 0.6))),
                    );
                }
            }
            MarkShape::Diamond => {
                paint_diamond(scene, centre, DIAMOND_HALF * unit, Some(fill), edge, unit);
                if mark.current {
                    let half = (DIAMOND_HALF + FRAME_GAP * std::f32::consts::SQRT_2) * unit;
                    paint_diamond(
                        scene,
                        centre,
                        half,
                        None,
                        tint.alpha(tint.a * 0.6),
                        0.8 * unit,
                    );
                }
            }
        }
    }
}

fn paint_diamond(
    scene: &mut Scene,
    centre: Vector2,
    half: f32,
    fill: Option<Color>,
    edge: Color,
    width: f32,
) {
    let bounds = Rect::from_xywh(centre.x - half, centre.y - half, half * 2.0, half * 2.0);
    let points = vec![
        Vector2::new(0.0, -half),
        Vector2::new(half, 0.0),
        Vector2::new(0.0, half),
        Vector2::new(-half, 0.0),
    ];
    let mut shape = Shape::new(bounds)
        .path(points, true)
        .stroke(Stroke::new(width, edge));
    if let Some(fill) = fill {
        shape = shape.fill(fill);
    }
    scene.shape(shape);
}

/// Paints a compact track: 3-unit squares at a 5-unit pitch, unfilled
/// steps in the track grey, each top step as a small diamond.
pub fn paint_compact(
    scene: &mut Scene,
    rect: Rect,
    levels: u8,
    current: u8,
    tiers: u8,
    colour: ColorToken,
    vertical: bool,
) {
    let unit = if vertical {
        rect.size.width / 3.0
    } else {
        rect.size.height / 3.0
    };
    let tint = color(colour);
    let empty = color(ground.track);
    let count = levels as usize + tiers as usize;
    for index in 0..count {
        let offset = index as f32 * 5.0 * unit;
        let (x, y) = if vertical {
            (rect.origin.x, rect.origin.y + offset)
        } else {
            (rect.origin.x + offset, rect.origin.y)
        };
        let fill = if index <= current as usize {
            tint
        } else {
            empty
        };
        let cell = Rect::from_xywh(x, y, 3.0 * unit, 3.0 * unit);
        if index >= levels as usize {
            let half = 2.0 * unit;
            let centre = cell.center();
            paint_diamond(scene, centre, half, Some(fill), fill, 0.0);
        } else {
            scene.shape(Shape::new(cell).fill(fill));
        }
    }
}
