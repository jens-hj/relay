//! Markdown events to display runs. Every run records the source bytes it
//! came from, so selections over rendered text can recover the original
//! Markdown exactly.
use crate::theme::{Host, Theme};
use mosaic::text::{FontFamily, FontStyle, TextStyle};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd,
};
use std::ops::Range;

/// The Markdown extensions this crate renders: CommonMark plus the GitHub
/// Flavored Markdown tables, task lists, strikethrough, footnotes and alerts.
pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// Which laid-out block a run belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Wrapped text at a list/quote nesting level.
    Flow { indent: u16, quote: u16 },
    /// A fenced or indented code block, indexing [`Parsed::code`].
    Code {
        index: usize,
        indent: u16,
        quote: u16,
    },
    /// A table, indexing [`Parsed::tables`].
    Table {
        index: usize,
        indent: u16,
        quote: u16,
    },
    /// A horizontal rule.
    Rule {
        index: usize,
        indent: u16,
        quote: u16,
    },
}

impl Place {
    pub fn nesting(self) -> (u16, u16) {
        match self {
            Place::Flow { indent, quote }
            | Place::Code { indent, quote, .. }
            | Place::Table { indent, quote, .. }
            | Place::Rule { indent, quote, .. } => (indent, quote),
        }
    }
}

/// How a run's glyphs are colored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// The paint supplied by the caller.
    #[default]
    Ink,
    /// [`Colors::muted`](crate::Colors::muted).
    Muted,
}

/// Column alignment of a GFM table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// A table's structure in display offsets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub header: bool,
    /// Display range of each cell's text, in column order.
    pub cells: Vec<Range<usize>>,
}

/// A code block's metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeBlock {
    /// The first word of a fence's info string.
    pub language: Option<String>,
}

/// One styled span of displayed text and the source bytes it represents.
#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub source: Range<usize>,
    pub display: Range<usize>,
    pub style: TextStyle,
    pub code: bool,
    pub link: Option<String>,
    pub tone: Tone,
    pub place: Place,
    pub(crate) source_points: Vec<(usize, usize)>,
}

#[derive(Default)]
pub(crate) struct Parsed {
    pub runs: Vec<Run>,
    pub tables: Vec<Table>,
    pub code: Vec<CodeBlock>,
}

#[derive(Clone)]
struct State {
    style: TextStyle,
    code: bool,
    link: Option<String>,
    tone: Tone,
}

struct Builder<'a> {
    source: &'a str,
    theme: &'a Theme,
    parsed: Parsed,
    stack: Vec<State>,
    lists: Vec<Option<u64>>,
    quote: u16,
    /// Code block or table currently open, with its index.
    container: Option<Place>,
    rules: usize,
    /// Index of the current item's synthetic marker run, for task lists.
    marker: Option<usize>,
    /// Index of the latest list marker or footnote label run.
    prefix: Option<usize>,
    /// Inside a table head.
    header: bool,
    /// Whether the open image produced any alt text.
    image: Option<(Range<usize>, String, bool)>,
    /// Contiguous literal text awaiting autolink detection.
    pending: Option<(Range<usize>, State)>,
}

pub(crate) fn parse(source: &str, theme: &Theme, host: &dyn Host) -> Parsed {
    let mut builder = Builder {
        source,
        theme,
        parsed: Parsed::default(),
        stack: vec![State {
            style: TextStyle::inherited(),
            code: false,
            link: None,
            tone: Tone::Ink,
        }],
        lists: Vec::new(),
        quote: 0,
        container: None,
        rules: 0,
        marker: None,
        prefix: None,
        header: false,
        image: None,
        pending: None,
    };
    for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
        builder.event(event, range, host);
    }
    builder.flush();
    let runs = &mut builder.parsed.runs;
    // Block separators map to the end of the original block; a final one
    // would only add an empty line.
    if runs.last().is_some_and(|run| {
        run.text == "\n" && run.source.is_empty() && run.source.end == source.len()
    }) {
        runs.pop();
    }
    builder.parsed
}

