use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayTheme {
        base:Color, sidebar:Color, surface:Color, raised:Color,
        ink:Color, muted:Color, edge:Color, accent:Color, accent-soft:Color,
        danger:Color,
    }
}

pub fn palette(light: bool) -> RelayTheme {
    let hex = Color::from_rgb_hex;
    let base_color = hex(if light { 0xF6F7F9 } else { 0x12151B });
    let sidebar_color = hex(if light { 0xECEFF4 } else { 0x0E1117 });
    let surface_color = hex(if light { 0xFFFFFF } else { 0x1B2029 });
    let raised_color = hex(if light { 0xE8EDF5 } else { 0x272F3C });
    let ink_color = hex(if light { 0x1C2433 } else { 0xE7ECF5 });
    let muted_color = hex(if light { 0x647087 } else { 0x96A3B9 });
    let edge_color = hex(if light { 0xD8DFEA } else { 0x303A4A });
    let accent_color = hex(if light { 0x405CDA } else { 0x91A8FF });
    let accent_soft_color = hex(if light { 0xE7EDFF } else { 0x273252 });
    let danger_color = hex(if light { 0xB92B45 } else { 0xFF97A8 });
    mosaic::theme! { RelayTheme {
        base:base_color, sidebar:sidebar_color, surface:surface_color, raised:raised_color,
        ink:ink_color, muted:muted_color, edge:edge_color, accent:accent_color,
        accent-soft:accent_soft_color, danger:danger_color,
    } }
}

mosaic::style! {
    pub #action width:max-content height:min-content shrink:0 radius:7px pad:(horizontal:12px vertical:8px) fill:raised font-color:ink font-size:13px
        hover { fill:accent-soft } focused { stroke:(width:2px color:accent offset:2px) }
        disabled { opacity:0.45 }
    pub #input-field
        fill:surface font-color:ink font-size:14px stroke:(width:1px color:edge offset:-1px)
        radius:7px pad:10px
        focused { stroke:(width:2px color:accent offset:-1px) }
    pub #area
        fill:surface font-color:ink font-size:14px stroke:(width:1px color:edge offset:-1px)
        radius:7px pad:10px
        focused { stroke:(width:2px color:accent offset:-1px) }
}
