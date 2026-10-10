//! Relay's Labelism palettes: semantic token groups, the six selectable
//! palettes and the embedded icon scheme. Reusable classes live in
//! `styles.rs`.

use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayTheme {
        surface { base:Color, sidebar:Color, panel:Color, raised:Color, selected:Color },
        ink { fg:Color, muted:Color, inverse:Color, inverse-hover:Color, inverse-pressed:Color, on-inverse:Color },
        rule { line:Color, hair:Color },
        accent { focus:Color, soft:Color },
        run { fill:Color, text:Color, on:Color },
        attention { fill:Color, text:Color, on:Color },
        tint { lilac:Color, sky:Color, mint:Color, sand:Color },
        meter { input:Color, cached:Color, output:Color },
        status { success:Color, warning:Color, danger:Color },
        scrim:Color, ui-scale:Scalar = 1,
    }
}

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
/// The six palettes a user can select: two soft families per mode plus the
/// stark high-contrast variant of each mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Paper,
    Warm,
    LightContrast,
    Slate,
    Neutral,
    DarkContrast,
}

impl Palette {
    #[cfg(test)]
    pub const ALL: [Self; 6] = [
        Self::Paper,
        Self::Warm,
        Self::LightContrast,
        Self::Slate,
        Self::Neutral,
        Self::DarkContrast,
    ];

    pub fn select(light: bool, alternate: bool, high_contrast: bool) -> Self {
        match (light, high_contrast, alternate) {
            (true, true, _) => Self::LightContrast,
            (true, false, false) => Self::Paper,
            (true, false, true) => Self::Warm,
            (false, true, _) => Self::DarkContrast,
            (false, false, false) => Self::Slate,
            (false, false, true) => Self::Neutral,
        }
    }
}

/// Resolved colors of one palette. Fill-only roles (`run`, `attention`,
/// tints, `selected`, `accent_soft`) carry text in their paired `on_*`
/// color or `ink`; text-grade roles (`muted`, `*_text`, status colors) are
/// readable on every neutral background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub base: Color,
    pub sidebar: Color,
    pub surface: Color,
    pub raised: Color,
    pub selected: Color,
    pub ink: Color,
    pub muted: Color,
    pub edge: Color,
    pub line: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub inverse: Color,
    pub inverse_hover: Color,
    pub inverse_pressed: Color,
    pub on_inverse: Color,
    pub run: Color,
    pub run_text: Color,
    pub on_run: Color,
    pub attention: Color,
    pub attention_text: Color,
    pub on_attention: Color,
    pub tints: [Color; 4],
    /// Usage meter fills: uncached input (blue), cached input (lavender),
    /// output (peach).
    pub meter: [Color; 3],
    pub scrim: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

#[cfg(test)]
impl Colors {
    /// Backgrounds that ordinary text, status words and glyphs may sit on.
    pub fn neutrals(&self) -> [Color; 5] {
        [
            self.base,
            self.sidebar,
            self.surface,
            self.raised,
            self.selected,
        ]
    }
}

struct Neutrals {
    base: u32,
    sidebar: u32,
    surface: u32,
    raised: u32,
    selected: u32,
    ink: u32,
    muted: u32,
    edge: u32,
    line: u32,
    inverse: u32,
    inverse_hover: u32,
    inverse_pressed: u32,
}

fn soft_light(n: Neutrals) -> Colors {
    let hex = Color::from_rgb_hex;
    Colors {
        base: hex(n.base),
        sidebar: hex(n.sidebar),
        surface: hex(n.surface),
        raised: hex(n.raised),
        selected: hex(n.selected),
        ink: hex(n.ink),
        muted: hex(n.muted),
        edge: hex(n.edge),
        line: hex(n.line),
        accent: hex(0x4F63C2),
        accent_soft: hex(0xD7DCF1),
        inverse: hex(n.inverse),
        inverse_hover: hex(n.inverse_hover),
        inverse_pressed: hex(n.inverse_pressed),
        on_inverse: hex(n.surface),
        run: hex(0xB5D6C3),
        run_text: hex(0x2F6347),
        on_run: hex(0x1F3328),
        attention: hex(0xF2B8A0),
        attention_text: hex(0x8E3F22),
        on_attention: hex(0x3A2A24),
        tints: [hex(0xCFC0EC), hex(0xB8D5EE), hex(0xBDE0C9), hex(0xEDD797)],
        meter: [hex(0x8FA9CF), hex(0xB3A7DE), hex(0xF2B8A0)],
        scrim: Color::from_srgb8(0x1C, 0x1E, 0x22, 0x66),
        success: hex(0x2F6347),
        warning: hex(0x6E4E08),
        danger: hex(0x8F3330),
    }
}

