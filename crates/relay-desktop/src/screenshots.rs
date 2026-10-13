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
    let (sender, commands) = tokio::sync::mpsc::unbounded_channel();
    let commands = std::cell::RefCell::new(commands);
    let receipt = std::cell::RefCell::new(None::<String>);
    let outcome_serial = std::cell::Cell::new(0u64);
    let model = scope.run(|| crate::model::Model::new(&ui, sender));
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    // Demo conversations predate project connections. Include real resource
    // rows here so Connections screenshots exercise their embedded actions.
    let project_id = snapshot.projects[0].id.clone();
    snapshot.connections = vec![
        ProjectConnection {
            id: "shot-workspace".into(),
            project_id: project_id.clone(),
            name: "Workspace".into(),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Directory {
                path: "/srv/relay/workspace".into(),
            },
        },
        ProjectConnection {
            id: "shot-repository".into(),
            project_id: project_id.clone(),
            name: "jens-hj/relay".into(),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Repository {
                remote: "https://github.com/jens-hj/relay.git".into(),
                checkout: Some("/srv/relay/repos/github.com/jens-hj/relay".into()),
                owned: true,
            },
        },
    ];
    demo_orchestration(&mut snapshot);
    let now = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    demo_usage(&mut snapshot, now);
    // What the server answers for the example sessions' discovery.
    model.catalogs.set(std::collections::BTreeMap::from([
        (
            "session-plan".to_string(),
            Ok(demo_catalog("session-plan", Harness::ClaudeCode)),
        ),
        (
            "session-backfill".to_string(),
            Ok(demo_catalog("session-backfill", Harness::Codex)),
        ),
    ]));
    model.receive(NetworkState {
        snapshot,
        connected: true,
        status: "Connected".into(),
        round_trip_ms: Some(42),
        ..Default::default()
    });
    // A selected single shot must not depend on an earlier conversation
    // opening the demo project.
    model.project.set(project_id);
    model.server_endpoint.set("http://127.0.0.1:7331".into());
    // One harness ready and one needing a sign-in, so both availability
    // colours show.
    model.harnesses.set(
        [
            (Harness::Codex, "ready"),
            (Harness::ClaudeCode, "signed_out"),
        ]
        .into_iter()
        .map(|(harness, state)| HarnessStatus {
            harness,
            executable: String::new(),
            version: Some("1.0.0".into()),
            state: state.into(),
            detail: String::new(),
            checked_at: now,
        })
        .collect(),
    );
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
    let submit_shot = || {
        model.open_session("session-plan".into());
        let parts = vec![Part::text("Please check the next turn.")];
        crate::buffer::edit(model, "session-plan", parts.clone());
        model.submit(
            Command::SubmitTurn {
                session_id: "session-plan".into(),
                draft_revision: 0,
                parts: parts.clone(),
                approve_implementation: true,
            },
            model.snapshot.get_untracked().revision,
            crate::model::Saved::Buffer(Draft {
                session_id: "session-plan".into(),
                revision: 0,
                parts,
            }),
        );
        let request = commands.borrow_mut().try_recv().unwrap();
        *receipt.borrow_mut() = Some(request.request_id.clone());
        request.request_id
    };
    let shots: [(&str, &dyn Fn()); 20] = [
        ("notice-command-feedback", &|| {
            model.open_session("session-plan".into());
            model.command_feedback.set("Configure this feature on the server; the harness does not expose its interactive editor here".into());
        }),
        ("notice-sending", &|| {
            submit_shot();
        }),
        ("notice-conflict", &|| {
            let id = submit_shot();
            outcome_serial.set(outcome_serial.get() + 1);
            model.receive(NetworkState {
                snapshot: model.snapshot.get_untracked(),
                connected: true,
                outcome: Some((
                    id,
                    Err("Revision conflict; review latest state before retrying.".into()),
                )),
                outcome_serial: outcome_serial.get(),
                outcome_conflict: true,
                ..Default::default()
            });
        }),
        ("composer-reply", &|| {
            composer_reply_shot(model, &ui, false, &settle)
        }),
        ("composer-reply-collapsed", &|| {
            composer_reply_shot(model, &ui, true, &settle)
        }),
        ("command-model", &|| {
            model.open_session("session-plan".into());
            crate::command_ui::open(model, "model");
        }),
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
        ("profile", &|| {
            model.open_profile(crate::model::EditTarget::Director("director-main".into()))
        }),
        ("new-director", &|| {
            model.open_profile(crate::model::EditTarget::New)
        }),
        ("connections", &|| model.page.set(Page::Connections)),
        ("palette", &|| {
            model.open_session("session-plan".into());
            model.palette.set(true);
        }),
        ("connection-collapsed", &|| {
            connection_shot(model, &ui, ConnectionShot::Connected, false, &settle)
        }),
        ("connection-expanded", &|| {
            connection_shot(model, &ui, ConnectionShot::Connected, true, &settle)
        }),
        ("connection-reconnecting", &|| {
            connection_shot(model, &ui, ConnectionShot::Reconnecting, true, &settle)
        }),
        ("connection-offline", &|| {
            connection_shot(model, &ui, ConnectionShot::Offline, true, &settle)
        }),
        ("transcript-wrap", &|| {
            model.snapshot.update(|snapshot| {
                snapshot.messages.retain(|m| m.session_id != "session-plan");
                for (id, kind, body) in [
                    (
                        "wrap-journal",
                        "prompt",
                        format!(
                            "journalctl --user\n{}",
                            format!(
                                "Oct 10 systemd: /nix/store/{} /home/user/workspace/deploy.py\n",
                                "long-unbroken-path".repeat(7)
                            )
                            .repeat(3)
                        ),
                    ),
                    (
                        "wrap-context",
                        "issue-context",
                        "Context sent to agent follows the journal.".into(),
                    ),
                    (
                        "wrap-fence",
                        "assistant",
                        "Completed.\n```bash\njournalctl --user -fu relay\n```".into(),
                    ),
                    (
                        "wrap-reply",
                        "assistant",
                        "Completion is reported after checks pass.".into(),
                    ),
                ] {
                    snapshot.messages.push(Message {
                        id: id.into(),
                        session_id: "session-plan".into(),
                        author: "Relay".into(),
                        kind: kind.into(),
                        body,
                        parts: vec![],
                        tool: None,
                    });
                }
            });
            model.open_session("session-plan".into());
        }),
        ("projects", &|| {
            model.open_session("session-plan".into());
            settle();
            let position = ui
                .inspection_snapshot()
                .nodes
                .iter()
                .find(|n| n.label.as_deref() == Some("Toggle project Relay · demo"))
                .unwrap()
                .rect
                .center();
            for kind in [
                PointerEventKind::Down(PointerButton::Primary),
                PointerEventKind::Up(PointerButton::Primary),
            ] {
                ui.dispatch_pointer(PointerEvent {
                    kind,
                    position,
                    pointer_type: PointerType::Mouse,
                    modifiers: Modifiers::default(),
                    timestamp: Duration::ZERO,
                });
            }
        }),
    ];
    for (name, open) in shots {
        if !only.is_empty()
            && only != name
            && !(only == "connection" && name.starts_with("connection-"))
        {
            continue;
        }
        scope.run(open);
        settle();
        render(&ui, logical, 1.0, &format!("{directory}/{name}.png"));
        if name.starts_with("notice-") {
            scope.run(|| {
                if model.busy.get_untracked() {
                    outcome_serial.set(outcome_serial.get() + 1);
                    model.receive(NetworkState {
                        snapshot: model.snapshot.get_untracked(),
                        connected: true,
                        outcome: receipt.borrow_mut().take().map(|id| (id, Ok(()))),
                        outcome_serial: outcome_serial.get(),
                        ..Default::default()
                    });
                } else if model.can_rebase() {
                    model.rebase_conflict();
                }
                model.command_feedback.set(String::new());
                model.notice.set(String::new());
                crate::buffer::edit(model, "session-plan", vec![]);
            });
        }
        if name.starts_with("composer-reply") {
            let snapshot = ui.inspection_snapshot();
            let source = image::open(format!("{directory}/{name}.png")).unwrap();
            for (suffix, rect) in [
                (
                    "reply",
                    snapshot
                        .nodes
                        .iter()
                        .rev()
                        .find(|n| n.label.as_deref() == Some("Draft reply strip"))
                        .unwrap()
                        .rect,
                ),
                (
                    "glow",
                    snapshot
                        .nodes
                        .iter()
                        .find(|n| {
                            n.label.as_deref().is_some_and(|label| {
                                label.starts_with("Model:") || label.starts_with("Agent settings:")
                            })
                        })
                        .unwrap()
                        .rect,
                ),
            ] {
                source
                    .crop_imm(
                        rect.origin.x.floor() as u32,
                        rect.origin.y.floor() as u32,
                        rect.size.width.ceil() as u32,
                        rect.size.height.ceil() as u32,
                    )
                    .save(format!("{directory}/{name}-{suffix}.png"))
                    .unwrap();
            }
            scope.run(|| {
                crate::buffer::edit(model, "session-plan", vec![]);
                set_ledger_collapsed(&ui, false);
            });
        }
        if name == "command-model" {
            scope.run(|| model.command_flow.set(String::new()));
        }
    }
    scope.dispose();
}

