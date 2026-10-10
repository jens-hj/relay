use serde::{Deserialize, Serialize};

/// Identity of a running binary, independent of persisted workspace revisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    pub revision: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            revision: env!("RELAY_BUILD_REVISION").into(),
        }
    }

    pub fn short_revision(&self) -> String {
        if self.revision.len() >= 40
            && self.revision.as_bytes()[..40]
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            format!("{}{}", &self.revision[..7], &self.revision[40..])
        } else {
            self.revision.clone()
        }
    }
}