impl Builder<'_> {
    fn state(&self) -> State {
        self.stack.last().cloned().unwrap()
    }

    fn place(&self) -> Place {
        let indent = self.lists.len().saturating_sub(1) as u16;
        self.container.unwrap_or(Place::Flow {
            indent,
            quote: self.quote,
        })
    }

    fn nesting(&self) -> (u16, u16) {
        (self.lists.len().saturating_sub(1) as u16, self.quote)
    }

    fn ends_line(&self) -> bool {
        self.parsed
            .runs
            .iter()
            .rev()
            .find(|run| !run.text.is_empty())
            .is_none_or(|run| run.text.ends_with('\n'))
    }

    /// Start a new display line before a block, mapped to the block start.
    fn line(&mut self, at: usize) {
        // Block content directly after a list marker or footnote label stays
        // on the marker's line.
        let prefixed = self
            .prefix
            .is_some_and(|index| index + 1 == self.parsed.runs.len());
        if !prefixed && !self.ends_line() {
            let state = self.state();
            self.push("\n".into(), at..at, &state);
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>, host: &dyn Host) {
        if let Event::Text(text) = &event
            && self.source.get(range.clone()) == Some(text.as_ref())
        {
            self.literal(range);
            return;
        }
        self.flush();
        let state = self.state();
        match event {
            Event::Start(tag) => self.start(tag, range, state, host),
            Event::End(tag) => self.end(tag, range, state),
            Event::Text(text) => {
                if let Some(image) = &mut self.image {
                    image.2 = true;
                }
                self.push(text.into_string(), range, &state);
            }
            Event::Code(text) => {
                let source = self.source;
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
                let state = State {
                    style: state.style.family(FontFamily::Monospace),
                    code: true,
                    ..state
                };
                self.push(text.into_string(), content, &state);
            }
            Event::SoftBreak | Event::HardBreak => self.push("\n".into(), range, &state),
            // HTML stays inert, literal text.
            Event::Html(text) | Event::InlineHtml(text) => {
                self.push(text.into_string(), range, &state)
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.push(text.into_string(), range, &state)
            }
            Event::FootnoteReference(label) => {
                let state = State {
                    tone: Tone::Muted,
                    ..state
                };
                self.push(format!("[{label}]"), range, &state);
            }
            Event::TaskListMarker(checked) => {
                if let Some(index) = self.marker.take()
                    && index + 1 == self.parsed.runs.len()
                {
                    self.parsed.runs.pop();
                }
                let state = State {
                    tone: Tone::Muted,
                    ..state
                };
                let marker = if checked { "☑ " } else { "☐ " };
                self.prefix = Some(self.parsed.runs.len());
                self.push(marker.into(), range, &state);
            }
            Event::Rule => {
                self.line(range.start);
                let (indent, quote) = self.nesting();
                let place = Place::Rule {
                    index: self.rules,
                    indent,
                    quote,
                };
                self.rules += 1;
                self.push_at(String::new(), range.clone(), &state, place);
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>, state: State, host: &dyn Host) {
        let theme = self.theme;
        let mut next = state.clone();
        match tag {
            Tag::Strong => next.style = next.style.weight(theme.bold_weight),
            Tag::Emphasis => next.style = next.style.style(FontStyle::Italic),
            Tag::Strikethrough => next.style = next.style.strikethrough(true),
            Tag::Superscript | Tag::Subscript => {}
            Tag::Heading { level, .. } => {
                self.line(range.start);
                next.style = TextStyle::new(theme.heading_sizes[level as usize - 1])
                    .weight(theme.bold_weight);
            }
            Tag::Paragraph | Tag::HtmlBlock | Tag::MetadataBlock(_) => self.line(range.start),
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.line(range.start)
            }
            Tag::BlockQuote(kind) => {
                self.line(range.start);
                self.quote += 1;
                if let Some(kind) = kind {
                    let label = match kind {
                        BlockQuoteKind::Note => "Note",
                        BlockQuoteKind::Tip => "Tip",
                        BlockQuoteKind::Important => "Important",
                        BlockQuoteKind::Warning => "Warning",
                        BlockQuoteKind::Caution => "Caution",
                    };
                    let label_state = State {
                        style: next.style.clone().weight(theme.bold_weight),
                        tone: Tone::Muted,
                        ..next.clone()
                    };
                    self.push(format!("{label}\n"), range.start..range.start, &label_state);
                }
            }
            Tag::CodeBlock(kind) => {
                self.line(range.start);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|word| !word.is_empty())
                        .map(str::to_owned),
                    CodeBlockKind::Indented => None,
                };
                let (indent, quote) = self.nesting();
                let place = Place::Code {
                    index: self.parsed.code.len(),
                    indent,
                    quote,
                };
                self.parsed.code.push(CodeBlock { language });
                self.container = Some(place);
                next.style = next.style.family(FontFamily::Monospace);
                next.code = true;
                // Materialize the block even when the fence is empty.
                self.push_at(String::new(), range.start..range.start, &next, place);
            }
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let url = if link_type == LinkType::Email && !dest_url.starts_with("mailto:") {
                    format!("mailto:{dest_url}")
                } else {
                    dest_url.into_string()
                };
                if host.allow_link(&url) {
                    next.link = Some(url);
                    next.style = next.style.underline(true);
                }
            }
            Tag::Image { dest_url, .. } => {
                let url = dest_url.into_string();
                let label = State {
                    tone: Tone::Muted,
                    ..next.clone()
                };
                self.push("Image: ".into(), range.start..range.start, &label);
                if host.allow_link(&url) {
                    next.link = Some(url.clone());
                    next.style = next.style.underline(true);
                }
                next.tone = Tone::Muted;
                self.image = Some((range.clone(), url, false));
            }
            Tag::List(start) => {
                self.line(range.start);
                self.lists.push(start);
            }
            Tag::Item => {
                self.line(range.start);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let marker = format!("{n}. ");
                        *n += 1;
                        marker
                    }
                    _ => "• ".into(),
                };
                let marker_state = State {
                    tone: Tone::Muted,
                    ..next.clone()
                };
                self.marker = Some(self.parsed.runs.len());
                self.prefix = Some(self.parsed.runs.len());
                self.push(marker, range.start..range.start, &marker_state);
            }
            Tag::FootnoteDefinition(label) => {
                self.line(range.start);
                let label_state = State {
                    tone: Tone::Muted,
                    ..next.clone()
                };
                self.prefix = Some(self.parsed.runs.len());
                self.push(
                    format!("[{label}] "),
                    range.start..range.start,
                    &label_state,
                );
            }
            Tag::Table(alignments) => {
                self.line(range.start);
                let (indent, quote) = self.nesting();
                let place = Place::Table {
                    index: self.parsed.tables.len(),
                    indent,
                    quote,
                };
                self.parsed.tables.push(Table {
                    alignments: alignments
                        .into_iter()
                        .map(|alignment| match alignment {
                            pulldown_cmark::Alignment::None => Alignment::None,
                            pulldown_cmark::Alignment::Left => Alignment::Left,
                            pulldown_cmark::Alignment::Center => Alignment::Center,
                            pulldown_cmark::Alignment::Right => Alignment::Right,
                        })
                        .collect(),
                    rows: Vec::new(),
                });
                self.container = Some(place);
                self.push(String::new(), range.start..range.start, &next);
            }
            Tag::TableHead | Tag::TableRow => {
                let table = self.parsed.tables.last_mut().unwrap();
                let first = table.rows.is_empty();
                table.rows.push(Row {
                    header: matches!(tag, Tag::TableHead),
                    cells: Vec::new(),
                });
                self.header = matches!(tag, Tag::TableHead);
                if !first {
                    self.push("\n".into(), range.start..range.start, &next);
                }
            }
            Tag::TableCell => {
                let first = self
                    .parsed
                    .tables
                    .last()
                    .and_then(|table| table.rows.last())
                    .is_none_or(|row| row.cells.is_empty());
                if !first {
                    self.push("\t".into(), range.start..range.start, &next);
                }
                let start = self.display_end();
                if let Some(row) = self
                    .parsed
                    .tables
                    .last_mut()
                    .and_then(|table| table.rows.last_mut())
                {
                    row.cells.push(start..start);
                }
                if self.header {
                    next.style = next.style.weight(theme.bold_weight);
                }
            }
        }
        self.stack.push(next);
    }

    fn end(&mut self, tag: TagEnd, range: Range<usize>, state: State) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {
                if !self.ends_line() {
                    self.push("\n".into(), range.end..range.end, &state);
                }
            }
            TagEnd::CodeBlock => {
                if !self.ends_line() {
                    self.push("\n".into(), range.end..range.end, &state);
                }
                self.container = None;
            }
            TagEnd::TableCell => {
                let end = self.display_end();
                if let Some(cell) = self
                    .parsed
                    .tables
                    .last_mut()
                    .and_then(|table| table.rows.last_mut())
                    .and_then(|row| row.cells.last_mut())
                {
                    cell.end = end;
                }
            }
            TagEnd::TableHead => self.header = false,
            TagEnd::Table => {
                self.push("\n".into(), range.end..range.end, &state);
                self.container = None;
            }
            TagEnd::Image => {
                if let Some((source, url, seen)) = self.image.take()
                    && !seen
                {
                    self.push(url, source, &state);
                }
            }
            _ => {}
        }
        if matches!(tag, TagEnd::BlockQuote(_)) {
            self.quote = self.quote.saturating_sub(1);
        }
        if matches!(tag, TagEnd::List(_)) {
            self.lists.pop();
        }
        if matches!(tag, TagEnd::Item) {
            self.marker = None;
        }
        self.stack.pop();
    }

    fn display_end(&self) -> usize {
        self.parsed.runs.last().map_or(0, |run| run.display.end)
    }

    /// Queue literal text so adjacent fragments are scanned for URLs together.
    fn literal(&mut self, range: Range<usize>) {
        if let Some(image) = &mut self.image {
            image.2 = true;
        }
        let state = self.state();
        if let Some((pending, _)) = &mut self.pending
            && pending.end == range.start
        {
            pending.end = range.end;
            return;
        }
        self.flush();
        self.pending = Some((range, state));
    }

    fn flush(&mut self) {
        let Some((range, state)) = self.pending.take() else {
            return;
        };
        let text = &self.source[range.clone()];
        if state.code || state.link.is_some() {
            self.push(text.into(), range, &state);
            return;
        }
        let mut cursor = 0;
        for (url, dest) in find_urls(text) {
            if cursor < url.start {
                self.push(
                    text[cursor..url.start].into(),
                    range.start + cursor..range.start + url.start,
                    &state,
                );
            }
            let linked = State {
                style: state.style.clone().underline(true),
                link: Some(dest),
                ..state.clone()
            };
            self.push(
                text[url.clone()].into(),
                range.start + url.start..range.start + url.end,
                &linked,
            );
            cursor = url.end;
        }
        if cursor < text.len() || text.is_empty() {
            self.push(
                text[cursor..].into(),
                range.start + cursor..range.end,
                &state,
            );
        }
    }

    fn push(&mut self, text: String, source: Range<usize>, state: &State) {
        let place = self.place();
        self.push_at(text, source, state, place);
    }

    fn push_at(&mut self, text: String, source: Range<usize>, state: &State, place: Place) {
        let start = self.display_end();
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
            } else if state.code {
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
        if !aligned || (!state.code && consumed != raw.len()) {
            source_points.clear();
            source_points.push((0, source.start));
        }
        source_points.push((text.len(), source.end));
        self.parsed.runs.push(Run {
            display: start..start + text.len(),
            text,
            source,
            style: state.style.clone(),
            code: state.code,
            link: state.link.clone(),
            tone: state.tone,
            place,
            source_points,
        });
    }
}

