use crate::{model::Model, theme};
use mosaic::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub mode: ThemeMode,
    pub light_warm: bool,
    pub dark_neutral: bool,
    /// Omitted while false so files stay readable by clients that predate it.
    #[serde(skip_serializing_if = "is_false")]
    pub light_high_contrast: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub dark_high_contrast: bool,
    pub scale: f32,
    pub sidebar_width: f32,
    pub selected_boards: std::collections::BTreeMap<String, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            light_warm: false,
            dark_neutral: false,
            light_high_contrast: false,
            dark_high_contrast: false,
            scale: 1.0,
            sidebar_width: 220.0,
            selected_boards: Default::default(),
        }
    }
}

/// Local wire format: absence means follow the application default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LightPalette {
    Paper,
    Warm,
    HighContrast,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DarkPalette {
    Slate,
    Neutral,
    HighContrast,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Overrides {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<ThemeMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light_palette: Option<LightPalette>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark_palette: Option<DarkPalette>,
    // Remember alternate families behind a high-contrast selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light_warm: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark_neutral: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_width: Option<f32>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    selected_boards: std::collections::BTreeMap<String, String>,
}
impl Overrides {
    fn from_preferences(p: &Preferences) -> Self {
        let d = Preferences::default();
        Self {
            version: 2,
            mode: (p.mode != d.mode).then_some(p.mode),
            light_palette: if p.light_high_contrast {
                Some(LightPalette::HighContrast)
            } else if p.light_warm {
                Some(LightPalette::Warm)
            } else {
                None
            },
            dark_palette: if p.dark_high_contrast {
                Some(DarkPalette::HighContrast)
            } else if p.dark_neutral {
                Some(DarkPalette::Neutral)
            } else {
                None
            },
            light_warm: (p.light_high_contrast && p.light_warm).then_some(true),
            dark_neutral: (p.dark_high_contrast && p.dark_neutral).then_some(true),
            scale: (p.scale != d.scale).then_some(p.scale),
            sidebar_width: (p.sidebar_width != d.sidebar_width).then_some(p.sidebar_width),
            selected_boards: p.selected_boards.clone(),
        }
    }
    fn resolve(self) -> Result<Preferences, String> {
        if self.version != 2 {
            return Err(format!(
                "Unsupported display settings version {}",
                self.version
            ));
        }
        let mut p = Preferences::default();
        p.mode = self.mode.unwrap_or(p.mode);
        p.light_high_contrast = self.light_palette == Some(LightPalette::HighContrast);
        p.dark_high_contrast = self.dark_palette == Some(DarkPalette::HighContrast);
        p.light_warm = self.light_palette == Some(LightPalette::Warm)
            || (p.light_high_contrast && self.light_warm.unwrap_or(false));
        p.dark_neutral = self.dark_palette == Some(DarkPalette::Neutral)
            || (p.dark_high_contrast && self.dark_neutral.unwrap_or(false));
        p.scale = self.scale.unwrap_or(p.scale);
        p.sidebar_width = self.sidebar_width.unwrap_or(p.sidebar_width);
        p.selected_boards = self.selected_boards;
        p.validate()?;
        Ok(p)
    }
}

#[derive(Clone, Copy)]
pub enum Setting {
    Mode,
    LightPalette,
    DarkPalette,
    Scale,
}
impl Preferences {
    pub fn overridden(&self, setting: Setting) -> bool {
        let d = Self::default();
        match setting {
            Setting::Mode => self.mode != d.mode,
            Setting::LightPalette => self.light_warm || self.light_high_contrast,
            Setting::DarkPalette => self.dark_neutral || self.dark_high_contrast,
            Setting::Scale => self.scale != d.scale,
        }
    }
    pub fn reset(&mut self, setting: Setting) {
        let d = Self::default();
        match setting {
            Setting::Mode => self.mode = d.mode,
            Setting::LightPalette => {
                self.light_warm = d.light_warm;
                self.light_high_contrast = d.light_high_contrast;
            }
            Setting::DarkPalette => {
                self.dark_neutral = d.dark_neutral;
                self.dark_high_contrast = d.dark_high_contrast;
            }
            Setting::Scale => self.scale = d.scale,
        }
    }
}

pub fn reset_sidebar(model: Model) {
    model
        .preferences
        .update(|p| p.sidebar_width = Preferences::default().sidebar_width);
    model.sidebar_reset.update(|serial| *serial += 1);
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
        let value: toml::Value = toml::from_str(&source)
            .map_err(|error| format!("Invalid display settings: {error}"))?;
        let preferences: Self = if value.get("version").is_some() {
            toml::from_str::<Overrides>(&source)
                .map_err(|error| format!("Invalid display settings: {error}"))?
                .resolve()?
        } else {
            toml::from_str(&source).map_err(|error| format!("Invalid display settings: {error}"))?
        };
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
        let source = toml::to_string_pretty(&Overrides::from_preferences(self))
            .map_err(|e| e.to_string())?;
        let result =
            std::fs::write(&temporary, source).and_then(|()| std::fs::rename(&temporary, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|e| format!("Cannot save display settings: {e}"))
    }
}

/// Whether display preferences may be written to disk.
#[derive(Clone, Debug, PartialEq)]
pub enum Persistence {
    Enabled,
    /// The file on disk could not be used. Preferences apply to this session
    /// only and nothing is written until the user explicitly resolves it.
    Suspended {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    pub path: Option<PathBuf>,
    pub persistence: Persistence,
    /// The backup created by the most recent explicit recovery.
    pub backup: Option<PathBuf>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            path: None,
            persistence: Persistence::Enabled,
            backup: None,
        }
    }
}

