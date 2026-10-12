//! Design tokens: the `UiTheme` scheme, its four variants, the UI-scale
//! helper, gauge thresholds, and the resolved Rust-side palettes for code
//! that paints imperatively.
//!
//! The dark variant uses the exact `design/orchestrator-ui/tokens.json`
//! values. The light and high-contrast variants keep every semantic role and
//! hue identity, and are checked by the contrast tests in `tests.rs`.

use mosaic::prelude::*;

mosaic::scheme! {
    /// Every colour and size the orchestrator design names.
    pub UiTheme {
        /// Structure and text. Never used for signals.
        ground {
            page:Color, panel:Color, panel-raised:Color, code:Color,
            hairline:Color, rule:Color, rule-strong:Color,
            ink:Color, muted:Color, faint:Color,
            /// Outlines of secondary controls and chips.
            outline:Color,
            /// Unfilled steps of compact tracks.
            track:Color,
            /// The selected row.
            selected:Color,
            /// Cell hover and pressed washes.
            hover:Color, pressed:Color,
            /// Text on signal fills (the amber primary button).
            inverse:Color,
        },
        /// State of an agent or call.
        status { live:Color, needs-you:Color, failed:Color, full-steam:Color },
        /// Which model thinks.
        model { sol:Color, opus:Color, sonnet:Color, haiku:Color },
        /// Which tool type was called.
        tool {
            plan:Color, create:Color, edit:Color, test:Color, search:Color, fetch:Color,
            read:Color, run:Color, spawn:Color, message:Color, wait:Color, result:Color,
            approval:Color,
        },
        /// Greys of the context breakdown.
        context {
            system:Color, files-read:Color, conversation:Color, sub-agent-results:Color,
            compacted:Color,
        },
        /// Type sizes in logical pixels, before `ui-scale`.
        text-size {
            micro:Scalar = 9.5, caption:Scalar = 10.5, label:Scalar = 11.5, body:Scalar = 12.5,
            body-large:Scalar = 13, title:Scalar = 26, display:Scalar = 92,
        },
        /// Geometry in logical pixels, before `ui-scale`.
        shape {
            cut-small:Scalar = 10, cut-card:Scalar = 12, cut-block:Scalar = 14,
            status-square:Scalar = 10, hairline:Scalar = 1, indent:Scalar = 14,
            /// Tree depth after which a branch folds.
            fold-depth:Scalar = 4,
        },
        /// Motion periods in seconds.
        motion { live-period:Scalar = 2.4, glow-period:Scalar = 3.6, hover-seconds:Scalar = 0.15 },
        ui-scale:Scalar = 1,
    }
}

/// Multiplies a logical size by the installed `ui-scale`. Reactive when read
/// inside a binding, so scaled layouts follow a scale change.
pub fn px(value: f32) -> f32 {
    value * mosaic::core::theme::scalar(ui_scale)
}

/// The four selectable variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeVariant {
    Dark,
    Light,
    DarkContrast,
    LightContrast,
}

impl ThemeVariant {
    pub const ALL: [Self; 4] = [
        Self::Dark,
        Self::Light,
        Self::DarkContrast,
        Self::LightContrast,
    ];

    /// The variant for a mode and contrast preference.
    pub fn select(light: bool, high_contrast: bool) -> Self {
        match (light, high_contrast) {
            (false, false) => Self::Dark,
            (true, false) => Self::Light,
            (false, true) => Self::DarkContrast,
            (true, true) => Self::LightContrast,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::DarkContrast => "Dark, high contrast",
            Self::LightContrast => "Light, high contrast",
        }
    }

    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &Palette::DARK,
            Self::Light => &Palette::LIGHT,
            Self::DarkContrast => &Palette::DARK_CONTRAST,
            Self::LightContrast => &Palette::LIGHT_CONTRAST,
        }
    }
}

/// Resolved ground colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    pub page: Color,
    pub panel: Color,
    pub panel_raised: Color,
    pub code: Color,
    pub hairline: Color,
    pub rule: Color,
    pub rule_strong: Color,
    pub ink: Color,
    pub muted: Color,
    pub faint: Color,
    pub outline: Color,
    pub track: Color,
    pub selected: Color,
    pub hover: Color,
    pub pressed: Color,
    pub inverse: Color,
}

