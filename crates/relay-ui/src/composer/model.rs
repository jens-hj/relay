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
        self.step_name(self.selected())
    }
    pub fn selected(self) -> u8 {
        self.track_step()
    }
    pub fn caption(self) -> String {
        format!(
            "effort · {} · {:02}/{:02}",
            self.name(),
            self.selected() as u16 + 1,
            self.levels as u16 + self.tiers as u16
        )
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerView {
    pub harness: crate::glyph::Harness,
    pub model: ModelIdentity,
    pub effort: Effort,
    /// Requested step awaiting a server response.
    pub pending_effort: Option<u8>,
    /// The selected effort by name, shown without a step track when the
    /// model's steps are unknown (`effort` has no levels). Without it
    /// and without steps, the model cell fills the ledger.
    pub effort_label: Option<String>,
    pub collapsed: bool,
}
crate::value::live_value!(LedgerView);
