mod controls;
mod fonts;
mod model;
mod network;
mod settings;
#[cfg(test)]
mod tests;
mod theme;
mod ui;

use mosaic::prelude::*;

fn main() -> Result<(), String> {
    let config = network::Config::from_env()?;
    let path = settings::path()?;
    let (mut preferences, warning) = match settings::Preferences::load(&path) {
        Ok(preferences) => (preferences, String::new()),
        Err(error) => (settings::Preferences::default(), error),
    };
    match std::env::var("RELAY_THEME").as_deref() {
        Ok("light") => preferences.mode = settings::ThemeMode::Light,
        Ok("dark") => preferences.mode = settings::ThemeMode::Dark,
        Ok("system") => preferences.mode = settings::ThemeMode::System,
        Err(_) => {}
        _ => return Err("RELAY_THEME must be system, light, or dark".into()),
    }
    App::new("Relay")
        .window(WindowConfig::new(1380.0, 900.0))
        .theme(theme::configured_palette(
            false,
            preferences.dark_neutral,
            preferences.scale,
        ))
        .clear(theme::base)
        .run(move |ui, context| {
            fonts::configure(&mut ui.fonts().borrow_mut());
            let (updates, sender) = state_channel(network::NetworkState::default());
            let commands = network::start(config.clone(), sender);
            let model = model::Model::new(ui, commands);
            model.preferences.set(preferences.clone());
            model.notice.set(warning.clone());
            settings::bind(model, context.clone(), Some(path.clone()));
            Effect::new(move || model.receive(updates.get()));
            ui::shell(model)
        });
    Ok(())
}
