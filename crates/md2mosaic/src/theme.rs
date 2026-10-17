//! Styling and host integration supplied by the embedding application.
use mosaic::{
    core::Color,
    text::{FontFamily, LineHeight, TextStyle},
};

/// Colors the document paints itself. Default glyphs use the paint passed to
/// [`Document::paint_text`](crate::Document::paint_text).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// Decorations (underline, strikethrough) and default ink.
    pub ink: Color,
    /// Secondary text: list markers, footnote labels, code languages.
    pub muted: Color,
    /// Table dividers, block quote bars and horizontal rules.
    pub rule: Color,
    /// The divider below a table's header row and the overflow indicator.
    pub rule_strong: Color,
    /// Inline code and code block fills.
    pub code: Color,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            ink: Color::from_rgb_hex(0xe6e6e6),
            muted: Color::from_rgb_hex(0x999999),
            rule: Color::from_rgb_hex(0x4d4d4d),
            rule_strong: Color::from_rgb_hex(0x808080),
            code: Color::from_rgb_hex(0x2e2e2e),
        }
    }
}

/// Typography and spacing, in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The base style every run resolves against.
    pub text: TextStyle,
    /// Font sizes for headings 1 through 6.
    pub heading_sizes: [f32; 6],
    /// Weight for headings, strong text and table headers.
    pub bold_weight: u16,
    /// Horizontal offset per nested list or block quote level.
    pub indent: f32,
    /// Vertical space around code blocks, tables and rules.
    pub block_gap: f32,
    /// Padding inside code blocks.
    pub code_padding: f32,
    /// Horizontal and vertical padding inside table cells.
    pub cell_padding: (f32, f32),
    /// Thickness of dividers, rules and quote bars.
    pub rule_width: f32,
    /// Font size of the code block language label.
    pub label_size: f32,
    pub colors: Colors,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: TextStyle::new(13.0)
                .family(FontFamily::Monospace)
                .line_height(LineHeight::Scale(1.7)),
            heading_sizes: [20.0, 19.0, 18.0, 17.0, 16.0, 15.0],
            bold_weight: 700,
            indent: 16.0,
            block_gap: 6.0,
            code_padding: 6.0,
            cell_padding: (8.0, 2.0),
            rule_width: 1.0,
            label_size: 11.0,
            colors: Colors::default(),
        }
    }
}

/// The embedding application's hooks. Every method has a default, so a host
/// only overrides what it needs.
pub trait Host {
    /// The current theme. Queried before each layout, so a host can follow
    /// live theme or UI scale changes; the document re-parses when it differs.
    fn theme(&self) -> Theme {
        Theme::default()
    }
    /// Whether a link destination may become interactive. Rejected links
    /// render as plain text.
    fn allow_link(&self, url: &str) -> bool {
        safe_url(url)
    }
    /// Activate an allowed link, for example by opening a browser.
    fn open_link(&self, _url: &str) {}
}

/// A host with the default theme, the default link policy and no link
/// activation.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultHost;
impl Host for DefaultHost {}

/// The default link policy: absolute `http`, `https` and `mailto` URLs
/// without embedded credentials, whitespace or control characters.
pub fn safe_url(value: &str) -> bool {
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http" | "mailto")
            && url.username().is_empty()
            && url.password().is_none()
    })
}
