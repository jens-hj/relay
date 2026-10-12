//! Headless harness for the composite tests: mounting, input, queries, and
//! offscreen screenshots.

use crate::tokens::{ThemeVariant, theme};
use mosaic::{core::reactive::flush, prelude::*, text::FontContext};
use std::time::Duration;

pub struct Mounted {
    scope: Scope,
    pub ui: Ui,
    pub size: Size,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

pub fn mount(size: Size, build: impl FnOnce() -> Element) -> Mounted {
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
    pub fn settle(&self) {
        flush();
        for _ in 0..12 {
            self.ui.tick(Duration::from_millis(16));
            flush();
            if self.ui.frame(self.size, 1.0).is_none() {
                break;
            }
        }
    }

    pub fn labels(&self) -> Vec<String> {
        self.ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .filter_map(|node| node.label)
            .collect()
    }

    pub fn has(&self, label: &str) -> bool {
        self.labels().iter().any(|each| each == label)
    }

    pub fn count(&self, label: &str) -> usize {
        self.labels().iter().filter(|each| *each == label).count()
    }

    /// Every rect labelled `label`, in tree order.
    pub fn rects(&self, label: &str) -> Vec<Rect> {
        self.ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|node| node.label.as_deref() == Some(label))
            .map(|node| node.rect)
            .collect()
    }

    pub fn rect(&self, label: &str) -> Rect {
        *self
            .rects(label)
            .first()
            .unwrap_or_else(|| panic!("missing {label}: {:?}", self.labels()))
    }

    pub fn click(&self, position: Vector2) {
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

    pub fn click_label(&self, label: &str) {
        self.click(self.rect(label).center());
    }

    pub fn hover(&self, position: Vector2) {
        self.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        self.settle();
    }

    pub fn key(&self, key: Key) {
        self.ui.dispatch_key(KeyEvent {
            kind: KeyEventKind::Down { repeat: false },
            key,
            modifiers: Modifiers::default(),
        });
        self.settle();
    }

    /// The label of the focused element.
    pub fn focused_label(&self) -> Option<String> {
        let focused = self.ui.focused()?;
        let id = focused.id();
        self.ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|node| node.id == id)
            .and_then(|node| node.label)
    }
}

/// Renders `build` offscreen in the dark theme and writes
/// `RELAY_UI_SCREENSHOTS/<name>.png` (default `target/relay-ui-gallery`).
pub fn screenshot(name: &str, logical: Size, build: impl FnOnce() -> Element) {
    use mosaic::render::Renderer;
    let directory = std::env::var("RELAY_UI_SCREENSHOTS").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/relay-ui-gallery").into()
    });
    std::fs::create_dir_all(&directory).unwrap();
    let scale = std::env::var("RELAY_UI_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2.0f32);
    let mounted = mount(logical, build);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let (width, height) = (
        (logical.width * scale) as u32,
        (logical.height * scale) as u32,
    );
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
    let path = format!("{directory}/{name}.png");
    image::RgbaImage::from_raw(width, height, pixels)
        .unwrap()
        .save(&path)
        .unwrap();
    eprintln!("wrote {path}");
}

// The tests module loads the whole gallery; this loads only its composites
// page, for a screenshot of that page alone.
#[allow(clippy::duplicate_mod)]
#[path = "../examples/gallery/agents.rs"]
mod agents_gallery;

/// Renders the orchestration composites page of the gallery offscreen and
/// writes `agents.png` (see [`screenshot`]).
#[test]
#[ignore = "manual: needs a GPU adapter; writes screenshots"]
fn agents_gallery_screenshot() {
    use crate::tokens::ground;
    let height = std::env::var("RELAY_UI_HEIGHT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2400.0f32);
    screenshot("agents", Size::new(1280.0, height), || {
        view! {
            col fill:ground.page font-color:ground.ink pad:(horizontal:36px vertical:28px) {
                {
                    parent.adopt(&agents_gallery::sections());
                }
            }
        }
    });
}
