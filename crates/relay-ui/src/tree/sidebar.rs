//! The agents sidebar and its sections: status counts, Needs you, the
//! focus breadcrumb, and the tree itself.

use super::logic::{
    Approval, Flatten, StatusTally, TreeLine, approvals, find_branch, flatten, levels_below,
};
use super::row::TreeNav;
use super::{AgentNode, OnAgent, OnFilter, OnFocus, OnToggle};
use crate::marks::{mark, sized_text};
use crate::overlay::{Tooltip, TooltipProps};
use crate::prelude::*;
use crate::tokens::{Signal, ground, px, shape};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

crate::value::live_value!(StatusTally);
crate::value::live_value!(Vec<AgentNode>);
crate::value::live_value!(Vec<Approval>);
crate::value::live_value!(Option<AgentStatus>);
crate::value::live_value!(Option<String>);

/// The five status counts as filter cells. The waiting cell turns amber
/// while anything waits; the active filter cell is highlighted, and
/// pressing it again clears the filter.
#[component]
pub fn StatusCounts(
    tally: Live<StatusTally>,
    /// The active filter.
    #[prop(default = Live::fixed(None))]
    active: Live<Option<AgentStatus>>,
    /// Called with the new filter.
    #[prop(optional)]
    on_filter: Option<OnFilter>,
) -> Element {
    let grid = view! {
        grid cols:(repeat(5 1fr)) height:min-content shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:(top bottom)) {}
    };
    for status in AgentStatus::ACTIVITY {
        let (count, label, selected) = (tally.clone(), tally.clone(), active.clone());
        let amber = {
            let tally = tally.clone();
            move || status == AgentStatus::Waiting && tally.get().get(status) > 0
        };
        let tone = {
            let amber = amber.clone();
            move || {
                if amber() {
                    Signal::NeedsYou.token()
                } else {
                    ground.ink
                }
            }
        };
        let word_amber = amber.clone();
        let current = active.clone();
        let notify = on_filter.clone();
        let press = move || {
            let next = if current.get_untracked() == Some(status) {
                None
            } else {
                Some(status)
            };
            if let Some(notify) = &notify {
                notify(next);
            }
        };
        let cell = view! {
            button @click:{press()} width:fill height:min-content min-width:0px radius:0px
                pad:(left:{px(14.0)}px top:{px(10.0)}px bottom:{px(10.0)}px right:{px(8.0)}px)
                label:{format!("{} {}", label.get().get(status), status.word())}
                fill:{if selected.get() == Some(status) { color(ground.selected) } else { Color::TRANSPARENT }}
                hover { fill:ground.hover } pressed { fill:ground.pressed }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                col min-width:0px height:min-content gap:{px(4.0)}px {
                    row width:max-content height:min-content gap:{px(6.0)}px align:center {
                        StatusSquare status:(status)
                        text font-family:monospace font-size:{px(15.0)}px font-weight:500
                            font-color:{color(tone())} {count.get().get(status).to_string()}
                    }
                    // Narrow sidebars (the phone drawer) trim the word
                    // rather than run it into the next cell.
                    // Ellipsis takes the row's free width (1fr), so it
                    // sits in a full-width row rather than the column.
                    row min-width:0px height:min-content {
                        if word_amber() {
                            Ellipsis text:(status.word()) role:(TextRole::Caption)
                                tone:(Tone::Signal(Signal::NeedsYou))
                        } else {
                            Ellipsis text:(status.word()) role:(TextRole::Caption)
                                tone:(Tone::Muted)
                        }
                    }
                }
            }
        };
        grid.adopt(&cell);
    }
    grid
}

