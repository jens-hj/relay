use crate::*;
use mosaic::{
    core::{Rect, Vector2},
    text::{CaretMotion, EditBuffer, FontContext, FontStyle, TextStyle},
};
use std::{cell::RefCell, rc::Rc};

fn fonts() -> FontContext {
    FontContext::embedded_only()
}
fn doc(source: &str) -> Document {
    Document::with_defaults(source)
}
fn visible(doc: &Document) -> String {
    doc.display_text().to_owned()
}
fn run<'a>(doc: &'a Document, text: &str) -> &'a Run {
    doc.runs()
        .iter()
        .find(|run| run.text == text)
        .unwrap_or_else(|| panic!("no run {text:?} in {:?}", visible(doc)))
}
fn display_range(doc: &Document, text: &str) -> std::ops::Range<usize> {
    let start = doc.display_text().find(text).unwrap();
    start..start + text.len()
}
fn rect_of(doc: &mut Document, fonts: &mut FontContext, width: f32, text: &str) -> Rect {
    let range = display_range(doc, text);
    doc.rects(fonts, width, range)[0]
}

// Tables.

const WORKTREES: &str = "Worktrees:\n\n| Worktree | Size | Status |\n|---|---:|:---:|\n| relay-1 | 1.2 GB | clean |\n| relay-2 | 300 MB | dirty |\n\nDone.";

#[test]
fn gfm_table_renders_cells_without_pipe_or_delimiter_text() {
    let doc = doc(WORKTREES);
    let text = visible(&doc);
    assert!(!text.contains('|'), "{text:?}");
    assert!(!text.contains("---"), "{text:?}");
    assert_eq!(
        text,
        "Worktrees:\nWorktree\tSize\tStatus\nrelay-1\t1.2 GB\tclean\nrelay-2\t300 MB\tdirty\nDone."
    );
    let table = &doc.tables()[0];
    assert_eq!(
        table.alignments,
        [Alignment::None, Alignment::Right, Alignment::Center]
    );
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header && !table.rows[1].header);
    let cells: Vec<_> = table.rows[1]
        .cells
        .iter()
        .map(|cell| &doc.display_text()[cell.clone()])
        .collect();
    assert_eq!(cells, ["relay-1", "1.2 GB", "clean"]);
    assert_eq!(run(&doc, "Worktree").style.weight, 700);
    assert_ne!(run(&doc, "relay-1").style.weight, 700);
}

#[test]
fn table_columns_align_and_rows_stack_with_shared_dividers() {
    let mut fonts = fonts();
    let mut doc = doc(WORKTREES);
    let width = 600.0;
    let head = rect_of(&mut doc, &mut fonts, width, "Worktree\t");
    let first = rect_of(&mut doc, &mut fonts, width, "relay-1");
    let second = rect_of(&mut doc, &mut fonts, width, "relay-2");
    // Left-aligned column: same left edge, rows stacked downwards.
    assert!((head.origin.x - first.origin.x).abs() < 0.5);
    assert!((first.origin.x - second.origin.x).abs() < 0.5);
    assert!(head.max_y() <= first.origin.y && first.max_y() <= second.origin.y);
    // Right-aligned column: equal right edges.
    let size_a = rect_of(&mut doc, &mut fonts, width, "1.2 GB");
    let size_b = rect_of(&mut doc, &mut fonts, width, "300 MB");
    let size_head = rect_of(&mut doc, &mut fonts, width, "Size");
    assert!((size_a.max_x() - size_b.max_x()).abs() < 0.5);
    assert!((size_a.max_x() - size_head.max_x()).abs() < 0.5);
    // Centered column: equal centers.
    let clean = rect_of(&mut doc, &mut fonts, width, "clean");
    let status = rect_of(&mut doc, &mut fonts, width, "Status");
    assert!((clean.center().x - status.center().x).abs() < 1.0);
    // Columns are side by side, and the table sits between its paragraphs.
    assert!(first.max_x() < size_a.origin.x && size_a.max_x() < clean.origin.x);
    let before = rect_of(&mut doc, &mut fonts, width, "Worktrees:");
    let after = rect_of(&mut doc, &mut fonts, width, "Done.");
    assert!(before.max_y() <= head.origin.y && second.max_y() <= after.origin.y);
    let size = doc.size(&mut fonts, Some(width));
    assert!(size.width <= width && size.height >= after.max_y());
}

