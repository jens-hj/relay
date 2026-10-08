use mosaic::prelude::*;

mosaic::scheme! {
    pub RelayTheme {
        base:Color, sidebar:Color, surface:Color, raised:Color, selected-fill:Color,
        ink:Color, muted:Color, edge:Color, rule:Color, accent:Color, accent-soft:Color,
        inverse:Color, on-inverse:Color,
        run-fill:Color, run-text:Color, on-run:Color,
        attention-fill:Color, attention-text:Color, on-attention:Color,
        tint-lilac:Color, tint-sky:Color, tint-mint:Color, tint-sand:Color, scrim:Color,
        success:Color, warning:Color, danger:Color, ui-scale:Scalar = 1,
        tree-chevron-right:Svg, tree-chevron-down:Svg, board-icon:Svg,
        harness-codex:Svg, harness-claude:Svg, harness-ready:Svg, harness-warning:Svg, harness-failed:Svg, harness-neutral:Svg,
        director-icon:Svg, worker-icon:Svg, gear-icon:Svg, plus-icon:Svg, sliders-icon:Svg, command-icon:Svg,
    }
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
    pub on_inverse: Color,
    pub run: Color,
    pub run_text: Color,
    pub on_run: Color,
    pub attention: Color,
    pub attention_text: Color,
    pub on_attention: Color,
    pub tints: [Color; 4],
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
        on_inverse: hex(n.surface),
        run: hex(0xB5D6C3),
        run_text: hex(0x2F6347),
        on_run: hex(0x1F3328),
        attention: hex(0xF2B8A0),
        attention_text: hex(0x8E3F22),
        on_attention: hex(0x3A2A24),
        tints: [hex(0xD9D3EE), hex(0xCADDED), hex(0xD2E6D8), hex(0xEFE3B8)],
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
        on_inverse: hex(n.base),
        run: hex(0x9CC7AE),
        run_text: hex(0xA7D3B9),
        on_run: hex(0x1B2A21),
        attention: hex(0xE8A78C),
        attention_text: hex(0xEFB39A),
        on_attention: hex(0x2A1D18),
        tints: [hex(0x45475B), hex(0x3F4A5A), hex(0x3F4D4F), hex(0x4F4F47)],
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
            on_inverse: hex(0xEDF0F2),
            run: hex(0xFF4E00),
            run_text: hex(0xA33600),
            on_run: hex(0x15181D),
            attention: hex(0xFF4E00),
            attention_text: hex(0xA33600),
            on_attention: hex(0x15181D),
            tints: [hex(0xC9CED4), hex(0xB8BFC6), hex(0xD7DBDF), hex(0xA9B0B7)],
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
            on_inverse: hex(0x0D0E10),
            run: hex(0xFF5A14),
            run_text: hex(0xFF6A2A),
            on_run: hex(0x0D0E10),
            attention: hex(0xFF5A14),
            attention_text: hex(0xFF6A2A),
            on_attention: hex(0x0D0E10),
            tints: [hex(0x3A3F45), hex(0x4A5057), hex(0x2F3338), hex(0x5A6168)],
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
    mosaic::theme! { RelayTheme {
        base:(c.base), sidebar:(c.sidebar), surface:(c.surface), raised:(c.raised), selected-fill:(c.selected),
        ink:(c.ink), muted:(c.muted), edge:(c.edge), rule:(c.line), accent:(c.accent),
        accent-soft:(c.accent_soft), inverse:(c.inverse), on-inverse:(c.on_inverse),
        run-fill:(c.run), run-text:(c.run_text), on-run:(c.on_run),
        attention-fill:(c.attention), attention-text:(c.attention_text), on-attention:(c.on_attention),
        tint-lilac:lilac, tint-sky:sky, tint-mint:mint, tint-sand:sand,
        scrim:(c.scrim), success:(c.success), warning:(c.warning), danger:(c.danger), ui-scale:scale,
        tree-chevron-right:"assets/icons/chevron-right.svg", tree-chevron-down:"assets/icons/chevron-down.svg",
        board-icon:"assets/icons/board.svg", director-icon:"assets/icons/director.svg",
        worker-icon:"assets/icons/worker.svg", gear-icon:"assets/icons/gear.svg",
        harness-codex:"assets/icons/harness-codex.svg", harness-claude:"assets/icons/harness-claude.svg", harness-ready:"assets/icons/harness-ready.svg", harness-warning:"assets/icons/harness-warning.svg", harness-failed:"assets/icons/harness-failed.svg", harness-neutral:"assets/icons/harness-neutral.svg",
        plus-icon:"assets/icons/plus.svg", sliders-icon:"assets/icons/sliders.svg", command-icon:"assets/icons/command.svg",
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

mosaic::style! {
    pub #tree-leaf hover { fill:raised }
    pub #tree-row hover { fill:raised }
    pub #scale-stepper width:{px(160.0)}px height:{px(36.0)}px fill:surface radius:0px
        stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)})
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) } {
        decrement fill:raised font-color:ink hover { fill:selected-fill }
        field font-size:{px(14.0)}px font-color:ink
        increment fill:raised font-color:ink hover { fill:selected-fill }
    }
    pub #tree-control height:{px(34.0)}px min-width:0px shrink:0 justify:center radius:0px
        pad:0px fill:(Color::TRANSPARENT) font-color:muted font-size:{px(12.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-2.0)}) }
    pub #tree-label width:1fr height:{px(34.0)}px align:center clip
    pub #tree-tooltip fill:surface radius:0px font-color:ink font-size:{px(12.0)}px max-width:{px(320.0)}px
        pad:{px(9.0)}px stroke:(width:{px(1.0)} color:rule)
    pub #action width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px) fill:surface font-color:ink font-size:{px(13.0)}px
        stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)})
        hover { fill:raised } focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) }
        disabled { opacity:0.5 }
    pub #primary width:max-content height:min-content shrink:0 radius:0px
        pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px) fill:inverse font-color:on-inverse font-size:{px(13.0)}px font-weight:700
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) }
        disabled { opacity:0.5 }
    pub #input-field
        fill:surface radius:0px font-color:ink font-size:{px(14.0)}px stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)})
        pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-1.0)}) }
    pub #area
        fill:surface radius:0px font-color:ink font-size:{px(14.0)}px stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)})
        pad:{px(10.0)}px
        focused { stroke:(width:{px(2.0)} color:accent offset:{px(-1.0)}) }
}
