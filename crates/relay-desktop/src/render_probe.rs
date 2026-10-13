//! Synthetic continuous workloads: no connection, credentials, or settings file.
use mosaic::{prelude::*, render::Shape, text::FontContext, widgets::Tick};
use std::{cell::Cell, rc::Rc};

pub fn run(workload: &str) -> Result<(), String> {
    if !matches!(workload, "clear" | "solid" | "fixture") {
        return Err("RELAY_RENDER_PROBE must be clear, solid, or fixture".into());
    }
    let workload = workload.to_owned();
    App::new("Relay render probe")
        .window(crate::window_chrome::window_config())
        .theme(relay_ui::tokens::theme_dark())
        .theme(relay_ui::icons::icons())
        .theme(crate::icons::icons())
        .clear(relay_ui::tokens::ground.page)
        .run(move |ui, context| {
            let mut fonts = FontContext::embedded_only();
            relay_ui::fonts::install(&mut fonts);
            ui.set_fonts(fonts);
            relay_ui::attach(ui);
            let elapsed = Rc::new(Cell::new(0.0_f32));
            let (root, receiver) = if workload == "fixture" {
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                let model = crate::model::Model::new(ui, sender);
                model.window.set(context.window());
                model.receive(crate::network::NetworkState {
                    snapshot: relay_core::demo_snapshot(relay_core::DirectorProfile::default()),
                    connected: true,
                    status: "Connected".into(),
                    ..Default::default()
                });
                (crate::ui::shell(model), Some(receiver))
            } else {
                let canvas =
                    Element::orphan(Style::default().size(Dimension::Fill, Dimension::Fill));
                let paint_elapsed = elapsed.clone();
                if workload == "solid" {
                    canvas.paint(move |ctx| {
                        ctx.scene.shape(
                            Shape::new(Rect::from_xywh(
                                40.0 + (paint_elapsed.get() * 100.0) % 400.0,
                                40.0,
                                256.0,
                                256.0,
                            ))
                            .fill(Color::WHITE),
                        );
                    });
                }
                (canvas, None)
            };
            let dirty = root.clone();
            ui.animate(move |dt| {
                // Keep the synthetic command channel open for the lifetime of the UI.
                let _ = &receiver;
                elapsed.set(elapsed.get() + dt.as_secs_f32());
                dirty.paint_dirty();
                Tick::Continue
            });
            root
        });
    Ok(())
}