fn composer_reply_shot(model: crate::model::Model, ui: &Ui, collapsed: bool, settle: &impl Fn()) {
    model.open_session("session-plan".into());
    let source = model
        .snapshot
        .get_untracked()
        .messages
        .into_iter()
        .find(|m| m.session_id == "session-plan" && m.tool.is_none())
        .unwrap();
    let quote = source.body.lines().next().unwrap().to_owned();
    crate::buffer::edit(
        model,
        "session-plan",
        vec![relay_core::Part {
            id: "screenshot-reply".into(),
            kind: relay_core::PartKind::Reply {
                anchor: relay_core::Anchor {
                    message_id: source.id,
                    start: 0,
                    end: quote.len(),
                    quote,
                },
                parts: vec![relay_core::Part::text("Keep this in the next turn.")],
            },
        }],
    );
    settle();
    set_ledger_collapsed(ui, collapsed);
}

fn set_ledger_collapsed(ui: &Ui, collapsed: bool) {
    let label = if collapsed {
        "Collapse agent settings"
    } else {
        "Expand agent settings"
    };
    if let Some(control) = ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some(label))
    {
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            ui.dispatch_pointer(PointerEvent {
                kind,
                position: control.rect.center(),
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
    }
}

enum ConnectionShot {
    Connected,
    Reconnecting,
    Offline,
}
fn connection_shot(
    model: crate::model::Model,
    ui: &Ui,
    state: ConnectionShot,
    expanded: bool,
    settle: &dyn Fn(),
) {
    model.palette.set(false);
    model.open_session("session-plan".into());
    let now = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    model
        .connected
        .set(matches!(state, ConnectionShot::Connected));
    model.status.set(
        match state {
            ConnectionShot::Connected => "Connected",
            ConnectionShot::Reconnecting => "Connection lost · retrying",
            ConnectionShot::Offline => "Offline",
        }
        .into(),
    );
    model
        .round_trip_ms
        .set(matches!(state, ConnectionShot::Connected).then_some(67));
    model.connection_telemetry.set((
        vec![
            57, 62, 55, 68, 81, 60, 59, 52, 74, 110, 63, 55, 62, 72, 59, 65, 54, 81, 67, 67,
        ],
        Some(now - 900),
        matches!(state, ConnectionShot::Reconnecting).then_some(now + 4),
    ));
    model.server_build.set(Some(BuildInfo {
        version: "0.1.0".into(),
        revision: "9c1f0a2".into(),
    }));
    settle();
    let button = ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|label| label.starts_with("Connection:") && label.ends_with("details"))
        })
        .unwrap();
    let open = ui
        .inspection_snapshot()
        .nodes
        .iter()
        .any(|node| node.label.as_deref() == Some("Connection details"));
    if expanded != open {
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            ui.dispatch_pointer(PointerEvent {
                kind,
                position: button.rect.center(),
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
    }
    settle();
    assert_eq!(
        ui.inspection_snapshot()
            .nodes
            .iter()
            .any(|node| node.label.as_deref() == Some("Connection details")),
        expanded,
        "connection screenshot must show the requested opening"
    );
}
