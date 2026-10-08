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

use crate::{model::Model, settings::ThemeMode, theme::*};
use mosaic::prelude::*;

#[component]
pub fn AppearanceSegments(model: Model, field: usize) -> Element {
    let options: Vec<&str> = match field {
        0 => vec!["Dark", "Light", "System"],
        1 => vec!["Paper", "Warm"],
        _ => vec!["Slate", "Neutral"],
    };
    let count = options.len();
    let focus: std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<usize, Element>>> =
        Default::default();
    let width = State::new(86.0 * count as f32);
    let index = Derived::new(move || {
        let p = model.preferences.get();
        match field {
            0 => match p.mode {
                ThemeMode::Dark => 0,
                ThemeMode::Light => 1,
                ThemeMode::System => 2,
            },
            1 => usize::from(p.light_warm),
            _ => usize::from(p.dark_neutral),
        }
    });
    let select = move |slot: usize| {
        model.preferences.update(|p| match field {
            0 => {
                p.mode = match slot {
                    0 => ThemeMode::Dark,
                    1 => ThemeMode::Light,
                    _ => ThemeMode::System,
                }
            }
            1 => p.light_warm = slot == 1,
            _ => p.dark_neutral = slot == 1,
        })
    };
    view! {
        stack width:{px(86.0*count as f32)}px max-width:100% height:{px(38.0)}px
            @layout:{move |rect:Rect|width.set(rect.size.width)} fill:raised radius:{px(7.0)}px
            label:{match field{0=>"Theme",1=>"Light palette",_=>"Dark palette"}} {
            el nohit width:{width.get()/count as f32}px height:fill fill:accent-soft
                radius:{px(7.0)}px translate:(x:{width.get()/count as f32*index.get() as f32}px)
                transition:(translate:ease(140.0)) {}
            row {
                let focus=focus.clone();
                for (slot, label) in options.into_iter().enumerate() {
                    let name=State::new(label.to_string());
                    button #tree-control @click:{select(slot);} width:1fr height:fill role:radio
                        label:{format!("{}: {}",match field{0=>"Theme",1=>"Light palette",_=>"Dark palette"},name.get())}
                        font-color:{mosaic::core::theme::color(if index.get()==slot {accent}else{ink})} {
                        text {name.get()}
                    } as option
                    {let semantic_option=option.clone();Effect::new(move || {semantic_option.toggled(index.get()==slot);});}
                    {let keys=focus.clone();keys.borrow_mut().insert(slot,option.clone());let cleanup=keys.clone();on_cleanup(move ||{cleanup.borrow_mut().remove(&slot);});option.on_key(move |event,ctx| {if matches!(event.kind,KeyEventKind::Down{..}) {let next=match event.key {Key::ArrowRight|Key::ArrowDown=>(index.get_untracked()+1)%count,Key::ArrowLeft|Key::ArrowUp=>(index.get_untracked()+count-1)%count,Key::Home=>0,Key::End=>count-1,_=>return};select(next);let target=keys.borrow().get(&next).cloned();if let Some(target)=target{target.focus();}ctx.stop_propagation();}});}
                }
            }
        }
    }
}
