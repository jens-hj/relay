use crate::dither::{DitherSpec, Gradient, Turbulence, live_reach};
use crate::effort::{MarkShape, effort_label, ledger_marks};
use crate::prelude::*;
use crate::text::fit_text;
use crate::tokens::{
    ContextPart, ModelFamily, Palette, Signal, ThemeVariant, ToolKind, contrast, theme,
    thresholds::{self, Level},
};
use mosaic::{core::reactive::flush, prelude::*, text::FontContext};
use std::time::Duration;

#[path = "../examples/gallery/sections.rs"]
pub(super) mod gallery;

pub(super) struct Mounted {
    scope: Scope,
    pub(super) ui: Ui,
    size: Size,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

pub(super) fn mount(size: Size, build: impl FnOnce() -> Element) -> Mounted {
    mosaic::core::builtins::install();
    install_theme(&theme(ThemeVariant::Dark, 1.0));
    install_theme(&crate::icons::icons());
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    ui.set_fonts(FontContext::embedded_only());
    crate::install(&ui);
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&build());
    });
    let mounted = Mounted { scope, ui, size };
    mounted.settle();
    mounted
}

impl Mounted {
    pub(super) fn settle(&self) {
        flush();
        for _ in 0..12 {
            self.ui.tick(Duration::from_millis(16));
            flush();
            if self.ui.frame(self.size, 1.0).is_none() {
                break;
            }
        }
    }

    pub(super) fn labels(&self) -> Vec<String> {
        self.ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter_map(|node| node.label)
            .collect()
    }

    pub(super) fn rect(&self, label: &str) -> Rect {
        self.ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|node| node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing {label}"))
            .rect
    }

    pub(super) fn click(&self, position: Vector2) {
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            self.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        self.settle();
    }

    pub(super) fn key(&self, key: Key) {
        self.ui.dispatch_key(KeyEvent {
            kind: KeyEventKind::Down { repeat: false },
            key,
            modifiers: Modifiers::default(),
        });
        self.settle();
    }
}

#[test]
fn status_squares_name_every_state_and_follow_changes() {
    let status = State::new(AgentStatus::Working);
    let mounted = mount(Size::new(400.0, 100.0), || {
        let row = view! {
            row {}
        };
        for each in AgentStatus::ALL {
            row.adopt(&view! {
                StatusSquare status:(each)
            });
        }
        row.adopt(&view! {
            StatusSquare status:(status)
        });
        row
    });
    let labels = mounted.labels();
    for expected in [
        "Working",
        "Waiting for approval",
        "Failed",
        "Queued",
        "Done",
    ] {
        assert!(labels.iter().any(|label| label == expected), "{labels:?}");
    }
    assert_eq!(mounted.rect("Failed").size, Size::new(10.0, 10.0));
    let working = labels.iter().filter(|label| *label == "Working").count();
    status.set(AgentStatus::Done);
    mounted.settle();
    let labels = mounted.labels();
    assert_eq!(
        labels.iter().filter(|label| *label == "Working").count(),
        working - 1
    );
}

#[test]
fn breathing_runs_only_while_working() {
    use crate::motion::Breath;
    assert_eq!(Breath::LIVE.at(0.0), 1.0);
    assert!((Breath::LIVE.at(1.2) - 0.45).abs() < 1e-4);
    assert!((Breath::GLOW.at(1.8) - 0.6).abs() < 1e-4);
    let status = State::new(AgentStatus::Working);
    let mounted = mount(Size::new(100.0, 100.0), || {
        view! {
            StatusSquare status:(status)
        }
    });
    assert!(mounted.ui.is_animating());
    status.set(AgentStatus::Queued);
    mounted.settle();
    mounted.ui.tick(Duration::from_millis(100));
    flush();
    assert!(!mounted.ui.is_animating());
}

