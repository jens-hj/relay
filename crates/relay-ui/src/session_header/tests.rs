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

#[test]
fn the_pause_cell_offers_resume_while_the_queue_is_held() {
    use crate::prelude::*;
    use mosaic::prelude::*;
    use std::rc::Rc;
    let noop: OnPress = Rc::new(|| {});
    let (pause, resume) = (noop.clone(), noop.clone());
    let mounted = crate::testing::mount(Size::new(900.0, 100.0), move || {
        view! {
            col {
                ControlBlock status:(AgentStatus::Working) on-pause:(Some(pause.clone()))
                ControlBlock status:(AgentStatus::Done) on-pause:(Some(resume.clone())) paused:true
            }
        }
    });
    assert!(mounted.has("Pause"));
    assert!(mounted.has("Resume"));
}

#[test]
fn descendant_marks_group_semantic_states_and_keep_navigation() {
    use AgentStatus::*;
    let pressed = Rc::new(Cell::new(0));
    let count = pressed.clone();
    let mounted = mount(Size::new(1200.0, 100.0), move || {
        let on_below: OnPress = Rc::new(move || count.set(count.get() + 1));
        view! {
            ControlBlock status:(Working)
                below:(vec![Done, Queued, Done, Idle, Waiting, Done, Queued])
                on-below:(Some(on_below))
        }
    });
    assert_eq!(
        mounted.count("Done"),
        1,
        "one mark for three completed workers"
    );
    assert_eq!(mounted.count("Queued"), 1);
    assert_eq!(mounted.count("Idle"), 1, "idle and queued remain distinct");
    assert_eq!(
        mounted.count("Working"),
        2,
        "selected status mark and title"
    );
    assert!(!mounted.has("Failed"));
    assert!(mounted.has("3"));
    assert!(mounted.has("2"));
    assert!(!mounted.has("1"), "singletons have no numeric suffix");
    assert!(mounted.has("Done: 3 agents below"));
    assert!(mounted.has("Waiting for approval: 1 agent below"));
    let positions = [
        "Waiting for approval: 1 agent below",
        "Queued: 2 agents below",
        "Done: 3 agents below",
        "Idle: 1 agent below",
    ]
    .map(|label| mounted.rect(label).origin.x);
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    mounted
        .click_label("Working. 7 agents below: 1 waiting, 2 queued, 3 done, 1 idle. Show in tree");
    assert_eq!(pressed.get(), 1);
}
