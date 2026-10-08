//! Content-sized scrolling disclosures. Measure the mounted content rather
//! than estimating its height, and keep it mounted as the viewport changes.
use mosaic::prelude::*;

#[component]
pub(crate) fn BoundedPanel(limit: Derived<f32>, #[prop(optional)] children: Children) -> Element {
    let extent = State::new(0.0f32);
    view! {
        col height:min-content shrink:0 min-width:0px {
            scroll {
                col height:min-content min-width:0px
                    @layout:{move |rect:Rect| extent.set(rect.size.height)} {
                    children
                }
            } as panel
            {
                panel.root().style_dyn(move || Style::stack()
                    .width(Dimension::Fill)
                    .height(extent.get().min(limit.get().max(0.0)))
                    .basis(Dimension::Auto).grow(0.0).shrink(0.0));
            }
        }
    }
}