#[test]
fn ledger_steps_grow_and_max_appears_only_when_offered() {
    let plain = ledger_marks(5, 4, false, 158.0);
    assert_eq!(plain.len(), 5);
    assert!(plain.iter().all(|mark| mark.shape == MarkShape::Square));
    assert!(plain.windows(2).all(|pair| pair[1].size > pair[0].size));
    assert!(plain.windows(2).all(|pair| pair[1].at > pair[0].at));
    assert_eq!(plain[0].at, 12.0);
    assert_eq!(plain[4].at, 146.0);
    assert!(plain[4].current && plain.iter().all(|mark| mark.filled));

    let high = ledger_marks(5, 3, true, 158.0);
    assert_eq!(high.len(), 6);
    assert_eq!(high[5].shape, MarkShape::Diamond);
    assert!(!high[5].filled && !high[5].current);
    assert!(high[3].current && high[3].filled && !high[4].filled);
    assert!(high[..5].windows(2).all(|pair| pair[1].size > pair[0].size));

    let max = ledger_marks(5, 5, true, 158.0);
    assert!(max[5].filled && max[5].current);
    assert_eq!(max.iter().filter(|mark| mark.current).count(), 1);

    assert_eq!(effort_label(5, 4, false), "Effort 5 of 5");
    assert_eq!(effort_label(5, 5, true), "Effort max");
}

#[test]
fn effort_buttons_offer_max_only_when_the_model_does() {
    let chosen = State::new(None::<u8>);
    let select: crate::effort::OnEffort = std::rc::Rc::new(move |step| chosen.set(Some(step)));
    let five = select.clone();
    let mounted = mount(Size::new(400.0, 200.0), move || {
        view! {
            col {
                EffortTrack levels:5u8 current:2u8 colour:(ModelFamily::Sol.token())
                    on-select:(Some(five))
                EffortTrack levels:5u8 current:2u8 has-max:true colour:(ModelFamily::Opus.token())
                    on-select:(Some(select))
            }
        }
    });
    let labels = mounted.labels();
    assert_eq!(
        labels
            .iter()
            .filter(|label| *label == "Effort xhigh")
            .count(),
        2
    );
    assert_eq!(
        labels.iter().filter(|label| *label == "Effort max").count(),
        1
    );
    mounted.click(mounted.rect("Effort max").center());
    assert_eq!(chosen.get_untracked(), Some(5));
}

#[test]
fn compact_track_is_three_pixel_steps_at_five_pixel_pitch() {
    let mounted = mount(Size::new(200.0, 100.0), || {
        view! {
            row {
                EffortTrack levels:5u8 current:3u8 colour:(ModelFamily::Opus.token())
                    size:(TrackSize::Compact)
            }
        }
    });
    let rect = mounted.rect("Effort 4 of 5");
    assert_eq!(rect.size, Size::new(23.0, 3.0));
}

#[test]
fn thresholds_turn_amber_at_80_and_red_at_95() {
    assert_eq!(thresholds::level(0.0), Level::Room);
    assert_eq!(thresholds::level(79.9), Level::Room);
    assert_eq!(thresholds::level(80.0), Level::Amber);
    assert_eq!(thresholds::level(94.9), Level::Amber);
    assert_eq!(thresholds::level(95.0), Level::Red);
    assert_eq!(thresholds::level(130.0), Level::Red);
}

#[test]
fn dither_is_deterministic_and_denser_with_reach() {
    let spec = |reach: f32| {
        let mut spec = DitherSpec::new(200.0, 34.0, 2.0, Gradient::Horizontal, reach);
        spec.seed = 7;
        spec
    };
    let first = spec(0.6).mask();
    assert_eq!(first, spec(0.6).mask());
    assert_eq!(first.len(), 400 * 68);
    let mut reseeded = spec(0.6);
    reseeded.seed = 3;
    assert_ne!(first, reseeded.mask());

    let mut previous = 0;
    for level in 0..=6 {
        let mask = spec(live_reach(level)).mask();
        let count = mask.iter().filter(|on| **on).count();
        assert!(count >= previous, "level {level}: {count} < {previous}");
        previous = count;
    }
    let sparse = spec(live_reach(0)).mask().iter().filter(|on| **on).count();
    assert!(previous > sparse * 2, "{previous} vs {sparse}");
    assert!(previous < 400 * 68 / 2, "a wash stays a dither: {previous}");
    assert_eq!(spec(0.0).mask().iter().filter(|on| **on).count(), 0);

    let noise = Turbulence::new(7);
    for y in 0..20 {
        for x in 0..20 {
            let value = noise.fractal_alpha(x as f64 * 0.5, y as f64 * 0.5, 0.85, 2);
            assert!((0.0..=1.0).contains(&value));
        }
    }
    assert_eq!(live_reach(2), 0.48);
    assert_eq!(live_reach(6), 1.0);
}

