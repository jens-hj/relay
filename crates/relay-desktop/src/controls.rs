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
            disabled:(Derived::new(|| false)) fill-width:true framed:false
    }
}

#[component]
pub fn ResetSetting(
    model: Model,
    setting: crate::settings::Setting,
    name: String,
    #[prop(default = false)] flush: bool,
) -> Element {
    use crate::styles::*;
    use crate::theme::*;
    let name = State::new(name);
    view! {
        row width:{px(34.0)}px height:{px(34.0)}px shrink:0 {
            button #relay.icon-action @click:{model.preferences.update(|p|p.reset(setting));}
                stroke:(width:{px(1.0)} color:rule.line
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
