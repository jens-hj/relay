use serde::{Deserialize, Serialize};

/// Identity of a running binary, independent of persisted workspace revisions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    pub revision: String,
    /// The absolute directory on the server that holds projects created
    /// without an explicit root. Empty when the server has none or is older.
    /// Only a running server fills it in; it is not part of the build.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub projects_root: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            revision: env!("RELAY_BUILD_REVISION").into(),
            projects_root: String::new(),
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
