//! Relay's own icons: navigation, window controls and page actions. Agent,
//! harness and tool marks come from relay-ui.

use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayIcons {
        tree-chevron-right:Svg, tree-chevron-down:Svg, board-icon:Svg,
        director-icon:Svg, worker-icon:Svg, gear-icon:Svg, plus-icon:Svg, sliders-icon:Svg,
        window-minimize:Svg, window-maximize:Svg, window-close:Svg,
        command-icon:Svg, more-icon:Svg, connections-icon:Svg, reset-icon:Svg, navigation-icon:Svg,
    }
}

pub fn icons() -> RelayIcons {
    mosaic::theme! { RelayIcons {
        tree-chevron-right:"assets/icons/chevron-right.svg", tree-chevron-down:"assets/icons/chevron-down.svg",
        board-icon:"assets/icons/board.svg", director-icon:"assets/icons/director.svg",
        worker-icon:"assets/icons/worker.svg", gear-icon:"assets/icons/gear.svg",
        plus-icon:"assets/icons/plus.svg", sliders-icon:"assets/icons/sliders.svg",
        window-minimize:"assets/icons/window-minimize.svg", window-maximize:"assets/icons/window-maximize.svg", window-close:"assets/icons/window-close.svg",
        command-icon:"assets/icons/command.svg", more-icon:"assets/icons/more.svg", connections-icon:"assets/icons/connections.svg", reset-icon:"assets/icons/reset.svg", navigation-icon:"assets/icons/navigation.svg",
    } }
}