#[test]
fn table_selection_and_hits_map_to_cell_source() {
    let mut fonts = fonts();
    let mut doc = doc(WORKTREES);
    let rect = rect_of(&mut doc, &mut fonts, 600.0, "relay-2");
    let hit = doc.hit(
        &mut fonts,
        600.0,
        Vector2::new(rect.origin.x + 1.0, rect.center().y),
    );
    assert_eq!(hit, WORKTREES.find("relay-2").unwrap());
    let start = WORKTREES.find("1.2 GB").unwrap();
    let (a, b) = doc.selection_range(start, start + "1.2 GB".len());
    assert_eq!(&WORKTREES[a..b], "1.2 GB");
    let range = display_range(&doc, "relay-1");
    assert_eq!(
        doc.source_offset(range.start),
        WORKTREES.find("relay-1").unwrap()
    );
    // Moving right from the end of a cell crosses into the next cell.
    let end = WORKTREES.find("relay-1").unwrap() + "relay-1".len();
    let next = doc.motion(&mut fonts, 600.0, end, CaretMotion::Right);
    assert!(next > end);
    assert!(doc.display_offset(next) > doc.display_offset(end));
    // Down from the header lands in the first body row of the same column.
    let header = WORKTREES.find("Worktree |").unwrap();
    let down = doc.motion(&mut fonts, 600.0, header, CaretMotion::Down);
    assert_eq!(down, WORKTREES.find("relay-1").unwrap());
}

#[test]
fn wide_tables_wrap_columns_then_scroll_horizontally_within_the_width() {
    let mut fonts = fonts();
    let wrapping =
        "| a | b |\n|---|---|\n| one two three four five six | seven eight nine ten eleven |";
    let mut narrow = doc(wrapping);
    let natural = narrow.size(&mut fonts, None);
    let size = narrow.size(&mut fonts, Some(natural.width * 0.7));
    assert!(size.width <= natural.width * 0.7 + 0.5);
    assert!(
        size.height > natural.height,
        "cells wrap instead of overflowing"
    );
    assert!(!narrow.scroll(
        &mut fonts,
        natural.width * 0.7,
        Vector2::new(10.0, 10.0),
        Vector2::new(-40.0, 0.0)
    ));

    let header = (0..8)
        .map(|i| format!("column_{i}_header"))
        .collect::<Vec<_>>();
    let source = format!(
        "| {} |\n|{}|\n| {} |",
        header.join(" | "),
        ["---"; 8].join("|"),
        (0..8)
            .map(|i| format!("unbreakable_value_{i}"))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let mut wide = doc(&source);
    let width = 300.0;
    let size = wide.size(&mut fonts, Some(width));
    assert!(size.width <= width + 0.5, "{size:?}");
    let before = rect_of(&mut wide, &mut fonts, width, "unbreakable_value_0");
    let caret_before = wide.caret(
        &mut fonts,
        width,
        source.find("unbreakable_value_3").unwrap(),
    );
    let point = Vector2::new(width / 2.0, before.center().y);
    // Vertical wheel input belongs to the surrounding scroll view.
    assert!(!wide.scroll(&mut fonts, width, point, Vector2::new(0.0, -40.0)));
    assert!(wide.scroll(&mut fonts, width, point, Vector2::new(-40.0, 0.0)));
    let value = source.find("unbreakable_value_3").unwrap();
    let after = wide.caret(&mut fonts, width, value);
    assert!((caret_before.origin.x - after.origin.x - 40.0).abs() < 0.5);
    // Offscreen cells are clipped to the table viewport.
    let last = display_range(&wide, "unbreakable_value_7");
    assert!(wide.rects(&mut fonts, width, last).is_empty());
    // Scrolling clamps at the right edge, and keyboard motion reveals cells.
    for _ in 0..200 {
        wide.scroll(&mut fonts, width, point, Vector2::new(-40.0, 0.0));
    }
    assert!(!wide.scroll(&mut fonts, width, point, Vector2::new(-40.0, 0.0)));
    let last = display_range(&wide, "unbreakable_value_7");
    assert!(!wide.rects(&mut fonts, width, last).is_empty());
    let start = source.find("column_0").unwrap();
    wide.motion(&mut fonts, width, start + 1, CaretMotion::Left);
    let first = display_range(&wide, "column_0_header");
    let rects = wide.rects(&mut fonts, width, first);
    assert!(!rects.is_empty() && rects[0].origin.x >= 0.0);
}

#[test]
fn tables_with_empty_and_missing_cells_and_inline_markup() {
    let source = "| **a** | `b` | [c](https://example.com) |\n|:--|--|--|\n| | x |\n| y |";
    let mut fonts = fonts();
    let mut doc = doc(source);
    let table = &doc.tables()[0];
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows.iter().all(|row| row.cells.len() == 3));
    assert!(run(&doc, "b").code);
    assert_eq!(run(&doc, "c").link.as_deref(), Some("https://example.com"));
    let size = doc.size(&mut fonts, Some(400.0));
    assert!(size.height > 0.0);
    let rect = rect_of(&mut doc, &mut fonts, 400.0, "c");
    assert_eq!(
        doc.link_at(&mut fonts, 400.0, rect.center()).as_deref(),
        Some("https://example.com")
    );
}

