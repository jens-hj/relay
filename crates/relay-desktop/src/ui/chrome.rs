//! Shared page chrome: relay-ui's page header with the navigation toggle,
//! the command palette entry and the window controls' space, and the
//! workspace notice.

use super::*;
use crate::window_chrome::{WindowControlsSpace, WindowControlsSpaceProps};
use relay_ui::prelude::{CellButton, CellButtonProps, Label, LabelProps, TextRole, Tone, UiText};
use relay_ui::surfaces::{
    Notice as NoticeStrip, NoticeAction, NoticeProps as NoticeStripProps, PageHeader as PageStrip,
    PageHeaderProps as PageStripProps,
};
use std::rc::Rc;

/// The page header: an eyebrow naming the page above the subject, page cells
/// supplied by the caller, and the command palette entry. `compact` collapses
/// the palette entry to its icon.
#[component]
pub(crate) fn PageHeader(
    model: Model,
    eyebrow: Derived<String>,
    title: Derived<String>,
    compact: Derived<bool>,
    #[prop(optional)] children: Children,
) -> Element {
    let leading = view! {
        NavigationButton model:(model)
    };
    let header = view! {
        PageStrip title:(UiText::from(title)) eyebrow:(Some(UiText::from(eyebrow)))
            height:(crate::window_chrome::NATIVE_HEADER_HEIGHT) scaled:(!cfg!(target_os = "macos"))
            title-size:(relay_ui::prelude::SerifSize::Medium)
            label:(Some("Page header".to_string())) leading:(Some(leading)) {
            children
            CommandPaletteButton model:(model) compact:(compact)
            if model.page.get() != Page::Board || model.issue.get().is_none() || model.snapshot.get().projects.is_empty() {
                WindowControlsSpace
            }
        }
    };
    header.on_pointer(move |event, ctx| crate::window_chrome::drag_header(model, event, ctx));
    header
}

#[component]
pub(crate) fn NavigationButton(model: Model) -> Element {
    view! {
        row width:{if model.mobile_navigation.get() {44.0} else {0.0}}px shrink:0 {
            if model.mobile_navigation.get() {
                CellButton label:"Open navigation"
                    on-press:(press(move || model.sidebar_open.set(true)))
                    rule:(relay_ui::prelude::Rule::Right) horizontal-inset:(0.0) {
                    icon size:18px navigation-icon
                }
            }
        }
    }
}

/// The shared command palette entry, including the platform shortcut.
#[component]
pub(crate) fn CommandPaletteButton(model: Model, compact: Derived<bool>) -> Element {
    view! {
        row width:{if compact.get() {Dimension::Px(px(36.0))} else {Dimension::MaxContent}}
            shrink:0 {
            CellButton label:"Open command palette"
                on-press:(press(move || model.palette.set(true)))
                rule:(relay_ui::prelude::Rule::Left) horizontal-inset:(0.0) {
                if compact.get() {
                    icon size:{px(16.0)}px command-icon
                }
                if !compact.get() {
                    row width:max-content height:min-content gap:{px(10.0)}px
                        pad:(horizontal:{px(16.0)}px vertical:0px) {
                        Label text:"Commands" role:(TextRole::Label) weight:500
                        Label
                            text:(String::from(if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" }))
                            role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
            }
        }
    }
}

/// The workspace notice: the message, the original request when it can be
/// retried, and its actions in cells that wrap in narrow panes.
#[component]
pub(crate) fn Notice(model: Model) -> Element {
    let host = view! {
        col height:min-content shrink:0 {}
    };
    host.switch(
        move || {
            let notice = model.notice.get();
            (!notice.is_empty()).then(|| {
                let retry = model.can_retry();
                (
                    notice,
                    model.can_rebase(),
                    retry,
                    retry.then(|| model.retry_summary()),
                )
            })
        },
        move |parent, shown| {
            let Some((text, rebase, retry, summary)) = shown.clone() else {
                return;
            };
            let mut actions: Vec<NoticeAction> = Vec::new();
            if rebase {
                actions.push((
                    "Review latest state".into(),
                    Rc::new(move || {
                        if !model.busy.get_untracked() {
                            model.review_latest();
                        }
                    }),
                ));
                actions.push((
                    "Review conflict for new request".into(),
                    Rc::new(move || model.rebase_conflict()),
                ));
            }
            if retry {
                actions.push((
                    "Retry original request".into(),
                    Rc::new(move || model.retry_pending()),
                ));
            }
            actions.push((
                "Dismiss".into(),
                Rc::new(move || model.notice.set(String::new())),
            ));
            parent.adopt(&view! {
                NoticeStrip text:(text) actions:(actions) detail:(summary.map(UiText::from))
            });
        },
    );
    host
}