/// One approval request: amber cut-corner card with an amber wash, the
/// waiting agent, its model, its path, and Approve / Review diff.
#[component]
pub fn NeedsYouCard(
    approval: Approval,
    #[prop(optional)] on_approve: Option<OnAgent>,
    #[prop(optional)] on_review: Option<OnAgent>,
) -> Element {
    let approve: OnPress = {
        let id = approval.id.clone();
        Rc::new(move || {
            if let Some(approve) = &on_approve {
                approve(id.clone());
            }
        })
    };
    let review: OnPress = {
        let id = approval.id.clone();
        Rc::new(move || {
            if let Some(review) = &on_review {
                review(id.clone());
            }
        })
    };
    let model = approval.model.as_ref().map(|model| model.short_name());
    let path = approval.path.clone();
    view! {
        CutCorner cut:(Cut::Card) edge:(Signal::NeedsYou.token()) edge-opacity:0.32 {
            stack height:min-content {
                Wash signal:(Signal::NeedsYou) opacity:0.3
                col height:min-content gap:{px(8.0)}px pad:{px(15.0)}px {
                    row height:min-content gap:{px(8.0)}px align:center {
                        StatusSquare status:(AgentStatus::Waiting)
                        HarnessGlyph harness:(approval.harness)
                        Ellipsis text:(approval.name.clone()) role:(TextRole::BodyLarge)
                            weight:(Weight::SEMIBOLD)
                        {
                            if let Some(model) = &model {
                                parent.adopt(&sized_text(model.clone().into(), 11.0, 400, ground.muted));
                            }
                        }
                    }
                    {
                        if !path.is_empty() {
                            parent.adopt(&view! {
                                PathText segments:(path.clone())
                            });
                        }
                    }
                    row height:min-content gap:{px(8.0)}px align:center {
                        PrimaryButton text:"Approve" on-press:(approve)
                        SecondaryButton text:"Review diff" on-press:(review)
                    }
                }
            }
        }
    }
}

/// The Needs you section: serif italic amber title, count, and a card per
/// waiting agent. Takes no space while nothing waits.
#[component]
pub fn NeedsYou(
    approvals: Live<Vec<Approval>>,
    #[prop(optional)] on_approve: Option<OnAgent>,
    #[prop(optional)] on_review: Option<OnAgent>,
) -> Element {
    let host = view! {
        col height:min-content shrink:0 {}
    };
    let list = approvals.clone();
    host.switch(
        move || list.get(),
        move |parent, approvals| {
            if approvals.is_empty() {
                return;
            }
            let count = approvals.len().to_string();
            parent.adopt(&view! {
                row height:min-content gap:{px(8.0)}px align:end
                    pad:(left:{px(18.0)}px right:{px(18.0)}px top:{px(16.0)}px bottom:{px(8.0)}px) {
                    SerifTitle text:"Needs you" size:(SerifSize::Small) italic:true
                        tone:(Tone::Signal(Signal::NeedsYou))
                    col width:max-content height:min-content pad:(bottom:{px(3.0)}px) {
                        {
                            parent.adopt(&sized_text(count.clone().into(), 11.0, 400, ground.muted));
                        }
                    }
                }
            });
            for approval in approvals {
                let (approve, review) = (on_approve.clone(), on_review.clone());
                parent.adopt(&view! {
                    col height:min-content
                        pad:(left:{px(12.0)}px right:{px(12.0)}px bottom:{px(8.0)}px) {
                        NeedsYouCard approval:(approval.clone()) on-approve:(approve)
                            on-review:(review)
                    }
                });
            }
        },
    );
    host
}

/// The breadcrumb of a focused branch: a back-to-root cell, the ancestors'
/// path, the branch name in the serif, and a chip with its true depth.
#[component]
pub fn FocusCrumb(
    /// Ancestor names, root first.
    path: Vec<String>,
    name: String,
    depth: usize,
    on_back: OnPress,
) -> Element {
    view! {
        row height:min-content shrink:0 gap:{px(12.0)}px align:center
            pad:(left:{px(12.0)}px right:{px(12.0)}px top:{px(12.0)}px bottom:{px(14.0)}px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
            IconCell label:"Back to root" on-press:(on_back) {
                {
                    parent.adopt(&mark(Mark::ArrowUp, 12.0, ground.ink));
                }
            }
            col width:1fr min-width:0px height:min-content gap:{px(4.0)}px {
                PathText segments:(path)
                SerifTitle text:(name) size:(SerifSize::Medium)
            }
            Chip text:(format!("depth {depth}"))
        }
    }
}

