//! The bundled typefaces: Geist Mono for every label and value, Instrument
//! Serif for named moments. See `assets/fonts/README.md` for provenance.

use mosaic::text::FontContext;

/// Family name of the default face.
pub const MONO: &str = "Geist Mono";
/// Family name of the moment face.
pub const SERIF: &str = "Instrument Serif";

/// Registers the fonts and makes them the monospace, serif and sans-serif
/// families, so unstyled text is set in Geist Mono.
pub fn install(fonts: &mut FontContext) {
    for data in [
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf").as_slice(),
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf").as_slice(),
        include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf").as_slice(),
        include_bytes!("../assets/fonts/InstrumentSerif-Regular.ttf").as_slice(),
        include_bytes!("../assets/fonts/InstrumentSerif-Italic.ttf").as_slice(),
    ] {
        fonts.load_font_data(data.to_vec());
    }
    fonts.set_monospace_family(MONO);
    fonts.set_sans_serif_family(MONO);
    fonts.set_serif_family(SERIF);
}
