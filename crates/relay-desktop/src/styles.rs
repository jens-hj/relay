//! Reusable Labelism classes: square, rule-framed controls and the shared
//! chrome, module and typography vocabulary. Typography classes belong on the
//! container around a text leaf, so reactive text inherits them.

use crate::theme::*;
use mosaic::prelude::*;

/// Line height of clipped previews, in logical pixels.
const PREVIEW_LINE: f32 = 17.0;

mosaic::style! {
    // Controls.
    pub(crate) #relay.tree-leaf hover { fill:surface.raised }
    pub(crate) #relay.tree-row hover { fill:surface.raised }
    pub(crate) #relay.tree-control height:{px(30.0)}px min-width:0px shrink:0 align:center justify:center radius:0px
        pad:0px fill:(Color::TRANSPARENT) font-color:ink.muted font-size:{px(12.0)}px
        hover { fill:(Color::TRANSPARENT) } pressed { fill:(Color::TRANSPARENT) }
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-2.0)}) }
    pub(crate) #relay.tree-label width:1fr height:{px(30.0)}px align:center clip
    pub(crate) #relay.tooltip fill:surface.panel radius:0px font-color:ink.fg font-size:{px(12.0)}px
        max-width:{px(320.0)}px pad:{px(9.0)}px stroke:(width:{px(1.0)} color:rule.line)
    pub(crate) #relay.action width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(7.0)}px) fill:surface.panel font-color:ink.fg
        font-size:{px(13.0)}px stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
        hover { fill:surface.raised } pressed { fill:surface.selected }
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(2.0)}) }
        disabled { opacity:0.5 }
    pub(crate) #relay.primary width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(7.0)}px) fill:ink.inverse font-color:ink.on-inverse
        font-size:{px(13.0)}px font-weight:700
        hover { fill:ink.inverse } pressed { fill:ink.inverse }
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(2.0)}) }
        disabled { opacity:0.5 }
    pub(crate) #relay.field fill:surface.panel radius:0px font-color:ink.fg font-size:{px(14.0)}px
        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-1.0)}) }
    pub(crate) #relay.area fill:surface.panel radius:0px font-color:ink.fg font-size:{px(14.0)}px
        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-1.0)}) }
    pub(crate) #relay.scale-stepper width:{px(120.0)}px height:{px(34.0)}px fill:surface.panel radius:0px
        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(2.0)}) } {
        decrement width:{px(34.0)}px height:fill pad:0px align:center justify:center fill:surface.raised font-color:ink.fg radius:0px stroke:(width:{px(1.0)} color:rule.line edges:right) hover { fill:surface.selected }
        field width:{px(52.0)}px height:fill align:center justify:center font-size:{px(14.0)}px font-color:ink.fg
        increment width:{px(34.0)}px height:fill pad:0px align:center justify:center fill:surface.raised font-color:ink.fg radius:0px stroke:(width:{px(1.0)} color:rule.line edges:left) hover { fill:surface.selected }
    }

    pub(crate) #relay.icon-action width:{px(34.0)}px height:{px(34.0)}px shrink:0
        pad:0px radius:0px align:center justify:center fill:surface.panel font-color:ink.fg
        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
        hover { fill:surface.raised } pressed { fill:surface.selected }
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-2.0)}) }
        disabled { opacity:0.4 }
    pub(crate) #relay.header-action height:fill width:max-content shrink:0 radius:0px
        align:center justify:center pad:(horizontal:{px(16.0)}px vertical:0px)
        fill:(Color::TRANSPARENT) font-color:ink.fg font-size:{px(13.0)}px
        stroke:(width:{px(1.0)} color:rule.line edges:right)
        hover { fill:surface.raised } pressed { fill:surface.selected }
        focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-2.0)}) }
        disabled { opacity:0.5 }

    // Chrome and modules.
    // A full-width strip closed by a strong rule (bars, column heads).
    pub(crate) #relay.strip stroke:(width:{px(1.0)} color:rule.line edges:bottom)
    // A header bar cell, separated from the next by a rule.
    pub(crate) #relay.cell height:fill pad:(horizontal:{px(14.0)}px vertical:0px) justify:center
        gap:{px(3.0)}px stroke:(width:{px(1.0)} color:rule.line edges:right)
    // A framed module: panel fill inside a 1px rule.
    pub(crate) #relay.module height:min-content fill:surface.panel
        stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
    // The 30px key header of a module.
    pub(crate) #relay.module-head height:{px(30.0)}px shrink:0 align:center justify:between
        pad:(horizontal:{px(12.0)}px vertical:0px) stroke:(width:{px(1.0)} color:rule.line edges:bottom)

    // A single-line label that keeps its full text and fades out at the
    // right edge when space runs out (as in the text editor example).
    pub(crate) #relay.fade-label width:1fr min-width:0px height:fill
    pub(crate) #relay.fade-line height:fill align:center clip
        mask:linear(to:right stops:(0%:#FFFFFF 82%:#FFFFFF 100%:#FFFFFF00))
    // A multi-line preview clipped to three laid-out lines, fading at the
    // bottom. The line height is set here so the clip follows it exactly.
    pub(crate) #relay.preview height:min-content clip font-size:{px(12.0)}px
        line-height:{px(PREVIEW_LINE)}px max-height:{px(PREVIEW_LINE * 3.0)}px
        mask:linear(to:bottom stops:(0%:#FFFFFF 62%:#FFFFFF 100%:#FFFFFF00))

    // Typography: apply to the container around a text leaf.
    pub(crate) #relay.eyebrow font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
        letter-spacing:{px(0.6)}px
    pub(crate) #relay.caption font-size:{px(12.0)}px font-color:ink.muted
    pub(crate) #relay.value font-size:{px(13.0)}px font-color:ink.fg
    pub(crate) #relay.title font-family:sans-serif font-weight:600 font-color:ink.fg
    pub(crate) #relay.crumb font-family:sans-serif font-weight:600 font-size:{px(17.0)}px
        font-color:ink.fg
    pub(crate) #relay.id-label font-weight:700 font-color:ink.on-inverse
}
