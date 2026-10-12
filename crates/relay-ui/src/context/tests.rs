use super::*;
use crate::testing::mount;
use std::cell::Cell;

#[test]
fn levels_tokens_and_gap_marks() {
    let mut context = ContextView::new(124_000, 200_000, 2);
    assert_eq!(context.percent(), 62.0);
    assert_eq!(context.status(), "free");
    context.used = 168_000;
    assert_eq!(context.status(), "compacts soon");
    context.used = 192_000;
    assert_eq!(context.status(), "compacting next turn");
    assert_eq!(tokens(124_000), "124k");
    assert_eq!(tokens(850), "850");
    assert_eq!(tokens(1_200_000), "1.2M");
    assert_eq!(tokens(2_000_000), "2M");
    assert_eq!(GapMarks::of(0), GapMarks::None);
    assert_eq!(GapMarks::of(3), GapMarks::Squares(3));
    assert_eq!(GapMarks::of(5), GapMarks::Number(5));
}

#[test]
fn the_ring_starts_at_the_gap_and_runs_clockwise() {
    let ring = ring_path(40.0);
    assert_eq!(ring[0], (13.0, 37.0));
    assert_eq!(ring[5], (27.0, 37.0));
    // Total 10 + 34 + 34 + 34 + 10 = 122; half is 61: up the left side
    // and 17 along the top.
    let half = partial_path(&ring, 0.5);
    assert_eq!(half.len(), 4);
    let end = half[3];
    assert!((end.0 - 20.0).abs() < 1e-3 && end.1 == 3.0, "{end:?}");
    assert_eq!(partial_path(&ring, 0.0).len(), 1);
    assert_eq!(partial_path(&ring, 1.0).len(), 6);
}

#[test]
fn the_panel_lists_only_supplied_parts() {
    let compacted = std::rc::Rc::new(Cell::new(false));
    let seen = compacted.clone();
    let mut context = ContextView::new(124_000, 200_000, 2);
    context.model = "Sol 6.1".into();
    context.history = "Compacted twice, last at 182k".into();
    context.parts = vec![
        ContextShare {
            part: ContextPart::System,
            tokens: 14_000,
        },
        ContextShare {
            part: ContextPart::FilesRead,
            tokens: 38_000,
        },
    ];
    let mounted = mount(Size::new(900.0, 700.0), move || {
        let compact: OnPress = std::rc::Rc::new(move || seen.set(true));
        view! {
            row {
                ContextGauge context:(context) on-compact:(Some(compact))
            }
        }
    });
    let gauge = "Context 62 percent used, 124k of 200k, compacted 2 times";
    assert!(mounted.has("free"));
    assert!(!mounted.has("Context breakdown"));
    mounted.click_label(gauge);
    assert!(mounted.has("Context breakdown"));
    assert!(mounted.has("124k of 200k used · Sol 6.1"));
    assert!(mounted.has("System + tools"));
    assert!(mounted.has("Files read"));
    assert!(!mounted.has("Conversation"), "unsupplied parts are absent");
    assert!(mounted.has("Free"));
    assert!(
        mounted.has("148k"),
        "free space is the window less the parts"
    );
    assert!(mounted.has("Compacted twice, last at 182k"));
    mounted.click_label("Compact now");
    assert!(compacted.get());
}
