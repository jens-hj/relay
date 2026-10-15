//! Read-only Markdown projection. Source bytes remain the document's authority.
use mosaic::{
    core::{Rect, Vector2},
    text::{FontContext, FontFamily, FontStyle, ShapedText, TextRun, TextStyle},
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use relay_ui::tokens::px;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

// EditBuffer's public byte setter normalizes direction. Keep the active end for
// read-only pointer drags and Shift navigation, while honoring external selections.
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

pub struct Run {
    pub text: String,
    pub source: Range<usize>,
    pub display: Range<usize>,
    pub style: TextStyle,
    pub code: bool,
    pub link: Option<String>,
    source_points: Vec<(usize, usize)>,
}
#[derive(Default)]
pub struct Document {
    source: String,
    pub runs: Vec<Run>,
    shaped: Option<ShapedText>,
    width: Option<f32>,
    scale: f32,
}
impl Document {
    pub fn new(source: &str) -> Self {
        let mut doc = Self {
            source: source.into(),
            scale: px(1.0),
            ..Self::default()
        };
        let mut stack = vec![(TextStyle::inherited(), false, None)];
        let mut lists = Vec::new();
        for (event, range) in
            Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH).into_offset_iter()
        {
            let (style, code, link) = stack.last().unwrap().clone();
            match event {
                Event::Start(tag) => {
                    let mut next = (style, code, link);
                    match tag {
                        Tag::Strong => next.0 = next.0.weight(700),
                        Tag::Emphasis => next.0 = next.0.style(FontStyle::Italic),
                        Tag::Strikethrough => next.0 = next.0.strikethrough(true),
                        Tag::Heading { level, .. } => {
                            next.0 = TextStyle::new(px(21.0 - level as u8 as f32)).weight(700)
                        }
                        Tag::CodeBlock(_) => {
                            next.0 = next.0.family(FontFamily::Monospace);
                            next.1 = true;
                        }
                        Tag::Link { dest_url, .. } => {
                            if safe_url(&dest_url) {
                                next.2 = Some(dest_url.into_string());
                                next.0 = next.0.underline(true);
                            }
                        }
                        Tag::List(start) => lists.push(start),
                        Tag::Item => {
                            if doc.runs.last().is_some_and(|run| !run.text.ends_with('\n')) {
                                doc.push("\n".into(), range.start..range.start, &next);
                            }
                            let marker = match lists.last_mut() {
                                Some(Some(n)) => {
                                    let marker = format!("{n}. ");
                                    *n += 1;
                                    marker
                                }
                                _ => "• ".into(),
                            };
                            doc.push(
                                format!("{}{marker}", "  ".repeat(lists.len().saturating_sub(1))),
                                range.start..range.start,
                                &next,
                            );
                        }
                        _ => {}
                    }
                    stack.push(next);
                }
                Event::End(tag) => {
                    if matches!(
                        tag,
                        TagEnd::Paragraph
                            | TagEnd::Heading(_)
                            | TagEnd::CodeBlock
                            | TagEnd::Item
                            | TagEnd::BlockQuote(_)
                    ) && !doc.runs.last().is_some_and(|r| r.text.ends_with('\n'))
                    {
                        // Block separators map to the end of the original block.
                        doc.push("\n".into(), range.end..range.end, &(style, code, link));
                    }
                    if matches!(tag, TagEnd::List(_)) {
                        lists.pop();
                    }
                    stack.pop();
                }
                Event::Text(text) => doc.push(text.into_string(), range, &(style, code, link)),
                Event::Code(text) => {
                    let ticks = source[range.clone()]
                        .bytes()
                        .take_while(|b| *b == b'`')
                        .count();
                    let mut content = range.start + ticks..range.end - ticks;
                    if source[content.clone()].starts_with(' ')
                        && source[content.clone()].ends_with(' ')
                        && source[content.clone()]
                            .chars()
                            .any(|c| !matches!(c, ' ' | '\n' | '\r'))
                    {
                        content.start += 1;
                        content.end -= 1;
                    }
                    doc.push(
                        text.into_string(),
                        content,
                        &(style.family(FontFamily::Monospace), true, link),
                    );
                }
                Event::SoftBreak | Event::HardBreak => {
                    doc.push("\n".into(), range, &(style, code, link))
                }
                // HTML stays inert text. Images remain text; Relay assets are rendered separately.
                Event::Html(text) | Event::InlineHtml(text) => {
                    doc.push(text.into_string(), range, &(style, code, link))
                }
                Event::Rule => doc.push("────────\n".into(), range, &(style, code, link)),
                _ => {}
            }
        }
        if doc.runs.last().is_some_and(|run| {
            run.text == "\n" && run.source.is_empty() && run.source.end == source.len()
        }) {
            doc.runs.pop();
        }
        doc
    }
    fn push(
        &mut self,
        text: String,
        source: Range<usize>,
        state: &(TextStyle, bool, Option<String>),
    ) {
        let start = self.runs.last().map_or(0, |r| r.display.end);
        // Match source bytes in order, allowing newline normalization. Code
        // may additionally omit container prefixes on continuation lines.
        // OffsetIter emits decoded entities as individual spans: treat a
        // transformed span atomically rather than searching its spelling for
        // characters that merely resemble the decoded output.
        let raw = &self.source[source.clone()];
        let mut consumed = 0;
        let mut aligned = true;
        let mut source_points = Vec::new();
        for (display, ch) in text.char_indices() {
            let remaining = &raw[consumed..];
            let next = if remaining.starts_with(ch) {
                Some((0, ch.len_utf8()))
            } else if ch == ' ' && remaining.starts_with(['\r', '\n']) {
                Some((0, 1))
            } else if ch == '\n' && remaining.starts_with('\r') {
                Some((0, if remaining.starts_with("\r\n") { 2 } else { 1 }))
            } else if state.1 {
                remaining
                    .char_indices()
                    .find(|(_, c)| *c == ch || (ch == ' ' && matches!(*c, '\n' | '\r')))
                    .map(|(offset, c)| (offset, c.len_utf8()))
            } else {
                None
            };
            if let Some((offset, length)) = next {
                source_points.push((display, source.start + consumed + offset));
                consumed += offset + length;
            } else {
                aligned = false;
                break;
            }
        }
        if !aligned || (!state.1 && consumed != raw.len()) {
            source_points.clear();
            source_points.push((0, source.start));
        }
        source_points.push((text.len(), source.end));
        self.runs.push(Run {
            display: start..start + text.len(),
            text,
            source,
            style: state.0.clone(),
            code: state.1,
            link: state.2.clone(),
            source_points,
        });
    }
    pub fn update(&mut self, source: &str) {
        if self.source != source || self.scale != px(1.0) {
            *self = Self::new(source);
        }
    }
    pub fn display_offset(&self, source: usize) -> usize {
        for run in &self.runs {
            if source < run.source.start {
                return run.display.start;
            }
            if source < run.source.end {
                let index = run.source_points.partition_point(|(_, raw)| *raw <= source);
                return run.display.start
                    + index
                        .checked_sub(1)
                        .map_or(0, |index| run.source_points[index].0);
            }
        }
        self.runs.last().map_or(0, |r| r.display.end)
    }
    pub fn source_offset(&self, display: usize) -> usize {
        self.offset_with_affinity(display, false)
    }
    fn offset_with_affinity(&self, display: usize, end: bool) -> usize {
        for run in &self.runs {
            if (display < run.display.end || (end && display == run.display.end))
                && !run.text.is_empty()
            {
                let index = run.source_points.partition_point(|(offset, _)| {
                    *offset <= display.saturating_sub(run.display.start)
                });
                return index
                    .checked_sub(1)
                    .map_or(run.source.start, |index| run.source_points[index].1);
            }
        }
        self.runs.last().map_or(0, |r| r.source.end)
    }
    pub fn selection_range(&self, anchor: usize, head: usize) -> (usize, usize) {
        let a = self.display_offset(anchor);
        let b = self.display_offset(head);
        (
            self.offset_with_affinity(a, a > b),
            self.offset_with_affinity(b, b > a),
        )
    }
    pub fn shape(&mut self, fonts: &mut FontContext, width: Option<f32>) -> &ShapedText {
        if self.scale != px(1.0) {
            *self = Self::new(&self.source.clone());
        }
        if self.shaped.is_none() || self.width != width {
            let runs: Vec<_> = self
                .runs
                .iter()
                .map(|r| TextRun {
                    text: &r.text,
                    style: &r.style,
                })
                .collect();
            let base = TextStyle::new(px(13.0))
                .family(FontFamily::Monospace)
                .line_height(mosaic::text::LineHeight::Scale(1.7));
            self.shaped = Some(fonts.shape_runs(&runs, &base, width));
            self.width = width;
        }
        self.shaped.as_ref().unwrap()
    }
    pub fn caret(&mut self, fonts: &mut FontContext, width: f32, source: usize) -> Rect {
        let offset = self.display_offset(source);
        self.shape(fonts, Some(width)).caret_rect(offset)
    }
    pub fn hit(&mut self, fonts: &mut FontContext, width: f32, point: Vector2) -> usize {
        let offset = self.shape(fonts, Some(width)).hit(point).unwrap_or(0);
        self.source_offset(offset)
    }
    pub fn link_at(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        point: Vector2,
    ) -> Option<String> {
        let links = self
            .runs
            .iter()
            .filter_map(|run| run.link.clone().map(|url| (run.display.clone(), url)))
            .collect::<Vec<_>>();
        let shaped = self.shape(fonts, Some(width));
        // Caret hits round the right half of a final glyph to the offset just
        // after the link. Link activation instead uses the actual run bounds,
        // including each wrapped line, without expanding into neighboring text.
        links.into_iter().find_map(|(range, url)| {
            shaped
                .selection_rects(range.start, range.end)
                .iter()
                .any(|rect| rect.contains(point))
                .then_some(url)
        })
    }
    pub fn unit(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        source: usize,
        line: bool,
    ) -> (usize, usize) {
        let offset = self.display_offset(source);
        let shaped = self.shape(fonts, Some(width));
        let range = if line {
            shaped.line_at(offset)
        } else {
            shaped.word_at(offset)
        };
        (
            self.source_offset(range.start),
            self.offset_with_affinity(range.end, true),
        )
    }
    pub fn motion(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        source: usize,
        motion: mosaic::text::CaretMotion,
    ) -> usize {
        use mosaic::text::CaretMotion;
        let offset = self.display_offset(source);
        let shaped = self.shape(fonts, Some(width));
        let text = shaped.text();
        let mut target = match motion {
            CaretMotion::Left => text[..offset]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(i, _)| i),
            CaretMotion::Right => text[offset..]
                .graphemes(true)
                .next()
                .map_or(offset, |c| offset + c.len()),
            CaretMotion::LineStart => shaped.line_at(offset).start,
            CaretMotion::LineEnd => shaped.line_at(offset).end,
            CaretMotion::WordLeft => {
                shaped
                    .word_at(
                        text[..offset]
                            .char_indices()
                            .next_back()
                            .map_or(0, |(i, _)| i),
                    )
                    .start
            }
            CaretMotion::WordRight => {
                // Match Mosaic's Unicode/AppKit word boundaries, but seek a
                // word ending strictly after the caret. word_at includes the
                // word's right edge for double-click selection and can stall.
                let separated: String = text
                    .chars()
                    .map(|c| match c {
                        ':' => '!',
                        '\u{FE55}' => '\u{FE57}',
                        '\u{FF1A}' => '\u{FF01}',
                        c => c,
                    })
                    .collect();
                separated
                    .unicode_word_indices()
                    .map(|(start, word)| start + word.len())
                    .find(|end| *end > offset)
                    .unwrap_or(text.len())
            }
            CaretMotion::Up | CaretMotion::Down => {
                let caret = shaped.caret_rect(offset);
                let y = caret.origin.y
                    + caret.size.height
                        * if matches!(motion, CaretMotion::Up) {
                            -0.5
                        } else {
                            1.5
                        };
                shaped
                    .hit(Vector2::new(caret.origin.x, y))
                    .unwrap_or(offset)
            }
            CaretMotion::TextStart => 0,
            CaretMotion::TextEnd => text.len(),
        };
        // An entity can decode to multiple graphemes but has no intermediate
        // source caret position. Walk past the entire source atom, rather
        // than repeatedly mapping the first displayed grapheme back to it.
        if matches!(motion, CaretMotion::Right) {
            while target < text.len() && self.source_offset(target) <= source {
                target += text[target..].graphemes(true).next().unwrap().len();
            }
        }
        let mapped = self.source_offset(target);
        if matches!(motion, CaretMotion::Right | CaretMotion::WordRight) {
            mapped.max(source)
        } else {
            mapped
        }
    }
}
pub fn safe_url(value: &str) -> bool {
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http" | "mailto")
            && url.username().is_empty()
            && url.password().is_none()
    })
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
    fn fonts() -> FontContext {
        mosaic::core::theme::install(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::Dark,
            1.0,
        ));
        let mut fonts = FontContext::embedded_only();
        relay_ui::fonts::install(&mut fonts);
        fonts
    }
    fn visible(doc: &Document) -> String {
        doc.runs.iter().map(|r| r.text.as_str()).collect()
    }
    #[test]
    fn decoded_entity_does_not_steal_a_later_literal_source_position() {
        let doc = Document::new("&copy; ©");
        assert_eq!(visible(&doc), "© ©");
        assert_eq!(doc.source_offset(0), 0);
        assert_eq!(doc.offset_with_affinity("©".len(), true), "&copy;".len());
        assert_eq!(doc.source_offset("© ".len()), "&copy; ".len());
    }
    #[test]
    fn decoded_entities_are_atomic_source_spans_even_when_their_names_contain_output() {
        for (encoded, decoded) in [
            ("&semi;", ";"),
            ("&fjlig;", "fj"),
            ("&#169;", "©"),
            ("&#xA9;", "©"),
            ("&NotEqualTilde;", "≂\u{338}"),
        ] {
            let source = format!("{encoded} {decoded}");
            let doc = Document::new(&source);
            assert_eq!(visible(&doc), format!("{decoded} {decoded}"));
            assert_eq!(doc.display_offset(0), 0, "{encoded}");
            for (offset, _) in decoded.char_indices() {
                assert_eq!(doc.source_offset(offset), 0, "{encoded} at {offset}");
            }
            assert_eq!(doc.offset_with_affinity(decoded.len(), true), encoded.len());
            assert_eq!(doc.source_offset(decoded.len() + 1), encoded.len() + 1);
            let (start, end) = doc.selection_range(0, encoded.len());
            assert_eq!(&source[start..end], encoded);
            assert_eq!(doc.selection_range(encoded.len(), 0), (end, start));
        }
    }
    #[test]
    fn entity_selection_copies_the_whole_source_and_caret_crosses_its_decoded_graphemes() {
        use mosaic::text::CaretMotion::{Left, Right};
        let mut fonts = fonts();
        for (encoded, decoded) in [("&fjlig;", "fj"), ("&NotEqualTilde;", "≂\u{338}")] {
            let source = format!("{encoded} x");
            let mut doc = Document::new(&source);
            let visible_end = doc.source_offset(decoded.len());
            let (start, end) = doc.selection_range(doc.source_offset(0), visible_end);
            assert_eq!((start, end), (0, encoded.len()));
            let mut editor = mosaic::text::EditBuffer::new(TextStyle::new(13.0));
            editor.set_text(&mut fonts, &source);
            editor.set_selection_bytes(&mut fonts, start, end);
            assert_eq!(editor.selected_text().as_deref(), Some(encoded));
            assert_eq!(source.get(start..end), Some(encoded)); // anchored reply quote
            assert_eq!(doc.display_offset(0), 0);
            let expected = doc.shape(&mut fonts, Some(600.0)).caret_rect(0);
            assert_eq!(doc.caret(&mut fonts, 600.0, 0), expected);
            assert_eq!(
                doc.motion(&mut fonts, 600.0, 0, Right),
                encoded.len(),
                "{encoded}"
            );
            assert_eq!(
                doc.motion(&mut fonts, 600.0, encoded.len(), Left),
                0,
                "{encoded}"
            );
        }
    }
    #[test]
    fn repeated_shift_right_and_reverse_left_cross_atomic_entity_next_to_plain_text() {
        use mosaic::text::CaretMotion::{Left, Right};
        let mut fonts = fonts();
        let source = "&fjlig; xy";
        let mut doc = Document::new(source);
        assert_eq!(doc.motion(&mut fonts, 600.0, 0, Right), 7);
        assert_eq!(doc.motion(&mut fonts, 600.0, 7, Left), 0);
        let mut editor = mosaic::text::EditBuffer::new(TextStyle::new(13.0));
        editor.set_text(&mut fonts, source);
        editor.set_selection_bytes(&mut fonts, 0, 0);
        let mut remembered = (0, 0);
        for (expected_head, copied) in [
            (7, "&fjlig;"),
            (8, "&fjlig; "),
            (9, "&fjlig; x"),
            (10, source),
            (10, source),
        ] {
            let (anchor, head) = selection(&editor, remembered);
            let head = doc.motion(&mut fonts, 600.0, head, Right);
            remembered = doc.selection_range(anchor, head);
            editor.set_selection_bytes(&mut fonts, remembered.0, remembered.1);
            assert_eq!(remembered, (0, expected_head));
            assert_eq!(editor.selected_text().as_deref(), Some(copied));
        }
        editor.set_selection_bytes(&mut fonts, source.len(), source.len());
        remembered = (source.len(), source.len());
        for (expected_head, copied) in [(9, "y"), (8, "xy"), (7, " xy"), (0, source), (0, source)] {
            let (anchor, head) = selection(&editor, remembered);
            let head = doc.motion(&mut fonts, 600.0, head, Left);
            remembered = doc.selection_range(anchor, head);
            editor.set_selection_bytes(&mut fonts, remembered.0, remembered.1);
            assert_eq!(remembered, (source.len(), expected_head));
            assert_eq!(editor.selected_text().as_deref(), Some(copied));
        }
        let mut head = source.len();
        for expected in [9, 8, 7, 0, 0] {
            head = doc.motion(&mut fonts, 600.0, head, Left);
            assert_eq!(head, expected);
        }
    }
    #[test]
    fn word_right_advances_past_word_end_boundaries() {
        let mut fonts = fonts();
        let mut doc = Document::new("one two");
        use mosaic::text::CaretMotion::WordRight;
        assert_eq!(doc.motion(&mut fonts, 600.0, 0, WordRight), 3);
        assert_eq!(doc.motion(&mut fonts, 600.0, 3, WordRight), 7);
        assert_eq!(doc.motion(&mut fonts, 600.0, 7, WordRight), 7);
        let mut doc = Document::new("**one**  `λ:two`!");
        let mut position = 0;
        for expected in [
            "**one**".len(),
            "**one**  `λ".len(),
            "**one**  `λ:two`!".len() - 1,
            "**one**  `λ:two`!".len(),
        ] {
            position = doc.motion(&mut fonts, 600.0, position, WordRight);
            assert_eq!(position, expected);
        }
    }
    #[test]
    fn repeated_shift_alt_right_extends_selection_through_word_boundaries() {
        use mosaic::text::CaretMotion::WordRight;
        let mut fonts = fonts();
        let source = "**one** `two` three";
        let mut doc = Document::new(source);
        let mut editor = mosaic::text::EditBuffer::new(TextStyle::new(13.0));
        editor.set_text(&mut fonts, source);
        editor.set_selection_bytes(&mut fonts, 0, 0);
        let mut remembered = (0, 0);
        for expected in [
            "one",
            "one** `two",
            "one** `two` three",
            "one** `two` three",
        ] {
            // The source controller handles Shift+Alt+Right by moving the
            // active end by WordRight and preserving its remembered anchor.
            let (anchor, head) = selection(&editor, remembered);
            let head = doc.motion(&mut fonts, 600.0, head, WordRight);
            remembered = doc.selection_range(anchor, head);
            editor.set_selection_bytes(&mut fonts, remembered.0, remembered.1);
            assert_eq!(editor.selected_text().as_deref(), Some(expected));
            assert_eq!(remembered.0, source.find("one").unwrap());
        }
    }
    #[test]
    fn final_glyph_right_half_is_inside_its_link_including_wrapped_and_styled_labels() {
        let mut fonts = fonts();
        for (source, label, width) in [
            ("[go](https://example.com)", "go", 400.0),
            ("[go](https://example.com) tail", "go", 400.0),
            (
                "[go](https://example.com)[next](https://other.example)",
                "go",
                400.0,
            ),
            ("[go**W**](https://example.com) tail", "goW", 400.0),
            (
                "[wrapped long label](https://example.com) tail",
                "wrapped long label",
                60.0,
            ),
            ("[go©](https://example.com) tail", "go©", 400.0),
        ] {
            let mut doc = Document::new(source);
            let last = label.char_indices().next_back().unwrap().0;
            let shaped = doc.shape(&mut fonts, Some(width));
            let glyph = shaped.selection_rects(last, label.len()).pop().unwrap();
            let point = Vector2::new(glyph.origin.x + glyph.size.width * 0.85, glyph.center().y);
            assert_eq!(shaped.hit(point), Some(label.len()), "{source}");
            assert_eq!(
                doc.link_at(&mut fonts, width, point).as_deref(),
                Some("https://example.com"),
                "{source}"
            );
        }
    }
    #[test]
    fn one_character_links_include_both_halves_and_styled_boundaries_but_not_outside_points() {
        let mut fonts = fonts();
        let mut doc = Document::new("[x](https://example.com) tail");
        let rect = doc.shape(&mut fonts, Some(600.0)).selection_rects(0, 1)[0];
        for fraction in [0.25, 0.85] {
            let point = Vector2::new(rect.origin.x + rect.size.width * fraction, rect.center().y);
            assert_eq!(
                doc.link_at(&mut fonts, 600.0, point).as_deref(),
                Some("https://example.com")
            );
        }
        for point in [
            Vector2::new(rect.origin.x - 0.25, rect.center().y),
            Vector2::new(rect.max_x() + 0.25, rect.center().y),
            Vector2::new(rect.center().x, rect.origin.y - 0.25),
            Vector2::new(rect.center().x, rect.max_y() + 0.25),
        ] {
            assert!(doc.link_at(&mut fonts, 600.0, point).is_none());
        }
        for source in [
            "[**x**y](https://example.com) tail",
            "[x](https://example.com)[y](https://other.example) tail",
        ] {
            let mut doc = Document::new(source);
            for (range, url) in [
                (0..1, "https://example.com"),
                (
                    1..2,
                    if source.contains("other.example") {
                        "https://other.example"
                    } else {
                        "https://example.com"
                    },
                ),
            ] {
                let rect = doc
                    .shape(&mut fonts, Some(600.0))
                    .selection_rects(range.start, range.end)[0];
                for fraction in [0.25, 0.85] {
                    let point =
                        Vector2::new(rect.origin.x + rect.size.width * fraction, rect.center().y);
                    assert_eq!(
                        doc.link_at(&mut fonts, 600.0, point).as_deref(),
                        Some(url),
                        "{source}"
                    );
                }
            }
        }
    }
    #[test]
    fn escapes_code_and_crlf_keep_literal_source_boundaries() {
        for (source, rendered, first) in [
            (r"\* *", "* *", 1),
            (r"\&semi; ;", "&semi; ;", 1),
            ("`&fjlig; fj`", "&fjlig; fj", 1),
            ("`` a\r\nλ b ``", "a  λ b", 3),
        ] {
            let doc = Document::new(source);
            assert_eq!(visible(&doc), rendered);
            assert_eq!(doc.source_offset(0), first);
            for needle in ["*", ";", "fj", "λ", "b"] {
                if let Some(display) = rendered.rfind(needle) {
                    let raw = source.rfind(needle).unwrap();
                    assert_eq!(doc.source_offset(display), raw, "{source:?}: {needle}");
                    assert_eq!(doc.display_offset(raw), display, "{source:?}: {needle}");
                }
            }
        }
    }
    #[test]
    fn ui_scale_changes_refresh_cached_styles_and_geometry() {
        let mut fonts = fonts();
        let mut doc = Document::new("# Heading\n\n`code`");
        let before = doc.shape(&mut fonts, Some(600.0)).metrics().size.height;
        mosaic::core::theme::install(&relay_ui::tokens::theme(
            relay_ui::tokens::ThemeVariant::Dark,
            2.0,
        ));
        let after = doc.shape(&mut fonts, Some(600.0)).metrics().size.height;
        assert!(after > before * 1.8);
    }
    #[test]
    fn renders_links_code_emphasis_headings_and_lists_without_markup() {
        let doc = Document::new(
            "# Heading\n\n**bold** and *italic* [issue #124](https://github.com/jens-hj/relay/issues/124) `let x = 1;`\n\n- first\n- second\n\n```rust\nfn main() {}\n```\n",
        );
        assert_eq!(
            visible(&doc),
            "Heading\nbold and italic issue #124 let x = 1;\n• first\n• second\nfn main() {}\n"
        );
        assert!(doc.runs.iter().any(|r| r.text == "issue #124"
            && r.link.as_deref() == Some("https://github.com/jens-hj/relay/issues/124")));
        assert!(doc.runs.iter().any(|r| r.text == "let x = 1;" && r.code));
        assert!(
            doc.runs
                .iter()
                .any(|r| r.text == "fn main() {}\n" && r.code)
        );
        assert!(
            doc.runs
                .iter()
                .any(|r| r.text == "bold" && r.style.weight == 700)
        );
        assert!(
            doc.runs
                .iter()
                .any(|r| r.text == "italic" && r.style.style == FontStyle::Italic)
        );
    }
    #[test]
    fn source_mapping_keeps_original_bytes_for_selection_and_replies() {
        let source = "before **λ word** and `code` &amp; [link](https://example.com)";
        let doc = Document::new(source);
        for needle in ["λ word", "code", "link"] {
            let run = doc.runs.iter().find(|r| r.text == needle).unwrap();
            for (offset, _) in needle.char_indices().filter(|(i, _)| *i > 0) {
                let raw = run.source.start + offset;
                assert_eq!(doc.source_offset(doc.display_offset(raw)), raw);
                assert!(source.is_char_boundary(raw));
            }
        }
        let start = source.find('λ').unwrap();
        let end = start + "λ word".len();
        assert_eq!(&source[start..end], "λ word");
        assert_eq!(
            doc.display_offset(end) - doc.display_offset(start),
            "λ word".len()
        );
        assert!(visible(&doc).contains(" & link"));
        assert_eq!(doc.source, source);
    }
    #[test]
    fn streaming_reparses_completed_delimiters_and_invalidates_layout() {
        let mut fonts = fonts();
        let mut doc = Document::new("Read [issue");
        let first = doc.shape(&mut fonts, Some(150.0)).text();
        assert!(first.contains("[issue"));
        doc.update("Read [issue](https://example.com) and `λ`\n\n```\nstreaming");
        assert_eq!(visible(&doc), "Read issue and λ\nstreaming");
        assert!(doc.runs.iter().any(|r| r.link.is_some()));
        assert!(doc.runs.iter().any(|r| r.text == "streaming" && r.code));
        let narrow = doc.shape(&mut fonts, Some(40.0)).metrics().size.height;
        let wide = doc.shape(&mut fonts, Some(600.0)).metrics().size.height;
        assert!(narrow > wide);
        doc.update("Read [issue](https://example.com) and `λ`\n\n```\nstreaming\n```\n");
        assert!(!doc.shape(&mut fonts, Some(600.0)).text().contains("```"));
    }
    #[test]
    fn unsafe_links_and_html_stay_inert() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "file:///etc/passwd",
            "https://user:password@example.com",
            "https://example.com\n",
            "//example.com",
            "relay:execute",
        ] {
            assert!(!safe_url(url), "{url}");
        }
        for url in [
            "https://example.com/path?q=1#anchor",
            "http://localhost:7331",
            "mailto:hello@example.com",
        ] {
            assert!(safe_url(url), "{url}");
        }
        let doc = Document::new("[bad](javascript:alert) <script>alert('x')</script>");
        assert!(doc.runs.iter().all(|r| r.link.is_none()));
        assert!(visible(&doc).contains("<script>"));
    }
    #[test]
    fn shaped_links_use_real_bounds_and_code_hit_testing_maps_to_source() {
        let source = "[issue](https://example.com) and `λ code`";
        let mut doc = Document::new(source);
        let mut fonts = fonts();
        let rect = doc.shape(&mut fonts, Some(400.0)).selection_rects(0, 5)[0];
        assert_eq!(
            doc.link_at(&mut fonts, 400.0, rect.center()).as_deref(),
            Some("https://example.com")
        );
        assert!(
            doc.link_at(&mut fonts, 400.0, Vector2::new(399.0, rect.center().y))
                .is_none()
        );
        let offset = source.find("code").unwrap();
        let caret = doc.caret(&mut fonts, 400.0, offset);
        let hit = doc.hit(
            &mut fonts,
            400.0,
            Vector2::new(caret.origin.x, caret.center().y),
        );
        assert_eq!(hit, offset);
    }
}

