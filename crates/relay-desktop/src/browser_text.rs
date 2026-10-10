//! Browser input methods mirror Mosaic's focused editor, including byte/UTF-16 conversion.
use mosaic::prelude::*;

fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

fn replacement<'a>(before: &str, after: &'a str) -> (usize, usize, &'a str) {
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let suffix = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    (
        prefix,
        before.len() - suffix,
        &after[prefix..after.len() - suffix],
    )
}

pub(crate) fn update(ui: &Ui, id: u64, text: &str, start: usize, end: usize, composing: bool) {
    if !ui.wants_text_input() || ui.focused().is_none_or(|field| field.id().raw() != id) {
        return;
    }
    ui.dispatch_ime(ImeEvent::Preedit {
        text: String::new(),
        cursor: None,
    });
    let Some(contents) = ui.text_input_contents() else {
        return;
    };
    let (first, last, insert) = replacement(&contents.text, text);
    if composing {
        ui.dispatch_ime(ImeEvent::SetSelection {
            start: first,
            end: last,
        });
        let cursor = byte_offset(text, end)
            .saturating_sub(first)
            .min(insert.len());
        ui.dispatch_ime(ImeEvent::Preedit {
            text: insert.into(),
            cursor: Some((cursor, cursor)),
        });
    } else {
        if first != last || !insert.is_empty() {
            ui.dispatch_ime(ImeEvent::SetSelection {
                start: first,
                end: last,
            });
            ui.dispatch_ime(ImeEvent::Commit(insert.into()));
        }
        ui.dispatch_ime(ImeEvent::SetSelection {
            start: byte_offset(text, start),
            end: byte_offset(text, end),
        });
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use wasm_bindgen::{JsCast, closure::Closure, prelude::wasm_bindgen};

    #[wasm_bindgen(module = "/src/browser_text.js")]
    extern "C" {
        fn installTextInput(
            snapshot: &js_sys::Function,
            edit: &js_sys::Function,
            key: &js_sys::Function,
            resize: &js_sys::Function,
        );
        #[wasm_bindgen(js_name = focusTextInput)]
        pub(crate) fn focus(x: f32, y: f32);
    }
    pub(crate) fn install(ui: Ui, viewport: State<Option<Size>>) {
        let read_ui = ui.clone();
        let mut metadata = None::<(u64, String, bool)>;
        let snapshot = Closure::<dyn FnMut() -> String>::new(move || {
            if !read_ui.wants_text_input() {
                return String::new();
            }
            let Some(field) = read_ui.focused() else {
                return String::new();
            };
            let Some(contents) = read_ui.text_input_contents() else {
                return String::new();
            };
            let id = field.id().raw();
            if metadata.as_ref().is_none_or(|(old, _, _)| *old != id) {
                let tree = read_ui.inspection_snapshot();
                let node = tree.node(field.id());
                let label = node
                    .and_then(|n| n.label.clone())
                    .unwrap_or_else(|| "Text input".into());
                let multiline =
                    node.is_none_or(|n| n.element_name.as_deref() != Some("text_input"));
                metadata = Some((id, label, multiline));
            }
            let (_, label, multiline) = metadata.as_ref().unwrap();
            let rect = read_ui
                .inspection_rect(field.id())
                .unwrap_or_else(|| field.layout_rect());
            let caret = read_ui.ime_cursor_area().unwrap_or(rect);
            let start = contents.text[..contents.selection.0].encode_utf16().count();
            let end = contents.text[..contents.selection.1].encode_utf16().count();
            serde_json::json!({"id":id.to_string(),"text":contents.text,"start":start,"end":end,
                "label":label,"multiline":multiline,"rect":[rect.origin.x,rect.origin.y,rect.size.width,rect.size.height],
                "caret":[caret.origin.x,caret.origin.y]}).to_string()
        });
        let edit_ui = ui.clone();
        let edit = Closure::<dyn FnMut(String, String, u32, u32, bool)>::new(
            move |id: String, text: String, start, end, composing| {
                if let Ok(id) = id.parse() {
                    update(&edit_ui, id, &text, start as usize, end as usize, composing);
                    mosaic::core::reactive::flush();
                }
            },
        );
        let key = Closure::<dyn FnMut(String, bool, bool, bool, bool) -> bool>::new(
            move |name: String, shift, ctrl, alt, meta| {
                let key = match name.as_str() {
                    "Tab" => Key::Tab,
                    "Escape" => Key::Escape,
                    "Enter" => Key::Enter,
                    "ArrowLeft" => Key::ArrowLeft,
                    "ArrowRight" => Key::ArrowRight,
                    "ArrowUp" => Key::ArrowUp,
                    "ArrowDown" => Key::ArrowDown,
                    "Home" => Key::Home,
                    "End" => Key::End,
                    _ => Key::Character(name),
                };
                let consumed = ui.dispatch_key(KeyEvent {
                    key,
                    kind: KeyEventKind::Down { repeat: false },
                    modifiers: Modifiers {
                        shift,
                        ctrl,
                        alt,
                        meta,
                    },
                });
                // These events enter through the DOM editor rather than the
                // window driver. Settle focus/page effects before JS mirrors
                // the editor again, even while a GPU frame is pending.
                mosaic::core::reactive::flush();
                consumed
            },
        );
        let resize = Closure::<dyn FnMut(f32, f32)>::new(move |width, height| {
            viewport.set(Some(Size::new(width, height)));
            mosaic::core::reactive::flush();
        });
        installTextInput(
            snapshot.as_ref().unchecked_ref(),
            edit.as_ref().unchecked_ref(),
            key.as_ref().unchecked_ref(),
            resize.as_ref().unchecked_ref(),
        );
        snapshot.forget();
        edit.forget();
        key.forget();
        resize.forget();
    }
}
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{focus, install};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacements_preserve_unicode_boundaries_and_autocorrect_ranges() {
        assert_eq!(replacement("teh cat", "the cat"), (1, 3, "he"));
        assert_eq!(replacement("a🦀éz", "a🙂øz"), (1, 7, "🙂ø"));
        assert_eq!(replacement("a🙂", "a"), (1, 5, ""));
        assert_eq!(byte_offset("a🙂é", 2), 1);
        assert_eq!(byte_offset("a🙂é", 3), 5);
        assert_eq!(byte_offset("a🙂é", 4), 7);
    }
    #[test]
    fn browser_edits_reach_the_existing_editor_and_composition_commits_once() {
        let scope = Scope::new(|| {});
        scope.run(|| {
            let ui = Ui::new();
            let value = State::new("a🙂 teh".to_string());
            let input = text_area(&ui.root(), value);
            input.focus();
            let id = input.id().raw();
            update(&ui, id, "a🙂 the", 7, 7, false);
            assert_eq!(value.get(), "a🙂 the");
            update(&ui, id, "a🙂 th", 6, 6, false);
            assert_eq!(value.get(), "a🙂 th");
            update(&ui, id, "a🙂 你", 5, 5, true);
            assert_eq!(
                value.get(),
                "a🙂 ",
                "The replacement removes the selection, but candidate text stays in preedit"
            );
            assert_eq!(ui.text_input_contents().unwrap().text, "a🙂 你");
            update(&ui, id, "a🙂 你好", 6, 6, true);
            update(&ui, id, "a🙂 你好", 6, 6, false);
            update(&ui, id, "a🙂 你好", 6, 6, false);
            assert_eq!(value.get(), "a🙂 你好");
            assert_eq!(ui.text_input_contents().unwrap().selection, (12, 12));
            ui.clear_focus();
            update(&ui, id, "stale", 5, 5, false);
            assert_eq!(value.get(), "a🙂 你好");
        });
        scope.dispose();
    }
}