#[test]
fn dither_images_flood_one_colour_at_opacity() {
    let spec = DitherSpec::new(40.0, 10.0, 1.0, Gradient::Amber, 1.0);
    let image = spec.image(Color::from_rgb_hex(0xE3B266), 0.22);
    assert_eq!((image.width(), image.height()), (40, 10));
    let pixels: Vec<&[u8]> = image.data().chunks(4).collect();
    assert!(pixels.iter().any(|pixel| pixel[3] > 0));
    for pixel in pixels {
        assert!(pixel[3] == 0 || *pixel == [0xE3, 0xB2, 0x66, 56]);
    }
}

#[test]
fn every_theme_variant_meets_contrast() {
    for variant in ThemeVariant::ALL {
        let palette: &Palette = variant.palette();
        let g = palette.ground;
        let text_grounds = g.surfaces();
        let name = variant.name();
        for surface in text_grounds {
            for (role, colour) in [("ink", g.ink), ("muted", g.muted)] {
                let ratio = contrast(colour, surface);
                assert!(ratio >= 4.5, "{name}: {role} on {surface:?} is {ratio}");
            }
        }
        for surface in [g.page, g.panel, g.panel_raised] {
            let ratio = contrast(g.faint, surface);
            assert!(ratio >= 3.0, "{name}: faint is {ratio}");
            for signal in Signal::ALL {
                let ratio = contrast(palette.signal(signal), surface);
                assert!(ratio >= 4.5, "{name}: {signal:?} is {ratio}");
            }
            for family in ModelFamily::ALL {
                let ratio = contrast(palette.model(family), surface);
                assert!(ratio >= 4.5, "{name}: {family:?} is {ratio}");
            }
            for tool in ToolKind::ALL {
                let ratio = contrast(palette.tool(tool), surface);
                assert!(ratio >= 4.5, "{name}: {tool:?} is {ratio}");
            }
        }
        let ratio = contrast(g.inverse, palette.status.needs_you);
        assert!(ratio >= 4.5, "{name}: primary button text is {ratio}");
        let ratio = contrast(g.inverse, palette.status.failed);
        assert!(ratio >= 3.0, "{name}: failed cross is {ratio}");
        for pair in ContextPart::ALL.windows(2) {
            let (a, b) = (palette.context(pair[0]), palette.context(pair[1]));
            assert!(
                contrast(a, g.panel) > contrast(b, g.panel),
                "{name}: context greys must step down: {pair:?}"
            );
        }
    }
}

#[test]
fn the_dark_theme_uses_the_design_pack_values() {
    let dark = Palette::DARK;
    assert_eq!(dark.ground.page, Color::from_rgb_hex(0x0D0D0F));
    assert_eq!(dark.ground.ink, Color::from_rgb_hex(0xEDEBE6));
    assert_eq!(dark.status.live, Color::from_rgb_hex(0xA5A1F2));
    assert_eq!(
        dark.model(ModelFamily::Sonnet),
        Color::from_rgb_hex(0x48BFF5)
    );
    assert_eq!(dark.tool(ToolKind::Result), Color::from_rgb_hex(0xEEA6BD));
    assert_eq!(dark.tool(ToolKind::Approval), dark.status.needs_you);
    assert_eq!(
        dark.context(ContextPart::Compacted),
        Color::from_rgb_hex(0x46454A)
    );
}

#[test]
fn ellipsis_trims_to_the_measured_width() {
    let mut fonts = FontContext::embedded_only();
    crate::fonts::install(&mut fonts);
    assert!(fonts.has_family("Geist Mono"));
    assert!(fonts.has_family("Instrument Serif"));
    let style = TextStyle::new(12.5)
        .family(FontFamily::Monospace)
        .weight(400);
    let long = "Release 2.4 / API migration / Backfill script / Verify rows";
    let fitted = fit_text(&mut fonts, long, &style, 120.0);
    assert!(fitted.ends_with('…'), "{fitted}");
    assert!(long.starts_with(fitted.trim_end_matches('…')));
    let width = fonts.shape(&fitted, &style, None).metrics().size.width;
    assert!(width <= 120.01, "{width}");
    let longer = format!("{}…", &long[..fitted.chars().count()]);
    assert!(fonts.shape(&longer, &style, None).metrics().size.width > 120.0);
    assert_eq!(fit_text(&mut fonts, "Fits", &style, 120.0), "Fits");
    assert_eq!(fit_text(&mut fonts, long, &style, 0.0), "…");

    let mounted = mount(Size::new(400.0, 100.0), || {
        view! {
            row {
                col width:{150.0}px {
                    Ellipsis text:(long)
                }
            }
        }
    });
    let shown = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|node| node.role == mosaic::widgets::Role::Label)
        .filter_map(|node| node.label)
        .find(|label| label.ends_with('…'))
        .expect("trimmed line");
    assert!(long.starts_with(shown.trim_end_matches('…')), "{shown}");
}