#[cfg(test)]
mod mapping_regressions {
    use super::*;
    #[test]
    fn normalized_inline_code_keeps_internal_source_positions() {
        let source = "before `` a\nλ b `` after";
        let doc = Document::new(source);
        let code = doc.runs.iter().find(|run| run.code).unwrap();
        assert_eq!(code.text, "a λ b");
        for needle in ["λ", "b "] {
            let raw = source.find(needle).unwrap();
            assert_eq!(doc.source_offset(doc.display_offset(raw)), raw);
        }
    }
    #[test]
    fn reverse_selection_remembers_the_active_end() {
        let mut fonts = FontContext::embedded_only();
        let mut editor = mosaic::text::EditBuffer::new(TextStyle::new(13.0));
        editor.set_text(&mut fonts, "λ source text");
        editor.set_selection_bytes(&mut fonts, 10, 3);
        assert_eq!(selection(&editor, (10, 3)), (10, 3));
        editor.select_all(&mut fonts);
        assert_eq!(selection(&editor, (10, 3)), (0, editor.text().len()));
    }
    #[test]
    fn end_of_paragraph_has_no_synthetic_empty_line() {
        let source = "**bold**";
        let doc = Document::new(source);
        assert_eq!(
            doc.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "bold"
        );
        assert_eq!(doc.display_offset(source.len()), 4);
    }
}

#[cfg(test)]
mod boundary_affinity {
    use super::*;
    #[test]
    fn selection_endpoints_exclude_hidden_link_and_code_delimiters_in_both_directions() {
        let source = "before [label](https://example.com) `code` after";
        let doc = Document::new(source);
        for needle in ["label", "code"] {
            let start = source.find(needle).unwrap();
            let end = start + needle.len();
            let a = doc.source_offset(doc.display_offset(start));
            let b = doc.source_offset(doc.display_offset(end));
            assert_eq!(doc.selection_range(a, b), (start, end));
            assert_eq!(doc.selection_range(b, a), (end, start));
            assert_eq!(&source[start..end], needle);
        }
    }
}
