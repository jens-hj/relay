use super::*;
use crate::context::ContextView;
use crate::testing::mount;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn status_cell_names_the_agents_below() {
    use AgentStatus::*;
    assert_eq!(
        status_cell_label(Working, &[Done, Waiting, Working, Queued]),
        "Working. 4 agents below: 1 working, 1 waiting, 1 queued, 1 done. Show in tree"
    );
    assert_eq!(status_cell_label(Done, &[]), "Done");
}

#[test]
fn header_shows_title_gauge_and_only_the_controls_it_is_given() {
    let pressed = Rc::new(Cell::new(0));
    let status = State::new(AgentStatus::Working);
    let count = pressed.clone();
    let mounted = mount(Size::new(1200.0, 300.0), move || {
        let stop: OnPress = Rc::new(move || count.set(count.get() + 1));
        let below: OnPress = Rc::new(|| {});
        view! {
            col {
                SessionHeader path:(vec!["Release 2.4".to_string()]) title:"API migration"
                    status:(status) effort:(Some(Effort::new(5, 4)))
                    context:(Some(ContextView::new(124_000, 200_000, 2)))
                    below:(vec![AgentStatus::Done, AgentStatus::Waiting]) on-below:(Some(below))
                    on-stop:(Some(stop))
            }
        }
    });
    assert_eq!(mounted.rect("API migration").size.height, 88.0);
    assert!(mounted.has("Release 2.4 /"));
    assert!(mounted.has("Context 62 percent used, 124k of 200k, compacted 2 times"));
    assert!(mounted.has("Stop"));
    assert!(!mounted.has("Pause"), "no pause handler, no pause cell");
    let cell = mounted.rect("Working. 2 agents below: 1 waiting, 1 done. Show in tree");
    assert_eq!(cell.size.height, 88.0, "cells fill the header height");
    let stop = mounted.rect("Stop");
    assert_eq!(stop.origin.x + stop.size.width, 1200.0, "flush right");
    mounted.click_label("Stop");
    assert_eq!(pressed.get(), 1);
    status.set(AgentStatus::Done);
    mounted.settle();
    assert!(mounted.has("Done. 2 agents below: 1 waiting, 1 done. Show in tree"));
}
