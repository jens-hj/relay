//! Plain view-model types the composites share. They carry only what the
//! design shows, so callers map their own data into them.

pub use crate::glyph::Harness;
pub use crate::status::AgentStatus;
pub use crate::tokens::ToolKind;

use crate::tokens::{ModelFamily, ground};
use mosaic::prelude::ColorToken;

/// The identity colour of a model: one of the four families, or neutral
/// for a model without one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ModelHue {
    Sol,
    Opus,
    Sonnet,
    Haiku,
    #[default]
    Neutral,
}

impl ModelHue {
    /// The colour token: the family colour, or muted grey when neutral.
    pub fn token(self) -> ColorToken {
        match self {
            ModelHue::Sol => ModelFamily::Sol.token(),
            ModelHue::Opus => ModelFamily::Opus.token(),
            ModelHue::Sonnet => ModelFamily::Sonnet.token(),
            ModelHue::Haiku => ModelFamily::Haiku.token(),
            ModelHue::Neutral => ground.muted,
        }
    }
}

impl From<ModelFamily> for ModelHue {
    fn from(family: ModelFamily) -> Self {
        match family {
            ModelFamily::Sol => ModelHue::Sol,
            ModelFamily::Opus => ModelHue::Opus,
            ModelFamily::Sonnet => ModelHue::Sonnet,
            ModelFamily::Haiku => ModelHue::Haiku,
        }
    }
}

/// Which model thinks: family in the serif, version in bold mono, vendor
/// as a caption.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModelIdentity {
    /// "Sol", "Opus", …
    pub family: String,
    /// "6.1", "5.5", …
    pub version: String,
    /// "GPT", "Claude", …; empty when unknown.
    pub vendor: String,
    pub hue: ModelHue,
}

impl ModelIdentity {
    pub fn new(family: impl Into<String>, version: impl Into<String>, hue: ModelHue) -> Self {
        ModelIdentity {
            family: family.into(),
            version: version.into(),
            vendor: String::new(),
            hue,
        }
    }

    pub fn vendor(mut self, vendor: impl Into<String>) -> Self {
        self.vendor = vendor.into();
        self
    }

    /// "Sol 6.1", or the family alone without a version.
    pub fn short_name(&self) -> String {
        if self.version.is_empty() {
            self.family.clone()
        } else {
            format!("{} {}", self.family, self.version)
        }
    }
}

/// A model's effort setting: `levels` ordinary steps, the zero-based
/// `current` step, and whether a max step exists and is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Effort {
    pub levels: u8,
    pub current: u8,
    pub has_max: bool,
    pub at_max: bool,
}

impl Effort {
    /// Step `current` (zero-based) of `levels`, without a max step.
    pub fn new(levels: u8, current: u8) -> Self {
        Effort {
            levels,
            current: current.min(levels.saturating_sub(1)),
            has_max: false,
            at_max: false,
        }
    }

    /// Offers a max step, selected when `at_max`.
    pub fn with_max(mut self, at_max: bool) -> Self {
        self.has_max = true;
        self.at_max = at_max;
        self
    }

    /// The step a track draws as current: `levels` at max.
    pub fn track_step(&self) -> u8 {
        if self.has_max && self.at_max {
            self.levels
        } else {
            self.current
        }
    }

    /// The 1-based level that sets a live wash's reach: max counts as one
    /// past the last ordinary level.
    pub fn level(&self) -> u8 {
        self.track_step() + 1
    }
}
