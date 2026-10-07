mod model;
mod network;
#[cfg(test)]
mod tests;
mod theme;
mod ui;

use mosaic::prelude::*;

fn main() -> Result<(), String> {
    let config = network::Config::from_env()?;
    let app = App::new("Relay").window(WindowConfig::new(1380.0, 900.0));
    let app = match std::env::var("RELAY_THEME").as_deref() {
        Ok("light") => app.theme(theme::palette(true)),
        Ok("dark") => app.theme(theme::palette(false)),
        Ok("system") | Err(_) => app.themes(theme::palette(true), theme::palette(false)),
        _ => return Err("RELAY_THEME must be system, light, or dark".into()),
    };
    app.clear(theme::base).run(move |ui, _| {
        let fonts = ui.fonts();
        let mut fonts = fonts.borrow_mut();
        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["SF Pro Text", "Helvetica Neue", "DejaVu Sans"]
        } else if cfg!(target_os = "windows") {
            &["Segoe UI", "DejaVu Sans"]
        } else {
            &["Inter", "Noto Sans", "DejaVu Sans"]
        };
        if let Some(family) = candidates.iter().find(|family| fonts.has_family(family)) {
            fonts.set_sans_serif_family(*family);
        }
        drop(fonts);
        let (updates, sender) = state_channel(network::NetworkState::default());
        let commands = network::start(config.clone(), sender);
        let model = model::Model::new(ui, commands);
        Effect::new(move || model.receive(updates.get()));
        ui::shell(model)
    });
    Ok(())
}
