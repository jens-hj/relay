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
            col width:max-content justify:center pad:(horizontal:{px(12.0)}px vertical:0px) {
                if compact.get() {
                    button #relay.action @click:{ model.palette.set(true); } width:{px(34.0)}px
                        pad:0px justify:center label:"Open command palette" {
                        icon size:{px(15.0)}px command-icon
                    }
                } else {
                    button #relay.action @click:{ model.palette.set(true); }
                        label:"Open command palette" gap:{px(10.0)}px {
                        text "Commands"
                        row #relay.caption height:min-content width:max-content {
                            text
                                {String::from(if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" })}
                        }
                    }
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
pub(crate) fn Module(title: String, #[prop(optional)] children: Children) -> Element {
    view! {
        col #relay.module max-width:{px(760.0)}px {
            row #relay.module-head {
                row #relay.eyebrow height:min-content width:max-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px (title.clone())
                }
            }
            col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                children
            }
        }
    }
}
