use std::{sync::Arc, time::Instant};

use anyhow::Result;
use gecko_core::diagnostics::LogBuffer;
use gecko_editor::Editor;
use gecko_renderer::{
    scene_renderer::SceneRenderer,
    surface::{Frame, Surface},
};
use gecko_rhi::{Rhi, context::ContextConfig};
use gecko_runtime::Scene;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

struct EngineState {
    window: Arc<Window>,
    rhi: Rhi,
    surface: Surface,
    editor: Editor,
    scene: Scene,
    scene_renderer: SceneRenderer,

    last_frame: Instant,
    fps_accumulator: f32,
    fps_frame_count: u32,
}

impl EngineState {
    fn new(event_loop: &ActiveEventLoop, log_buffer: Arc<LogBuffer>) -> Result<Self> {
        let window: Arc<Window> = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Gecko Engine")
                    .with_inner_size(LogicalSize::new(1600.0, 900.0)),
            )?,
        );

        let (rhi, raw_surface) = Rhi::new(&ContextConfig::default(), window.clone())?;

        let PhysicalSize { width, height } = window.inner_size();
        let surface = Surface::new(&rhi, raw_surface, width, height);

        let editor = Editor::new(&rhi, &surface, &window, log_buffer)?;

        let scene = Scene::new();
        let scene_renderer = SceneRenderer::new(&rhi.device(), surface.format());

        tracing::info!(width, height, "initialized");

        Ok(Self {
            window,
            rhi,
            surface,
            editor,
            scene,
            scene_renderer,

            last_frame: Instant::now(),
            fps_accumulator: 0.0,
            fps_frame_count: 0,
        })
    }

    fn redraw(&mut self) -> Result<()> {
        let _frame = tracing::debug_span!("frame").entered();

        let now = Instant::now();
        let delta_time = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        self.fps_accumulator += delta_time;
        self.fps_frame_count += 1;

        if self.fps_accumulator >= 1.0 {
            let fps = self.fps_frame_count as f32 / self.fps_accumulator;
            let frame_time_ms = self.fps_accumulator * 1000.0 / self.fps_frame_count as f32;

            tracing::info!(fps = format!("{fps:.1}"), frame_time_ms = format!("{frame_time_ms:.2}"));

            self.fps_accumulator = 0.0;
            self.fps_frame_count = 0;
        }

        self.editor.begin_frame_maintenance(&self.rhi, self.surface.format());

        self.scene.update(delta_time);

        let (surface_texture, reconfigure_after_present) = match self.surface.acquire_frame()? {
            Frame::Ready(surface_texture, reconfigure) => (surface_texture, reconfigure),
            Frame::Skip => return Ok(()),
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .rhi
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        {
            let _span = tracing::debug_span!("game_pass").entered();

            let viewport = &self.editor.viewport;
            let view_proj = self.scene.camera.proj(viewport.aspect()) * self.scene.camera.view();
            self.scene_renderer.render(
                &mut encoder,
                &self.rhi.queue(),
                &viewport.color_view,
                &viewport.depth_view,
                view_proj,
                &self.scene.draw_list(),
                self.scene.show_grid,
            );
        }

        {
            let _span = tracing::debug_span!("editor_ui_pass").entered();

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor_ui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.07,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.editor.render(
                &self.window,
                &mut self.scene,
                &mut render_pass,
                self.surface.width(),
                self.surface.height(),
            )?;
        }

        tracing::debug_span!("queue_submit").in_scope(|| self.rhi.queue().submit(Some(encoder.finish())));

        self.surface.present(surface_texture, reconfigure_after_present);

        self.editor.update_platform_windows();

        Ok(())
    }
}

impl Drop for EngineState {
    fn drop(&mut self) {
        self.rhi.context().wait_idle();
    }
}

struct App {
    state: Option<EngineState>,
    log_buffer: Arc<LogBuffer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match EngineState::new(event_loop, self.log_buffer.clone()) {
            Ok(state) => {
                state.window.request_redraw();
                self.state = Some(state);

                #[cfg(feature = "multi-viewport")]
                if let Some(state) = self.state.as_mut() {
                    state.editor.install_viewport_callbacks().unwrap();
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "failed to initialise");
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(state) = &self.state else { return };
        state.window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        let is_main_window = window_id == state.window.id();

        state.editor.handle_window_event(&state.window, window_id, &event);

        match event {
            WindowEvent::CloseRequested if is_main_window => event_loop.exit(),
            WindowEvent::Resized(size) if is_main_window => state.surface.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } if is_main_window => {
                let PhysicalSize { width, height } = state.window.inner_size();
                state.surface.resize(width, height);
            }
            WindowEvent::RedrawRequested if is_main_window => {
                #[cfg(feature = "multi-viewport")]
                let _event_loop_guard = gecko_editor::set_event_loop_for_frame(event_loop);

                if let Err(e) = state.redraw() {
                    tracing::error!(error = ?e, "failed to redraw");
                }

                if state.editor.wants_quit() {
                    event_loop.exit();
                    return;
                }

                state.window.request_redraw();
            }
            _ => {}
        }
    }
}

pub fn run() -> Result<()> {
    let log_buffer = gecko_core::diagnostics::init();
    tracing::info!(
        multi_viewport = cfg!(feature = "multi-viewport"),
        tracy = cfg!(feature = "tracy"),
        "initializing..."
    );

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        state: None,
        log_buffer,
    };
    event_loop.run_app(&mut app)?;

    Ok(())
}