// Inline and block constructs.

#[test]
fn task_lists_replace_bullets_with_checkboxes() {
    let doc = doc("- [ ] todo\n- [x] done\n- plain");
    assert_eq!(visible(&doc), "☐ todo\n☑ done\n• plain");
    assert_eq!(run(&doc, "☐ ").tone, Tone::Muted);
}

#[test]
fn strikethrough_and_emphasis_combinations() {
    let doc = doc("~~gone~~ ***both*** **bold _inner_** _a **b**_");
    assert_eq!(visible(&doc), "gone both bold inner a b");
    assert!(run(&doc, "gone").style.strikethrough == true);
    let both = run(&doc, "both");
    assert!(both.style.weight == 700 && both.style.style == FontStyle::Italic);
    let inner = run(&doc, "inner");
    assert!(inner.style.weight == 700 && inner.style.style == FontStyle::Italic);
    assert!(run(&doc, "a ").style.style == FontStyle::Italic);
}

#[test]
fn footnote_references_and_definitions() {
    let doc = doc("Claim[^note].\n\n[^note]: Source text.");
    assert_eq!(visible(&doc), "Claim[note].\n[note] Source text.");
    assert_eq!(run(&doc, "[note]").tone, Tone::Muted);
}

#[test]
fn autolinks_and_bare_urls_become_links_with_trailing_punctuation_excluded() {
    let doc = doc(
        "<https://a.example> see https://example.com/path. and www.example.org, (https://x.example/a_(b)) <me@example.com> `https://code.example`",
    );
    assert_eq!(
        run(&doc, "https://a.example").link.as_deref(),
        Some("https://a.example")
    );
    assert_eq!(
        run(&doc, "https://example.com/path").link.as_deref(),
        Some("https://example.com/path")
    );
    assert_eq!(
        run(&doc, "www.example.org").link.as_deref(),
        Some("http://www.example.org")
    );
    assert_eq!(
        run(&doc, "https://x.example/a_(b)").link.as_deref(),
        Some("https://x.example/a_(b)")
    );
    assert_eq!(
        run(&doc, "me@example.com").link.as_deref(),
        Some("mailto:me@example.com")
    );
    assert!(run(&doc, "https://code.example").link.is_none());
    assert!(visible(&doc).contains("https://example.com/path. and"));
    for (text, urls) in [
        ("nothing here", 0),
        ("http://", 0),
        ("xhttps://example.com", 0),
        ("www.nodot", 0),
        ("https://λ.example/π", 1),
    ] {
        assert_eq!(find_urls(text).len(), urls, "{text}");
    }
}

#[test]
fn bare_url_source_mapping_matches_literal_bytes() {
    let source = "go to https://example.com/x now";
    let doc = doc(source);
    let url = run(&doc, "https://example.com/x");
    assert_eq!(&source[url.source.clone()], "https://example.com/x");
    let start = source.find("example").unwrap();
    assert_eq!(doc.source_offset(doc.display_offset(start)), start);
}

