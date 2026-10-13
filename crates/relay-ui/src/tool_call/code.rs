use super::*;
/// Shared code and diff presentation. Contents are supplied, never synthesized.
#[component]
pub fn CodeBlock(lines: Vec<CodeLine>, #[prop(default = 0)] more: usize) -> Element {
    let numbered = lines.iter().any(|line| line.number.is_some());
    let diff = lines.iter().any(|line| line.change != LineChange::Context);
    let block = view! {
        col height:min-content gap:0px pad:(vertical:{px(6.0)}px horizontal:0px) fill:ground.code
            stroke:(width:{px(1.0)} color:ground.hairline) {}
    };
    for line in lines {
        let removed = line.change == LineChange::Removed;
        let tone = if removed {
            Tone::Colour(status.failed)
        } else {
            Tone::Ink
        };
        let sign = match line.change {
            LineChange::Context => "",
            LineChange::Added => "+",
            LineChange::Removed => "−",
        };
        let row = view! {
            row pad:(left:{px(if numbered || diff { 0.0 } else { 12.0 })}px right:{px(10.0)}px)
                height:{px(19.0)}px align:center min-width:0px
                fill:{if line.change == LineChange::Added { mosaic::core::theme::color(ground.hover) } else { Color::TRANSPARENT }} {
                if numbered {
                    col width:{px(42.0)}px shrink:0 align:end pad:(right:{px(10.0)}px) {
                        Label text:(line.number.map(|n| n.to_string()).unwrap_or_default())
                            role:(TextRole::Label) tone:(Tone::Faint)
                    }
                }
                if diff {
                    col width:{px(14.0)}px shrink:0 {
                        Label text:(sign) role:(TextRole::Label) tone:(tone)
                    }
                }
                stack width:1fr min-width:0px align:center strikethrough:(removed) {
                    Ellipsis text:(line.text) role:(TextRole::Label)
                        tone:(if removed { Tone::Colour(status.failed) } else { Tone::Ink })
                }
            }
        };
        block.adopt(&row);
    }
    if more > 0 {
        block.adopt(&view! {
            Label text:(format!("{more} more lines")) role:(TextRole::Label) tone:(Tone::Muted)
        });
    }
    block
}
#[component]
pub fn KeyValue(entries: Vec<(String, String)>) -> Element {
    let grid = view! {
        col height:min-content gap:{px(5.0)}px {}
    };
    for (key, value) in entries {
        grid.adopt(&view! {
            row height:min-content gap:{px(10.0)}px min-width:0px {
                col width:{px(76.0)}px shrink:0 {
                    Label text:(key) role:(TextRole::Label) tone:(Tone::Muted)
                }
                col width:1fr min-width:0px height:min-content {
                    text font-family:monospace font-size:{px(12.0)}px line-height:1.55
                        {value.clone()}
                }
            }
        });
    }
    grid
}
