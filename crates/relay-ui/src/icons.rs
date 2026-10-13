//! The icon scheme. Icons are authored with `currentColor`, so they take the
//! colour of the text around them.

use crate::tokens::px;
use mosaic::prelude::*;
use mosaic::widgets::{IconStyle, icon_dyn};

mosaic::scheme! {
    pub UiIcons {
        harness { codex:Svg, claude-code:Svg, copilot:Svg, cursor:Svg },
        call { read:Svg, edit:Svg, create:Svg, search:Svg, run:Svg, test:Svg, fetch:Svg, plan:Svg, spawn:Svg, message:Svg, wait:Svg, approval:Svg, result:Svg, other:Svg },
        /// Command palette categories.
        command { navigate:Svg, profile:Svg, create:Svg, search:Svg, sync:Svg, worker:Svg, settings:Svg, project:Svg, board:Svg },
        /// Execution approval modes: automatic, ask each time, unrestricted.
        execution { automatic:Svg, ask:Svg, unrestricted:Svg },
    }
}

/// The default icon theme. Install it beside a [`UiTheme`](crate::tokens::UiTheme).
pub fn icons() -> UiIcons {
    mosaic::theme! { UiIcons {
        call {
            read:"assets/icons/tool-read.svg", edit:"assets/icons/tool-edit.svg",
            create:"assets/icons/tool-create.svg", search:"assets/icons/tool-search.svg",
            run:"assets/icons/tool-run.svg", test:"assets/icons/tool-test.svg",
            fetch:"assets/icons/tool-fetch.svg", plan:"assets/icons/tool-plan.svg",
            spawn:"assets/icons/tool-spawn.svg", message:"assets/icons/tool-message.svg",
            wait:"assets/icons/tool-wait.svg", approval:"assets/icons/tool-approval.svg",
            result:"assets/icons/tool-result.svg",
            other:"assets/icons/tool-other.svg",
        },
        harness {
            codex:"assets/icons/harness-codex.svg",
            claude-code:"assets/icons/harness-claude.svg",
            copilot:"assets/icons/harness-copilot.svg",
            cursor:"assets/icons/harness-cursor.svg",
        },
        command {
            navigate:"assets/icons/command-navigate.svg", profile:"assets/icons/command-profile.svg",
            create:"assets/icons/command-create.svg", search:"assets/icons/command-search.svg",
            sync:"assets/icons/command-sync.svg", worker:"assets/icons/command-worker.svg",
            settings:"assets/icons/command-settings.svg",
            project:"assets/icons/command-project.svg", board:"assets/icons/command-board.svg",
        },
        execution {
            automatic:"assets/icons/approval-automatic.svg", ask:"assets/icons/approval-ask.svg",
            unrestricted:"assets/icons/approval-unrestricted.svg",
        },
    } }
}

/// An icon drawn `size` logical pixels square (before UI scale) in the
/// surrounding text colour.
pub fn icon_mark(token: SvgToken, size: f32) -> Element {
    let host = view! {
        el width:{px(size)}px height:{px(size)}px shrink:0 nohit {}
    };
    let glyph = icon_dyn(&host, move || token, IconStyle::default());
    glyph.style_dyn(move || Style::default().size(px(size), px(size)));
    host
}
