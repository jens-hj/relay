use crate::{model::Model, theme};
use mosaic::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub mode: ThemeMode,
    pub light_warm: bool,
    pub dark_neutral: bool,
    pub scale: f32,
    pub sidebar_width: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            light_warm: false,
            dark_neutral: false,
            scale: 1.0,
            sidebar_width: 220.0,
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if !self.scale.is_finite() || !(0.8..=2.0).contains(&self.scale) {
            return Err("Interface scale must be between 80% and 200%.".into());
        }
        if !self.sidebar_width.is_finite() || !(160.0..=360.0).contains(&self.sidebar_width) {
            return Err("Sidebar width must be between 160 and 360.".into());
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("Cannot read display settings: {error}")),
        };
        let preferences: Self = toml::from_str(&source)
            .map_err(|error| format!("Invalid display settings: {error}"))?;
        preferences.validate()?;
        Ok(preferences)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create settings directory: {e}"))?;
        let temporary = parent.join(format!(".relay-settings-{}.tmp", uuid::Uuid::new_v4()));
        let source = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let result =
            std::fs::write(&temporary, source).and_then(|()| std::fs::rename(&temporary, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|e| format!("Cannot save display settings: {e}"))
    }
}

pub fn path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("RELAY_SETTINGS_PATH") {
        return Ok(path.into());
    }
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
    };
    root.map(|p| p.join("relay/settings.toml"))
        .ok_or_else(|| "Set RELAY_SETTINGS_PATH to store local display settings.".into())
}

pub fn bind(model: Model, context: AppContext, path: Option<PathBuf>) {
    let mut previous = model.preferences.get_untracked();
    Effect::new(move || {
        let preferences = model.preferences.get();
        let light = theme::configured_palette(true, preferences.light_warm, preferences.scale);
        let dark = theme::configured_palette(false, preferences.dark_neutral, preferences.scale);
        context.set_themes(light.clone(), dark.clone());
        match preferences.mode {
            ThemeMode::Dark => context.set_theme(dark),
            ThemeMode::Light => context.set_theme(light),
            ThemeMode::System => context.follow_system(),
        }
        if preferences != previous {
            if let Some(path) = &path
                && let Err(error) = preferences.save(path)
            {
                model.notice.set(error);
            }
            previous = preferences;
        }
    });
}