/// The agents sidebar: serif head with the count and a New agent cell, a
/// focus breadcrumb while a branch is focused, the status counts (each a
/// filter), Needs you, and the tree. Children are placed at its foot
/// (usage, connection).
///
/// Expansion, focus and the filter work on their own and are reported
/// through the callbacks; selection comes from [`AgentNode::selected`].
#[component]
pub fn AgentSidebar(
    roots: Live<Vec<AgentNode>>,
    /// Width in logical pixels before UI scale.
    #[prop(default = 380.0)]
    width: f32,
    /// Displayed depth at which branches fold; the `fold-depth` token by
    /// default.
    #[prop(optional)]
    fold_depth: Option<usize>,
    /// Give roots one outer elbow column when embedded under a parent row.
    #[prop(default = false)]
    root_indent: bool,
    /// The focused branch, initially.
    #[prop(default = Live::fixed(None))]
    focus: Live<Option<String>>,
    #[prop(optional)] on_new: Option<OnPress>,
    #[prop(optional)] on_spawn: Option<OnAgent>,
    #[prop(optional)] on_open: Option<OnAgent>,
    #[prop(optional)] on_toggle: Option<OnToggle>,
    #[prop(optional)] on_focus: Option<OnFocus>,
    #[prop(optional)] on_filter: Option<OnFilter>,
    #[prop(optional)] on_approve: Option<OnAgent>,
    #[prop(optional)] on_review: Option<OnAgent>,
    /// The sidebar foot.
    #[prop(optional)]
    children: Children,
) -> Element {
    let focused = State::new(focus.get_untracked());
    {
        let focus = focus.clone();
        Effect::new(move || focused.set(focus.get()));
    }
    let filter = State::new(None::<AgentStatus>);
    let toggled = State::new(HashMap::<String, bool>::new());

    let all = roots.clone();
    let scoped = Derived::new(move || {
        let roots = all.get();
        let toggled = toggled.get();
        let roots: Vec<AgentNode> = roots.iter().map(|root| apply(root, &toggled)).collect();
        match focused.get() {
            Some(id) => match find_branch(&roots, &id) {
                Some(branch) => vec![branch.node.clone()],
                None => roots,
            },
            None => roots,
        }
    });
    let all = roots.clone();
    let branch = Derived::new(move || {
        let id = focused.get()?;
        let roots = all.get();
        let branch = find_branch(&roots, &id)?;
        Some((
            branch.path.clone(),
            branch.node.name.clone(),
            branch.depth,
            branch.depth + levels_below(branch.node),
        ))
    });
    let total = {
        let roots = roots.clone();
        Derived::new(move || StatusTally::of_forest(&roots.get()).total())
    };
    let tally = Derived::new(move || StatusTally::of_forest(&scoped.get()));
    let waiting = Derived::new(move || approvals(&scoped.get()));
    let fold = move || {
        fold_depth.unwrap_or_else(|| mosaic::core::theme::scalar(shape.fold_depth).round() as usize)
    };
    let lines = Derived::new(move || {
        let mut lines = flatten(
            &scoped.get(),
            Flatten {
                fold_depth: fold(),
                filter: filter.get(),
            },
        );
        if root_indent && focused.get().is_none() {
            let mut later_root = false;
            for line in lines.iter_mut().rev() {
                let guides = match line {
                    TreeLine::Agent(agent) => &mut agent.guides,
                    TreeLine::Folded(folded) => &mut folded.guides,
                };
                let root = guides.is_empty();
                let guide = match (root, later_root) {
                    (true, true) => super::Guide::Branch,
                    (true, false) => super::Guide::Last,
                    (false, true) => super::Guide::Pass,
                    (false, false) => super::Guide::Blank,
                };
                guides.insert(0, guide);
                later_root |= root;
            }
        }
        lines
    });

    let set_filter: OnFilter = {
        let notify = on_filter.clone();
        Rc::new(move |next| {
            filter.set(next);
            if let Some(notify) = &notify {
                notify(next);
            }
        })
    };
    let set_focus: OnFocus = {
        let notify = on_focus.clone();
        Rc::new(move |next: Option<String>| {
            focused.set(next.clone());
            if let Some(notify) = &notify {
                notify(next);
            }
        })
    };
    let toggle: OnToggle = {
        let notify = on_toggle.clone();
        Rc::new(move |id: String, open: bool| {
            toggled.update(|map| {
                map.insert(id.clone(), open);
            });
            if let Some(notify) = &notify {
                notify(id, open);
            }
        })
    };
    let new_agent: OnPress = on_new.unwrap_or_else(|| Rc::new(|| {}));
    let action_cell = {
        view! {
            Tooltip text:"New Director" width:(Dimension::Fill) height:(Dimension::Fill) {
                CellButton label:"New agent" rule:(Rule::Left) horizontal-inset:0.0
                    on-press:(new_agent) {
                    { parent.adopt(&mark(Mark::Plus, 12.0, ground.ink)); }
                }
            }
        }
    };

    let crumb_host = view! {
        col height:min-content shrink:0 {}
    };
    {
        let set_focus = set_focus.clone();
        crumb_host.switch(
            move || branch.get(),
            move |parent, branch| {
                if let Some((path, name, depth, _)) = branch {
                    let back = set_focus.clone();
                    let back: OnPress = Rc::new(move || back(None));
                    parent.adopt(&view! {
                        FocusCrumb path:(path.clone()) name:(name.clone()) depth:(*depth)
                            on-back:(back)
                    });
                }
            },
        );
    }
    let section = view! {
        row height:min-content shrink:0 gap:{px(8.0)}px align:end
            pad:(left:{px(18.0)}px right:{px(18.0)}px top:{px(20.0)}px bottom:{px(8.0)}px) {}
    };
    section.switch(
        move || branch.get().map(|(_, _, from, to)| (from, to)),
        move |parent, range| match range {
            Some((from, to)) => {
                let range = format!("depth {from} → {to}");
                parent.adopt(&view! {
                    text font-family:monospace font-size:{px(11.0)}px font-weight:500
                        font-color:ground.muted shrink:0 "This branch"
                });
                parent.adopt(&view! {
                    el width:1fr height:{px(1.0)}px {}
                });
                parent.adopt(&sized_text(range.into(), 11.0, 400, ground.muted));
            }
            None => {
                parent.adopt(&view! {
                    SerifTitle text:"All agents" size:(SerifSize::Small) italic:true
                });
            }
        },
    );

    let tree = view! {
        col height:min-content pad:(top:{px(2.0)}px bottom:{px(14.0)}px) {}
    };
    tree.role(mosaic::widgets::Role::List);
    tree.label("Agent tree");
    let nav = TreeNav::new();
    {
        let nav = nav.clone();
        Effect::new(move || nav.set_lines(lines.get()));
    }
    {
        let (open, toggle, nav, spawn) = (
            on_open.clone(),
            toggle.clone(),
            nav.clone(),
            on_spawn.clone(),
        );
        let focus_branch = set_focus.clone();
        tree.keyed(
            move || lines.get().into_iter().map(|line| (line, ())),
            move |parent, line, _| {
                let built = match line {
                    TreeLine::Agent(agent) => {
                        let (open, toggle, nav) = (open.clone(), toggle.clone(), nav.clone());
                        view! {
                            AgentTreeRow line:(agent.clone()) on-open:(open)
                                on-spawn:(spawn.clone()) on-toggle:(Some(toggle)) nav:(Some(nav))
                        }
                    }
                    TreeLine::Folded(folded) => {
                        let focus = focus_branch.clone();
                        let focus: OnAgent = Rc::new(move |id| focus(Some(id)));
                        view! {
                            FoldedBranch line:(folded.clone()) on-focus:(Some(focus))
                        }
                    }
                };
                parent.adopt(&built);
            },
        );
    }

    let filter_value: Live<Option<AgentStatus>> = filter.into();
    let tally: Live<StatusTally> = tally.into();
    let waiting: Live<Vec<Approval>> = waiting.into();
    view! {
        col width:{px(width)}px shrink:0 fill:ground.panel font-color:ground.ink label:"Agents"
            stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
            row height:{px(60.0)}px shrink:0 {
                row width:1fr gap:{px(10.0)}px align:center
                    pad:(horizontal:{px(18.0)}px vertical:0px) {
                    SerifTitle text:"Agents" size:(SerifSize::Large)
                    col width:max-content height:min-content pad:(top:{px(6.0)}px) {
                        text font-family:monospace font-size:{px(11.0)}px font-color:ground.muted
                            {total.get().to_string()}
                    }
                }
                row width:{px(60.0)}px shrink:0 {
                    { parent.adopt(&action_cell); }
                }
            }
            {
                parent.adopt(&crumb_host);
            }
            StatusCounts tally:(tally) active:(filter_value) on-filter:(Some(set_filter))
            NeedsYou approvals:(waiting) on-approve:(on_approve) on-review:(on_review)
            {
                parent.adopt(&section);
            }
            col height:1fr min-height:0px {
                scroll {
                    {
                        parent.adopt(&tree);
                    }
                }
            }
            col height:min-content shrink:0 {
                children
            }
        }
    }
}

/// `node` with the sidebar's own expand/collapse choices applied.
fn apply(node: &AgentNode, toggled: &HashMap<String, bool>) -> AgentNode {
    let mut node = node.clone();
    if let Some(&open) = toggled.get(&node.id) {
        node.expanded = open;
    }
    node.children = node
        .children
        .iter()
        .map(|child| apply(child, toggled))
        .collect();
    node
}
