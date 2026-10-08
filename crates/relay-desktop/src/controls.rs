use mosaic::prelude::{Element, TextStyle};

// The pinned Mosaic DSL creates both static and reactive button labels with
// ButtonStyle::default(). Inherit typography so the app's family and reactive
// scale apply to labels as well as the button's padding and hit rectangle.
pub struct ButtonStyle;

impl ButtonStyle {
    pub fn default() -> mosaic::prelude::ButtonStyle {
        mosaic::prelude::ButtonStyle {
            label: TextStyle::inherited(),
            radius: 0.0,
            ..Default::default()
        }
    }
}

pub fn button(
    parent: &Element,
    label: impl Into<String>,
    on_click: impl FnMut() + 'static,
) -> Element {
    mosaic::widgets::button_styled(parent, label, ButtonStyle::default(), on_click)
}

use crate::{
    labels::{SlidingSegments, SlidingSegmentsProps},
    model::Model,
    settings::ThemeMode,
};
use mosaic::prelude::*;

#[component]
pub fn AppearanceSegments(model: Model, field: usize) -> Element {
    let (name, options): (&str, [&str; 3]) = match field {
        0 => ("Theme", ["Dark", "Light", "System"]),
        1 => ("Light palette", ["Paper", "Warm", "High contrast"]),
        _ => ("Dark palette", ["Slate", "Neutral", "High contrast"]),
    };
    let index = Derived::new(move || {
        let p = model.preferences.get();
        match field {
            0 => match p.mode {
                ThemeMode::Dark => 0,
                ThemeMode::Light => 1,
                ThemeMode::System => 2,
            },
            1 if p.light_high_contrast => 2,
            1 => usize::from(p.light_warm),
            _ if p.dark_high_contrast => 2,
            _ => usize::from(p.dark_neutral),
        }
    });
    // Choosing High contrast keeps the family flag, so returning to a soft
    // palette restores the family previously chosen.
    let select: crate::labels::Select = std::rc::Rc::new(move |slot: usize| {
        model.preferences.update(|p| match field {
            0 => {
                p.mode = match slot {
                    0 => ThemeMode::Dark,
                    1 => ThemeMode::Light,
                    _ => ThemeMode::System,
                }
            }
            1 if slot == 2 => p.light_high_contrast = true,
            1 => {
                p.light_high_contrast = false;
                p.light_warm = slot == 1;
            }
            _ if slot == 2 => p.dark_high_contrast = true,
            _ => {
                p.dark_high_contrast = false;
                p.dark_neutral = slot == 1;
            }
        })
    });
    view! {
        SlidingSegments name:(name.to_string())
            options:(options.iter().map(|o| o.to_string()).collect::<Vec<_>>()) index:(index)
            select:(select) attention-slot:(None) cell-width:(112.0)
            disabled:(Derived::new(|| false))
    }
}
