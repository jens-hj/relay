#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn compact_navigation(width: f32) -> bool {
    #[cfg(target_arch = "wasm32")]
    if crate::browser::touch_device() {
        // Keep phone navigation folded after rotating into landscape.
        return width < 1000.0;
    }
    width < 640.0
}

pub(crate) fn top_clearance() -> f32 {
    #[cfg(target_arch = "wasm32")]
    if crate::browser::installed_touch_app() {
        // A solid breathing strip beneath the reported safe area keeps
        // installed-phone system blur away from the header. This is spacing,
        // not an assumed status-bar height, and does not scale with text.
        return 16.0;
    }
    0.0
}

pub fn pick_files(model: crate::model::Model) {
    #[cfg(target_arch = "wasm32")]
    crate::browser::pick_files(model);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = model;
}

pub fn logout(model: crate::model::Model) {
    #[cfg(target_arch = "wasm32")]
    crate::browser::logout(model);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = model;
}

pub fn demo() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("RELAY_DEMO").as_deref() == Ok("1")
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}