impl Ground {
    /// Opaque surfaces that text and marks sit on.
    pub fn surfaces(&self) -> [Color; 5] {
        [
            self.page,
            self.panel,
            self.panel_raised,
            self.code,
            self.selected,
        ]
    }
}

/// Resolved status colours, indexed like [`Signal`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Statuses {
    pub live: Color,
    pub needs_you: Color,
    pub failed: Color,
    pub full_steam: Color,
}

/// One theme variant's colours, for imperative painting and tests. Views
/// should prefer the scheme tokens so they follow theme switches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub ground: Ground,
    pub status: Statuses,
    /// Indexed by [`ModelFamily`].
    pub model: [Color; 4],
    /// Indexed by [`ToolKind`].
    pub tool: [Color; 13],
    /// Indexed by [`ContextPart`].
    pub context: [Color; 5],
}

const fn hex(value: u32) -> Color {
    Color::from_rgb_hex(value)
}

const fn white(alpha: u8) -> Color {
    Color::from_srgb8(0xFF, 0xFF, 0xFF, alpha)
}

const fn black(alpha: u8) -> Color {
    Color::from_srgb8(0x00, 0x00, 0x00, alpha)
}

impl Palette {
    /// The canonical palette: exact design-pack values.
    pub const DARK: Palette = Palette {
        ground: Ground {
            page: hex(0x0D0D0F),
            panel: hex(0x161618),
            panel_raised: hex(0x1C1C1F),
            code: hex(0x0F0F11),
            hairline: hex(0x26262B),
            rule: hex(0x2E2E33),
            rule_strong: hex(0x56555A),
            ink: hex(0xEDEBE6),
            muted: hex(0x8F8C86),
            faint: hex(0x6E6C68),
            outline: hex(0x3A3A40),
            track: hex(0x34343A),
            selected: hex(0x202024),
            hover: white(0x0A),
            pressed: white(0x12),
            inverse: hex(0x111113),
        },
        status: Statuses {
            live: hex(0xA5A1F2),
            needs_you: hex(0xE3B266),
            failed: hex(0xE07A6E),
            full_steam: hex(0x9BD7A9),
        },
        model: [hex(0x65C98C), hex(0xDF92D8), hex(0x48BFF5), hex(0xB3B94F)],
        tool: [
            hex(0xCBC07B),
            hex(0xB7C684),
            hex(0xA1CC93),
            hex(0x8CD0A7),
            hex(0x7BD1BB),
            hex(0x72D0D0),
            hex(0x74CDE1),
            hex(0x81C8EF),
            hex(0x93C2F7),
            hex(0xD6ACE4),
            hex(0xE4A8D2),
            hex(0xEEA6BD),
            hex(0xE3B266),
        ],
        context: [
            hex(0xEDEBE6),
            hex(0xB9B7B1),
            hex(0x8F8C86),
            hex(0x64625E),
            hex(0x46454A),
        ],
    };

    /// Warm off-white ground with the same roles; signal hues darkened to
    /// stay readable as text.
    pub const LIGHT: Palette = Palette {
        ground: Ground {
            page: hex(0xF4F2EE),
            panel: hex(0xFAF9F6),
            panel_raised: hex(0xECEAE4),
            code: hex(0xF7F6F2),
            hairline: hex(0xE0DDD6),
            rule: hex(0xD3CFC7),
            rule_strong: hex(0xA19D95),
            ink: hex(0x1C1B19),
            muted: hex(0x5E5B55),
            faint: hex(0x7A766F),
            outline: hex(0xC9C5BD),
            track: hex(0xD6D2CA),
            selected: hex(0xE6E3DC),
            hover: black(0x0A),
            pressed: black(0x12),
            inverse: hex(0xFFFFFF),
        },
        status: Statuses {
            live: hex(0x5A52C8),
            needs_you: hex(0x8A5A0E),
            failed: hex(0xB23A2A),
            full_steam: hex(0x26703F),
        },
        model: [hex(0x1B6F41), hex(0x9A3F93), hex(0x0B6A9C), hex(0x646812)],
        tool: [
            hex(0x6E6418),
            hex(0x5C6A22),
            hex(0x42703A),
            hex(0x27704C),
            hex(0x126E5E),
            hex(0x0F6C6C),
            hex(0x136A80),
            hex(0x1F6493),
            hex(0x3A5FA6),
            hex(0x85509A),
            hex(0x964A80),
            hex(0xA4466A),
            hex(0x8A5A0E),
        ],
        context: [
            hex(0x1C1B19),
            hex(0x4D4A45),
            hex(0x7A766F),
            hex(0xA8A49C),
            hex(0xC7C3BB),
        ],
    };