#[test]
fn headings_one_to_six_shrink_and_are_bold() {
    let source = (1..=6)
        .map(|level| format!("{} H{level}", "#".repeat(level)))
        .collect::<Vec<_>>()
        .join("\n");
    let doc = doc(&source);
    assert_eq!(visible(&doc), "H1\nH2\nH3\nH4\nH5\nH6");
    let sizes: Vec<f32> = (1..=6)
        .map(|level| *run(&doc, &format!("H{level}")).style.size_px)
        .collect();
    assert!(sizes.windows(2).all(|pair| pair[0] > pair[1]), "{sizes:?}");
    assert!((1..=6).all(|level| run(&doc, &format!("H{level}")).style.weight == 700));
    let doc = doc_with_setext();
    assert_eq!(visible(&doc), "Title\nbody");
}

fn doc_with_setext() -> Document {
    doc("Title\n=====\n\nbody")
}

#[test]
fn ordered_lists_keep_start_numbers_and_nested_lists_indent() {
    let mut fonts = fonts();
    let mut doc = doc("3. three\n4. four\n   - nested\n     1. deep\n5. five");
    assert_eq!(
        visible(&doc),
        "3. three\n4. four\n• nested\n1. deep\n5. five"
    );
    assert_eq!(
        run(&doc, "three").place,
        Place::Flow {
            indent: 0,
            quote: 0
        }
    );
    assert_eq!(
        run(&doc, "nested").place,
        Place::Flow {
            indent: 1,
            quote: 0
        }
    );
    assert_eq!(
        run(&doc, "deep").place,
        Place::Flow {
            indent: 2,
            quote: 0
        }
    );
    let three = rect_of(&mut doc, &mut fonts, 600.0, "3. three");
    let nested = rect_of(&mut doc, &mut fonts, 600.0, "• nested");
    let deep = rect_of(&mut doc, &mut fonts, 600.0, "1. deep");
    assert!(three.origin.x < nested.origin.x && nested.origin.x < deep.origin.x);
    assert_eq!(
        visible(&self::doc("1. a\n\n   para\n2. b")),
        "1. a\npara\n2. b"
    );
}

#[test]
fn nested_block_quotes_and_alerts_indent_text() {
    let mut fonts = fonts();
    let mut doc = doc("> outer\n> > inner\n\nafter");
    assert_eq!(visible(&doc), "outer\ninner\nafter");
    assert_eq!(
        run(&doc, "outer").place,
        Place::Flow {
            indent: 0,
            quote: 1
        }
    );
    assert_eq!(
        run(&doc, "inner").place,
        Place::Flow {
            indent: 0,
            quote: 2
        }
    );
    let outer = rect_of(&mut doc, &mut fonts, 600.0, "outer");
    let inner = rect_of(&mut doc, &mut fonts, 600.0, "inner");
    let after = rect_of(&mut doc, &mut fonts, 600.0, "after");
    assert!(after.origin.x < outer.origin.x && outer.origin.x < inner.origin.x);
    let alert = self::doc("> [!WARNING]\n> Check this.");
    assert_eq!(visible(&alert), "Warning\nCheck this.");
}

#[test]
fn fenced_and_indented_code_blocks_keep_language_and_monospace() {
    let mut fonts = fonts();
    let mut fenced = doc("```rust ignore\nfn main() {}\n```\n\n    indented();\n\n```\n```");
    assert_eq!(visible(&fenced), "fn main() {}\nindented();\n");
    let languages: Vec<_> = fenced
        .code_blocks()
        .iter()
        .map(|block| block.language.as_deref())
        .collect();
    assert_eq!(languages, [Some("rust"), None, None]);
    assert!(run(&fenced, "fn main() {}\n").code);
    assert!(run(&fenced, "indented();\n").code);
    let labelled = fenced.size(&mut fonts, Some(400.0)).height;
    let mut plain = doc("```\nfn main() {}\n```\n\n    indented();\n\n```\n```");
    assert!(labelled > plain.size(&mut fonts, Some(400.0)).height);
    // Code inside list items and quotes nests with its container.
    let nested = doc("- item\n\n  ```\n  code\n  ```\n> ```\n> quoted\n> ```");
    assert!(matches!(
        run(&nested, "code\n").place,
        Place::Code {
            indent: 0,
            quote: 0,
            ..
        }
    ));
    assert!(matches!(
        run(&nested, "quoted\n").place,
        Place::Code { quote: 1, .. }
    ));
}

