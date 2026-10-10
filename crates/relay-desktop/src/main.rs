#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_text;
mod buffer;
mod buffer_network;
mod controls;
mod conversation;
mod fonts;
mod labels;
mod model;
mod network;
mod panels;
mod platform;
mod project_network;
mod projects;
mod settings;
mod sidebar;
mod styles;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod window_chrome;

use mosaic::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), String> {
    run(network::Config::from_env()?)
}
#[cfg(target_arch = "wasm32")]
fn main() {
    let config = match network::Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            browser::startup_error(&error);
            return;
        }
    };
    wasm_bindgen_futures::spawn_local(async move {
        match browser::prepare(&config).await {
            Ok(()) => {
                if let Err(error) = run(config) {
                    browser::startup_error(&error);
                }
            }
            Err(error) => browser::startup_error(&error),
        }
    });
}
fn run(config: network::Config) -> Result<(), String> {
    let path = settings::path()?;
    let (preferences, persistence) = settings::open(&path);
    let warning = match &persistence {
        settings::Persistence::Enabled => String::new(),
        settings::Persistence::Suspended { reason } => settings::suspended_notice(reason),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let mut preferences = preferences;
    #[cfg(not(target_arch = "wasm32"))]
    match std::env::var("RELAY_THEME").as_deref() {
        Ok("light") => preferences.mode = settings::ThemeMode::Light,
        Ok("dark") => preferences.mode = settings::ThemeMode::Dark,
        Ok("system") => preferences.mode = settings::ThemeMode::System,
        Err(_) => {}
        _ => return Err("RELAY_THEME must be system, light, or dark".into()),
    }
    App::new("Relay")
        .window(window_chrome::window_config())
        .theme(settings::themes(&preferences).1)
        .theme(theme::icons())
        .clear(theme::surface.base)
        .run(move |ui, context| {
            fonts::configure(&mut ui.fonts().borrow_mut());
            let (updates, sender) = state_channel(network::NetworkState::default());
            let (commands, harness_refresh) = network::start(config.clone(), sender);
            let model = model::Model::new(ui, commands);
            #[cfg(target_arch = "wasm32")]
            browser::install_input(model);
            model.window.set(context.window());
            model.server_endpoint.set(config.endpoint.to_string());
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
            model.notice.set(warning.clone());
            buffer::load_journal(model, &path, &config);
            Effect::new(move || buffer::receive(model, buffer_updates.get()));
            model.preferences.set(preferences.clone());
            model.settings_store.set(settings::Store {
                path: Some(path.clone()),
                persistence: persistence.clone(),
                backup: None,
            });
            settings::bind(model, context.clone());
            let setup_offered = State::new(false);
            Effect::new(move || {
                model.receive(updates.get());
                if model.connected.get_untracked() && !setup_offered.get_untracked() {
                    setup_offered.set(true);
                    if !platform::demo()
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