fn soft_dark(n: Neutrals) -> Colors {
    let hex = Color::from_rgb_hex;
    Colors {
        base: hex(n.base),
        sidebar: hex(n.sidebar),
        surface: hex(n.surface),
        raised: hex(n.raised),
        selected: hex(n.selected),
        ink: hex(n.ink),
        muted: hex(n.muted),
        edge: hex(n.edge),
        line: hex(n.line),
        accent: hex(0x9DB0F0),
        accent_soft: hex(0x313A58),
        inverse: hex(n.inverse),
        inverse_hover: hex(n.inverse_hover),
        inverse_pressed: hex(n.inverse_pressed),
        on_inverse: hex(n.base),
        run: hex(0x9CC7AE),
        run_text: hex(0xA7D3B9),
        on_run: hex(0x1B2A21),
        attention: hex(0xE8A78C),
        attention_text: hex(0xEFB39A),
        on_attention: hex(0x2A1D18),
        tints: [hex(0x51466D), hex(0x344F72), hex(0x315C4C), hex(0x635332)],
        meter: [hex(0x8DA3BD), hex(0xB4ABD6), hex(0xE8A78C)],
        scrim: Color::from_srgb8(0x08, 0x09, 0x0B, 0x8C),
        success: hex(0xA7D3B9),
        warning: hex(0xE6C37A),
        danger: hex(0xF0A0A0),
    }
}

pub fn colors(palette: Palette) -> Colors {
    let hex = Color::from_rgb_hex;
    match palette {
        Palette::Paper => soft_light(Neutrals {
            base: 0xE9EAEC,
            sidebar: 0xE2E4E7,
            surface: 0xF1F2F3,
            raised: 0xDCDFE3,
            selected: 0xD3DCE8,
            ink: 0x2E3238,
            muted: 0x4E555E,
            edge: 0xCDD1D6,
            line: 0x6A717A,
            inverse: 0x33373D,
            inverse_hover: 0x4B525C,
            inverse_pressed: 0x606A77,
        }),
        Palette::Warm => soft_light(Neutrals {
            base: 0xE8E5DF,
            sidebar: 0xE1DDD5,
            surface: 0xF0EDE8,
            raised: 0xDAD5CC,
            selected: 0xDDD6CA,
            ink: 0x33363B,
            muted: 0x53565C,
            edge: 0xD2CDC4,
            line: 0x6E6B66,
            inverse: 0x33363B,
            inverse_hover: 0x4D5158,
            inverse_pressed: 0x656A73,
        }),
        Palette::Slate => soft_dark(Neutrals {
            base: 0x1C2028,
            sidebar: 0x181B22,
            surface: 0x232833,
            raised: 0x2C3340,
            selected: 0x323B4C,
            ink: 0xE3E6EA,
            muted: 0xA3ABB8,
            edge: 0x343C4A,
            line: 0x7C8696,
            inverse: 0xDCE0E6,
            inverse_hover: 0xBCC4CF,
            inverse_pressed: 0xA5B0C0,
        }),
        Palette::Neutral => soft_dark(Neutrals {
            base: 0x1F2125,
            sidebar: 0x1A1C1F,
            surface: 0x25282C,
            raised: 0x2E3136,
            selected: 0x383B41,
            ink: 0xE2DFD8,
            muted: 0xADACA8,
            edge: 0x383B40,
            line: 0x8C8E92,
            inverse: 0xDCD8D0,
            inverse_hover: 0xBCB7AE,
            inverse_pressed: 0xA6A198,
        }),
        // The stark concept variant: ink rules and one orange signal. Orange
        // is a fill; text and glyph strokes use the text-grade orange.
        Palette::LightContrast => Colors {
            base: hex(0xE4E7EA),
            sidebar: hex(0xDDE1E5),
            surface: hex(0xEDF0F2),
            raised: hex(0xD5DADF),
            selected: hex(0xD0D6DD),
            ink: hex(0x15181D),
            muted: hex(0x50585F),
            edge: hex(0xB9C0C7),
            line: hex(0x15181D),
            accent: hex(0x2B4BC7),
            accent_soft: hex(0xCBD2F2),
            inverse: hex(0x15181D),
            inverse_hover: hex(0x343B45),
            inverse_pressed: hex(0x4D5865),
            on_inverse: hex(0xEDF0F2),
            run: hex(0xFF4E00),
            run_text: hex(0xA33600),
            on_run: hex(0x15181D),
            attention: hex(0xFF4E00),
            attention_text: hex(0xA33600),
            on_attention: hex(0x15181D),
            tints: [hex(0xC9CED4), hex(0xB8BFC6), hex(0xD7DBDF), hex(0xA9B0B7)],
            meter: [hex(0x4A6FB0), hex(0x8A7CC2), hex(0xFF4E00)],
            scrim: Color::from_srgb8(0x15, 0x18, 0x1D, 0x66),
            success: hex(0x1F5A3C),
            warning: hex(0x664500),
            danger: hex(0x9A2A22),
        },
        Palette::DarkContrast => Colors {
            base: hex(0x0D0E10),
            sidebar: hex(0x0A0B0C),
            surface: hex(0x131518),
            raised: hex(0x1C1F23),
            selected: hex(0x252A30),
            ink: hex(0xE3E6E8),
            muted: hex(0x9AA1A8),
            edge: hex(0x2A2E33),
            line: hex(0xE3E6E8),
            accent: hex(0x7FA0FF),
            accent_soft: hex(0x1E2740),
            inverse: hex(0xE3E6E8),
            inverse_hover: hex(0xC2C8CE),
            inverse_pressed: hex(0xAAB4BF),
            on_inverse: hex(0x0D0E10),
            run: hex(0xFF5A14),
            run_text: hex(0xFF6A2A),
            on_run: hex(0x0D0E10),
            attention: hex(0xFF5A14),
            attention_text: hex(0xFF6A2A),
            on_attention: hex(0x0D0E10),
            tints: [hex(0x3A3F45), hex(0x4A5057), hex(0x2F3338), hex(0x5A6168)],
            meter: [hex(0x7FA0FF), hex(0xB8A8FF), hex(0xFF5A14)],
            scrim: Color::from_srgb8(0x00, 0x00, 0x00, 0x99),
            success: hex(0x8FD3A8),
            warning: hex(0xF0C670),
            danger: hex(0xFF8A80),
        },
    }
}

