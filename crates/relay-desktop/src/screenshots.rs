//! Offscreen screenshots of the shell over the demo orchestration, for
//! visual review against `design/orchestrator-ui/screenshots`. Run with a
//! GPU adapter, e.g. Mesa lavapipe in `nix develop .#ui-test`:
//!
//! ```sh
//! cargo test -p relay-desktop shell_screenshots -- --ignored --nocapture
//! ```

use crate::{model::Page, network::NetworkState, ui};
use mosaic::{core::reactive::flush, prelude::*, text::FontContext};
use relay_core::*;
use relay_ui::tokens::{ThemeVariant, theme};
use std::time::Duration;

fn render(ui: &Ui, logical: Size, scale: f32, path: &str) {
    use mosaic::render::Renderer;
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
    let scene = ui.scene().clone();
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
    image::RgbaImage::from_raw(width, height, pixels)
        .unwrap()
        .save(path)
        .unwrap();
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "manual: needs a GPU adapter; writes screenshots"]
fn shell_screenshots() {
    let directory = std::env::var("RELAY_SCREENSHOTS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/relay-desktop-shots"
        )
        .into()
    });
    std::fs::create_dir_all(&directory).unwrap();
    let only = std::env::var("RELAY_SHOT").unwrap_or_default();
    let logical = Size::new(1440.0, 960.0);
    mosaic::core::builtins::install();
    install_theme(&theme(ThemeVariant::Dark, 1.0));
    install_theme(&relay_ui::icons::icons());
    install_theme(&crate::icons::icons());
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    let mut fonts = FontContext::embedded_only();
    relay_ui::fonts::install(&mut fonts);
    ui.set_fonts(fonts);
    relay_ui::attach(&ui);
    let (sender, _commands) = tokio::sync::mpsc::unbounded_channel();
    let model = scope.run(|| crate::model::Model::new(&ui, sender));
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    demo_orchestration(&mut snapshot);
    let now = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    demo_usage(&mut snapshot, now);
    model.receive(NetworkState {
        snapshot,
        connected: true,
        status: "Connected".into(),
        round_trip_ms: Some(42),
        ..Default::default()
    });
    model.server_endpoint.set("http://127.0.0.1:7331".into());
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&ui::shell(model));
    });
    let settle = || {
        flush();
        for _ in 0..20 {
            ui.tick(Duration::from_millis(16));
            flush();
            ui.frame(logical, 1.0);
        }
    };
    let shots: [(&str, &dyn Fn()); 4] = [
        ("conversation", &|| {
            model.open_session("session-plan".into())
        }),
        ("manager", &|| {
            model.open_session("session-migration".into())
        }),
        ("board", &|| {
            model.page.set(Page::Board);
            model.issue.set(Some("issue-2".into()));
        }),
        ("settings", &|| model.page.set(Page::Settings)),
    ];
    for (name, open) in shots {
        if !only.is_empty() && only != name {
            continue;
        }
        scope.run(open);
        settle();
        render(&ui, logical, 1.0, &format!("{directory}/{name}.png"));
    }
    scope.dispose();
}