#[test]
fn inline_code_is_monospace_code_run() {
    let doc = doc("call `run()` now");
    assert!(run(&doc, "run()").code);
    assert!(!run(&doc, "call ").code);
}

#[test]
fn horizontal_rules_render_as_blocks_without_text() {
    let mut fonts = fonts();
    let mut doc = doc("above\n\n---\n\n***\nbelow");
    assert_eq!(visible(&doc), "above\nbelow");
    let above = rect_of(&mut doc, &mut fonts, 400.0, "above");
    let below = rect_of(&mut doc, &mut fonts, 400.0, "below");
    let theme = Theme::default();
    assert!(below.origin.y - above.max_y() >= theme.block_gap * 2.0);
}

#[test]
fn hard_and_soft_breaks_render_as_line_breaks() {
    let doc = doc("hard  \nbreak\\\nslash\nsoft");
    assert_eq!(visible(&doc), "hard\nbreak\nslash\nsoft");
}

#[test]
fn images_show_alt_text_linked_to_safe_sources() {
    let doc = doc(
        "![diagram of flow](https://example.com/a.png) ![](https://example.com/b.png) ![x](javascript:alert(1))",
    );
    assert_eq!(
        visible(&doc),
        "Image: diagram of flow Image: https://example.com/b.png Image: x"
    );
    assert_eq!(
        run(&doc, "diagram of flow").link.as_deref(),
        Some("https://example.com/a.png")
    );
    assert_eq!(run(&doc, "Image: ").tone, Tone::Muted);
    assert!(run(&doc, "x").link.is_none());
}

#[test]
fn html_blocks_and_inline_html_stay_literal_text() {
    let doc = doc(
        "<div onclick=\"x()\">\n<b>bold?</b>\n</div>\n\ntext <span>inline</span> <script>alert('x')</script>",
    );
    let text = visible(&doc);
    for literal in [
        "<div onclick=\"x()\">",
        "<b>bold?</b>",
        "</div>",
        "<span>",
        "</span>",
        "<script>",
    ] {
        assert!(text.contains(literal), "{literal} in {text:?}");
    }
    assert!(doc.runs().iter().all(|run| run.link.is_none()));
}

#[test]
fn backslash_escapes_render_literal_punctuation() {
    let doc = doc(r"\*not\* \# \| \[x\](y) \`tick\`");
    assert_eq!(visible(&doc), "*not* # | [x](y) `tick`");
    assert!(doc.runs().iter().all(|run| run.link.is_none() && !run.code));
}

#[test]
fn malformed_input_neither_panics_nor_loses_text() {
    let mut fonts = fonts();
    for source in [
        "```rust\nunclosed fence",
        "| a | b\n|--\n| c",
        "| a |\n|---|\n| b | extra | cells |",
        "[link](",
        "[ref][missing] **unclosed *mixed",
        "<div",
        "> > > deep\n>\n- - - item",
        "1) one\n2) two\n\n10. ten",
        "- [ ]\n- [x]",
        "![](",
        "[^dangling]",
        "\u{0}\u{7f}\r\n\r\ncr lf\r\n",
        "",
        "\n\n\n",
        "    \n\t\n",
        "~~~\n~~~~",
        "***\n---\n___",
        "# \n######## too deep",
        "&bogus; &#xFFFFFFFF; &#0;",
    ] {
        let mut doc = doc(source);
        let size = doc.size(&mut fonts, Some(200.0));
        assert!(
            size.width.is_finite() && size.height.is_finite(),
            "{source:?}"
        );
        for word in source.split(|c: char| !c.is_alphanumeric()).filter(|word| {
            // Fence languages are labels, entity names decode, and GFM
            // ignores cells beyond the header width.
            let skipped = matches!(*word, "x" | "u" | "FFFFFFFF" | "0" | "bogus" | "rust")
                || (source.contains("extra") && matches!(*word, "extra" | "cells"));
            !word.is_empty() && !skipped
        }) {
            assert!(
                doc.display_text().contains(word),
                "{word:?} lost from {source:?}"
            );
        }
        for offset in 0..=source.len() {
            if !source.is_char_boundary(offset) {
                continue;
            }
            for motion in [
                CaretMotion::Left,
                CaretMotion::Right,
                CaretMotion::Up,
                CaretMotion::Down,
                CaretMotion::WordLeft,
                CaretMotion::WordRight,
                CaretMotion::LineStart,
                CaretMotion::LineEnd,
            ] {
                let moved = doc.motion(&mut fonts, 200.0, offset, motion);
                assert!(moved <= source.len(), "{source:?}");
            }
            doc.caret(&mut fonts, 200.0, offset);
            doc.unit(&mut fonts, 200.0, offset, false);
        }
        for y in [-10.0, 0.0, 5.0, 40.0, 1000.0] {
            assert!(doc.hit(&mut fonts, 200.0, Vector2::new(5.0, y)) <= source.len());
        }
    }
}