#[cfg(test)]
pub fn palette(light: bool) -> RelayTheme {
    configured_palette(light, false, false, 1.0)
}

pub fn px(value: f32) -> f32 {
    value * mosaic::core::theme::scalar(ui_scale)
}

pub fn configured_palette(
    light: bool,
    alternate: bool,
    high_contrast: bool,
    scale: f32,
) -> RelayTheme {
    let c = colors(Palette::select(light, alternate, high_contrast));
    let [lilac, sky, mint, sand] = c.tints;
    let [input_fill, cached_fill, output_fill] = c.meter;
    mosaic::theme! { RelayTheme {
        surface { base:(c.base), sidebar:(c.sidebar), panel:(c.surface), raised:(c.raised), selected:(c.selected) },
        ink { fg:(c.ink), muted:(c.muted), inverse:(c.inverse), inverse-hover:(c.inverse_hover), inverse-pressed:(c.inverse_pressed), on-inverse:(c.on_inverse) },
        rule { line:(c.line), hair:(c.edge) },
        accent { focus:(c.accent), soft:(c.accent_soft) },
        run { fill:(c.run), text:(c.run_text), on:(c.on_run) },
        attention { fill:(c.attention), text:(c.attention_text), on:(c.on_attention) },
        tint { lilac:lilac, sky:sky, mint:mint, sand:sand },
        meter { input:input_fill, cached:cached_fill, output:output_fill },
        status { success:(c.success), warning:(c.warning), danger:(c.danger) },
        scrim:(c.scrim), ui-scale:scale,
    } }
}

/// WCAG 2.x contrast ratio between two opaque colors. Mosaic stores linear
/// sRGB, so relative luminance is a weighted sum of the components.
#[cfg(test)]
pub fn contrast(a: Color, b: Color) -> f32 {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}
