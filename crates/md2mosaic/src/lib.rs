//! CommonMark and GitHub Flavored Markdown rendered as read-only, selectable
//! Mosaic text.
//!
//! A [`Document`] parses Markdown with `pulldown-cmark`, lays it out as
//! blocks of shaped text (paragraphs, lists, quotes, code blocks, tables and
//! rules) and paints them into a Mosaic [`Scene`](mosaic::render::Scene).
//! Every displayed character maps back to the Markdown bytes it came from,
//! so a host can select rendered text and copy or quote the original source.
//!
//! The crate owns no application state. A [`Host`] supplies the [`Theme`],
//! the link policy and link activation:
//!
//! ```no_run
//! use md2mosaic::{Document, Host, Theme};
//! use std::rc::Rc;
//!
//! struct App;
//! impl Host for App {
//!     fn theme(&self) -> Theme {
//!         Theme::default()
//!     }
//!     fn open_link(&self, url: &str) {
//!         println!("open {url}");
//!     }
//! }
//!
//! let mut fonts = mosaic::text::FontContext::embedded_only();
//! let mut document = Document::new("| a | b |\n|---|--:|\n| 1 | 2 |", Rc::new(App));
//! let size = document.size(&mut fonts, Some(480.0));
//! # let _ = size;
//! ```
//!
//! Painting is split so hosts can interleave their own layers: call
//! [`Document::paint_under`], then [`Document::fill`] for selections or
//! search matches, then [`Document::paint_text`].
//!
//! Raw HTML is shown as literal text and never interpreted. Images are shown
//! as their alt text (or URL), linked to the image when the link policy
//! allows it.

mod document;
mod parse;
mod theme;

pub use document::Document;
pub use parse::{Alignment, CodeBlock, Place, Row, Run, Table, Tone, find_urls, options};
pub use theme::{Colors, DefaultHost, Host, Theme, safe_url};

/// The active selection ends of a read-only source surface.
///
/// [`EditBuffer`](mosaic::text::EditBuffer)'s byte setter normalizes
/// direction. Keep the remembered active end for pointer drags and Shift
/// navigation, while honoring selections set elsewhere (such as select all).
pub fn selection(editor: &mosaic::text::EditBuffer, remembered: (usize, usize)) -> (usize, usize) {
    let sorted = (
        remembered.0.min(remembered.1),
        remembered.0.max(remembered.1),
    );
    if editor.selection_bytes() == sorted {
        remembered
    } else {
        editor.selection_bytes()
    }
}

#[cfg(test)]
mod tests;
