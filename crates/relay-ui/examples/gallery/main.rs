//! Every relay-ui primitive in every state, on the dark theme, with a theme
//! switch for the variants: `cargo run -p relay-ui --example gallery`.

mod sections;

use mosaic::prelude::*;
use relay_ui::tokens::{ThemeVariant, ground, theme};
use std::rc::Rc;

fn main() {
    App::new("relay-ui gallery")
        .window(WindowConfig::new(1280.0, 960.0))
        .theme(theme(ThemeVariant::Dark, 1.0))
        .theme(relay_ui::icons::icons())
        .clear(ground.page)
        .run(|ui, context| {
            relay_ui::install(ui);
            ui.default_text_color(ground.ink);
            let context = context.clone();
            sections::gallery(Rc::new(move |variant| {
                context.set_theme(theme(variant, 1.0));
            }))
        });
}
