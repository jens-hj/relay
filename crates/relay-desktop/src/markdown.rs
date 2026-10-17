//! Relay's host for transcript Markdown: design tokens and link opening.
//! Parsing, layout and source mapping live in `md2mosaic`.
use md2mosaic::{Colors, Document, Host, Theme};
use mosaic::{
    core::theme::color,
    text::{FontFamily, LineHeight, TextStyle},
};
use relay_ui::tokens::{ground, px};
use std::rc::Rc;

pub use md2mosaic::{safe_url, selection};

struct Relay;

impl Host for Relay {
    fn theme(&self) -> Theme {
        theme()
    }
    fn open_link(&self, url: &str) {
        open_url(url);
    }
}

/// Transcript Markdown styling at the current UI scale and theme.
pub fn theme() -> Theme {
    Theme {
        text: TextStyle::new(px(13.0))
            .family(FontFamily::Monospace)
            .line_height(LineHeight::Scale(1.7)),
        heading_sizes: [20.0, 19.0, 18.0, 17.0, 16.0, 15.0].map(px),
        bold_weight: 700,
        indent: px(16.0),
        block_gap: px(6.0),
        code_padding: px(6.0),
        cell_padding: (px(8.0), px(2.0)),
        rule_width: px(1.0),
        label_size: px(11.0),
        colors: Colors {
            ink: color(ground.ink),
            muted: color(ground.muted),
            rule: color(ground.rule),
            rule_strong: color(ground.rule_strong),
            code: color(ground.hover),
        },
    }
}

/// A transcript Markdown document using Relay's theme and link handling.
pub fn document(source: &str) -> Document {
    Document::new(source, Rc::new(Relay))
}

pub fn open_url(value: &str) {
    if !safe_url(value) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window() {
        let _ =
            window.open_with_url_and_target_and_features(value, "_blank", "noopener,noreferrer");
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = open::that_detached(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosaic::text::FontContext;

    fn fonts() -> FontContext {
        mosaic::core::theme::install(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::Dark,
            1.0,
        ));
        let mut fonts = FontContext::embedded_only();
        relay_ui::fonts::install(&mut fonts);
        fonts
    }

    #[test]
    fn ui_scale_changes_refresh_cached_styles_and_geometry() {
        let mut fonts = fonts();
        let mut doc = document("# Heading\n\n`code`");
        let before = doc.size(&mut fonts, Some(600.0)).height;
        mosaic::core::theme::install(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::Dark,
            2.0,
        ));
        let after = doc.size(&mut fonts, Some(600.0)).height;
        assert!(after > before * 1.8);
    }

    #[test]
    fn transcript_tables_render_with_relay_fonts_inside_the_width() {
        let mut fonts = fonts();
        let source = "| Worktree | Size | Status |\n|---|---|---|\n| relay-1 | 1.2 GB | clean |";
        let mut doc = document(source);
        assert!(!doc.display_text().contains('|'));
        let size = doc.size(&mut fonts, Some(320.0));
        assert!(size.width <= 320.0 && size.height > 0.0);
        let link = document("[x](relay:execute) <https://example.com>");
        assert!(
            link.runs()
                .iter()
                .all(|run| run.link.as_deref() != Some("relay:execute"))
        );
        assert!(link.runs().iter().any(|run| run.link.is_some()));
    }
}
