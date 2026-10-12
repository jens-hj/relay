//! The icon scheme. Icons are authored with `currentColor`, so they take the
//! colour of the text around them.

use mosaic::prelude::*;

mosaic::scheme! {
    pub UiIcons {
        harness { codex:Svg, claude-code:Svg, copilot:Svg, cursor:Svg },
    }
}

/// The default icon theme. Install it beside a [`UiTheme`](crate::tokens::UiTheme).
pub fn icons() -> UiIcons {
    mosaic::theme! { UiIcons {
        harness {
            codex:"assets/icons/harness-codex.svg",
            claude-code:"assets/icons/harness-claude.svg",
            copilot:"assets/icons/harness-copilot.svg",
            cursor:"assets/icons/harness-cursor.svg",
        },
    } }
}
