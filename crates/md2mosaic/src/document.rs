//! The rendered document: blocks of shaped text positioned in one coordinate
//! space, with display offsets that map back to Markdown source bytes.
use crate::parse::{self, Alignment, Parsed, Place, Run, Tone};
use crate::theme::{Host, Theme};
use mosaic::{
    core::{Color, Rect, Size, Vector2},
    render::{ClipShape, Paint, Scene},
    text::{CaretMotion, FontContext, ShapedText, TextRun, TextStyle},
    widgets::Visual,
};
use std::{ops::Range, rc::Rc};
use unicode_segmentation::UnicodeSegmentation;

/// A contiguous display range rendered as one block.
#[derive(Clone, Debug)]
struct Block {
    place: Place,
    display: Range<usize>,
}

/// One shaped text placed in the document: a text block or a table cell.
struct Piece {
    display: Range<usize>,
    origin: Vector2,
    shaped: ShapedText,
    /// Display range and run index of each styled slice, in shaping order.
    slices: Vec<(Range<usize>, usize)>,
    /// Index into [`Layout::tables`] when the piece scrolls with a table.
    table: Option<usize>,
}

#[derive(Clone, Copy)]
enum Fill {
    Code,
    Rule,
    RuleStrong,
}

struct TableLayout {
    /// [`Parsed::tables`] index, which also keys the scroll offset.
    index: usize,
    viewport: Rect,
    content_width: f32,
    /// Column boundaries relative to the viewport's left edge.
    columns: Vec<f32>,
    /// Row boundaries in document coordinates.
    rows: Vec<f32>,
    /// Pieces in row-major order, one per (row, column).
    pieces: Range<usize>,
    width: usize,
}

/// A vertical slice of the document, for hit testing.
struct Band {
    top: f32,
    bottom: f32,
    pieces: Range<usize>,
    table: Option<usize>,
}

struct Layout {
    width: Option<f32>,
    size: Size,
    pieces: Vec<Piece>,
    bands: Vec<Band>,
    tables: Vec<TableLayout>,
    fills: Vec<(Rect, Fill, Option<usize>)>,
    labels: Vec<(Vector2, ShapedText)>,
}

/// A read-only Markdown projection. Source bytes remain the document's
/// authority: rendering builds display text whose offsets map back to them,
/// so selection and copy can operate on the original Markdown.
pub struct Document {
    source: String,
    host: Rc<dyn Host>,
    theme: Theme,
    parsed: Parsed,
    display: String,
    blocks: Vec<Block>,
    layout: Option<Layout>,
    scroll: Vec<f32>,
}

impl Document {
    /// Parse `source` with `host`'s theme and link policy.
    pub fn new(source: &str, host: Rc<dyn Host>) -> Self {
        let mut document = Self {
            source: String::new(),
            theme: host.theme(),
            host,
            parsed: Parsed::default(),
            display: String::new(),
            blocks: Vec::new(),
            layout: None,
            scroll: Vec::new(),
        };
        document.rebuild(source);
        document
    }

    /// Parse `source` with the default theme and link policy.
    pub fn with_defaults(source: &str) -> Self {
        Self::new(source, Rc::new(crate::DefaultHost))
    }

    fn rebuild(&mut self, source: &str) {
        self.source = source.into();
        self.parsed = parse::parse(source, &self.theme, &*self.host);
        self.display = self
            .parsed
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        self.blocks = blocks(&self.parsed.runs, &self.display);
        self.layout = None;
        self.scroll.resize(self.parsed.tables.len(), 0.0);
        self.scroll.truncate(self.parsed.tables.len());
    }

    /// Re-parse when the source or the host's theme changed.
    pub fn update(&mut self, source: &str) {
        let theme = self.host.theme();
        if self.source != source || self.theme != theme {
            self.theme = theme;
            let source = source.to_owned();
            self.rebuild(&source);
        }
    }

