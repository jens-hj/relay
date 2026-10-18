//! Ruled composer with replaceable editor and application-owned settings.
mod model;
use crate::prelude::*;
use crate::tokens::{ground, px};
pub use model::*;
use mosaic::prelude::*;
use mosaic::render::{Shape, Stroke};
use std::rc::Rc;

#[derive(Clone)]
pub struct LedgerActions {
    pub change_harness: OnPress,
    pub change_model: OnPress,
    pub set_effort: OnEffort,
    pub toggle: OnPress,
}
// The corner radius is relative to cell width, including in the narrow strip.
// Keep identity visible throughout the same 3.6-second breathing cycle.
pub(crate) const MODEL_GLOW_OPEN_REACH: f32 = 0.85;
pub(crate) const MODEL_GLOW_COLLAPSED_REACH: f32 = 2.3;
pub(crate) const MODEL_GLOW_OPACITY: f32 = 0.8;
/// The idle key hint's opacity; progress hints show at full strength.
pub(crate) const HINT_IDLE_OPACITY: f32 = 0.8;
fn quiet_send(fill: bool) -> Color {
    let dark = mosaic::core::theme::color(ground.page).to_srgb8()[0] < 128;
    if dark {
        Color::from_rgb_hex(if fill { 0x222225 } else { 0xB9B7B1 })
    } else {
        mosaic::core::theme::color(if fill { ground.selected } else { ground.muted })
    }
}
#[component]
pub fn Ledger(
    data: Live<LedgerView>,
    actions: LedgerActions,
    #[prop(default = false)] framed: bool,
) -> Element {
    let width_data = data.clone();
    let animation_clock = Rc::new(std::cell::Cell::new(0.0f64));
    let edges = if framed {
        StrokeEdges::ALL
    } else {
        StrokeEdges::RIGHT
    };
    let host = view! {
        stack width:{px(if width_data.get().collapsed { 66.0 } else { 212.0 })}px shrink:0
            label:"Agent ledger" {}
    };
    host.switch(
        move || data.get(),
        move |parent, vm| {
            let content = ledger(vm.clone(), actions.clone(), animation_clock.clone(), framed);
            if framed {
                content.restyle_dyn(|style| style.size(Dimension::Fill, Dimension::MinContent));
            }
            parent.adopt(&content);
            parent.adopt(&view! {
                el width:fill height:fill nohit label:"Ledger border"
                    stroke:(width:{px(1.0)} color:ground.rule edges:edges offset:{px(if framed {-0.5} else {0.0})}) {}
            });
        },
    );
    host
}
fn ledger(
    vm: LedgerView,
    actions: LedgerActions,
    animation_clock: Rc<std::cell::Cell<f64>>,
    framed: bool,
) -> Element {
    let colour = vm.model.hue.token();
    let harness_label = if vm.harness == Harness::ClaudeCode {
        "CLAUDE".to_string()
    } else {
        vm.harness.name().to_uppercase()
    };
    let mut e = vm.effort;
    if let Some(step) = vm.pending_effort {
        e.current = step;
    }
    let effort_colour = if vm.pending_effort.is_some() {
        ground.muted
    } else {
        colour
    };
    let track = e.levels > 0 || e.has_max();
    let named = (!track).then(|| vm.effort_label.clone()).flatten();
    let tall = !track && named.is_none();
    let named_cell = named.map(|name| {
        view! {
            col height:min-content shrink:0 justify:start
                pad:(horizontal:{px(10.0)}px vertical:{px(9.0)}px) {
                CutCorner cut:(Cut::Small) corner:(Corner::TopRightBottomLeft)
                    height:(Dimension::MinContent) shrink:0.0 edge:(ground.rule) {
                    row height:{px(20.0)}px justify:between align:center
                        pad:(horizontal:{px(8.0)}px vertical:0px) label:"Selected effort" {
                        Label text:"EFFORT" role:(TextRole::Micro) weight:500 tone:(Tone::Muted)
                        Label text:(name) role:(TextRole::BodyLarge) weight:600
                    }
                }
            }
        }
    });
    if vm.collapsed {
        let toggle = actions.toggle;
        let label = format!(
            "Agent settings: {}, {}, effort {}. Expand",
            vm.harness.name().to_uppercase(),
            vm.model.name(),
            e.name()
        );
        return view! {
            col {
                row height:{if framed { Dimension::MinContent } else { Dimension::Fr(1.0) }}
                    min-height:0px shrink:0 {
                    stack width:{px(44.0)}px clip {
                        Dither colour:(colour) gradient:(Gradient::Corner)
                            reach:(MODEL_GLOW_COLLAPSED_REACH) opacity:(MODEL_GLOW_OPACITY) seed:3
                            breath:(Some(Breath::GLOW)) flowing:true
                            animation-clock:(Some(animation_clock.clone()))
                        CellButton label:(label) on-press:(toggle.clone()) horizontal-inset:0.0 {
                            col width:max-content align:center justify:between
                                pad:(vertical:{px(10.0)}px horizontal:0px) gap:{px(5.0)}px {
                                HarnessGlyph harness:(vm.harness) size:14.0
                                SerifTitle text:(vm.model.family) size:(SerifSize::Small)
                                Label text:(vm.model.version) role:(TextRole::Label) weight:600
                            }
                        }
                    }
                    col width:{px(22.0)}px align:center justify:center
                        stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                        { if e.levels > 0 || e.has_max() { let track = view! { EffortTrack levels:(e.levels) current:(e.selected()) tiers:(e.tiers) colour:(effort_colour) orientation:(Orientation::Vertical) names:(e.step_names()) on-select:(Some(actions.set_effort)) }; track.style_dyn(|| Style::default().size(px(12.0), px(78.0)).shrink(0.0)); parent.adopt(&track); } }
                    }
                }
                row height:{px(22.0)}px shrink:0 {
                    CellButton label:"Expand agent settings" on-press:(toggle) horizontal-inset:0.0
                        rule:(Rule::Top) {
                        { parent.adopt(&expand_chevron()); }
                    }
                }
            }
        };
    }
    view! {
        row {
            col width:{px(32.0)}px shrink:0 fill:ground.panel-raised
                stroke:(width:{px(1.0)} color:ground.rule edges:right) {
                CellButton
                    label:(format!("Harness: {}. Change harness", vm.harness.name().to_uppercase()))
                    on-press:(actions.change_harness) horizontal-inset:0.0 {
                    col width:max-content align:center gap:{px(10.0)}px
                        pad:(vertical:{px(11.0)}px horizontal:0px) {
                        HarnessGlyph harness:(vm.harness) size:14.0
                        VerticalLabel text:(harness_label.clone())
                    }
                }
            }
            col width:1fr min-width:0px
                height:{if tall { Dimension::Fill } else { Dimension::MinContent }} {
                row height:{if tall { Dimension::Fill } else { Dimension::Px(px(40.0)) }} shrink:0 {
                    stack width:1fr min-width:0px clip {
                        Dither colour:(colour) gradient:(Gradient::Corner)
                            reach:(MODEL_GLOW_OPEN_REACH) opacity:(MODEL_GLOW_OPACITY) seed:3
                            breath:(Some(Breath::GLOW)) flowing:true
                            animation-clock:(Some(animation_clock.clone()))
                        CellButton label:(format!("Model: {}. Change model", vm.model.name()))
                            on-press:(actions.change_model) horizontal-inset:0.0 {
                            row height:min-content align:end gap:{px(7.0)}px
                                pad:(left:{px(12.0)}px) {
                                SerifTitle text:(vm.model.family.clone()) size:(SerifSize::Large)
                                Label text:(vm.model.version.clone()) role:(TextRole::Label)
                                    weight:600
                            }
                        }
                        row height:min-content justify:end pad:(top:{px(5.0)}px right:{px(8.0)}px)
                            nohit {
                            text font-family:monospace font-size:{px(8.5)}px font-weight:500
                                letter-spacing:{px(0.85)}px font-color:ground.muted
                                {vm.model.vendor.clone()}
                        }
                    }
                    col width:{px(36.0)}px shrink:0
                        stroke:(width:{px(1.0)} color:ground.rule edges:left) {
                        CellButton label:"Collapse agent settings" on-press:(actions.toggle)
                            horizontal-inset:0.0 {
                            Label text:"‹" role:(TextRole::BodyLarge)
                        }
                    }
                }
                if !tall {
                    Hairline colour:(ground.rule)
                }
                { if let Some(cell) = &named_cell { parent.adopt(cell); } }
                if track {
                    col height:min-content shrink:0 gap:{px(8.0)}px
                        pad:(horizontal:{px(10.0)}px vertical:{px(9.0)}px) {
                        CutCorner cut:(Cut::Small) corner:(Corner::TopRightBottomLeft)
                            height:(Dimension::MinContent) shrink:0.0 label:"Effort selection frame"
                            edge:(if e.at_max() { colour } else { ground.rule }) {
                            row height:{px(20.0)}px justify:between align:center
                                pad:(horizontal:{px(8.0)}px vertical:0px) {
                                Label text:"EFFORT" role:(TextRole::Micro) weight:500
                                    tone:(Tone::Muted)
                                row width:max-content height:min-content align:center
                                    gap:{px(5.0)}px {
                                    Label text:(e.name()) role:(TextRole::BodyLarge) weight:600
                                        tone:(if vm.pending_effort.is_some() { Tone::Muted } else if e.at_max() { Tone::Colour(colour) } else { Tone::Ink })
                                    Label
                                        text:(format!("{:02}/{:02}", e.selected() as u16 + 1, e.levels as u16 + e.tiers as u16))
                                        role:(TextRole::Micro) tone:(Tone::Muted)
                                }
                            }
                        }
                        EffortTrack levels:(e.levels) current:(e.selected()) tiers:(e.tiers)
                            colour:(effort_colour) names:(e.step_names())
                            on-select:(Some(actions.set_effort.clone()))
                    }
                }
            }
        }
    }
}
// A path centers the visible mark independently of a font's glyph bearings.
fn expand_chevron() -> Element {
    let mark = view! {
        el width:{px(12.0)}px height:{px(12.0)}px shrink:0 nohit label:"›" {}
    };
    mark.paint(|ctx| {
        ctx.scene.shape(
            Shape::new(ctx.rect)
                .path(
                    vec![
                        Vector2::new(-px(2.0), -px(4.0)),
                        Vector2::new(px(2.0), 0.0),
                        Vector2::new(-px(2.0), px(4.0)),
                    ],
                    false,
                )
                .stroke(
                    Stroke::new(px(1.3), mosaic::core::theme::color(ground.ink))
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                ),
        );
    });
    mark
}
/// A quoted source and remove action embedded in one strongly ruled strip.
#[component]
pub fn ReplyStrip(quote: UiText, on_source: OnPress, on_remove: OnPress) -> Element {
    view! {
        row height:{px(32.0)}px min-width:0px shrink:0 gap:0px
            stroke:(width:{px(1.0)} color:ground.rule-strong) label:"Draft reply strip" {
            col width:1fr min-width:0px {
                CellButton label:"Go to reply source" start:true horizontal-inset:(12.0)
                    on-press:(on_source) {
                    row width:1fr min-width:0px height:min-content align:center gap:{px(8.0)}px
                        font-color:ground.muted {
                        { parent.adopt(&crate::icons::icon_mark(crate::icons::mark.reply, 16.0)); }
                        Ellipsis text:(quote) role:(TextRole::Label) tone:(Tone::Muted)
                    }
                }
            }
            col width:{px(32.0)}px shrink:0
                stroke:(width:{px(1.0)} color:ground.rule-strong edges:left) {
                CellButton label:"Remove draft reply" on-press:(on_remove) horizontal-inset:(0.0) {
                    Label text:"×" role:(TextRole::Label)
                }
            }
        }
    }
}

