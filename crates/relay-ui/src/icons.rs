//! The icon scheme. Icons are authored with `currentColor`, so they take the
//! colour of the text around them.

use mosaic::prelude::*;

mosaic::scheme! {
    pub UiIcons {
        harness { codex:Svg, claude-code:Svg, copilot:Svg, cursor:Svg },
        call { read:Svg, edit:Svg, create:Svg, search:Svg, run:Svg, test:Svg, fetch:Svg, plan:Svg, spawn:Svg, message:Svg, wait:Svg, approval:Svg, result:Svg, other:Svg },
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
    } }
}
