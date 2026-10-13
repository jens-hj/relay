//! Status squares: the one mark for the state of an agent or call.

use crate::Live;
use crate::motion::{Breath, breathe_while};
use crate::tokens::{Signal, ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{LineCap, Scene, Shape, Stroke};
use std::cell::Cell;
use std::rc::Rc;

/// The state of an agent or tool call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentStatus {
    Idle,
    Working,
    Waiting,
    Failed,
    Queued,
    Done,
}

crate::value::live_value!(AgentStatus);

impl AgentStatus {
    pub const ALL: [Self; 6] = [
        Self::Working,
        Self::Waiting,
        Self::Failed,
        Self::Queued,
        Self::Done,
        Self::Idle,
    ];
    pub const ACTIVITY: [Self; 5] = [
        Self::Working,
        Self::Waiting,
        Self::Failed,
        Self::Queued,
        Self::Done,
    ];

    /// The accessible name of the mark.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Working => "Working",
            Self::Waiting => "Waiting for approval",
            Self::Failed => "Failed",
            Self::Queued => "Queued",
            Self::Done => "Done",
        }
    }

    /// The short word for counts and filters.
    pub fn word(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Failed => "failed",
            Self::Queued => "queued",
            Self::Done => "done",
        }
    }

    /// The colour token the mark is drawn in.
    pub fn token(self) -> ColorToken {
        match self {
            Self::Working => Signal::Live.token(),
            Self::Waiting => Signal::NeedsYou.token(),
            Self::Failed => Signal::Failed.token(),
            Self::Idle | Self::Queued | Self::Done => ground.muted,
        }
    }
}

/// A 10px status mark. Working is a solid violet square that breathes;
/// Waiting is amber with a 45° cut corner; Failed is red with a cross;
/// Queued is a hollow outline; Done is a check with no box.
#[component]
pub fn StatusSquare(
    /// The state to show.
    status: Live<AgentStatus>,
) -> Element {
    let label_status = status.clone();
    let square = view! {
        el width:{px(10.0)}px height:{px(10.0)}px shrink:0 role:image
            label:{label_status.get().label().to_string()} {}
    };
    let level = Rc::new(Cell::new(1.0f32));
    {
        let element = square.clone();
        let level = level.clone();
        let status = status.clone();
        let running = Rc::new(Cell::new(false));
        Effect::new(move || {
            let working = status.get() == AgentStatus::Working;
            element.paint_dirty();
            if working && !running.get() {
                running.set(true);
                let watched = status.clone();
                let running = running.clone();
                breathe_while(&element, Breath::LIVE, level.clone(), move || {
                    let keep = watched.get_untracked() == AgentStatus::Working;
                    if !keep {
                        running.set(false);
                    }
                    keep
                });
            }
        });
    }
    square.paint(move |ctx| {
        paint_status(ctx.scene, ctx.rect, status.get_untracked(), level.get());
    });
    square
}

/// Paints a status mark into `rect` (a 10-unit box scaled to the rect).
pub fn paint_status(scene: &mut Scene, rect: Rect, status: AgentStatus, breath: f32) {
    let unit = rect.size.width / 10.0;
    let at = |x: f32, y: f32| Vector2::new(rect.origin.x + x * unit, rect.origin.y + y * unit);
    let local = |x: f32, y: f32| Vector2::new((x - 5.0) * unit, (y - 5.0) * unit);
    let inner = Rect::from_xywh(at(1.0, 1.0).x, at(1.0, 1.0).y, 8.0 * unit, 8.0 * unit);
    let tint = color(status.token());
    match status {
        AgentStatus::Idle => {}
        AgentStatus::Working => {
            scene.shape(Shape::new(inner).fill(tint.alpha(tint.a * breath)));
        }
        AgentStatus::Waiting => {
            let points = [
                local(1.0, 1.0),
                local(6.0, 1.0),
                local(9.0, 4.0),
                local(9.0, 9.0),
                local(1.0, 9.0),
            ];
            scene.shape(Shape::new(rect).path(points.to_vec(), true).fill(tint));
        }
        AgentStatus::Failed => {
            scene.shape(Shape::new(inner).fill(tint));
            let cross = color(ground.inverse);
            for (a, b) in [((3.2, 3.2), (6.8, 6.8)), ((6.8, 3.2), (3.2, 6.8))] {
                scene.shape(
                    Shape::new(rect)
                        .path(vec![local(a.0, a.1), local(b.0, b.1)], false)
                        .stroke(Stroke::new(1.2 * unit, cross)),
                );
            }
        }
        AgentStatus::Queued => {
            let hollow = Rect::from_xywh(at(1.5, 1.5).x, at(1.5, 1.5).y, 7.0 * unit, 7.0 * unit);
            scene.shape(Shape::new(hollow).stroke(Stroke::new(unit, tint)));
        }
        AgentStatus::Done => {
            let points = vec![local(1.5, 5.2), local(3.8, 7.5), local(8.6, 2.6)];
            scene.shape(
                Shape::new(rect)
                    .path(points, false)
                    .stroke(Stroke::new(1.3 * unit, tint).cap(LineCap::Square)),
            );
        }
    }
}
