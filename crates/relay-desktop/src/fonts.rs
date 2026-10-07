use mosaic::text::FontContext;

pub fn configure(fonts: &mut FontContext) {
    for data in [
        include_bytes!("../assets/fonts/RedditSans-Regular.ttf").as_slice(),
        include_bytes!("../assets/fonts/RedditSans-SemiBold.ttf").as_slice(),
        include_bytes!("../assets/fonts/RedditSans-Bold.ttf").as_slice(),
        include_bytes!("../assets/fonts/RedditSans-ExtraBold.ttf").as_slice(),
    ] {
        fonts.load_font_data(data.to_vec());
    }
    fonts.load_font_data(include_bytes!("../assets/fonts/zed-mono-regular.ttf").to_vec());
    fonts.load_font_data(include_bytes!("../assets/fonts/zed-mono-bold.ttf").to_vec());
    fonts.set_monospace_family("Zed Mono");
    fonts.set_sans_serif_family("Reddit Sans");
}
