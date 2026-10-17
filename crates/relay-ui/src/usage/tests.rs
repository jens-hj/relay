use super::*;
use crate::testing::mount;
use mosaic::prelude::*;
use std::cell::RefCell;

const HOUR: Duration = Duration::from_secs(3600);

#[test]
fn usage_foot_has_a_full_width_internal_hairline_above_its_heading() {
    let mounted = mount(Size::new(380.0, 300.0), || {
        view! {
            col justify:end {
                UsageFoot harnesses:(vec![claude(UsageInsight::default())])
            }
        }
    });
    let foot = mounted.rect("Usage foot");
    let rule = mounted.rect("Usage boundary");
    assert_eq!(rule.origin, foot.origin);
    assert_eq!(rule.size, Size::new(foot.size.width, 1.0));
    assert!(mounted.rect("Usage").origin.y > rule.origin.y);
    let id = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some("Usage boundary"))
        .unwrap()
        .id;
    let details = mounted.ui.inspection_details(id).unwrap();
    assert_eq!(
        details
            .attributes
            .iter()
            .find(|a| a.name == "fill")
            .unwrap()
            .value,
        format!(
            "{:?}",
            mosaic::render::PaintSpec::from(crate::tokens::ground.rule)
        )
    );
}

fn claude(insight: UsageInsight) -> HarnessUsage {
    HarnessUsage {
        harness: Harness::ClaudeCode,
        session: Some(
            UsageWindow::new(62.0, 5 * HOUR)
                .elapsed(2 * HOUR)
                .resets("resets in 3h 00m"),
        ),
        week: Some(UsageWindow::new(48.0, 7 * 24 * HOUR)),
        insight,
    }
}

/// The opaque colours painted inside the bar labelled `label`.
fn bar_colours(mounted: &crate::testing::Mounted, label: &str) -> Vec<(f32, f32, f32)> {
    let bar = mounted.rect(label);
    mounted
        .ui
        .scene()
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            mosaic::render::PaintCmd::Shape(shape)
                if bar.contains(shape.rect.origin)
                    && bar.contains(
                        shape.rect.origin
                            + Vector2::new(shape.rect.size.width, shape.rect.size.height),
                    ) =>
            {
                shape.fill.as_solid()
            }
            _ => None,
        })
        .filter(|fill| fill.a > 0.0)
        .map(|fill| (fill.r, fill.g, fill.b))
        .collect()
}

#[test]
fn each_harness_fills_its_bars_with_its_colour_until_a_threshold() {
    let codex = HarnessUsage {
        harness: Harness::Codex,
        session: Some(UsageWindow::new(35.0, 5 * HOUR)),
        week: Some(UsageWindow::new(86.0, 7 * 24 * HOUR)),
        insight: UsageInsight::default(),
    };
    let mounted = mount(Size::new(380.0, 300.0), move || {
        view! {
            col justify:end {
                UsageFoot harnesses:(vec![claude(UsageInsight::default()), codex])
            }
        }
    });
    for _ in 0..12 {
        mounted.ui.tick(Duration::from_millis(40));
        mounted.settle();
    }
    let rgb = |token: ColorToken| {
        let c = mosaic::core::theme::color(token);
        (c.r, c.g, c.b)
    };
    let ink = rgb(crate::tokens::ground.ink);
    for (label, tint) in [
        (
            "62 percent used, 40 percent of the window elapsed",
            Harness::ClaudeCode.tint(),
        ),
        ("48 percent used", Harness::ClaudeCode.tint()),
        ("35 percent used", Harness::Codex.tint()),
        ("86 percent used", crate::tokens::Signal::NeedsYou.token()),
    ] {
        let colours = bar_colours(&mounted, label);
        let count = colours.iter().filter(|c| **c == rgb(tint)).count();
        assert!(count > 1, "{label}: cap and particles in its tint");
        assert!(!colours.contains(&ink), "{label}: no ink particles");
    }
    assert_ne!(rgb(Harness::ClaudeCode.tint()), rgb(Harness::Codex.tint()));
}

#[test]
fn windows_report_elapsed_and_rate_units() {
    let session = UsageWindow::new(62.0, 5 * HOUR).elapsed(2 * HOUR);
    assert_eq!(session.elapsed_fraction(), Some(0.4));
    assert_eq!(session.rate_unit().1, "%/h");
    assert_eq!(UsageWindow::new(1.0, 7 * 24 * HOUR).rate_unit().1, "%/day");
    assert!(UsageWindow::new(1.0, HOUR).forecast().is_none());
    assert_eq!(
        claude(UsageInsight::default()).summary(),
        "Claude Code: session 62 percent, week 48 percent"
    );
}

#[test]
fn rows_open_into_a_detail_with_only_supplied_parts() {
    let applied = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = applied.clone();
    let insight = UsageInsight {
        heaviest: vec![("API migration".into(), 41.0)],
        suggestion: Some(Suggestion {
            subject: "Flaky tests".into(),
            text: "Sonnet lands the session".into(),
        }),
        ..Default::default()
    };
    let mounted = mount(Size::new(400.0, 900.0), move || {
        let apply: OnApply = std::rc::Rc::new(move |harness| seen.borrow_mut().push(harness));
        view! {
            col width:{380.0}px {
                UsageFoot harnesses:(vec![claude(insight)]) on-apply:(Some(apply))
            }
        }
    });
    let row = "Claude Code: session 62 percent, week 48 percent. Show details";
    assert!(mounted.has("1 harness"));
    assert!(mounted.has("62 percent used, 40 percent of the window elapsed"));
    assert!(
        mounted.has("48 percent used"),
        "no tick without elapsed time"
    );
    assert!(!mounted.has("Apply"));
    mounted.click_label(row);
    assert!(mounted.has("Claude Code: session 62 percent, week 48 percent. Hide details"));
    assert!(mounted.has("62 percent used, projected 155 percent by reset"));
    assert!(mounted.has("Runs out early"));
    assert!(mounted.has("31%/h"));
    assert!(mounted.has("empty in 1h 14m · 1h 46m before reset"));
    assert!(mounted.has("API migration"));
    assert!(!mounted.has("Model mix this week"), "no mix supplied");
    assert!(
        !mounted.has("On pace"),
        "no week forecast without elapsed time"
    );
    mounted.click_label("Apply");
    assert_eq!(applied.borrow().as_slice(), [Harness::ClaudeCode]);
    let bar = mounted.rect("62 percent used, projected 155 percent by reset");
    assert_eq!(bar.size.height, 16.0);
    mounted.click_label("Claude Code: session 62 percent, week 48 percent. Hide details");
    assert!(!mounted.has("Runs out early"));
}
