mod buffer;
mod buffer_network;
mod controls;
mod conversation;
mod fonts;
mod labels;
mod model;
mod network;
mod project_network;
mod projects;
mod settings;
mod sidebar;
#[cfg(test)]
mod tests;
mod theme;
mod ui;

use mosaic::prelude::*;

fn main() -> Result<(), String> {
    let config = network::Config::from_env()?;
    let path = settings::path()?;
    let (mut preferences, persistence) = settings::open(&path);
    let warning = match &persistence {
        settings::Persistence::Enabled => String::new(),
        settings::Persistence::Suspended { reason } => settings::suspended_notice(reason),
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
        .theme(settings::themes(&preferences).1)
        .clear(theme::base)
        .run(move |ui, context| {
            fonts::configure(&mut ui.fonts().borrow_mut());
            let (updates, sender) = state_channel(network::NetworkState::default());
            let (commands, harness_refresh) = network::start(config.clone(), sender);
            let model = model::Model::new(ui, commands);
            model.harness_refresh.set(Some(harness_refresh));
            let (discovery_updates, discovery_sender) =
                state_channel(project_network::DiscoveryUpdate::default());
            model.discovery_requests.set(Some(project_network::start(
                config.clone(),
                discovery_sender,
            )));
            Effect::new(move || model.discovery.set(discovery_updates.get()));
            let (recovery_updates, recovery_sender) =
                state_channel(project_network::RecoveryUpdate::default());
            model
                .recovery_requests
                .set(Some(project_network::start_recovery(
                    config.clone(),
                    recovery_sender,
                )));
            Effect::new(move || model.recovery.set(recovery_updates.get()));
            let (buffer_updates, buffer_sender) = state_channel(buffer_network::Update::default());
            model
                .buffer_requests
                .set(Some(buffer_network::start(config.clone(), buffer_sender)));
            buffer::load_journal(model, &path, &config);
            Effect::new(move || buffer::receive(model, buffer_updates.get()));
            model.preferences.set(preferences.clone());
            model.settings_store.set(settings::Store {
                path: Some(path.clone()),
                persistence: persistence.clone(),
                backup: None,
            });
            model.notice.set(warning.clone());
            settings::bind(model, context.clone());
            let setup_offered = State::new(false);
            Effect::new(move || {
                model.receive(updates.get());
                if model.connected.get_untracked() && !setup_offered.get_untracked() {
                    setup_offered.set(true);
                    if std::env::var("RELAY_DEMO").as_deref() != Ok("1")
                        && !model
                            .snapshot
                            .get_untracked()
                            .projects
                            .iter()
                            .any(|p| !p.fixture)
                    {
                        model.page.set(model::Page::NewProject);
                    }
                }
            });
            ui::shell(model)
        });
    Ok(())
}
