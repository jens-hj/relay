//! Shared page chrome: the 56px header strip with its integrated cells, and
//! the small key/value cell used across headers and readouts.

use super::*;
use crate::window_chrome::{WindowControls, WindowControlsProps};

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
    view! {
        row #relay.strip height:{crate::window_chrome::sidebar_header_height()}px shrink:0
            label:"Page header"
            @pointer:{move |event, ctx| crate::window_chrome::drag_header(model, event, ctx)} {
            NavigationButton model:(model)
            col #relay.cell width:1fr min-width:0px
                pad:(horizontal:{px(if compact.get() {8.0} else {14.0})}px vertical:0px) {
                row #relay.eyebrow height:min-content {
                    text text-transform:{TextTransform::Uppercase} letter-spacing:{px(0.6)}px
                        {eyebrow.get()}
                }
                row #relay.crumb height:min-content clip {
                    text text-wrap:none {title.get()}
                }
            }
            children
            CommandPaletteButton model:(model) compact:(compact)
            if model.page.get() != Page::Board || model.issue.get().is_none() || model.snapshot.get().projects.is_empty() {
                WindowControls model:(model)
            }
        }
    }
}

#[component]
pub(crate) fn NavigationButton(model: Model) -> Element {
    view! {
        row width:{if model.mobile_navigation.get() {44.0} else {0.0}}px shrink:0 {
            if model.mobile_navigation.get() {
                button #relay.header-action @click:{model.sidebar_open.set(true);} width:44px
                    pad:0px label:"Open navigation" {
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
        button #relay.header-action @click:{ model.palette.set(true); }
            width:{if compact.get() {Dimension::Px(px(36.0))} else {Dimension::MaxContent}}
            pad:(horizontal:{px(if compact.get() {0.0} else {16.0})}px vertical:0px)
            gap:{px(10.0)}px label:"Open command palette" {
            if compact.get() {
                icon size:{px(16.0)}px command-icon
            }
            if !compact.get() {
                text "Commands"
                row #relay.caption height:min-content width:max-content {
                    text {String::from(if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" })}
                }
            }
        }
    }
}

/// A header cell: a caps key above a value, with optional inline controls.
#[component]
pub(crate) fn HeaderCell(
    key: String,
    value: Derived<String>,
    #[prop(optional)] children: Children,
) -> Element {
    view! {
        col #relay.cell width:max-content max-width:{px(320.0)}px min-width:0px
            label:(key.clone()) {
            row #relay.eyebrow height:min-content {
                text text-transform:uppercase letter-spacing:{px(0.6)}px (key.clone())
            }
            row height:min-content gap:{px(8.0)}px align:center {
                row #relay.value height:min-content clip {
                    text text-wrap:none {value.get()}
                }
                children
            }
        }
    }
}

/// A framed form module: a 30px caps head, then padded content.
#[component]
pub(crate) fn Module(
    title: String,
    #[prop(default = false)] flush: bool,
    #[prop(optional)] children: Children,
) -> Element {
    view! {
        col #relay.module max-width:{px(760.0)}px {
            row #relay.module-head {
                row #relay.eyebrow height:min-content width:max-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px (title.clone())
                }
            }
            col height:min-content gap:{px(if flush {0.0} else {10.0})}px
                pad:{px(if flush {0.0} else {12.0})}px {
                children
            }
        }
    }
}

/// The workspace notice: the message, the original request when it can be
/// retried, and its actions in cells that wrap in narrow panes.
#[component]
pub(crate) fn Notice(model: Model) -> Element {
    view! {
        col height:min-content shrink:0 {
            if !model.notice.get().is_empty() {
                col height:min-content gap:{px(8.0)}px pad:{px(12.0)}px shrink:0 label:"Notice"
                    fill:ground.panel-raised
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                    stroke:+(width:{px(2.0)} color:status.needs_you edges:left) {
                    text font-size:{px(12.0)}px font-color:ground.ink { model.notice.get() }
                    if model.can_retry() {
                        text font-size:{px(11.0)}px font-color:ground.muted
                            { model.retry_summary() }
                    }
                    grid height:min-content gap:{px(6.0)}px
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(150.0).into(), GridTrack::fr(1.0)))} {
                        if model.can_rebase() {
                            button #relay.action @click:{ model.review_latest(); } width:fill
                                disabled:{ model.busy.get() } "Review latest state"
                        }
                        if model.can_rebase() {
                            button #relay.action @click:{ model.rebase_conflict(); } width:fill
                                label:"Review conflict for new request"
                                "Review conflict / new request"
                        }
                        if model.can_retry() {
                            button #relay.action @click:{ model.retry_pending(); } width:fill
                                label:"Retry original request" "Retry original request"
                        }
                        button #relay.action @click:{ model.notice.set(String::new()); } width:fill
                            "Dismiss"
                    }
                }
            }
        }
    }
}
