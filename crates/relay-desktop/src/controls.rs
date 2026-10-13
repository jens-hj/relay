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

// The DSL calls this constructor; retain Mosaic's behaviour and parts,
// while allowing every internal text leaf to inherit Relay's type scale.
pub fn stepper_with_options(
    parent: &Element,
    value: State<f32>,
    options: StepperOptions,
) -> mosaic::widgets::Stepper {
    mosaic::widgets::stepper_styled_with_options(
        parent,
        value,
        mosaic::prelude::StepperStyle {
            text: TextStyle::inherited(),
            button: ButtonStyle::default(),
            radius: 0.0,
            ..Default::default()
        },
        options,
    )
}

pub fn button(
    parent: &Element,
    label: impl Into<String>,
    on_click: impl FnMut() + 'static,
) -> Element {
    mosaic::widgets::button_styled(parent, label, ButtonStyle::default(), on_click)
}

use crate::{
    icons::*,
    model::Model,
    settings::{Setting, ThemeMode},
};
use mosaic::prelude::*;
use relay_ui::prelude::{Segments, SegmentsProps, Toggle, ToggleProps};
use relay_ui::style::*;
use relay_ui::tokens::{ground, px};
use std::rc::Rc;

/// The theme mode as ruled segments: Dark, Light, or following the system.
#[component]
pub fn ModeSegments(model: Model) -> Element {
    const MODES: [ThemeMode; 3] = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
    let index = Derived::new(move || {
        let mode = model.preferences.get().mode;
        MODES.iter().position(|m| *m == mode).unwrap_or(0)
    });
    let select: Rc<dyn Fn(usize)> = Rc::new(move |slot: usize| {
        model.preferences.update(|p| p.mode = MODES[slot.min(2)]);
    });
    view! {
        Segments
            options:(["Dark", "Light", "System"].iter().map(|o| o.to_string()).collect::<Vec<_>>())
            name:("Theme".to_string()) selected:(relay_ui::Live::from(index)) on-select:(select)
    }
}

/// The high-contrast switch; it applies to both modes.
#[component]
pub fn ContrastToggle(model: Model) -> Element {
    let on = Derived::new(move || model.preferences.get().high_contrast);
    let flip: Rc<dyn Fn(bool)> =
        Rc::new(move |on| model.preferences.update(|p| p.high_contrast = on));
    view! {
        Toggle label:("High contrast".to_string()) on:(relay_ui::Live::from(on)) on-toggle:(flip)
    }
}

/// A reset cell that returns one setting to its default.
#[component]
pub fn ResetSetting(
    model: Model,
    setting: Setting,
    name: String,
    #[prop(default = false)] flush: bool,
) -> Element {
    let name = State::new(name);
    view! {
        row width:{px(34.0)}px height:{px(34.0)}px shrink:0 {
            button #relay.icon-action @click:{model.preferences.update(|p|p.reset(setting));}
                stroke:(width:{px(1.0)} color:ground.hairline
                    edges:if flush {StrokeEdges::LEFT} else {StrokeEdges::ALL}
                    offset:{px(if flush {0.0} else {-1.0})})
                disabled:{!model.preferences.get().overridden(setting)}
                label:{format!("Reset {}",name.get())} {
                icon size:{px(16.0)}px reset-icon
                tooltip #relay.tooltip summary:"Reset to default" {
                    text {format!("Reset {} to default",name.get())}
                }
            }
        }
    }
}
