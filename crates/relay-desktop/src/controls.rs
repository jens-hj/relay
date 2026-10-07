use mosaic::prelude::{Element, TextStyle};

// The pinned Mosaic DSL creates both static and reactive button labels with
// ButtonStyle::default(). Inherit typography so the app's family and reactive
// scale apply to labels as well as the button's padding and hit rectangle.
pub struct ButtonStyle;

impl ButtonStyle {
    pub fn default() -> mosaic::prelude::ButtonStyle {
        mosaic::prelude::ButtonStyle {
            label: TextStyle::inherited(),
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