#[test]
fn vertical_labels_take_their_rotated_size() {
    let mounted = mount(Size::new(300.0, 300.0), || {
        view! {
            row {
                VerticalLabel text:"CLAUDE CODE"
            }
        }
    });
    let rect = mounted.rect("CLAUDE CODE");
    assert!(rect.size.height > rect.size.width * 4.0, "{rect:?}");
    assert!(
        rect.size.width >= 11.0 && rect.size.width < 20.0,
        "{rect:?}"
    );
}

#[test]
fn ruled_rows_put_one_rule_between_neighbours() {
    let mounted = mount(Size::new(400.0, 60.0), || {
        let press: crate::cells::OnPress = std::rc::Rc::new(|| {});
        view! {
            RuledRow {
                Cell {
                    Label text:"first"
                }
                CellButton label:"second" on-press:(press.clone()) {
                    Label text:"Second"
                }
                Cell {
                    Label text:"third"
                }
            }
        }
    });
    let snapshot = mounted.ui.inspection_snapshot();
    let ruled: Vec<_> = snapshot
        .nodes
        .iter()
        .filter(|node| node.label.as_deref() == Some("second"))
        .collect();
    assert_eq!(ruled.len(), 1);
    // Rules are strokes, not gaps: neighbours abut.
    let second = mounted.rect("second");
    assert!(second.origin.x > 0.0);
    let first = snapshot
        .nodes
        .iter()
        .find(|node| node.label.as_deref() == Some("first"))
        .unwrap()
        .rect;
    assert!(first.origin.x < second.origin.x);
}

#[test]
fn popover_opens_and_dismisses_on_escape_and_outside_clicks() {
    let open = State::new(false);
    let mounted = mount(Size::new(600.0, 400.0), move || {
        let toggle: crate::cells::OnPress =
            std::rc::Rc::new(move || open.set(!open.get_untracked()));
        let content: crate::overlay::PopoverContent = std::rc::Rc::new(|| {
            view! {
                col width:{120.0}px height:{80.0}px {
                    Label text:"Panel body"
                }
            }
        });
        view! {
            col {
                stack width:{100.0}px height:{40.0}px {
                    CellButton label:"Open" on-press:(toggle) {
                        Label text:"Open"
                    }
                    Popover open:(open) label:"Details" content:(content)
                }
            }
        }
    });
    let trigger = mounted.rect("Open").center();
    mounted.click(trigger);
    assert!(open.get_untracked());
    let panel = mounted.rect("Details");
    assert!(panel.origin.y >= 40.0, "opens below: {panel:?}");
    mounted.click(panel.center());
    assert!(open.get_untracked(), "clicks inside keep it open");
    mounted.key(Key::Escape);
    assert!(!open.get_untracked());
    mounted.click(trigger);
    assert!(open.get_untracked());
    mounted.click(trigger);
    assert!(!open.get_untracked(), "the trigger toggles it closed");
    mounted.click(trigger);
    mounted.click(Vector2::new(500.0, 350.0));
    assert!(!open.get_untracked(), "an outside click closes it");
    assert!(!mounted.labels().iter().any(|label| label == "Details"));
}

#[test]
fn every_variant_builds_the_gallery() {
    let mounted = mount(Size::new(1280.0, 2400.0), || {
        gallery::gallery(std::rc::Rc::new(|variant| {
            install_theme(&theme(variant, 1.0));
        }))
    });
    for variant in ThemeVariant::ALL {
        let center = mounted.rect(variant.name()).center();
        mounted.click(center);
    }
    let labels = mounted.labels();
    for expected in ["Working", "Claude Code", "Effort max", "Context"] {
        assert!(labels.iter().any(|label| label == expected), "{expected}");
    }
}

