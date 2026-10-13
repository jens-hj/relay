//! Shared control classes for application views that compose plain Mosaic
//! nodes: ruled cell buttons, outline actions, fields, modules and the mono
//! type roles. They use the design's tokens, so views keep one language
//! where no composite fits. Typography classes belong on the container
//! around a text leaf, so reactive text inherits them.

use crate::tokens::{ground, px, status};
use mosaic::prelude::*;

/// Line height of clipped previews, in logical pixels.
const PREVIEW_LINE: f32 = 17.0;

mosaic::style! {
    // Cells and controls. Controls fill their cell; neighbours share one
    // hairline.
    pub #relay.tree-leaf hover { fill:ground.hover }
    pub #relay.tree-row hover { fill:ground.hover }
    pub #relay.tree-control height:{px(30.0)}px min-width:0px shrink:0 align:center justify:center radius:0px
        pad:0px fill:(Color::TRANSPARENT) font-color:ground.muted font-size:{px(11.5)}px
        hover { fill:ground.hover } pressed { fill:ground.pressed }
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
    pub #relay.tree-label width:1fr height:{px(30.0)}px align:center clip
    pub #relay.tooltip fill:ground.panel-raised radius:0px font-color:ground.ink font-size:{px(11.5)}px
        max-width:{px(320.0)}px pad:{px(9.0)}px stroke:(width:{px(1.0)} color:ground.rule)
    pub #relay.action width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(7.0)}px) fill:(Color::TRANSPARENT) font-color:ground.ink
        font-size:{px(11.5)}px font-weight:500 stroke:(width:{px(1.0)} color:ground.outline offset:{px(-1.0)})
        hover { fill:ground.hover } pressed { fill:ground.pressed }
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
        disabled { opacity:0.45 }
    pub #relay.primary width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(7.0)}px) fill:status.needs-you font-color:ground.inverse
        font-size:{px(11.5)}px font-weight:600
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(1.0)}) }
        disabled { opacity:0.45 }
    pub #relay.field fill:ground.code radius:0px font-color:ground.ink font-size:{px(12.5)}px
        stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) pad:{px(10.0)}px
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
    pub #relay.area fill:ground.code radius:0px font-color:ground.ink font-size:{px(12.5)}px
        stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)}) pad:{px(10.0)}px
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
    pub #relay.scale-stepper width:1fr min-width:0px height:{px(34.0)}px fill:ground.code radius:0px
        stroke:(width:0px color:ground.rule)
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
        decrement width:{px(34.0)}px height:fill pad:0px align:center justify:center fill:(Color::TRANSPARENT) font-color:ground.ink radius:0px stroke:(width:{px(1.0)} color:ground.rule edges:right) hover { fill:ground.hover }
        field width:1fr min-width:{px(52.0)}px height:fill align:center justify:center font-size:{px(12.5)}px font-color:ground.ink
        increment width:{px(34.0)}px height:fill pad:0px align:center justify:center fill:(Color::TRANSPARENT) font-color:ground.ink radius:0px stroke:(width:{px(1.0)} color:ground.rule edges:left) hover { fill:ground.hover }
    }
    pub #relay.icon-action width:{px(34.0)}px height:{px(34.0)}px shrink:0
        pad:0px radius:0px align:center justify:center fill:(Color::TRANSPARENT) font-color:ground.ink
        stroke:(width:{px(1.0)} color:ground.outline offset:{px(-1.0)})
        hover { fill:ground.hover } pressed { fill:ground.pressed }
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
        disabled { opacity:0.4 }
    pub #relay.header-action height:fill width:max-content shrink:0 radius:0px
        align:center justify:center pad:(horizontal:{px(16.0)}px vertical:0px)
        fill:(Color::TRANSPARENT) font-color:ground.ink font-size:{px(11.5)}px
        stroke:(width:{px(1.0)} color:ground.hairline edges:right)
        hover { fill:ground.hover } pressed { fill:ground.pressed }
        focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) }
        disabled { opacity:0.45 }

    // Chrome and modules.
    // A full-width strip closed by a hairline (bars, column heads).
    pub #relay.strip stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
    // A header bar cell, separated from the next by a hairline.
    pub #relay.cell height:fill pad:(horizontal:{px(14.0)}px vertical:0px) justify:center
        gap:{px(3.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:right)
    // A module shares its parent's ground; one hairline frames it.
    pub #relay.module height:min-content fill:ground.panel
        stroke:(width:{px(1.0)} color:ground.hairline offset:{px(0.5)})
    // The 30px key header of a module.
    pub #relay.module-head height:{px(30.0)}px shrink:0 align:center justify:between
        pad:(horizontal:{px(12.0)}px vertical:0px) stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)

    // A single-line label that keeps its full text and fades out at the
    // right edge when space runs out.
    pub #relay.fade-label width:1fr min-width:0px height:fill
    pub #relay.fade-line height:fill align:center clip
        mask:linear(to:right stops:(0%:#FFFFFF 82%:#FFFFFF 100%:#FFFFFF00))
    // A multi-line preview clipped to three laid-out lines, fading at the
    // bottom. The line height is set here so the clip follows it exactly.
    pub #relay.preview height:min-content clip font-size:{px(11.5)}px
        line-height:{px(PREVIEW_LINE)}px max-height:{px(PREVIEW_LINE * 3.0)}px
        mask:linear(to:bottom stops:(0%:#FFFFFF 62%:#FFFFFF 100%:#FFFFFF00))

    // Mono type roles: apply to the container around a text leaf.
    pub #relay.eyebrow font-size:{px(10.5)}px font-weight:500 font-color:ground.muted text-transform:uppercase
        letter-spacing:{px(0.6)}px
    pub #relay.caption font-size:{px(11.5)}px font-color:ground.muted
    pub #relay.value font-size:{px(12.5)}px font-color:ground.ink
    pub #relay.title font-weight:600 font-color:ground.ink
    // Serif, for the one named moment of a page.
    pub #relay.crumb font-family:serif font-size:{px(20.0)}px font-color:ground.ink
    pub #relay.id-label font-weight:600 font-color:ground.ink
    // Failure text.
    pub #relay.danger font-size:{px(11.5)}px font-color:status.failed
}