/// Load preferences without ever discarding an unusable file: a missing
/// file is a fresh start, anything else suspends persistence.
pub fn open(path: &Path) -> (Preferences, Persistence) {
    match Preferences::load(path) {
        Ok(preferences) => (preferences, Persistence::Enabled),
        Err(reason) => (Preferences::default(), Persistence::Suspended { reason }),
    }
}

pub fn suspended_notice(reason: &str) -> String {
    format!(
        "{}. Display changes apply to this session only until settings are recovered.",
        reason.trim_end().trim_end_matches('.')
    )
}

/// Keep a byte-for-byte copy of the existing settings file, then save the
/// current preferences. A backup never replaces an existing file, and any
/// failure leaves the original in place.
pub fn backup_and_save(path: &Path, preferences: &Preferences) -> Result<Option<PathBuf>, String> {
    preferences.validate()?;
    let original = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Cannot read the existing settings file to back it up: {error}"
            ));
        }
    };
    let backup = match &original {
        Some(bytes) => Some(write_backup(
            path,
            bytes,
            &utc_stamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
        )?),
        None => None,
    };
    preferences.save(path).map_err(|error| match &backup {
        Some(backup) => format!(
            "{error}. The original file is unchanged; a copy is at {}",
            backup.display()
        ),
        None => error,
    })?;
    Ok(backup)
}

pub fn write_backup(path: &Path, bytes: &[u8], stamp: &str) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.toml".into());
    for attempt in 0..1000 {
        let candidate = path.with_file_name(if attempt == 0 {
            format!("{name}.unreadable-{stamp}")
        } else {
            format!("{name}.unreadable-{stamp}-{attempt}")
        });
        // `create_new` fails instead of replacing an existing file.
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create settings backup: {error}")),
        };
        if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            // Only the file created above is removed; the original is untouched.
            let _ = std::fs::remove_file(&candidate);
            return Err(format!("Cannot write settings backup: {error}"));
        }
        return match std::fs::read(&candidate) {
            Ok(copy) if copy == bytes => Ok(candidate),
            _ => Err(format!(
                "Settings backup at {} could not be verified; the original file is unchanged",
                candidate.display()
            )),
        };
    }
    Err("Cannot find an unused settings backup name".into())
}

/// `YYYYMMDDTHHMMSSZ` for a Unix timestamp (proleptic Gregorian, UTC).
pub fn utc_stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Explicitly resolve a suspended settings file by backing it up and saving
/// the current preferences. Persistence is only enabled once both succeed.
pub fn recover_by_backup(model: Model) {
    let store = model.settings_store.get_untracked();
    let Some(path) = store.path.clone() else {
        return;
    };
    match backup_and_save(&path, &model.preferences.get_untracked()) {
        Ok(backup) => model.settings_store.set(Store {
            path: Some(path),
            persistence: Persistence::Enabled,
            backup,
        }),
        Err(reason) => model.settings_store.set(Store {
            persistence: Persistence::Suspended { reason },
            ..store
        }),
    }
}

/// Read the file again; adopt it only if it is now valid.
pub fn retry_reading(model: Model) {
    let store = model.settings_store.get_untracked();
    let Some(path) = store.path.clone() else {
        return;
    };
    match open(&path) {
        (preferences, Persistence::Enabled) => {
            model.settings_store.set(Store {
                persistence: Persistence::Enabled,
                ..store
            });
            model.preferences.set(preferences);
        }
        (_, suspended) => model.settings_store.set(Store {
            persistence: suspended,
            ..store
        }),
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

pub fn themes(preferences: &Preferences) -> (theme::RelayTheme, theme::RelayTheme) {
    (
        theme::configured_palette(
            true,
            preferences.light_warm,
            preferences.light_high_contrast,
            preferences.scale,
        ),
        theme::configured_palette(
            false,
            preferences.dark_neutral,
            preferences.dark_high_contrast,
            preferences.scale,
        ),
    )
}

/// Apply preferences to the window and save them while persistence is
/// enabled. A suspended store is never written implicitly.
pub fn bind(model: Model, context: AppContext) {
    let mut previous = model.preferences.get_untracked();
    Effect::new(move || {
        let preferences = model.preferences.get();
        let (light, dark) = themes(&preferences);
        context.set_themes(light.clone(), dark.clone());
        match preferences.mode {
            ThemeMode::Dark => context.set_theme(dark),
            ThemeMode::Light => context.set_theme(light),
            ThemeMode::System => context.follow_system(),
        }
        if preferences != previous {
            let store = model.settings_store.get_untracked();
            if let (Some(path), Persistence::Enabled) = (&store.path, &store.persistence)
                && let Err(error) = preferences.save(path)
            {
                model.notice.set(error);
            }
            previous = preferences;
        }
    });
}