    /// Darker ground, white ink and brighter signals.
    pub const DARK_CONTRAST: Palette = Palette {
        ground: Ground {
            page: hex(0x08080A),
            panel: hex(0x0E0E10),
            panel_raised: hex(0x151518),
            code: hex(0x0A0A0C),
            hairline: hex(0x3A3A40),
            rule: hex(0x46464C),
            rule_strong: hex(0x7A7980),
            ink: hex(0xFFFFFF),
            muted: hex(0xBAB7B0),
            faint: hex(0x95928B),
            outline: hex(0x55555C),
            track: hex(0x46464C),
            selected: hex(0x26262B),
            hover: white(0x12),
            pressed: white(0x1C),
            inverse: hex(0x000000),
        },
        status: Statuses {
            live: hex(0xBBB8FF),
            needs_you: hex(0xF2C27A),
            failed: hex(0xF5938A),
            full_steam: hex(0xACE8BA),
        },
        model: [hex(0x7FE0A5), hex(0xF2AAEA), hex(0x74D2FF), hex(0xCBD166)],
        tool: [
            hex(0xDDD394),
            hex(0xCAD89B),
            hex(0xB6DDAA),
            hex(0xA3E1BC),
            hex(0x93E2CE),
            hex(0x8BE1E1),
            hex(0x8CDEF0),
            hex(0x9AD8FA),
            hex(0xABD3FF),
            hex(0xE3C0EF),
            hex(0xF0BDE1),
            hex(0xF8BACD),
            hex(0xF2C27A),
        ],
        context: [
            hex(0xFFFFFF),
            hex(0xCFCCC6),
            hex(0xA3A09A),
            hex(0x77756F),
            hex(0x55545A),
        ],
    };

    /// Near-white ground, black ink and deep signals.
    pub const LIGHT_CONTRAST: Palette = Palette {
        ground: Ground {
            page: hex(0xFCFBF8),
            panel: hex(0xFFFFFF),
            panel_raised: hex(0xF2F0EA),
            code: hex(0xF7F5F0),
            hairline: hex(0xBDB9B1),
            rule: hex(0xA9A59D),
            rule_strong: hex(0x5E5A54),
            ink: hex(0x000000),
            muted: hex(0x3D3A35),
            faint: hex(0x5A5650),
            outline: hex(0x8E8A83),
            track: hex(0xC2BEB6),
            selected: hex(0xE4E0D8),
            hover: black(0x10),
            pressed: black(0x1A),
            inverse: hex(0xFFFFFF),
        },
        status: Statuses {
            live: hex(0x4038B0),
            needs_you: hex(0x6E4706),
            failed: hex(0x96291C),
            full_steam: hex(0x1C5A31),
        },
        model: [hex(0x145A33), hex(0x7C2F76), hex(0x054F78), hex(0x4A4D08)],
        tool: [
            hex(0x575010),
            hex(0x48541A),
            hex(0x33592D),
            hex(0x1C593B),
            hex(0x0B584A),
            hex(0x095656),
            hex(0x0D5466),
            hex(0x164F76),
            hex(0x2C4A86),
            hex(0x6A3E7B),
            hex(0x783A66),
            hex(0x843654),
            hex(0x6E4706),
        ],
        context: [
            hex(0x000000),
            hex(0x3D3A35),
            hex(0x6B6760),
            hex(0x9A968E),
            hex(0xBDB9B1),
        ],
    };

