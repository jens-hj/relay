//! Small stroked marks drawn with the scene: icons for cells (plus, up
//! arrow, pause, stop, compaction) and the path line with dim separators.

use crate::prelude::{Ellipsis, EllipsisProps, Tone};
use crate::text::{TextRole, Weight, fit_text, mono_style};
use crate::tokens::{ground, px};
use crate::{UiText, attached};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{LineCap, LineJoin, Scene, Shape, Stroke};

/// A stroked icon: polylines in a `box_size`-unit artwork box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// "+": New agent (12-unit box).
    Plus,
    /// An archive tray with its lid.
    Archive,
    Trash,
    /// An arrow pointing up: Back to root (12-unit box).
    ArrowUp,
    /// Two bars: Pause (14-unit box, filled).
    Pause,
    /// A filled square: Stop (14-unit box).
    Stop,
    /// A filled triangle pointing right: Resume (14-unit box).
    Play,
    /// A rule with arrows pressing onto it from above and below: compaction
    /// (16-unit box).
    Compaction,
    /// A check: connected (12-unit box).
    Check,
    /// A cross: offline (12-unit box).
    Cross,
    /// A chevron pointing down: select (12-unit box).
    ChevronDown,
}

impl Mark {
    fn box_size(self) -> f32 {
        match self {
            Mark::Pause | Mark::Stop | Mark::Play => 14.0,
            Mark::Compaction => 16.0,
            _ => 12.0,
        }
    }
}

/// Paints `mark` into `rect` in `tint`.
pub fn paint_mark(scene: &mut Scene, rect: Rect, mark: Mark, tint: Color) {
    let size = mark.box_size();
    let unit = rect.size.width.min(rect.size.height) / size;
    let centre = rect.center();
    let local = |x: f32, y: f32| Vector2::new((x - size / 2.0) * unit, (y - size / 2.0) * unit);
    let at = |x: f32, y: f32| {
        Vector2::new(
            centre.x + (x - size / 2.0) * unit,
            centre.y + (y - size / 2.0) * unit,
        )
    };
    let bounds = Rect::from_xywh(
        centre.x - size / 2.0 * unit,
        centre.y - size / 2.0 * unit,
        size * unit,
        size * unit,
    );
    let line = |scene: &mut Scene, points: &[(f32, f32)], width: f32| {
        let points: Vec<Vector2> = points.iter().map(|&(x, y)| local(x, y)).collect();
        scene.shape(
            Shape::new(bounds).path(points, false).stroke(
                Stroke::new(width * unit, tint)
                    .cap(LineCap::Square)
                    .join(LineJoin::Miter),
            ),
        );
    };
    let block = |scene: &mut Scene, x: f32, y: f32, w: f32, h: f32| {
        let origin = at(x, y);
        scene.shape(Shape::new(Rect::from_xywh(origin.x, origin.y, w * unit, h * unit)).fill(tint));
    };
    match mark {
        Mark::Plus => {
            line(scene, &[(6.0, 1.0), (6.0, 11.0)], 1.3);
            line(scene, &[(1.0, 6.0), (11.0, 6.0)], 1.3);
        }
        Mark::Archive => {
            line(
                scene,
                &[(2.0, 4.0), (2.0, 10.5), (10.0, 10.5), (10.0, 4.0)],
                1.2,
            );
            line(
                scene,
                &[(1.0, 1.5), (11.0, 1.5), (11.0, 4.0), (1.0, 4.0), (1.0, 1.5)],
                1.2,
            );
            line(scene, &[(4.5, 6.5), (7.5, 6.5)], 1.2);
        }
        Mark::Trash => {
            line(
                scene,
                &[(3.0, 3.0), (3.0, 10.5), (9.0, 10.5), (9.0, 3.0)],
                1.2,
            );
            line(scene, &[(1.5, 2.5), (10.5, 2.5)], 1.2);
            line(
                scene,
                &[(4.5, 2.0), (4.5, 1.0), (7.5, 1.0), (7.5, 2.0)],
                1.2,
            );
            line(scene, &[(5.0, 5.0), (5.0, 8.5)], 1.0);
            line(scene, &[(7.0, 5.0), (7.0, 8.5)], 1.0);
        }
        Mark::ArrowUp => {
            line(scene, &[(6.0, 11.0), (6.0, 2.0)], 1.3);
            line(scene, &[(2.0, 6.0), (6.0, 2.0), (10.0, 6.0)], 1.3);
        }
        Mark::Pause => {
            block(scene, 3.0, 2.5, 2.5, 9.0);
            block(scene, 8.5, 2.5, 2.5, 9.0);
        }
        Mark::Stop => block(scene, 2.5, 2.5, 9.0, 9.0),
        Mark::Play => {
            let points: Vec<Vector2> = [(3.5, 2.5), (11.5, 7.0), (3.5, 11.5)]
                .iter()
                .map(|&(x, y)| local(x, y))
                .collect();
            scene.shape(Shape::new(bounds).path(points, true).fill(tint));
        }
        Mark::Compaction => {
            line(scene, &[(8.0, 1.5), (8.0, 5.5)], 1.4);
            line(scene, &[(5.5, 3.5), (8.0, 6.0), (10.5, 3.5)], 1.4);
            line(scene, &[(2.5, 8.0), (13.5, 8.0)], 1.4);
            line(scene, &[(8.0, 14.5), (8.0, 10.5)], 1.4);
            line(scene, &[(5.5, 12.5), (8.0, 10.0), (10.5, 12.5)], 1.4);
        }
        Mark::Check => line(scene, &[(1.8, 6.2), (4.6, 9.0), (10.4, 3.1)], 1.4),
        Mark::Cross => {
            line(scene, &[(2.5, 2.5), (9.5, 9.5)], 1.4);
            line(scene, &[(9.5, 2.5), (2.5, 9.5)], 1.4);
        }
        Mark::ChevronDown => line(scene, &[(3.0, 4.5), (6.0, 7.5), (9.0, 4.5)], 1.3),
    }
}

