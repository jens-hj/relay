//! Harness glyphs: terminal (Codex), square braces (Claude Code), paper
//! plane (Copilot) and pointer (Cursor).

use crate::Live;
use crate::tokens::px;
use mosaic::prelude::*;
use mosaic::widgets::{IconStyle, icon_dyn};

/// An agent harness with a glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Harness {
    Codex,
    ClaudeCode,
    Copilot,
    Cursor,
}

crate::value::live_value!(Harness);

impl Harness {
    pub const ALL: [Self; 4] = [Self::Codex, Self::ClaudeCode, Self::Copilot, Self::Cursor];

    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::Copilot => "Copilot",
            Self::Cursor => "Cursor",
        }
    }

    /// The icon token of the glyph.
    pub fn icon(self) -> SvgToken {
        match self {
            Self::Codex => crate::icons::harness.codex,
            Self::ClaudeCode => crate::icons::harness.claude_code,
            Self::Copilot => crate::icons::harness.copilot,
            Self::Cursor => crate::icons::harness.cursor,
        }
    }

    /// The identity colour of its usage bars: terracotta for Claude Code,
    /// blue for Codex, ink for harnesses without one.
    pub fn tint(self) -> ColorToken {
        match self {
            Self::Codex => crate::tokens::harness.codex,
            Self::ClaudeCode => crate::tokens::harness.claude_code,
            Self::Copilot | Self::Cursor => crate::tokens::ground.ink,
        }
    }
}

/// A harness glyph drawn in the surrounding text colour (24-unit artwork,
/// 1.8 stroke, square caps, mitred joins). Labelled with the harness name.
#[component]
pub fn HarnessGlyph(
    /// Which harness.
    harness: Live<Harness>,
    /// Edge length in logical pixels before UI scale.
    #[prop(default = 14.0)]
    size: f32,
) -> Element {
    let named = harness.clone();
    let host = view! {
        el width:{px(size)}px height:{px(size)}px shrink:0 align:center justify:center role:image
            label:{named.get().name().to_string()} {}
    };
    let glyph = icon_dyn(&host, move || harness.get().icon(), IconStyle::default());
    glyph.style_dyn(move || Style::default().size(px(size), px(size)));
    host
}
