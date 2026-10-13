//! Manual offscreen animation capture using the actual component renderer.

use crate::prelude::*;
use crate::tests::mount;
use crate::tokens::{ground, model, status};
use mosaic::prelude::*;
use std::time::Duration;

#[test]
#[ignore = "manual: needs a GPU adapter; writes animation frames"]
fn flowing_dither_preview() {
    let directory = std::env::var("RELAY_DITHER_PREVIEW").expect("Set RELAY_DITHER_PREVIEW");
    std::fs::create_dir_all(&directory).unwrap();
    let size = Size::new(640.0, 240.0);
    let mounted = mount(size, || {
        view! {
            col fill:ground.page pad:20px gap:14px {
                stack height:48px clip {
                    Dither colour:(status.live) flowing:true opacity:0.3
                    row align:center pad:12px {
                        Label text:"Thinking" role:(TextRole::Label)
                    }
                }
                stack height:56px clip {
                    Dither colour:(model.sol) gradient:(Gradient::Corner) flowing:true
                        breath:(Some(crate::motion::Breath::GLOW)) opacity:0.6
                    row align:center pad:12px {
                        SerifTitle text:"Sol 6.1" size:(SerifSize::Large)
                    }
                }
                ToolCallRow
                    data:(ToolCallView {
                    kind:crate::tokens::ToolKind::Run, status:AgentStatus::Working,
                    verb:"Run".into(), target:Some("cargo test".into()),
                    opening:Opening::Collapsed, detail:None,
                })
                    on-opening:(std::rc::Rc::new(|_| {}) as OnOpening)
                UsageBar window:(crate::usage::UsageWindow::new(62.0, Duration::from_secs(18000)))
            }
        }
    });
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut renderer = mosaic::render_wgpu::WgpuRenderer::from_device(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
        size,
    );
    let (width, height) = (size.width as u32, size.height as u32);
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for frame in 0..150 {
        mounted.ui.tick(Duration::from_millis(40));
        mounted.ui.frame(size, 1.0);
        let scene = mounted.ui.scene();
        queue.submit([renderer.encode_into(
            &scene,
            &view,
            mosaic::render_wgpu::TargetLoad::Clear(scene.clear),
        )]);
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
            extent,
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
            .save(format!("{directory}/frame-{frame:03}.png"))
            .unwrap();
        drop(mapped);
        buffer.unmap();
    }
}
