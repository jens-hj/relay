//! Text: mono roles, serif titles, single-line ellipsis, and vertical
//! labels.

use crate::tokens::{Signal, ground, px};
use crate::{UiText, attached};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::ImageData;
use mosaic::text::FontContext;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Mono type roles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextRole {
    /// 9.5px: bar captions ("5h", "wk").
    Micro,
    /// 10.5px: counts and secondary values.
    Caption,
    /// 11.5px: labels, paths and buttons.
    Label,
    /// 12.5px: names and values.
    #[default]
    Body,
    /// 13px: message text.
    BodyLarge,
}

impl TextRole {
    /// Size in logical pixels before UI scale.
    pub fn size(self) -> f32 {
        match self {
            TextRole::Micro => 9.5,
            TextRole::Caption => 10.5,
            TextRole::Label => 11.5,
            TextRole::Body => 12.5,
            TextRole::BodyLarge => 13.0,
        }
    }
}

/// A font weight: 400, 500 or 600 for Geist Mono. Accepts an integer
/// literal at call sites (`weight:500`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Weight(pub u16);

impl Weight {
    pub const REGULAR: Weight = Weight(400);
    pub const MEDIUM: Weight = Weight(500);
    pub const SEMIBOLD: Weight = Weight(600);
}

impl From<i32> for Weight {
    fn from(value: i32) -> Self {
        Weight(value.clamp(100, 900) as u16)
    }
}

impl From<u16> for Weight {
    fn from(value: u16) -> Self {
        Weight(value)
    }
}

/// Text colour roles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Tone {
    #[default]
    Ink,
    Muted,
    Faint,
    Signal(Signal),
    /// Any other colour token, such as a tool or model colour.
    Colour(ColorToken),
}

impl Tone {
    pub fn token(self) -> ColorToken {
        match self {
            Tone::Ink => ground.ink,
            Tone::Muted => ground.muted,
            Tone::Faint => ground.faint,
            Tone::Signal(signal) => signal.token(),
            Tone::Colour(token) => token,
        }
    }
}

/// The text style of a mono role at the current UI scale.
pub fn mono_style(role_size: f32, weight: Weight) -> TextStyle {
    TextStyle::new(px(role_size))
        .family(FontFamily::Monospace)
        .weight(weight.0)
}

/// A line of Geist Mono text in a type role and tone.
#[component]
pub fn Label(
    /// The text.
    text: UiText,
    /// Size role; body by default.
    #[prop(default = TextRole::Body)]
    role: TextRole,
    /// Colour role; ink by default.
    #[prop(default = Tone::Ink)]
    tone: Tone,
    /// Font weight: 400, 500 or 600.
    #[prop(default = Weight::REGULAR)]
    weight: Weight,
) -> Element {
    let token = tone.token();
    view! {
        text font-family:monospace font-size:{px(role.size())}px font-weight:(weight.0)
            font-color:(token) {text.get()}
    }
}

/// Instrument Serif sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SerifSize {
    /// 17px: named sections ("Needs you", "All agents").
    Small,
    /// 20px: panel titles ("Agents", "Context").
    #[default]
    Medium,
    /// 26px: session titles.
    Large,
}

impl SerifSize {
    pub fn size(self) -> f32 {
        match self {
            SerifSize::Small => 17.0,
            SerifSize::Medium => 20.0,
            SerifSize::Large => 26.0,
        }
    }
}

/// A title in Instrument Serif, reserved for named moments: one per
/// component.
#[component]
pub fn SerifTitle(
    /// The title.
    text: UiText,
    #[prop(default = SerifSize::Medium)] size: SerifSize,
    /// Set in italic.
    #[prop(default = false)]
    italic: bool,
    #[prop(default = Tone::Ink)] tone: Tone,
) -> Element {
    let token = tone.token();
    let style = if italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    view! {
        text font-family:serif font-size:{px(size.size())}px font-style:(style) font-color:(token)
            {text.get()}
    }
}

