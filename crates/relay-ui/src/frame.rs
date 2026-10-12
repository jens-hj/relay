//! The cut-corner frame: the one exception to square corners, used for
//! approvals, the max effort level and harness tabs.

use crate::tokens::{ground, px, srgb_alpha};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::{CornerExponents, CornerRadii};
use mosaic::widgets::Visual;

/// Size of the 45° cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cut {
    /// 10px: labels and small tabs.
    Small,
    /// 12px: cards such as Needs you.
    #[default]
    Card,
    /// 14px: transcript blocks such as approvals.
    Block,
}

impl Cut {
    /// Leg length of the cut in logical pixels, before UI scale.
    pub fn size(self) -> f32 {
        match self {
            Cut::Small => 10.0,
            Cut::Card => 12.0,
            Cut::Block => 14.0,
        }
    }
}

/// Which corner is cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    #[default]
    TopRight,
    BottomRight,
    BottomLeft,
    /// Two opposing cuts for the ledger effort label.
    TopRightBottomLeft,
}

/// Per-corner radii for the selected corner or opposing ledger corners.
pub fn cut_radii(corner: Corner, leg: f32) -> CornerRadii {
    match corner {
        Corner::TopLeft => CornerRadii::new(leg, 0.0, 0.0, 0.0),
        Corner::TopRight => CornerRadii::new(0.0, leg, 0.0, 0.0),
        Corner::BottomRight => CornerRadii::new(0.0, 0.0, leg, 0.0),
        Corner::BottomLeft => CornerRadii::new(0.0, 0.0, 0.0, leg),
        Corner::TopRightBottomLeft => CornerRadii::new(0.0, leg, 0.0, leg),
    }
}

/// The cut leg of a panel inset by `inset` inside a frame cut by `leg`,
/// which keeps the 45° edge an even `inset` wide.
pub fn inset_leg(leg: f32, inset: f32) -> f32 {
    (leg - (2.0 - std::f32::consts::SQRT_2) * inset).max(0.0)
}

/// A frame with 45° cut corners, drawn as an outer shape in the edge
/// colour and an inner panel inset 1px, so the edge stays even along the
/// cut. Children sit in the inner panel.
#[component]
pub fn CutCorner(
    /// Cut size: small 10, card 12 (default), block 14.
    #[prop(default = Cut::Card)]
    cut: Cut,
    /// The cut corner or opposing pair; top-right by default.
    #[prop(default = Corner::TopRight)]
    corner: Corner,
    /// Edge colour.
    #[prop(default = ground.rule)]
    edge: ColorToken,
    /// Edge opacity (the approval edge is amber at 0.32).
    #[prop(default = 1.0)]
    edge_opacity: f32,
    /// Panel colour.
    #[prop(default = ground.panel)]
    fill: ColorToken,
    /// The panel's contents.
    #[prop(optional)]
    children: Children,
) -> Element {
    let panel = view! {
        col grow:1 height:min-content min-width:0px {
            children
        }
    };
    let frame = view! {
        col height:min-content pad:{px(1.0)}px {}
    };
    frame.adopt(&panel);
    let exponents = CornerExponents::all(1.0);
    frame.visual_dyn(move || {
        let tint = color(edge);
        let alpha = srgb_alpha(tint, color(fill), edge_opacity);
        Visual::new()
            .fill(tint.alpha(tint.a * alpha))
            .radii(cut_radii(corner, px(cut.size())))
            .exponents(exponents)
    });
    panel.visual_dyn(move || {
        Visual::new()
            .fill(color(fill))
            .radii(cut_radii(corner, inset_leg(px(cut.size()), px(1.0))))
            .exponents(exponents)
    });
    frame
}
