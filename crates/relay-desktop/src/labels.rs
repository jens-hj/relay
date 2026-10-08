//! Shared Labelism primitives: square, rule-framed controls and readouts
//! whose every mark is a control or a reading.

use crate::theme::*;
use mosaic::core::theme::color;
use mosaic::prelude::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub type Select = Rc<dyn Fn(usize)>;

/// A single-choice selector whose square indicator slides to the selected
/// option. Options are radio buttons labelled "`name`: option".
#[component]
pub fn SlidingSegments(
    name: String,
    options: Vec<String>,
    index: Derived<usize>,
    select: Select,
    attention: Option<usize>,
    cell_width: f32,
) -> Element {
    let count = options.len().max(1);
    let focus: Rc<RefCell<BTreeMap<usize, Element>>> = Rc::default();
    let width = State::new(cell_width * count as f32);
    let group = State::new(name.clone());
    let is_attention = move || attention == Some(index.get());
    view! {
        stack width:{px(cell_width*count as f32)}px max-width:100% height:{px(34.0)}px shrink:0
            @layout:{move |rect:Rect|width.set(rect.size.width)} fill:surface
            stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)}) label:{group.get()} {
            el nohit width:{width.get()/count as f32}px height:fill
                fill:{color(if is_attention() {attention_fill} else {inverse})}
                translate:(x:{width.get()/count as f32*index.get() as f32}px)
                transition:(translate:ease(140.0)) {}
            row {
                let focus=focus.clone();
                let select=select.clone();
                for (slot, label) in options.into_iter().enumerate() {
                    let option_name=State::new(label);
                    let choose=select.clone();
                    button #tree-control @click:{choose(slot);} width:1fr height:fill role:radio
                        label:{format!("{}: {}",group.get(),option_name.get())}
                        stroke:(width:{px(if slot==0 {0.0} else {1.0})} color:edge edges:left)
                        font-color:{color(if index.get()!=slot {ink} else if is_attention() {on_attention} else {on_inverse})} {
                        text text-wrap:none {option_name.get()}
                    } as option
                    {let semantic_option=option.clone();Effect::new(move || {semantic_option.toggled(index.get()==slot);});}
                    {let keys=focus.clone();let choose=select.clone();keys.borrow_mut().insert(slot,option.clone());let cleanup=keys.clone();on_cleanup(move ||{cleanup.borrow_mut().remove(&slot);});option.on_key(move |event,ctx| {if matches!(event.kind,KeyEventKind::Down{..}) {let next=match event.key {Key::ArrowRight|Key::ArrowDown=>(index.get_untracked()+1)%count,Key::ArrowLeft|Key::ArrowUp=>(index.get_untracked()+count-1)%count,Key::Home=>0,Key::End=>count-1,_=>return};choose(next);let target=keys.borrow().get(&next).cloned();if let Some(target)=target{target.focus();}ctx.stop_propagation();}});}
                }
            }
        }
    }
}
