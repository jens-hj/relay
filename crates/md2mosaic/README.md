# md2mosaic

Render CommonMark and GitHub Flavored Markdown as read-only, selectable
[Mosaic](https://gitlab.com/unincorporated/mosaic) text.

`md2mosaic` parses Markdown with `pulldown-cmark`, lays it out as shaped text
blocks and paints them into a Mosaic scene. Each displayed character maps back
to the Markdown source bytes, so selections over rendered text can copy or
quote the original Markdown.

It has no application dependencies. The embedding application implements
`Host` to supply a `Theme`, a link policy and link activation.

## Supported Markdown

- Paragraphs, headings 1–6, emphasis, strong text and combinations
- Ordered lists (including start numbers), bullet lists, nesting, task lists
- Block quotes, nested quotes and GFM alerts (`> [!NOTE]`)
- Fenced code blocks with a language label, indented code blocks, inline code
- GFM tables with header rows, column alignment and horizontal scrolling for
  tables wider than the available width
- Strikethrough, footnote references and definitions
- Inline links, reference links, `<...>` autolinks and GFM bare URL autolinks
  (`https://`, `http://`, `www.`)
- Horizontal rules, hard and soft line breaks (soft breaks render as line
  breaks), backslash escapes and entities
- Images as their alt text (or URL), linked to the image
- Raw HTML blocks and inline HTML as literal text; HTML is never interpreted

Bare email addresses are not autolinked. Math, definition lists, superscript
and subscript extensions are not enabled.

## Use

```rust
use md2mosaic::{Document, Host, Theme};
use std::rc::Rc;

struct App;
impl Host for App {
    fn theme(&self) -> Theme {
        Theme::default()
    }
    fn open_link(&self, url: &str) {
        // Open url in a browser.
    }
}

let mut document = Document::new("# Title\n\n| a | b |\n|---|--:|\n| 1 | 2 |", Rc::new(App));
```

In a paint callback:

```rust,ignore
document.paint_under(scene, fonts, width, origin);
document.fill(scene, fonts, width, origin, selection, selection_color);
document.paint_text(scene, fonts, width, origin, scale, ink);
```

`Document::hit`, `caret`, `motion`, `unit` and `selection_range` work in
source offsets; `link_at` and `scroll` take points in document coordinates.