    pub fn model(&self, family: ModelFamily) -> Color {
        self.model[family as usize]
    }

    pub fn tool(&self, kind: ToolKind) -> Color {
        self.tool[kind as usize]
    }

    pub fn context(&self, part: ContextPart) -> Color {
        self.context[part as usize]
    }

    pub fn signal(&self, signal: Signal) -> Color {
        match signal {
            Signal::Live => self.status.live,
            Signal::NeedsYou => self.status.needs_you,
            Signal::Failed => self.status.failed,
            Signal::FullSteam => self.status.full_steam,
        }
    }
}

/// The status colour roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signal {
    Live,
    NeedsYou,
    Failed,
    FullSteam,
}

impl Signal {
    pub const ALL: [Self; 4] = [Self::Live, Self::NeedsYou, Self::Failed, Self::FullSteam];

    /// The scheme token, for views and paint closures that follow the theme.
    pub fn token(self) -> ColorToken {
        match self {
            Self::Live => status.live,
            Self::NeedsYou => status.needs_you,
            Self::Failed => status.failed,
            Self::FullSteam => status.full_steam,
        }
    }
}

/// Model families with an identity colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelFamily {
    Sol,
    Opus,
    Sonnet,
    Haiku,
}

impl ModelFamily {
    pub const ALL: [Self; 4] = [Self::Sol, Self::Opus, Self::Sonnet, Self::Haiku];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sol => "Sol",
            Self::Opus => "Opus",
            Self::Sonnet => "Sonnet",
            Self::Haiku => "Haiku",
        }
    }

    pub fn token(self) -> ColorToken {
        match self {
            Self::Sol => model.sol,
            Self::Opus => model.opus,
            Self::Sonnet => model.sonnet,
            Self::Haiku => model.haiku,
        }
    }
}

/// The thirteen tool categories, in fingerprint order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Plan,
    Create,
    Edit,
    Test,
    Search,
    Fetch,
    Read,
    Run,
    Spawn,
    Message,
    Wait,
    Result,
    Approval,
}

impl ToolKind {
    pub const ALL: [Self; 13] = [
        Self::Plan,
        Self::Create,
        Self::Edit,
        Self::Test,
        Self::Search,
        Self::Fetch,
        Self::Read,
        Self::Run,
        Self::Spawn,
        Self::Message,
        Self::Wait,
        Self::Result,
        Self::Approval,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Create => "Create",
            Self::Edit => "Edit",
            Self::Test => "Test",
            Self::Search => "Search",
            Self::Fetch => "Fetch",
            Self::Read => "Read",
            Self::Run => "Run",
            Self::Spawn => "Spawn",
            Self::Message => "Message",
            Self::Wait => "Wait",
            Self::Result => "Result",
            Self::Approval => "Approval",
        }
    }

    pub fn token(self) -> ColorToken {
        match self {
            Self::Plan => tool.plan,
            Self::Create => tool.create,
            Self::Edit => tool.edit,
            Self::Test => tool.test,
            Self::Search => tool.search,
            Self::Fetch => tool.fetch,
            Self::Read => tool.read,
            Self::Run => tool.run,
            Self::Spawn => tool.spawn,
            Self::Message => tool.message,
            Self::Wait => tool.wait,
            Self::Result => tool.result,
            Self::Approval => tool.approval,
        }
    }
}

/// Parts of the context breakdown, densest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextPart {
    System,
    FilesRead,
    Conversation,
    SubAgentResults,
    Compacted,
}

impl ContextPart {
    pub const ALL: [Self; 5] = [
        Self::System,
        Self::FilesRead,
        Self::Conversation,
        Self::SubAgentResults,
        Self::Compacted,
    ];

    pub fn token(self) -> ColorToken {
        match self {
            Self::System => context.system,
            Self::FilesRead => context.files_read,
            Self::Conversation => context.conversation,
            Self::SubAgentResults => context.sub_agent_results,
            Self::Compacted => context.compacted,
        }
    }
}

