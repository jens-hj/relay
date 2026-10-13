//! Ledger settings and formatting over the shared composite model types.
pub use crate::model::{Effort, ModelHue, ModelIdentity};
impl ModelIdentity {
    /// Accessible full identity; unknown vendor/version fields stay absent.
    pub fn name(&self) -> String {
        [&self.vendor, &self.family, &self.version]
            .into_iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}
impl Effort {
    pub fn name(self) -> &'static str {
        if self.has_max && self.at_max {
            "max"
        } else {
            crate::effort::EFFORT_NAMES
                .get(self.selected() as usize)
                .copied()
                .unwrap_or("level")
        }
    }
    pub fn selected(self) -> u8 {
        if self.has_max && self.at_max {
            self.levels
        } else {
            self.current.min(self.levels.saturating_sub(1))
        }
    }
    pub fn caption(self) -> String {
        format!(
            "effort · {} · {:02}/{:02}",
            self.name(),
            self.selected() as u16 + 1,
            self.levels as u16 + u16::from(self.has_max)
        )
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerView {
    pub harness: crate::glyph::Harness,
    pub model: ModelIdentity,
    pub effort: Effort,
    pub collapsed: bool,
}
crate::value::live_value!(LedgerView);