/// An element `size` logical pixels square that paints `mark` in `tone`.
/// Decorative: the control around it carries the label.
pub fn mark(mark: Mark, size: f32, tone: ColorToken) -> Element {
    let host = view! {
        el width:{px(size)}px height:{px(size)}px shrink:0 nohit {}
    };
    host.paint(move |ctx| paint_mark(ctx.scene, ctx.rect, mark, color(tone)));
    host
}

/// The part of `segments` joined by " / " that fits `width`, split back
/// into segments; the last kept segment ends in "…" when trimmed.
pub fn fit_path(
    fonts: &mut mosaic::text::FontContext,
    segments: &[String],
    trailing: bool,
    style: &TextStyle,
    width: f32,
) -> Vec<String> {
    let mut joined = segments.join(" / ");
    if trailing && !joined.is_empty() {
        joined.push_str(" /");
    }
    let fitted = fit_text(fonts, &joined, style, width);
    fitted
        .split(" /")
        .map(|part| part.trim().to_string())
        .collect()
}

/// A breadcrumb path in muted mono with dim " / " separators, trimmed with
/// an ellipsis to its width. `trailing` ends it with a separator, as the
/// session header does ("Release 2.4 /").
#[component]
pub fn PathText(
    /// Ancestor names, outermost first.
    segments: Vec<String>,
    #[prop(default = false)] trailing: bool,
    #[prop(default = TextRole::Label)] role: TextRole,
) -> Element {
    let full = {
        let mut text = segments.join(" / ");
        if trailing {
            text.push_str(" /");
        }
        text
    };
    let width = State::new(f32::MAX);
    let host = view! {
        row width:1fr min-width:0px height:min-content clip label:(full.clone()) role:label
            @layout:{move |rect: Rect| width.set(rect.size.width)} {}
    };
    let shown = Derived::new(move || {
        let available = width.get();
        match attached() {
            Some(ui) if available < f32::MAX => {
                let fonts = ui.fonts();
                let mut fonts = fonts.borrow_mut();
                fit_path(
                    &mut fonts,
                    &segments,
                    trailing,
                    &mono_style(role.size(), Weight::REGULAR),
                    available,
                )
            }
            _ => {
                let mut parts = segments.clone();
                if trailing {
                    parts.push(String::new());
                }
                parts
            }
        }
    });
    host.switch(
        move || shown.get(),
        move |parent, parts| {
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    parent.adopt(&separator(role));
                }
                if !part.is_empty() {
                    let part = part.clone();
                    parent.adopt(&view! {
                        text font-family:monospace font-size:{px(role.size())}px
                            font-color:ground.muted shrink:0 (part)
                    });
                }
            }
        },
    );
    host
}

fn separator(role: TextRole) -> Element {
    view! {
        text font-family:monospace font-size:{px(role.size())}px font-color:ground.rule-strong
            shrink:0 " / "
    }
}

/// Text that is fixed or reactive, at an exact size, weight and colour.
pub(crate) fn sized_text(text: UiText, size: f32, weight: u16, tone: ColorToken) -> Element {
    view! {
        text font-family:monospace font-size:{px(size)}px font-weight:(weight) font-color:(tone)
            shrink:0 {text.get()}
    }
}

/// The shaped width of `text` in a mono role and weight, or `None` without
/// an attached [`Ui`].
pub fn text_width(text: &str, size: f32, weight: Weight) -> Option<f32> {
    let ui = attached()?;
    let fonts = ui.fonts();
    let width = fonts
        .borrow_mut()
        .shape(text, &mono_style(size, weight), None)
        .metrics()
        .size
        .width;
    Some(width.ceil())
}

/// A single line that is as wide as its text when there is room and trims
/// with an ellipsis when there is not, so whatever follows it sits right
/// after the text (a name followed by a count).
pub fn hugging_ellipsis(text: String, role: TextRole, tone: Tone, weight: Weight) -> Element {
    let measured = text.clone();
    let host = view! {
        row height:min-content min-width:0px shrink:1 {
            Ellipsis text:(text) role:(role) tone:(tone) weight:(weight)
        }
    };
    host.style_dyn(move || {
        let width = text_width(&measured, role.size(), weight).map(|natural| natural + 1.0);
        Style::row()
            .height(Dimension::MinContent)
            .min_width(0.0)
            .shrink(1.0)
            .width(width.map_or(Dimension::Fill, Dimension::from))
    });
    host
}