/// Renders the gallery offscreen in every variant and writes PNGs to
/// `RELAY_UI_SCREENSHOTS` (default `target/relay-ui-gallery`).
#[test]
#[ignore = "manual: needs a GPU adapter; writes screenshots"]
fn gallery_screenshots() {
    use crate::tokens::ground;
    use mosaic::render::Renderer;
    let directory = std::env::var("RELAY_UI_SCREENSHOTS").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/relay-ui-gallery").into()
    });
    std::fs::create_dir_all(&directory).unwrap();
    let scale = std::env::var("RELAY_UI_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0f32);
    let specimen = std::env::var("RELAY_UI_SPECIMEN").unwrap_or_default();
    let logical = match specimen.as_str() {
        "agents" => Size::new(1280.0, 6800.0),
        "tools" => Size::new(1332.0, 3800.0),
        "conversation" => Size::new(960.0, 1400.0),
        "composer" => Size::new(848.0, 1300.0),
        _ => Size::new(1440.0, 14000.0),
    };
    let mounted = mount(logical, || {
        let content = match specimen.as_str() {
            "agents" => gallery::agents::sections(),
            "tools" => gallery::d2::tool_board(),
            "conversation" => gallery::d2::conversation(),
            "composer" => gallery::d2::composer_states(),
            _ => gallery::gallery(std::rc::Rc::new(|variant| {
                install_theme(&theme(variant, 1.0));
            })),
        };
        view! {
            col fill:ground.page font-color:ground.ink font-family:monospace {
                { parent.adopt(&content); }
            }
        }
    });
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let (width, height) = (
        (logical.width * scale) as u32,
        (logical.height * scale) as u32,
    );
    for variant in ThemeVariant::ALL {
        install_theme(&theme(variant, 1.0));
        flush();
        for _ in 0..12 {
            mounted.ui.tick(Duration::from_millis(16));
            flush();
            mounted.ui.frame(logical, scale);
        }
        let scene = mounted.ui.scene().clone();
        let mut renderer = mosaic::render_wgpu::WgpuRenderer::from_device(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8UnormSrgb,
            Size::new(width as f32, height as f32),
        );
        renderer.set_scale_factor(f64::from(scale));
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        queue.submit([renderer.encode_into(
            &scene,
            &view,
            mosaic::render_wgpu::TargetLoad::Clear(scene.clear),
        )]);
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for line in 0..height {
            let start = (line * row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        let name = variant
            .name()
            .to_lowercase()
            .replace(", ", "-")
            .replace(' ', "-");
        let prefix = if specimen.is_empty() {
            "gallery"
        } else {
            specimen.as_str()
        };
        let path = format!("{directory}/{prefix}-{name}.png");
        image::RgbaImage::from_raw(width, height, pixels)
            .unwrap()
            .save(&path)
            .unwrap();
        eprintln!("wrote {path}");
    }
}

#[test]
fn washes_match_srgb_blending() {
    let (tint, under) = (Palette::DARK.status.live, Palette::DARK.ground.panel);
    assert_eq!(crate::tokens::srgb_alpha(tint, under, 0.0), 0.0);
    assert!((crate::tokens::srgb_alpha(tint, under, 1.0) - 1.0).abs() < 1e-4);
    let header = crate::tokens::srgb_alpha(tint, under, 0.14);
    assert!(header > 0.02 && header < 0.08, "{header}");
}

/// The approval wash thins from left to right like the boards' `gWa`
/// (0.6 → 0.08). Coverage per eighth of a Needs-you card measured on the
/// board screenshot (text included) ran 0.73 → 0.08.
#[test]
fn approval_wash_thins_to_the_right_like_the_boards() {
    for (width, height) in [(356.0, 118.0), (720.0, 232.0)] {
        let spec = DitherSpec::new(width, height, 1.0, Gradient::Amber, 1.0);
        let mask = spec.mask();
        let (w, h) = spec.pixels();
        let coverage = |band: u32| {
            let (x0, x1) = (band * w / 8, (band + 1) * w / 8);
            let on = (0..h)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|(x, y)| mask[(*y * w + *x) as usize])
                .count();
            on as f32 / ((x1 - x0) * h) as f32
        };
        let bands: Vec<f32> = (0..8).map(coverage).collect();
        assert!(bands[0] > 0.5 && bands[0] < 0.8, "{bands:?}");
        assert!(bands[7] < 0.12, "{bands:?}");
        assert!(bands.windows(2).all(|p| p[1] <= p[0] + 0.02), "{bands:?}");
    }
}