#[test]
fn theme_changes_from_the_host_refresh_styles_and_geometry() {
    struct Scaled(Rc<RefCell<f32>>);
    impl Host for Scaled {
        fn theme(&self) -> Theme {
            let scale = *self.0.borrow();
            let base = Theme::default();
            Theme {
                text: TextStyle::new(13.0 * scale)
                    .line_height(mosaic::text::LineHeight::Scale(1.7)),
                heading_sizes: base.heading_sizes.map(|size| size * scale),
                ..base
            }
        }
    }
    let scale = Rc::new(RefCell::new(1.0));
    let mut fonts = fonts();
    let mut doc = Document::new("# Heading\n\n`code`", Rc::new(Scaled(scale.clone())));
    let before = doc.size(&mut fonts, Some(600.0)).height;
    *scale.borrow_mut() = 2.0;
    let after = doc.size(&mut fonts, Some(600.0)).height;
    assert!(after > before * 1.8);
}

#[test]
fn host_link_policy_and_activation() {
    struct Recorder(RefCell<Vec<String>>);
    impl Host for Recorder {
        fn allow_link(&self, url: &str) -> bool {
            url.starts_with("https://allowed.example")
        }
        fn open_link(&self, url: &str) {
            self.0.borrow_mut().push(url.into());
        }
    }
    let host = Rc::new(Recorder(RefCell::default()));
    let doc = Document::new(
        "[a](https://allowed.example/a) [b](https://other.example)",
        host.clone(),
    );
    assert!(run(&doc, "a").link.is_some());
    assert!(run(&doc, "b").link.is_none());
    doc.open_link("https://allowed.example/a");
    doc.open_link("https://other.example");
    assert_eq!(*host.0.borrow(), ["https://allowed.example/a"]);
}

#[test]
fn painting_emits_fills_glyphs_and_clips_tables() {
    let mut fonts = fonts();
    let mut doc = doc(&format!(
        "> quote\n\n```sh\nls\n```\n\n---\n\n`x` ~~y~~\n\n{WORKTREES}"
    ));
    let mut scene = mosaic::render::Scene::new(mosaic::core::Color::from_rgb_hex(0));
    let origin = Vector2::new(10.0, 20.0);
    doc.paint_under(&mut scene, &mut fonts, 500.0, origin);
    let range = 0..doc.display_len();
    doc.fill(
        &mut scene,
        &mut fonts,
        500.0,
        origin,
        range,
        Colors::default().rule,
    );
    doc.paint_text(
        &mut scene,
        &mut fonts,
        500.0,
        origin,
        1.0,
        Colors::default().ink.into(),
    );
    let debug = format!("{scene:?}");
    assert!(debug.contains("PushClip") && debug.contains("PopClip"));
}

// Source mapping, selection and links (moved from relay-desktop).