/// The longest prefix of `text` that fits `width` with "…" appended, or
/// `text` itself when it fits. Measured with the shaped advance of
/// `style`.
pub fn fit_text(fonts: &mut FontContext, text: &str, style: &TextStyle, width: f32) -> String {
    let mut measure = |candidate: &str| fonts.shape(candidate, style, None).metrics().size.width;
    if measure(text) <= width + 0.01 {
        return text.to_string();
    }
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .collect();
    let candidate = |count: usize| format!("{}…", text[..bounds[count]].trim_end());
    let (mut low, mut high) = (0usize, bounds.len() - 1);
    while low < high {
        let mid = (low + high).div_ceil(2);
        if measure(&candidate(mid)) <= width + 0.01 {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    candidate(low)
}

/// A single line that trims with "…" instead of wrapping.
///
/// Mosaic has no text truncation, so the line is measured with the
/// attached [`Ui`]'s font context (see [`crate::install`]) at its laid-out
/// width and the shown text is the longest prefix that fits. Without an
/// attached `Ui` it falls back to clipping with a fade at the right edge,
/// the technique Relay's desktop used before.
#[component]
pub fn Ellipsis(
    /// The full text; also the accessible label.
    text: UiText,
    #[prop(default = TextRole::Body)] role: TextRole,
    #[prop(default = Tone::Ink)] tone: Tone,
    #[prop(default = Weight::REGULAR)] weight: Weight,
) -> Element {
    let token = tone.token();
    let full = text.clone();
    let Some(ui) = attached() else {
        return view! {
            row width:1fr min-width:0px height:min-content clip
                mask:linear(to:right stops:(0%:#FFFFFF 82%:#FFFFFF 100%:#FFFFFF00)) {
                text font-family:monospace font-size:{px(role.size())}px font-weight:(weight.0)
                    font-color:(token) {full.get()}
            }
        };
    };
    let width = State::new(0.0f32);
    let shown = Derived::new(move || {
        let text = text.get();
        let available = width.get();
        let style = mono_style(role.size(), weight);
        let fonts = ui.fonts();
        let mut fonts = fonts.borrow_mut();
        fit_text(&mut fonts, &text, &style, available)
    });
    view! {
        row width:1fr min-width:0px height:min-content clip label:{full.get()}
            @layout:{move |rect: Rect| width.set(rect.size.width)} {
            text font-family:monospace font-size:{px(role.size())}px font-weight:(weight.0)
                font-color:(token) {shown.get()}
        }
    }
}

/// The last rasterised vertical label and its logical size.
type CachedLabel = (VerticalKey, Arc<ImageData>, Size);

#[derive(Clone, PartialEq)]
struct VerticalKey {
    text: String,
    size: f32,
    weight: Weight,
    spacing: f32,
    tint: [u8; 4],
    scale: f32,
}

/// Rasterises `text` horizontally into straight-alpha RGBA at `scale`.
/// Returns the image and its logical size.
fn rasterise(
    fonts: &mut FontContext,
    text: &str,
    style: &TextStyle,
    tint: Color,
    scale: f32,
) -> Option<(ImageData, Size)> {
    let shaped = fonts.shape(text, style, None);
    let metrics = shaped.metrics();
    let size = Size::new(metrics.size.width.ceil(), metrics.line_height.ceil());
    let (pw, ph) = (
        (size.width * scale).ceil() as usize,
        (size.height * scale).ceil() as usize,
    );
    if pw == 0 || ph == 0 {
        return None;
    }
    let run = shaped.place(fonts, tint, Vector2::ZERO, scale);
    let mut coverage = vec![0u8; pw * ph];
    for glyph in &run.glyphs {
        let image = &glyph.image;
        let left = (glyph.dest.origin.x * scale).round() as i64;
        let top = (glyph.dest.origin.y * scale).round() as i64;
        for gy in 0..image.height as i64 {
            for gx in 0..image.width as i64 {
                let (x, y) = (left + gx, top + gy);
                if x < 0 || y < 0 || x >= pw as i64 || y >= ph as i64 {
                    continue;
                }
                let value = image.data[(gy * image.width as i64 + gx) as usize];
                let slot = &mut coverage[y as usize * pw + x as usize];
                *slot = (*slot).max(value);
            }
        }
    }
    let [r, g, b, a] = tint.to_srgb8();
    let mut data = vec![0u8; pw * ph * 4];
    for (index, value) in coverage.into_iter().enumerate() {
        if value > 0 {
            let alpha = (value as u32 * a as u32 / 255) as u8;
            data[index * 4..index * 4 + 4].copy_from_slice(&[r, g, b, alpha]);
        }
    }
    Some((
        ImageData::new(pw as u32, ph as u32, data.into_boxed_slice()),
        size,
    ))
}

/// Text set bottom to top, like CSS `writing-mode: vertical-rl` turned 180°
/// (the ledger spine). Mosaic cannot rotate glyph runs, so the line is
/// rasterised into an image with the attached [`Ui`]'s fonts and drawn
/// rotated. Needs [`crate::install`]; without it the label takes no space
/// and draws nothing.
#[component]
pub fn VerticalLabel(
    /// The text; also the accessible label.
    text: UiText,
    /// Size in logical pixels before UI scale.
    #[prop(default = 11.0)]
    size: f32,
    #[prop(default = Weight::SEMIBOLD)] weight: Weight,
    /// Letter spacing as a fraction of the size (0.14 in the ledger).
    #[prop(default = 0.14)]
    tracking: f32,
    #[prop(default = Tone::Ink)] tone: Tone,
) -> Element {
    let named = text.clone();
    let label = view! {
        el width:min-content height:min-content shrink:0 role:label label:{named.get()} {}
    };
    let style = move || mono_style(size, weight).letter_spacing(px(size * tracking));
    let Some(ui) = attached() else {
        return label;
    };
    {
        let ui = ui.clone();
        let text = text.clone();
        let target = label.clone();
        let watched = text.clone();
        Effect::new(move || {
            let _ = (watched.get(), style());
            target.content_dirty();
        });
        let fonts = ui.fonts();
        label.measure(move |_| {
            let text = text.get_untracked();
            let shaped = fonts.borrow_mut().shape(&text, &style(), None);
            let metrics = shaped.metrics();
            Size::new(metrics.line_height.ceil(), metrics.size.width.ceil())
        });
    }
    let cache: Rc<RefCell<Option<CachedLabel>>> = Rc::new(RefCell::new(None));
    let fonts = ui.fonts();
    label.paint(move |ctx| {
        let tint = color(tone.token());
        let key = VerticalKey {
            text: text.get_untracked(),
            size: px(size),
            weight,
            spacing: px(size * tracking),
            tint: tint.to_srgb8(),
            scale: ctx.scale,
        };
        let mut slot = cache.borrow_mut();
        if slot.as_ref().is_none_or(|(cached, _, _)| *cached != key) {
            *slot = rasterise(
                &mut fonts.borrow_mut(),
                &key.text,
                &style(),
                tint,
                ctx.scale,
            )
            .map(|(image, size)| (key, Arc::new(image), size));
        }
        let Some((_, image, size)) = slot.as_ref() else {
            return;
        };
        let centre = ctx.rect.center();
        let dest = Rect::from_xywh(
            centre.x - size.width / 2.0,
            centre.y - size.height / 2.0,
            size.width,
            size.height,
        );
        let source = Rect::from_xywh(0.0, 0.0, image.width() as f32, image.height() as f32);
        ctx.scene
            .image_rotated(image.clone(), source, dest, -std::f32::consts::FRAC_PI_2);
    });
    label
}
