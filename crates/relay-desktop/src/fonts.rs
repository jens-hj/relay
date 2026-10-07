use mosaic::text::FontContext;

pub fn configure(fonts: &mut FontContext) -> Result<bool, String> {
    fonts.load_font_data(include_bytes!("../assets/fonts/zed-mono-regular.ttf").to_vec());
    fonts.load_font_data(include_bytes!("../assets/fonts/zed-mono-bold.ttf").to_vec());
    fonts.set_monospace_family("Zed Mono");
    if let Some(path) = std::env::var_os("RELAY_TITLE_FONT") {
        let data = std::fs::read(path).map_err(|e| format!("Cannot read RELAY_TITLE_FONT: {e}"))?;
        fonts.load_font_data(data);
    }
    let title = ["Neurath X", "RB Neurath X"]
        .into_iter()
        .find(|name| fonts.has_family(name));
    if let Some(family) = title.or_else(|| {
        ["Inter", "Noto Sans", "DejaVu Sans"]
            .into_iter()
            .find(|name| fonts.has_family(name))
    }) {
        fonts.set_sans_serif_family(family);
    }
    Ok(title.is_some())
}