/// GFM extended autolinks: `http://`, `https://` and `www.` URLs in plain
/// text, with trailing punctuation and unbalanced parentheses excluded.
/// Returns each URL's byte range in `text` and its link destination.
pub fn find_urls(text: &str) -> Vec<(Range<usize>, String)> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        let prefix = ["https://", "http://", "www."].into_iter().find(|prefix| {
            rest.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        });
        let boundary = text[..index]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '('));
        let Some(prefix) = prefix.filter(|_| boundary) else {
            index += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };
        let mut end = rest
            .find(|c: char| c.is_whitespace() || c == '<')
            .unwrap_or(rest.len());
        loop {
            let candidate = &rest[..end];
            let Some(last) = candidate.chars().next_back() else {
                break;
            };
            let trim = matches!(
                last,
                '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' | ';'
            ) || (last == ')'
                && candidate.matches('(').count() < candidate.matches(')').count());
            if !trim {
                break;
            }
            end -= last.len_utf8();
        }
        let host = &rest[prefix.len()..end];
        let domain = host.split(['/', '?', '#']).next().unwrap_or_default();
        let valid = !domain.is_empty()
            && domain
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
            && (prefix != "www." || domain.contains('.'))
            && domain.chars().any(char::is_alphanumeric);
        if valid {
            let url = &rest[..end];
            let dest = if prefix.eq_ignore_ascii_case("www.") {
                format!("http://{url}")
            } else {
                url.to_owned()
            };
            found.push((index..index + end, dest));
            index += end;
        } else {
            index += prefix.len();
        }
    }
    found
}