/// Builds the scheme theme for a variant at a UI scale.
pub fn theme(variant: ThemeVariant, scale: f32) -> UiTheme {
    let p = variant.palette();
    let g = p.ground;
    let s = p.status;
    let [sol, opus, sonnet, haiku] = p.model;
    let [
        plan,
        create,
        edit,
        test,
        search,
        fetch,
        read,
        run,
        spawn,
        message,
        wait,
        result,
        approval,
    ] = p.tool;
    let [
        system,
        files_read,
        conversation,
        sub_agent_results,
        compacted,
    ] = p.context;
    mosaic::theme! { UiTheme {
        ground {
            page:(g.page), panel:(g.panel), panel-raised:(g.panel_raised), code:(g.code),
            hairline:(g.hairline), rule:(g.rule), rule-strong:(g.rule_strong),
            ink:(g.ink), muted:(g.muted), faint:(g.faint), outline:(g.outline), track:(g.track),
            selected:(g.selected), hover:(g.hover), pressed:(g.pressed), inverse:(g.inverse),
        },
        status { live:(s.live), needs-you:(s.needs_you), failed:(s.failed), full-steam:(s.full_steam) },
        model { sol:sol, opus:opus, sonnet:sonnet, haiku:haiku },
        tool {
            plan:plan, create:create, edit:edit, test:test, search:search, fetch:fetch,
            read:read, run:run, spawn:spawn, message:message, wait:wait, result:result,
            approval:approval,
        },
        context {
            system:system, files-read:files_read, conversation:conversation,
            sub-agent-results:sub_agent_results, compacted:compacted,
        },
        ui-scale:scale,
    } }
}

/// The canonical theme at scale 1.
pub fn theme_dark() -> UiTheme {
    theme(ThemeVariant::Dark, 1.0)
}

pub fn theme_light() -> UiTheme {
    theme(ThemeVariant::Light, 1.0)
}

pub fn theme_dark_contrast() -> UiTheme {
    theme(ThemeVariant::DarkContrast, 1.0)
}

pub fn theme_light_contrast() -> UiTheme {
    theme(ThemeVariant::LightContrast, 1.0)
}

/// WCAG 2.x contrast ratio between two opaque colours. Mosaic stores linear
/// sRGB, so relative luminance is a weighted sum of the components.
pub fn contrast(a: Color, b: Color) -> f32 {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Thresholds shared by every gauge: ink while there is room, amber from
/// 80%, red from 95%.
pub mod thresholds {
    use super::{ColorToken, Signal, ground};

    pub const AMBER_FROM: f32 = 80.0;
    pub const RED_FROM: f32 = 95.0;

    /// How full a gauge is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Level {
        Room,
        Amber,
        Red,
    }

    impl Level {
        /// The ink, amber or red token for this level.
        pub fn token(self) -> ColorToken {
            match self {
                Level::Room => ground.ink,
                Level::Amber => Signal::NeedsYou.token(),
                Level::Red => Signal::Failed.token(),
            }
        }
    }

    /// The level of a percentage used.
    pub fn level(percent: f32) -> Level {
        if percent >= RED_FROM {
            Level::Red
        } else if percent >= AMBER_FROM {
            Level::Amber
        } else {
            Level::Room
        }
    }
}

/// The alpha that, composited in linear light as Mosaic does, matches a
/// CSS-style sRGB blend of `tint` over `under` at `alpha`. The design
/// boards specify their washes and tinted edges as sRGB blends, which read
/// several times weaker than the same alpha blended linearly.
pub fn srgb_alpha(tint: Color, under: Color, alpha: f32) -> f32 {
    fn encode(linear: f32) -> f32 {
        if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }
    }
    fn decode(encoded: f32) -> f32 {
        if encoded <= 0.040_45 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    }
    let weights = [0.2126, 0.7152, 0.0722];
    let channels = [(tint.r, under.r), (tint.g, under.g), (tint.b, under.b)];
    let (mut target, mut span) = (0.0f32, 0.0f32);
    for ((t, g), w) in channels.into_iter().zip(weights) {
        let mixed = decode(encode(g) + (encode(t) - encode(g)) * alpha);
        target += w * (mixed - g);
        span += w * (t - g);
    }
    if span.abs() < 1e-6 {
        alpha
    } else {
        (target / span).clamp(0.0, 1.0)
    }
}