/// A composer never rebuilds its editor when the ledger changes. Supply `value`
/// to read the plain editor's draft; injected editors may instead report
/// `has-content` and retain their own buffer and submission logic.
#[component]
pub fn Composer(
    #[prop(default = false)] inline: bool,
    #[prop(default = Live::fixed(false))] stacked: Live<bool>,
    /// Fixed control gutter, keeping the draft aligned when the ledger folds.
    #[prop(default = Live::fixed(0.0))]
    gutter_width: Live<f32>,
    #[prop(default = Live::fixed(116.0))] minimum_height: Live<f32>,
    settings: Live<LedgerView>,
    ledger_actions: LedgerActions,
    #[prop(default = State::new(String::new()))] value: State<String>,
    on_send: OnPress,
    on_attach: OnPress,
    #[prop(optional)] editor: Option<Element>,
    /// Rich editors can report attachments or other content independently of plain text.
    #[prop(optional)]
    has_content: Option<Live<bool>>,
    #[prop(default = Live::fixed(false))] pending: Live<bool>,
    #[prop(default = Live::fixed(false))] disabled: Live<bool>,
    #[prop(default = UiText::fixed(String::new()))] placeholder: UiText,
    /// The key hint under the message; injected editors describe their own keys.
    #[prop(default = UiText::fixed("↵ send · ⇧↵ new line".to_owned()))]
    hint: UiText,
    /// Whether the hint reports progress (sending, saving). The idle key hint
    /// recedes so it does not read as draft text.
    #[prop(default = Live::fixed(false))]
    hint_feedback: Live<bool>,
) -> Element {
    let pending = Derived::new(move || pending.get());
    let disabled = Derived::new(move || disabled.get());
    let content = Derived::new(move || {
        has_content
            .as_ref()
            .map(|content| content.get())
            .unwrap_or_else(|| !value.get().trim().is_empty())
    });
    let send = on_send.clone();
    let area = view! {
        col width:1fr min-width:0px justify:between gap:{px(8.0)}px
            pad:(horizontal:{px(16.0)}px vertical:{px(12.0)}px) {}
    };
    if let Some(editor) = editor {
        area.adopt(&editor);
    } else {
        let before_key = Rc::new(std::cell::RefCell::new(value.get_untracked()));
        let observed = before_key.clone();
        Effect::new(move || *observed.borrow_mut() = value.get());
        let field = mosaic::widgets::text_area_styled_with_options(
            &area,
            value,
            mosaic::widgets::TextInputStyle {
                text: crate::text::mono_style(12.5, Weight::REGULAR),
                padding: Edges::all(0.0),
                ..Default::default()
            },
            mosaic::widgets::TextInputOptions::default().placeholder_dyn(move || placeholder.get()),
        );
        field.label("Message");
        field.style(
            Style::column()
                .width(Dimension::Fill)
                .height(Dimension::MinContent)
                .min_height(px(68.0)),
        );
        field.visual(mosaic::widgets::Visual::new());
        let after_ime = before_key.clone();
        field.on_ime(move |_, _| *after_ime.borrow_mut() = value.get_untracked());
        let send_key = on_send;
        field.on_key(move |event, ctx| {
            if matches!(event.kind, KeyEventKind::Down { .. })
                && event.key == Key::Enter
                && !event.modifiers.shift
            {
                let previous = before_key.borrow().clone();
                value.set(previous.clone());
                if content.get_untracked() && !pending.get_untracked() && !disabled.get_untracked()
                {
                    send_key();
                }
                ctx.stop_propagation();
            } else {
                // Native edits run before this handler. Track them synchronously
                // so Enter in the same event batch never restores an older draft.
                *before_key.borrow_mut() = value.get_untracked();
            }
        });
    }
    area.adopt(&view! {
        row height:min-content opacity:{if hint_feedback.get() { 1.0 } else { HINT_IDLE_OPACITY }} {
            Label text:(hint) role:(TextRole::Micro) tone:(Tone::Faint)
        }
    });
    if inline {
        let controls_stacked = stacked.clone();
        // The grid cell follows the draft height. Its column keeps the controls
        // at their natural height instead of passing that height to the ledger.
        return view! {
            grid align:start
                cols:{if stacked.get() { GridTracks::new([GridTrack::fr(1.0)]) } else { GridTracks::new([if gutter_width.get() > 0.0 { px(gutter_width.get()).into() } else { GridTrack::MaxContent }, GridTrack::fr(1.0)]) }}
                height:min-content min-height:{px(minimum_height.get())}px font-family:monospace
                font-color:ground.ink {
                col justify:start {
                    row height:min-content align:start
                        justify:{if controls_stacked.get() {Justify::Start} else {Justify::End}} {
                        Ledger data:(settings) actions:(ledger_actions) framed:true
                            height:(Dimension::MinContent)
                    }
                }
                row height:min-content align:start {
                    { parent.adopt(&area); }
                    stack width:{px(40.0)}px shrink:0 height:min-content {
                        col height:min-content {
                            button
                                @click:{if content.get_untracked() && !pending.get_untracked() && !disabled.get_untracked() { send(); }}
                                width:fill height:{px(40.0)}px radius:0px pad:0px align:center
                                justify:center label:"Send message"
                                disabled:{!content.get() || pending.get() || disabled.get()}
                                fill:(Color::TRANSPARENT) font-color:ground.ink
                                stroke:(width:0px color:{Color::TRANSPARENT})
                                hover { fill:ground.hover } pressed { fill:ground.pressed }
                                focused {
                                    stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)})
                                } {
                                if pending.get() {
                                    { parent.adopt(&pending_mark()); }
                                } else {
                                    { parent.adopt(&crate::icons::icon_mark(crate::icons::mark.send, 20.0)); }
                                }
                            }
                            Hairline colour:(ground.rule)
                            button @click:{on_attach()} width:fill height:{px(40.0)}px align:center
                                justify:center pad:0px radius:0px label:"Attach file"
                                fill:(Color::TRANSPARENT) font-color:ground.ink
                                stroke:(width:0px color:{Color::TRANSPARENT})
                                hover { fill:ground.hover } pressed { fill:ground.pressed }
                                focused {
                                    stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)})
                                } {
                                { parent.adopt(&crate::marks::mark(crate::marks::Mark::Plus, 14.0, ground.ink)); }
                            }
                        }
                        el width:fill height:fill nohit label:"Composer actions border"
                            stroke:(width:{px(1.0)} color:ground.rule offset:{px(-0.5)}) {}
                    }
                }
            }
        };
    }
    view! {
        row height:min-content min-height:{px(116.0)}px fill:ground.panel font-family:monospace
            font-color:ground.ink stroke:(width:{px(1.0)} color:ground.rule) {
            Ledger data:(settings) actions:(ledger_actions)
            { parent.adopt(&area); }
            col width:{px(44.0)}px shrink:0 stroke:(width:{px(1.0)} color:ground.rule edges:left) {
                col height:{px(34.0)}px shrink:0 {
                    CellButton label:"Attach file" on-press:(on_attach) {
                        Label text:"+" role:(TextRole::BodyLarge)
                    }
                }
                button
                    @click:{if content.get_untracked() && !pending.get_untracked() && !disabled.get_untracked() { send(); }}
                    width:fill height:1fr align:center justify:center pad:0px radius:0px
                    label:"Send message"
                    disabled:{!content.get() || pending.get() || disabled.get()}
                    description:{if pending.get() {"Awaiting server acknowledgement"} else {""}}
                    fill:{if content.get() { mosaic::core::theme::color(ground.ink) } else { quiet_send(true) }}
                    font-color:{if content.get() { mosaic::core::theme::color(ground.inverse) } else { quiet_send(false) }}
                    stroke:(width:{px(1.0)} color:ground.rule edges:top)
                    focused { stroke:+(width:1px color:ground.ink offset:-2px) } {
                    if pending.get() {
                        { parent.adopt(&pending_mark()); }
                    } else {
                        { parent.adopt(&crate::icons::icon_mark(crate::icons::mark.send, 20.0)); }
                    }
                }
            }
        }
    }
}
fn pending_mark() -> Element {
    let mark = view! {
        el width:{px(14.0)}px height:{px(4.0)}px nohit
            label:"Submission awaiting acknowledgement" {}
    };
    let step = Rc::new(std::cell::Cell::new(0usize));
    let next = step.clone();
    let repaint = mark.clone();
    mark.animate(move |_| {
        next.set((next.get() + 1) % 3);
        repaint.paint_dirty();
        mosaic::widgets::Tick::Sleep(std::time::Duration::from_millis(200))
    });
    mark.paint(move |ctx| {
        let tint = ctx.text_color.clone();
        for i in 0..3 {
            ctx.scene
                .push_layer(if i == step.get() { 1.0 } else { 0.3 });
            ctx.scene.shape(
                mosaic::render::Shape::new(Rect::from_xywh(
                    ctx.rect.origin.x + px(i as f32 * 5.0),
                    ctx.rect.origin.y,
                    px(4.0),
                    px(4.0),
                ))
                .fill(tint.clone()),
            );
            ctx.scene.pop_layer();
        }
    });
    mark
}
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub selected: bool,
}
crate::value::live_value!(Vec<Choice>);
/// Generic choices, inline by default or anchored as a popover with `open`.
/// Selection closes the popover before reporting the option's identity.
#[component]
pub fn ChoiceList(
    options: Live<Vec<Choice>>,
    on_select: Rc<dyn Fn(String)>,
    #[prop(optional)] open: Option<State<bool>>,
    #[prop(default = UiText::fixed("Choices".to_owned()))] label: UiText,
) -> Element {
    if let Some(open) = open {
        let choose: Rc<dyn Fn(String)> = Rc::new(move |id| {
            open.set(false);
            on_select(id);
        });
        let content: PopoverContent = Rc::new(move || {
            view! {
                ChoiceList options:(options.clone()) on-select:(choose.clone())
            }
        });
        return view! {
            Popover open:(open) label:(label) content:(content)
        };
    }
    let list = view! {
        col width:{px(260.0)}px height:min-content {}
    };
    list.switch(
        move || options.get(),
        move |parent, choices| {
            choice_rows(parent, choices, on_select.clone());
        },
    );
    list
}
fn choice_rows(parent: &Element, choices: &[Choice], on_select: Rc<dyn Fn(String)>) {
    for option in choices.iter().cloned() {
        let select = on_select.clone();
        parent.adopt(&view! {
            button @click:{select(option.id.clone())} height:min-content min-height:{px(36.0)}px
                radius:0px fill:(Color::TRANSPARENT) pad:{px(10.0)}px label:(option.label.clone()) {
                col height:min-content gap:{px(4.0)}px {
                    Label
                        text:(format!("{}{}", if option.selected { "✓ " } else { "" }, option.label))
                        role:(TextRole::Label)
                    { if let Some(description) = option.description { parent.adopt(&view! { Ellipsis text:(description) role:(TextRole::Caption) tone:(Tone::Muted) }); } }
                }
            }
        });
    }
}
