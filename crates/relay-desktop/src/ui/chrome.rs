//! Shared page chrome: the 56px header strip with its integrated cells, and
//! the small key/value cell used across headers and readouts.

use super::*;

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
        row #relay.strip height:{px(56.0)}px shrink:0 label:"Page header" {
            col #relay.cell width:1fr min-width:0px {
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
        }
    }
}

/// The shared command palette entry, including the platform shortcut.
#[component]
pub(crate) fn CommandPaletteButton(model: Model, compact: Derived<bool>) -> Element {
    view! {
        button #relay.header-action @click:{ model.palette.set(true); }
            width:{if compact.get() {Dimension::Px(px(56.0))} else {Dimension::MaxContent}}
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