#[test]
fn decoded_entity_does_not_steal_a_later_literal_source_position() {
    let doc = doc("&copy; ©");
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
        let doc = doc(&source);
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
    use CaretMotion::{Left, Right};
    let mut fonts = fonts();
    for (encoded, decoded) in [("&fjlig;", "fj"), ("&NotEqualTilde;", "≂\u{338}")] {
        let source = format!("{encoded} x");
        let mut doc = doc(&source);
        let visible_end = doc.source_offset(decoded.len());
        let (start, end) = doc.selection_range(doc.source_offset(0), visible_end);
        assert_eq!((start, end), (0, encoded.len()));
        let mut editor = EditBuffer::new(TextStyle::new(13.0));
        editor.set_text(&mut fonts, &source);
        editor.set_selection_bytes(&mut fonts, start, end);
        assert_eq!(editor.selected_text().as_deref(), Some(encoded));
        assert_eq!(doc.display_offset(0), 0);
        assert_eq!(doc.caret(&mut fonts, 600.0, 0).origin.x, 0.0);
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
    use CaretMotion::{Left, Right};
    let mut fonts = fonts();
    let source = "&fjlig; xy";
    let mut doc = doc(source);
    assert_eq!(doc.motion(&mut fonts, 600.0, 0, Right), 7);
    assert_eq!(doc.motion(&mut fonts, 600.0, 7, Left), 0);
    let mut editor = EditBuffer::new(TextStyle::new(13.0));
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
    let mut doc = doc("one two");
    use CaretMotion::WordRight;
    assert_eq!(doc.motion(&mut fonts, 600.0, 0, WordRight), 3);
    assert_eq!(doc.motion(&mut fonts, 600.0, 3, WordRight), 7);
    assert_eq!(doc.motion(&mut fonts, 600.0, 7, WordRight), 7);
    let mut doc = self::doc("**one**  `λ:two`!");
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
    use CaretMotion::WordRight;
    let mut fonts = fonts();
    let source = "**one** `two` three";
    let mut doc = doc(source);
    let mut editor = EditBuffer::new(TextStyle::new(13.0));
    editor.set_text(&mut fonts, source);
    editor.set_selection_bytes(&mut fonts, 0, 0);
    let mut remembered = (0, 0);
    for expected in [
        "one",
        "one** `two",
        "one** `two` three",
        "one** `two` three",
    ] {
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
        let mut doc = doc(source);
        let last = label.char_indices().next_back().unwrap().0;
        let glyph = doc
            .rects(&mut fonts, width, last..label.len())
            .pop()
            .unwrap();
        let point = Vector2::new(glyph.origin.x + glyph.size.width * 0.85, glyph.center().y);
        let hit = doc.hit(&mut fonts, width, point);
        assert_eq!(doc.display_offset(hit), label.len(), "{source}");
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
    let mut doc = doc("[x](https://example.com) tail");
    let rect = doc.rects(&mut fonts, 600.0, 0..1)[0];
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
        let mut doc = self::doc(source);
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
            let rect = doc.rects(&mut fonts, 600.0, range)[0];
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
        let doc = doc(source);
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
fn renders_links_code_emphasis_headings_and_lists_without_markup() {
    let doc = doc(
        "# Heading\n\n**bold** and *italic* [issue #124](https://github.com/jens-hj/relay/issues/124) `let x = 1;`\n\n- first\n- second\n\n```rust\nfn main() {}\n```\n",
    );
    assert_eq!(
        visible(&doc),
        "Heading\nbold and italic issue #124 let x = 1;\n• first\n• second\nfn main() {}\n"
    );
    assert!(doc.runs().iter().any(|r| r.text == "issue #124"
        && r.link.as_deref() == Some("https://github.com/jens-hj/relay/issues/124")));
    assert!(doc.runs().iter().any(|r| r.text == "let x = 1;" && r.code));
    assert!(
        doc.runs()
            .iter()
            .any(|r| r.text == "fn main() {}\n" && r.code)
    );
    assert!(
        doc.runs()
            .iter()
            .any(|r| r.text == "bold" && r.style.weight == 700)
    );
    assert!(
        doc.runs()
            .iter()
            .any(|r| r.text == "italic" && r.style.style == FontStyle::Italic)
    );
}

#[test]
fn source_mapping_keeps_original_bytes_for_selection_and_replies() {
    let source = "before **λ word** and `code` &amp; [link](https://example.com)";
    let doc = doc(source);
    for needle in ["λ word", "code", "link"] {
        let run = run(&doc, needle);
        for (offset, _) in needle.char_indices().filter(|(i, _)| *i > 0) {
            let raw = run.source.start + offset;
            assert_eq!(doc.source_offset(doc.display_offset(raw)), raw);
            assert!(source.is_char_boundary(raw));
        }
    }
    let start = source.find('λ').unwrap();
    let end = start + "λ word".len();
    assert_eq!(
        doc.display_offset(end) - doc.display_offset(start),
        "λ word".len()
    );
    assert!(visible(&doc).contains(" & link"));
    assert_eq!(doc.source(), source);
}

#[test]
fn streaming_reparses_completed_delimiters_and_invalidates_layout() {
    let mut fonts = fonts();
    let mut doc = doc("Read [issue");
    doc.size(&mut fonts, Some(150.0));
    assert!(visible(&doc).contains("[issue"));
    doc.update("Read [issue](https://example.com) and `λ`\n\n```\nstreaming");
    assert_eq!(visible(&doc), "Read issue and λ\nstreaming");
    assert!(doc.runs().iter().any(|r| r.link.is_some()));
    assert!(doc.runs().iter().any(|r| r.text == "streaming" && r.code));
    let narrow = doc.size(&mut fonts, Some(40.0)).height;
    let wide = doc.size(&mut fonts, Some(600.0)).height;
    assert!(narrow > wide);
    doc.update("Read [issue](https://example.com) and `λ`\n\n```\nstreaming\n```\n");
    assert!(!visible(&doc).contains("```"));
    // Tables stream in once their delimiter row arrives.
    doc.update("| a | b |");
    assert!(doc.tables().is_empty());
    doc.update("| a | b |\n|---|---|\n| 1 |");
    assert_eq!(doc.tables().len(), 1);
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
    let doc = doc("[bad](javascript:alert) <script>alert('x')</script>");
    assert!(doc.runs().iter().all(|r| r.link.is_none()));
    assert!(visible(&doc).contains("<script>"));
}

#[test]
fn shaped_links_use_real_bounds_and_code_hit_testing_maps_to_source() {
    let source = "[issue](https://example.com) and `λ code`";
    let mut doc = doc(source);
    let mut fonts = fonts();
    let rect = doc.rects(&mut fonts, 400.0, 0..5)[0];
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

#[test]
fn code_block_hits_and_carets_map_into_the_fence_body() {
    let source = "Intro\n\n```rust\nlet answer = 42;\n```";
    let mut fonts = fonts();
    let mut doc = doc(source);
    let answer = source.find("answer").unwrap();
    let caret = doc.caret(&mut fonts, 400.0, answer);
    assert_eq!(doc.hit(&mut fonts, 400.0, caret.center()), answer);
    let (start, end) = doc.selection_range(answer, answer + 6);
    assert_eq!(&source[start..end], "answer");
}

#[test]
fn normalized_inline_code_keeps_internal_source_positions() {
    let source = "before `` a\nλ b `` after";
    let doc = doc(source);
    let code = doc.runs().iter().find(|run| run.code).unwrap();
    assert_eq!(code.text, "a λ b");
    for needle in ["λ", "b "] {
        let raw = source.find(needle).unwrap();
        assert_eq!(doc.source_offset(doc.display_offset(raw)), raw);
    }
}

#[test]
fn reverse_selection_remembers_the_active_end() {
    let mut fonts = fonts();
    let mut editor = EditBuffer::new(TextStyle::new(13.0));
    editor.set_text(&mut fonts, "λ source text");
    editor.set_selection_bytes(&mut fonts, 10, 3);
    assert_eq!(selection(&editor, (10, 3)), (10, 3));
    editor.select_all(&mut fonts);
    assert_eq!(selection(&editor, (10, 3)), (0, editor.text().len()));
}

#[test]
fn end_of_paragraph_has_no_synthetic_empty_line() {
    let source = "**bold**";
    let doc = doc(source);
    assert_eq!(visible(&doc), "bold");
    assert_eq!(doc.display_offset(source.len()), 4);
}

#[test]
fn selection_endpoints_exclude_hidden_link_and_code_delimiters_in_both_directions() {
    let source = "before [label](https://example.com) `code` after";
    let doc = doc(source);
    for needle in ["label", "code"] {
        let start = source.find(needle).unwrap();
        let end = start + needle.len();
        let a = doc.source_offset(doc.display_offset(start));
        let b = doc.source_offset(doc.display_offset(end));
        assert_eq!(doc.selection_range(a, b), (start, end));
        assert_eq!(doc.selection_range(b, a), (end, start));
    }
}