    fn refresh(&mut self) {
        let theme = self.host.theme();
        if self.theme != theme {
            self.theme = theme;
            let source = std::mem::take(&mut self.source);
            self.rebuild(&source);
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The styled runs, in display and source order.
    pub fn runs(&self) -> &[Run] {
        &self.parsed.runs
    }

    /// Table structure, indexed by [`Place::Table`].
    pub fn tables(&self) -> &[crate::Table] {
        &self.parsed.tables
    }

    /// Code block metadata, indexed by [`Place::Code`].
    pub fn code_blocks(&self) -> &[crate::CodeBlock] {
        &self.parsed.code
    }

    /// The rendered text that display offsets index. Table cells are
    /// separated by tabs and blocks by newlines.
    pub fn display_text(&self) -> &str {
        &self.display
    }

    pub fn display_len(&self) -> usize {
        self.display.len()
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Ask the host to open `url`, if its link policy allows it.
    pub fn open_link(&self, url: &str) {
        if self.host.allow_link(url) {
            self.host.open_link(url);
        }
    }

    // Source mapping.

    pub fn display_offset(&self, source: usize) -> usize {
        for run in &self.parsed.runs {
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
        self.parsed.runs.last().map_or(0, |r| r.display.end)
    }

    pub fn source_offset(&self, display: usize) -> usize {
        self.offset_with_affinity(display, false)
    }

    pub(crate) fn offset_with_affinity(&self, display: usize, end: bool) -> usize {
        for run in &self.parsed.runs {
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
        self.parsed.runs.last().map_or(0, |r| r.source.end)
    }

    /// Source offsets for a selection between two source offsets, snapped so
    /// each end excludes hidden delimiters on the outside of the selection.
    pub fn selection_range(&self, anchor: usize, head: usize) -> (usize, usize) {
        let a = self.display_offset(anchor);
        let b = self.display_offset(head);
        (
            self.offset_with_affinity(a, a > b),
            self.offset_with_affinity(b, b > a),
        )
    }

    // Layout.

    fn layout(&mut self, fonts: &mut FontContext, width: Option<f32>) -> &Layout {
        self.refresh();
        if self
            .layout
            .as_ref()
            .is_none_or(|layout| layout.width != width)
        {
            let layout = self.lay_out(fonts, width);
            for table in &layout.tables {
                let limit = (table.content_width - table.viewport.size.width).max(0.0);
                self.scroll[table.index] = self.scroll[table.index].clamp(0.0, limit);
            }
            self.layout = Some(layout);
        }
        self.layout.as_ref().unwrap()
    }

    /// The rendered size at a wrap width (`None` for unwrapped content).
    /// Wide tables never exceed a definite width; they scroll horizontally.
    pub fn size(&mut self, fonts: &mut FontContext, width: Option<f32>) -> Size {
        self.layout(fonts, width).size
    }

    fn shape(
        &self,
        fonts: &mut FontContext,
        display: Range<usize>,
        width: Option<f32>,
    ) -> (ShapedText, Vec<(Range<usize>, usize)>) {
        let mut slices = Vec::new();
        for (index, run) in self.parsed.runs.iter().enumerate() {
            let start = run.display.start.max(display.start);
            let end = run.display.end.min(display.end);
            if start < end {
                slices.push((start..end, index));
            }
        }
        let runs: Vec<_> = slices
            .iter()
            .map(|(range, index)| TextRun {
                text: &self.display[range.clone()],
                style: &self.parsed.runs[*index].style,
            })
            .collect();
        (fonts.shape_runs(&runs, &self.theme.text, width), slices)
    }

    fn lay_out(&self, fonts: &mut FontContext, width: Option<f32>) -> Layout {
        let theme = &self.theme;
        let line = theme.text.resolved_line_height();
        let mut layout = Layout {
            width,
            size: Size::new(0.0, 0.0),
            pieces: Vec::new(),
            bands: Vec::new(),
            tables: Vec::new(),
            fills: Vec::new(),
            labels: Vec::new(),
        };
        let mut y = 0.0f32;
        let mut right = 0.0f32;
        let mut rules = Vec::new();
        let mut previous: Option<(Place, f32)> = None;
        for block in &self.blocks {
            let (indent, quote) = block.place.nesting();
            let x = f32::from(indent + quote) * theme.indent;
            let spaced = |place: Place| !matches!(place, Place::Flow { .. });
            let gap = if previous.is_some_and(|(place, _)| spaced(place) || spaced(block.place)) {
                theme.block_gap
            } else {
                0.0
            };
            y += gap;
            let top = y;
            let available = width.map(|width| (width - x).max(1.0));
            match block.place {
                Place::Flow { .. } => {
                    let (shaped, slices) = self.shape(fonts, block.display.clone(), available);
                    let size = shaped.metrics().size;
                    right = right.max(x + size.width);
                    let height = size.height.max(line);
                    layout.bands.push(Band {
                        top,
                        bottom: top + height,
                        pieces: layout.pieces.len()..layout.pieces.len() + 1,
                        table: None,
                    });
                    layout.pieces.push(Piece {
                        display: block.display.clone(),
                        origin: Vector2::new(x, top),
                        shaped,
                        slices,
                        table: None,
                    });
                    y += height;
                }
                Place::Code { index, .. } => {
                    let pad = theme.code_padding;
                    let inner = available.map(|width| (width - pad * 2.0).max(1.0));
                    let mut text_top = top + pad;
                    let mut label_width = 0.0f32;
                    if let Some(language) = &self.parsed.code[index].language {
                        let style = TextStyle::new(theme.label_size);
                        let label = fonts.shape_runs(
                            &[TextRun {
                                text: language,
                                style: &style,
                            }],
                            &theme.text,
                            inner,
                        );
                        let size = label.metrics().size;
                        label_width = size.width;
                        layout.labels.push((Vector2::new(x + pad, text_top), label));
                        text_top += size.height;
                    }
                    let (shaped, slices) = self.shape(fonts, block.display.clone(), inner);
                    let size = shaped.metrics().size;
                    let height = text_top - top + size.height.max(line) + pad;
                    let box_width = available.unwrap_or(size.width.max(label_width) + pad * 2.0);
                    right = right.max(x + box_width);
                    layout.fills.push((
                        Rect::from_xywh(x, top, box_width, height),
                        Fill::Code,
                        None,
                    ));
                    layout.bands.push(Band {
                        top,
                        bottom: top + height,
                        pieces: layout.pieces.len()..layout.pieces.len() + 1,
                        table: None,
                    });
                    layout.pieces.push(Piece {
                        display: block.display.clone(),
                        origin: Vector2::new(x + pad, text_top),
                        shaped,
                        slices,
                        table: None,
                    });
                    y += height;
                }
                Place::Rule { .. } => {
                    rules.push(layout.fills.len());
                    layout.fills.push((
                        Rect::from_xywh(x, top, available.unwrap_or(0.0), theme.rule_width),
                        Fill::Rule,
                        None,
                    ));
                    y += theme.rule_width;
                }
                Place::Table { index, .. } => {
                    y += self.lay_out_table(fonts, &mut layout, index, x, top, available);
                    let table = layout.tables.last().unwrap();
                    right = right.max(x + table.viewport.size.width);
                }
            }
            // Quote bars span the block and the gap shared with an enclosing
            // quote's previous block.
            for level in 0..quote {
                let joined = previous.is_some_and(|(place, _)| place.nesting().1 > level);
                let start = if joined { top - gap } else { top };
                layout.fills.push((
                    Rect::from_xywh(
                        f32::from(indent + level) * theme.indent,
                        start,
                        theme.rule_width * 2.0,
                        y - start,
                    ),
                    Fill::Rule,
                    None,
                ));
                right =
                    right.max(f32::from(indent + level) * theme.indent + theme.rule_width * 2.0);
            }
            previous = Some((block.place, y));
        }
        let full = width.unwrap_or(right);
        for index in rules {
            let rect = &mut layout.fills[index].0;
            rect.size.width = (full - rect.origin.x).max(0.0);
        }
        layout.size = Size::new(width.map_or(right, |width| right.min(width)), y);
        layout
    }

    fn lay_out_table(
        &self,
        fonts: &mut FontContext,
        layout: &mut Layout,
        index: usize,
        x: f32,
        top: f32,
        available: Option<f32>,
    ) -> f32 {
        let theme = &self.theme;
        let line = theme.text.resolved_line_height();
        let (pad_x, pad_y) = theme.cell_padding;
        let table = &self.parsed.tables[index];
        let width = table
            .rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0)
            .max(table.alignments.len());
        let cell = |row: usize, column: usize| {
            table.rows[row]
                .cells
                .get(column)
                .cloned()
                .unwrap_or_else(|| {
                    let end = table.rows[row].cells.last().map_or(0, |cell| cell.end);
                    end..end
                })
        };
        let mut natural = vec![pad_x * 2.0; width];
        let mut minimum = vec![pad_x * 2.0; width];
        let mut shaped = Vec::new();
        for row in 0..table.rows.len() {
            for (column, (natural, minimum)) in
                natural.iter_mut().zip(minimum.iter_mut()).enumerate()
            {
                let range = cell(row, column);
                let (text, slices) = self.shape(fonts, range.clone(), None);
                *natural = natural.max(text.metrics().size.width + pad_x * 2.0);
                let (narrow, _) = self.shape(fonts, range.clone(), Some(0.0));
                *minimum = minimum.max(narrow.metrics().size.width + pad_x * 2.0);
                shaped.push((range, text, slices));
            }
        }
        let total: f32 = natural.iter().sum();
        let least: f32 = minimum.iter().sum();
        let columns: Vec<f32> = match available {
            Some(available) if total > available => {
                if least >= available || total <= least {
                    minimum.clone()
                } else {
                    let ratio = (available - least) / (total - least);
                    natural
                        .iter()
                        .zip(&minimum)
                        .map(|(natural, minimum)| minimum + (natural - minimum) * ratio)
                        .collect()
                }
            }
            _ => natural.clone(),
        };
        // Reshape cells whose column is narrower than their natural width.
        for (position, (range, text, slices)) in shaped.iter_mut().enumerate() {
            let column = position % width.max(1);
            if text.metrics().size.width + pad_x * 2.0 > columns[column] + 0.01 {
                let wrap = (columns[column] - pad_x * 2.0).max(1.0);
                (*text, *slices) = self.shape(fonts, range.clone(), Some(wrap));
            }
        }
        let mut boundaries = vec![0.0f32];
        for column in &columns {
            boundaries.push(boundaries.last().unwrap() + column);
        }
        let content_width = *boundaries.last().unwrap();
        let mut rows = vec![top];
        for row in 0..table.rows.len() {
            let height = shaped[row * width..(row + 1) * width]
                .iter()
                .map(|(_, text, _)| text.metrics().size.height.max(line))
                .fold(line, f32::max)
                + pad_y * 2.0;
            rows.push(rows.last().unwrap() + height);
        }
        let bottom = *rows.last().unwrap();
        let viewport = Rect::from_xywh(
            x,
            top,
            available.map_or(content_width, |available| content_width.min(available)),
            bottom - top,
        );
        let table_index = layout.tables.len();
        let first = layout.pieces.len();
        for (position, (range, text, slices)) in shaped.into_iter().enumerate() {
            let (row, column) = (position / width.max(1), position % width.max(1));
            let inner = columns[column] - pad_x * 2.0;
            let used = text.metrics().size.width;
            let offset = match table.alignments.get(column).copied().unwrap_or_default() {
                Alignment::Right => (inner - used).max(0.0),
                Alignment::Center => ((inner - used) / 2.0).max(0.0),
                Alignment::None | Alignment::Left => 0.0,
            };
            layout.pieces.push(Piece {
                display: range,
                origin: Vector2::new(x + boundaries[column] + pad_x + offset, rows[row] + pad_y),
                shaped: text,
                slices,
                table: Some(table_index),
            });
        }
        let rule = theme.rule_width;
        for (row, edge) in rows.iter().enumerate() {
            let strong = row == 1 && table.rows.first().is_some_and(|row| row.header);
            let y = if row == rows.len() - 1 {
                edge - rule
            } else {
                *edge
            };
            layout.fills.push((
                Rect::from_xywh(x, y, content_width, rule),
                if strong { Fill::RuleStrong } else { Fill::Rule },
                Some(table_index),
            ));
        }
        for (column, edge) in boundaries.iter().enumerate() {
            let edge = if column == boundaries.len() - 1 {
                edge - rule
            } else {
                *edge
            };
            layout.fills.push((
                Rect::from_xywh(x + edge, top, rule, bottom - top),
                Fill::Rule,
                Some(table_index),
            ));
        }
        layout.bands.push(Band {
            top,
            bottom,
            pieces: first..layout.pieces.len(),
            table: Some(table_index),
        });
        layout.tables.push(TableLayout {
            index,
            viewport,
            content_width,
            columns: boundaries,
            rows,
            pieces: first..layout.pieces.len(),
            width,
        });
        bottom - top
    }

    // Geometry over display offsets.

    fn offset_of(&self, table: Option<usize>) -> Vector2 {
        let layout = self.layout.as_ref().unwrap();
        table.map_or(Vector2::new(0.0, 0.0), |table| {
            Vector2::new(-self.scroll[layout.tables[table].index], 0.0)
        })
    }

    fn piece_at(&self, display: usize) -> Option<usize> {
        let pieces = &self.layout.as_ref()?.pieces;
        let index = pieces.partition_point(|piece| piece.display.start <= display);
        Some(index.saturating_sub(1)).filter(|_| !pieces.is_empty())
    }

    fn caret_display(&self, display: usize) -> Rect {
        let layout = self.layout.as_ref().unwrap();
        let line = self.theme.text.resolved_line_height();
        let Some(index) = self.piece_at(display) else {
            return Rect::from_xywh(0.0, 0.0, 1.0, line);
        };
        let piece = &layout.pieces[index];
        let local = display.clamp(piece.display.start, piece.display.end) - piece.display.start;
        let rect = piece.shaped.caret_rect(local);
        rect.translate(piece.origin + self.offset_of(piece.table))
    }

    fn rects_display(&self, start: usize, end: usize) -> Vec<Rect> {
        let (start, end) = (start.min(end), start.max(end));
        let layout = self.layout.as_ref().unwrap();
        let mut rects = Vec::new();
        for piece in &layout.pieces {
            let from = start.max(piece.display.start);
            let to = end.min(piece.display.end);
            if from >= to {
                continue;
            }
            let offset = piece.origin + self.offset_of(piece.table);
            let viewport = piece.table.map(|table| layout.tables[table].viewport);
            for rect in piece
                .shaped
                .selection_rects(from - piece.display.start, to - piece.display.start)
            {
                let rect = rect.translate(offset);
                match viewport {
                    Some(viewport) => rects.extend(rect.intersection(&viewport)),
                    None => rects.push(rect),
                }
            }
        }
        rects
    }

    fn hit_display(&self, point: Vector2) -> usize {
        let layout = self.layout.as_ref().unwrap();
        if layout.bands.is_empty() {
            return 0;
        }
        let band = layout
            .bands
            .iter()
            .position(|band| point.y < band.bottom)
            .map(|index| {
                // In a gap, choose whichever neighbor is closer.
                let band = &layout.bands[index];
                if index > 0 && point.y < band.top {
                    let previous = &layout.bands[index - 1];
                    if point.y - previous.bottom < band.top - point.y {
                        return index - 1;
                    }
                }
                index
            })
            .unwrap_or(layout.bands.len() - 1);
        let band = &layout.bands[band];
        let piece = match band.table {
            None => band.pieces.start,
            Some(table) => {
                let table = &layout.tables[table];
                let row = table.rows[1..]
                    .iter()
                    .position(|edge| point.y < *edge)
                    .unwrap_or(table.rows.len().saturating_sub(2));
                let x = point.x - table.viewport.origin.x + self.scroll[table.index];
                let column = table.columns[1..]
                    .iter()
                    .position(|edge| x < *edge)
                    .unwrap_or(table.width.saturating_sub(1));
                table.pieces.start + row * table.width + column
            }
        };
        let Some(piece) = layout.pieces.get(piece) else {
            return layout.pieces.last().map_or(0, |piece| piece.display.end);
        };
        let local = point - piece.origin - self.offset_of(piece.table);
        piece.display.start
            + piece
                .shaped
                .hit(local)
                .unwrap_or(0)
                .min(piece.display.len())
    }

    fn piece_unit(&self, display: usize, line: bool) -> Range<usize> {
        let layout = self.layout.as_ref().unwrap();
        let Some(piece) = self
            .piece_at(display)
            .map(|index| &layout.pieces[index])
            .filter(|piece| piece.display.contains(&display) || piece.display.end == display)
        else {
            return display..display;
        };
        let local = display - piece.display.start;
        let range = if line {
            piece.shaped.line_at(local)
        } else {
            piece.shaped.word_at(local)
        };
        piece.display.start + range.start..piece.display.start + range.end
    }

    /// Scroll the table containing `display`, if any, so its caret is
    /// visible after keyboard navigation.
    fn reveal(&mut self, display: usize) {
        let Some(index) = self.piece_at(display) else {
            return;
        };
        let layout = self.layout.as_ref().unwrap();
        let Some(table) = layout.pieces[index].table else {
            return;
        };
        let table = &layout.tables[table];
        let caret = self.caret_display(display);
        let viewport = table.viewport;
        let mut scroll = self.scroll[table.index];
        if caret.origin.x < viewport.origin.x {
            scroll -= viewport.origin.x - caret.origin.x;
        } else if caret.max_x() > viewport.max_x() {
            scroll += caret.max_x() - viewport.max_x();
        }
        let limit = (table.content_width - viewport.size.width).max(0.0);
        self.scroll[table.index] = scroll.clamp(0.0, limit);
    }

    /// The caret rect at a source offset, in document coordinates.
    pub fn caret(&mut self, fonts: &mut FontContext, width: f32, source: usize) -> Rect {
        let display = self.display_offset(source);
        self.layout(fonts, Some(width));
        self.caret_display(display)
    }

    /// The source offset nearest a point in document coordinates.
    pub fn hit(&mut self, fonts: &mut FontContext, width: f32, point: Vector2) -> usize {
        self.layout(fonts, Some(width));
        let display = self.hit_display(point);
        self.source_offset(display)
    }

    /// Rects covering the rendered text between two display offsets, clipped
    /// to scrolled table viewports.
    pub fn rects(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        display: Range<usize>,
    ) -> Vec<Rect> {
        self.layout(fonts, Some(width));
        self.rects_display(display.start, display.end)
    }

    /// The allowed link destination drawn under `point`, using the actual
    /// glyph bounds of each wrapped line.
    pub fn link_at(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        point: Vector2,
    ) -> Option<String> {
        self.layout(fonts, Some(width));
        // Caret hits round the right half of a final glyph to the offset just
        // after the link. Link activation instead uses the actual run bounds,
        // including each wrapped line, without expanding into neighboring text.
        self.parsed.runs.iter().find_map(|run| {
            let url = run.link.as_ref()?;
            self.rects_display(run.display.start, run.display.end)
                .iter()
                .any(|rect| rect.contains(point))
                .then(|| url.clone())
        })
    }

    /// Scroll an overflowing table under `point` by a wheel delta. Returns
    /// whether the delta was consumed; vertical-dominant deltas never are.
    pub fn scroll(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        point: Vector2,
        delta: Vector2,
    ) -> bool {
        if delta.x.abs() <= delta.y.abs() {
            return false;
        }
        self.layout(fonts, Some(width));
        let layout = self.layout.as_ref().unwrap();
        let Some(table) = layout
            .tables
            .iter()
            .find(|table| table.viewport.contains(point))
        else {
            return false;
        };
        let limit = (table.content_width - table.viewport.size.width).max(0.0);
        let current = self.scroll[table.index];
        let next = (current - delta.x).clamp(0.0, limit);
        self.scroll[table.index] = next;
        next != current
    }

    /// The word or (logical) line around a source offset, as source offsets.
    pub fn unit(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        source: usize,
        line: bool,
    ) -> (usize, usize) {
        let offset = self.display_offset(source);
        self.layout(fonts, Some(width));
        let range = self.piece_unit(offset, line);
        (
            self.source_offset(range.start),
            self.offset_with_affinity(range.end, true),
        )
    }

    /// Move a caret at a source offset, returning the new source offset.
    pub fn motion(
        &mut self,
        fonts: &mut FontContext,
        width: f32,
        source: usize,
        motion: CaretMotion,
    ) -> usize {
        let offset = self.display_offset(source);
        self.layout(fonts, Some(width));
        let text = self.display.clone();
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let mut target = match motion {
            CaretMotion::Left => text[..offset]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(i, _)| i),
            CaretMotion::Right => text[offset..]
                .graphemes(true)
                .next()
                .map_or(offset, |c| offset + c.len()),
            CaretMotion::LineStart => self.piece_unit(offset, true).start,
            CaretMotion::LineEnd => self.piece_unit(offset, true).end,
            CaretMotion::WordLeft => {
                self.piece_unit(
                    text[..offset]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i),
                    false,
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
                let caret = self.caret_display(offset);
                let y = caret.origin.y
                    + caret.size.height
                        * if matches!(motion, CaretMotion::Up) {
                            -0.5
                        } else {
                            1.5
                        };
                let layout = self.layout.as_ref().unwrap();
                if y < 0.0 {
                    0
                } else if y > layout.size.height {
                    text.len()
                } else {
                    self.hit_display(Vector2::new(caret.origin.x, y))
                }
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
        self.reveal(target.min(text.len()));
        let mapped = self.source_offset(target);
        if matches!(motion, CaretMotion::Right | CaretMotion::WordRight) {
            mapped.max(source)
        } else {
            mapped
        }
    }

    // Painting. Origins are the document's top-left in scene coordinates.

    /// Paint fills that sit under text and selection: code backgrounds,
    /// table dividers, quote bars and rules.
    pub fn paint_under(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontContext,
        width: f32,
        origin: Vector2,
    ) {
        self.layout(fonts, Some(width));
        let colors = self.theme.colors;
        let layout = self.layout.as_ref().unwrap();
        for (rect, fill, table) in &layout.fills {
            let color = match fill {
                Fill::Code => colors.code,
                Fill::Rule => colors.rule,
                Fill::RuleStrong => colors.rule_strong,
            };
            let mut rect = rect.translate(self.offset_of(*table));
            if let Some(table) = table {
                match rect.intersection(&layout.tables[*table].viewport) {
                    Some(clipped) => rect = clipped,
                    None => continue,
                }
            }
            fill_rect(scene, rect.translate(origin), color);
        }
        // Overflow indicator along a scrollable table's bottom divider.
        for table in &layout.tables {
            let viewport = table.viewport;
            if table.content_width > viewport.size.width + 0.5 {
                let share = viewport.size.width / table.content_width;
                let limit = table.content_width - viewport.size.width;
                let thumb = viewport.size.width * share;
                let x = viewport.origin.x
                    + (viewport.size.width - thumb) * (self.scroll[table.index] / limit);
                let height = self.theme.rule_width * 2.0;
                fill_rect(
                    scene,
                    Rect::from_xywh(x, viewport.max_y() - height, thumb, height).translate(origin),
                    colors.rule_strong,
                );
            }
        }
        let inline = self
            .parsed
            .runs
            .iter()
            .filter(|run| run.code && !matches!(run.place, Place::Code { .. }))
            .map(|run| run.display.clone())
            .collect::<Vec<_>>();
        for range in inline {
            for rect in self.rects_display(range.start, range.end) {
                fill_rect(scene, rect.translate(origin), colors.code);
            }
        }
    }

    /// Fill the rendered extent of a display range, such as a selection or a
    /// search match.
    pub fn fill(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontContext,
        width: f32,
        origin: Vector2,
        display: Range<usize>,
        color: Color,
    ) {
        for rect in self.rects(fonts, width, display) {
            fill_rect(scene, rect.translate(origin), color);
        }
    }

    /// Paint glyphs and decorations. `ink` colors ordinary text.
    pub fn paint_text(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontContext,
        width: f32,
        origin: Vector2,
        scale: f32,
        ink: Paint,
    ) {
        self.layout(fonts, Some(width));
        let colors = self.theme.colors;
        let layout = self.layout.as_ref().unwrap();
        for (position, label) in &layout.labels {
            scene.glyphs(label.place(fonts, colors.muted, origin + *position, scale));
        }
        for piece in &layout.pieces {
            let at = origin + piece.origin + self.offset_of(piece.table);
            if let Some(table) = piece.table {
                scene.push_clip(ClipShape::rect(
                    layout.tables[table].viewport.translate(origin),
                ));
            }
            let paints: Vec<Paint> = piece
                .slices
                .iter()
                .map(|(_, run)| match self.parsed.runs[*run].tone {
                    Tone::Ink => ink.clone(),
                    Tone::Muted => colors.muted.into(),
                })
                .collect();
            scene.glyphs(
                piece
                    .shaped
                    .place_runs(fonts, ink.clone(), &paints, at, scale),
            );
            for rect in piece.shaped.decorations(at, scale) {
                fill_rect(scene, rect, colors.ink);
            }
            if piece.table.is_some() {
                scene.pop_clip();
            }
        }
    }
}

fn fill_rect(scene: &mut Scene, rect: Rect, color: Color) {
    scene.shape(Visual::new().fill(color).shape(rect));
}

/// Group consecutive runs that share a place into blocks. Separating
/// newlines at the edges of text blocks are gaps, not rendered lines.
fn blocks(runs: &[Run], display: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for run in runs {
        match blocks.last_mut() {
            Some(block) if block.place == run.place => block.display.end = run.display.end,
            _ => blocks.push(Block {
                place: run.place,
                display: run.display.clone(),
            }),
        }
    }
    blocks.retain_mut(|block| match block.place {
        Place::Flow { .. } => {
            let text = &display[block.display.clone()];
            let start = text.len() - text.trim_start_matches('\n').len();
            let end = text.trim_end_matches('\n').len();
            if start >= end {
                return false;
            }
            block.display = block.display.start + start..block.display.start + end;
            true
        }
        Place::Code { .. } => {
            if display[block.display.clone()].ends_with('\n') {
                block.display.end -= 1;
            }
            true
        }
        Place::Table { .. } | Place::Rule { .. } => true,
    });
    blocks
}
