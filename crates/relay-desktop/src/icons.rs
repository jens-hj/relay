//! Relay's own icons: navigation, window controls and page actions.
//! Agent, harness and tool marks come from relay-ui.

use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayIcons {
        tree-chevron-right:Svg, tree-chevron-down:Svg, board-icon:Svg,
        harness-codex:Svg, harness-claude:Svg, harness-ready:Svg, harness-warning:Svg,
        harness-failed:Svg, harness-neutral:Svg,
        director-icon:Svg, worker-icon:Svg, gear-icon:Svg, plus-icon:Svg, sliders-icon:Svg,
        window-minimize:Svg, window-maximize:Svg, window-close:Svg,
        tool-terminal:Svg, tool-read:Svg, tool-edit:Svg, tool-search:Svg, tool-web:Svg, tool-agent:Svg, tool-other:Svg,
        command-icon:Svg, more-icon:Svg, connections-icon:Svg, reset-icon:Svg, navigation-icon:Svg,
    }
}

pub fn icons() -> RelayIcons {
    mosaic::theme! { RelayIcons {
        tree-chevron-right:"assets/icons/chevron-right.svg", tree-chevron-down:"assets/icons/chevron-down.svg",
        board-icon:"assets/icons/board.svg", director-icon:"assets/icons/director.svg",
        worker-icon:"assets/icons/worker.svg", gear-icon:"assets/icons/gear.svg",
        harness-codex:"assets/icons/harness-codex.svg", harness-claude:"assets/icons/harness-claude.svg",
        harness-ready:"assets/icons/harness-ready.svg", harness-warning:"assets/icons/harness-warning.svg",
        harness-failed:"assets/icons/harness-failed.svg", harness-neutral:"assets/icons/harness-neutral.svg",
        plus-icon:"assets/icons/plus.svg", sliders-icon:"assets/icons/sliders.svg",
        window-minimize:"assets/icons/window-minimize.svg", window-maximize:"assets/icons/window-maximize.svg", window-close:"assets/icons/window-close.svg",
        tool-terminal:"assets/icons/tool-terminal.svg", tool-read:"assets/icons/tool-read.svg", tool-edit:"assets/icons/tool-edit.svg", tool-search:"assets/icons/tool-search.svg", tool-web:"assets/icons/tool-web.svg", tool-agent:"assets/icons/tool-agent.svg", tool-other:"assets/icons/tool-other.svg",
        command-icon:"assets/icons/command.svg", more-icon:"assets/icons/more.svg", connections-icon:"assets/icons/connections.svg", reset-icon:"assets/icons/reset.svg", navigation-icon:"assets/icons/navigation.svg",
    } }
}
