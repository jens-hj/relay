use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayTheme {
        base:Color, sidebar:Color, surface:Color, raised:Color,
        ink:Color, muted:Color, edge:Color, accent:Color, accent-soft:Color,
        success:Color, warning:Color, danger:Color, ui-scale:Scalar = 1,
        tree-chevron-right:Svg, tree-chevron-down:Svg, board-icon:Svg,
        harness-codex:Svg, harness-claude:Svg, harness-ready:Svg, harness-warning:Svg, harness-failed:Svg, harness-neutral:Svg,
        director-icon:Svg, worker-icon:Svg, gear-icon:Svg, plus-icon:Svg, sliders-icon:Svg, command-icon:Svg,
    }
}

#[cfg(test)]
pub fn palette(light: bool) -> RelayTheme {
    configured_palette(light, false, 1.0)
}

pub fn px(value: f32) -> f32 {
    value * mosaic::core::theme::scalar(ui_scale)
}

pub fn configured_palette(light: bool, alternate: bool, scale: f32) -> RelayTheme {
    let hex = Color::from_rgb_hex;
    let base_color = hex(if light {
        if alternate { 0xF8F5EF } else { 0xF6F7F9 }
    } else if alternate {
        0x171717
    } else {
        0x12151B
    });
    let sidebar_color = hex(if light {
        if alternate { 0xEFEAE0 } else { 0xECEFF4 }
    } else if alternate {
        0x111111
    } else {
        0x0E1117
    });
    let surface_color = hex(if light {
        0xFFFFFF
    } else if alternate {
        0x222222
    } else {
        0x1B2029
    });
    let raised_color = hex(if light {
        if alternate { 0xEAE4D8 } else { 0xE8EDF5 }
    } else if alternate {
        0x303030
    } else {
        0x272F3C
    });
    let ink_color = hex(if light { 0x1C2433 } else { 0xE7ECF5 });
    let muted_color = hex(if light { 0x647087 } else { 0x96A3B9 });
    let edge_color = hex(if light { 0xD8DFEA } else { 0x303A4A });
    let accent_color = hex(if light { 0x405CDA } else { 0x91A8FF });
    let accent_soft_color = hex(if light { 0xE7EDFF } else { 0x273252 });
    let danger_color = hex(if light { 0xB92B45 } else { 0xFF97A8 });
    let success_color = hex(if light { 0x237C48 } else { 0x79D99B });
    let warning_color = hex(if light { 0x8C5A09 } else { 0xEAC16C });
    mosaic::theme! { RelayTheme {
        base:base_color, sidebar:sidebar_color, surface:surface_color, raised:raised_color,
        ink:ink_color, muted:muted_color, edge:edge_color, accent:accent_color,
        accent-soft:accent_soft_color, success:success_color, warning:warning_color, danger:danger_color, ui-scale:scale,
        tree-chevron-right:"assets/icons/chevron-right.svg", tree-chevron-down:"assets/icons/chevron-down.svg",
        board-icon:"assets/icons/board.svg", director-icon:"assets/icons/director.svg",
        worker-icon:"assets/icons/worker.svg", gear-icon:"assets/icons/gear.svg",
        harness-codex:"assets/icons/harness-codex.svg", harness-claude:"assets/icons/harness-claude.svg", harness-ready:"assets/icons/harness-ready.svg", harness-warning:"assets/icons/harness-warning.svg", harness-failed:"assets/icons/harness-failed.svg", harness-neutral:"assets/icons/harness-neutral.svg",
        plus-icon:"assets/icons/plus.svg", sliders-icon:"assets/icons/sliders.svg", command-icon:"assets/icons/command.svg",
    } }
}

mosaic::style! {
    pub #tree-leaf hover { fill:raised }
    pub #tree-row radius:{px(5.0)}px hover { fill:raised }
    pub #scale-stepper width:{px(160.0)}px height:{px(36.0)}px fill:surface radius:{px(6.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) } {
        decrement fill:raised font-color:ink hover { fill:accent-soft }
        field font-size:{px(14.0)}px font-color:ink
        increment fill:raised font-color:ink hover { fill:accent-soft }
    }
    pub #tree-control height:{px(34.0)}px min-width:0px shrink:0 justify:center radius:{px(5.0)}px
        pad:0px fill:(Color::TRANSPARENT) font-color:muted font-size:{px(12.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-2.0)}) }
    pub #tree-label width:1fr height:{px(34.0)}px align:center clip
    pub #tree-tooltip fill:surface font-color:ink font-size:{px(12.0)}px max-width:{px(320.0)}px
        pad:{px(9.0)}px radius:{px(6.0)}px stroke:(width:{px(1.0)} color:edge)
    pub #action width:max-content height:min-content shrink:0 radius:{px(7.0)}px pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px) fill:raised font-color:ink font-size:{px(13.0)}px
        hover { fill:accent-soft } focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) }
        disabled { opacity:0.45 }
    pub #input-field
        fill:surface font-color:ink font-size:{px(14.0)}px stroke:(width:{px(1.0)} color:edge offset:{px(-1.0)})
        radius:{px(7.0)}px pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-1.0)}) }
    pub #area
        fill:surface font-color:ink font-size:{px(14.0)}px stroke:(width:{px(1.0)} color:edge offset:{px(-1.0)})
        radius:{px(7.0)}px pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-1.0)}) }
}
