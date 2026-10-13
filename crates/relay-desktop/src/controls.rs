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
    icons::*,
    model::Model,
    settings::{Setting, ThemeMode},
};
use mosaic::prelude::*;
use relay_ui::overlay::{Tooltip, TooltipProps};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Rule, Segments, SegmentsProps, Toggle, ToggleProps,
};
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

/// A reset cell that returns one setting to its default, at the right end
/// of its row.
#[component]
pub fn ResetSetting(model: Model, setting: Setting, name: String) -> Element {
    let unchanged = Derived::new(move || !model.preferences.get().overridden(setting));
    view! {
        row width:{px(40.0)}px shrink:0 {
            CellButton label:(format!("Reset {name}")) rule:(Rule::Left) horizontal-inset:(0.0)
                disabled:(unchanged)
                on-press:(Rc::new(move || model.preferences.update(|p| p.reset(setting))) as relay_ui::prelude::OnPress) {
                Tooltip text:(format!("Reset {name} to default")) {
                    icon size:{px(16.0)}px stroke:ground.ink reset-icon
                }
            }
        }
    }
}

/// The icon of each execution approval mode, in [`relay_core::ApprovalMode::ALL`]
/// order: a checked loop, a question, an open lock.
pub fn approval_icons() -> Vec<mosaic::prelude::SvgToken> {
    use relay_ui::icons::execution;
    vec![execution.automatic, execution.ask, execution.unrestricted]
}

/// The chosen colour of each approval mode: automatic runs at full steam,
/// Ask waits on you, unrestricted access is a risk.
pub fn approval_tones() -> Vec<mosaic::prelude::ColorToken> {
    use relay_ui::tokens::Signal;
    vec![
        Signal::FullSteam.token(),
        Signal::NeedsYou.token(),
        Signal::Failed.token(),
    ]
}
