//! Shared page chrome: relay-ui's page header with the navigation toggle,
//! the command palette entry and the window controls' space, and the
//! workspace notice.

use super::*;
use crate::panels::{BoundedPanel, BoundedPanelProps};
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
            icon:(if model.page.get() == Page::Board { Some(relay_ui::icons::command.board) } else { None })
            height:(crate::window_chrome::NATIVE_HEADER_HEIGHT) scaled:(!cfg!(target_os = "macos"))
            title-size:(relay_ui::prelude::SerifSize::Medium)
            label:(Some("Page header".to_string())) leading:(Some(leading)) {
            children
            CommandPaletteButton model:(model) compact:(compact)
            if cfg!(target_arch = "wasm32") && model.page.get() == Page::Settings {
                row width:max-content shrink:0 {
                    CellButton label:"Sign out of Relay" rule:(relay_ui::prelude::Rule::Left)
                        on-press:(press(move || crate::platform::logout(model))) {
                        icon size:{px(16.0)}px sign-out-icon
                        if !compact.get() {
                            Label text:"Sign out" role:(TextRole::Label) weight:500
                        }
                    }
                }
            }
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
            let mut notice = model.notice.get();
            let retry = model.can_retry();
            // A plain notice that an unfinished operation row already shows
            // is not repeated; notices with actions always stay.
            if !retry
                && !model.can_rebase()
                && crate::projects::operation_shows_error(&model.snapshot.get(), &notice)
            {
                notice.clear();
            }
            if notice.is_empty() && retry {
                notice = "Resolve the previous request before submitting another action. Your draft is retained.".into();
            }
            (!notice.is_empty()).then(|| {
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
            if !retry {
                actions.push((
                    "Dismiss".into(),
                    Rc::new(move || model.notice.set(String::new())),
                ));
            }
            parent.adopt(&view! {
                NoticeStrip text:(text) actions:(actions) detail:(summary.map(UiText::from))
            });
        },
    );
    host
}

/// A deployment notice above the page; it neither takes focus nor changes layout.
#[component]
pub(crate) fn UpdateToast(
    model: Model,
    viewport_height: State<f32>,
    #[prop(default = cfg!(target_arch = "wasm32"))] browser: bool,
    #[prop(optional)] on_refresh: Option<relay_ui::prelude::OnPress>,
) -> Element {
    let update = Derived::new(move || model.available_update());
    let refresh = State::new(on_refresh.unwrap_or_else(|| Rc::new(crate::platform::refresh)));
    let width = State::new(0.0f32);
    let top = Derived::new(move || {
        px(if model.mobile_navigation.get() {
            12.0
        } else {
            64.0
        })
    });
    let body_limit = Derived::new(move || (viewport_height.get() - top.get() - px(56.0)).max(0.0));
    // The host occupies no hit area; its overflowing panel remains interactive.
    view! {
        col height:0px align:end @layout:{move |rect:Rect| width.set(rect.size.width)} {
            if update.get().is_some() {
                col width:{(width.get() - px(24.0)).max(0.0).min(px(480.0))}px height:min-content
                    min-width:0px shrink:0 gap:0px translate:(x:{-px(12.0)}px y:{top.get()}px)
                    fill:ground.panel-raised label:"Update notification"
                    stroke:(width:{px(1.0)} color:ground.rule-strong offset:{px(-1.0)}) {
                    BoundedPanel limit:(body_limit) {
                        col height:min-content min-width:0px gap:{px(6.0)}px pad:{px(12.0)}px {
                            Label text:"New Version Available" role:(TextRole::Body) weight:500
                            Label
                                text:(if browser { "Refresh Relay to load the update." } else { "Update and restart Relay to use the new version." })
                                role:(TextRole::Label) tone:(Tone::Muted)
                        }
                    }
                    row height:{px(44.0)}px shrink:0 gap:0px
                        stroke:(width:{px(1.0)} color:ground.rule edges:top) {
                        if browser {
                            row width:1fr min-width:0px {
                                CellButton label:"Refresh Relay"
                                    on-press:(refresh.get_untracked()) {
                                    Label text:"Refresh" role:(TextRole::Label)
                                }
                            }
                        }
                        row width:1fr min-width:0px
                            stroke:(width:{if browser {px(1.0)} else {0.0}} color:ground.rule edges:left) {
                            CellButton label:"Dismiss update notification"
                                on-press:(press(move || model.dismissed_update.set(update.get_untracked()))) {
                                Label text:"Dismiss" role:(TextRole::Label)
                            }
                        }
                    }
                }
            }
        }
    }
}
