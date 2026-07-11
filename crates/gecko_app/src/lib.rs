use std::{sync::Arc, time::Instant};

use anyhow::Result;
use gecko_editor::Editor;
use gecko_renderer::gpu::{Frame, Gpu};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

struct EngineState {
    window: Arc<Window>,
    gpu: Gpu,
    editor: Editor,

    last_frame: Instant,
    fps_accumulator: f32,
    fps_frame_count: u32,
}

impl EngineState {
    fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let window: Arc<Window> = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Gecko Engine")
                    .with_inner_size(LogicalSize::new(1600.0, 900.0)),
            )?,
        );

        let PhysicalSize { width, height } = window.inner_size();
        let gpu = Gpu::new(window.clone(), width, height)?;

        let editor = Editor::new(&gpu, &window)?;

        tracing::info!(adapter = %gpu.adapter.get_info().name, backend = ?gpu.adapter.get_info().backend, width, height, "initialized");

        Ok(Self {
            window,
            gpu,
            editor,

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

            tracing::info!(fps, frame_time_ms);

            self.fps_accumulator = 0.0;
            self.fps_frame_count = 0;
        }

        let (frame, reconfigure_after_present) = match self.gpu.acquire_frame()? {
            Frame::Ready(frame, reconfigure) => (frame, reconfigure),
            Frame::Skip => return Ok(()),
        };

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame_encoder"),
        });

        // {
        //     let _span = tracing::debug_span!("game_pass").entered();

        //     let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        //         label: Some("ui_pass"),
        //         color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        //             view: &view,
        //             resolve_target: None,
        //             ops: wgpu::Operations {
        //                 load: wgpu::LoadOp::Clear(wgpu::Color {
        //                     r: 0.06,
        //                     g: 0.07,
        //                     b: 0.09,
        //                     a: 1.0,
        //                 }),
        //                 store: wgpu::StoreOp::Store,
        //             },
        //             depth_slice: None,
        //         })],
        //         depth_stencil_attachment: None,
        //         timestamp_writes: None,
        //         occlusion_query_set: None,
        //         multiview_mask: None,
        //     });
        // }

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
                &mut render_pass,
                self.gpu.surface_config.width,
                self.gpu.surface_config.height,
            )?;
        }

        self.gpu.queue.submit(Some(encoder.finish()));
        self.gpu.queue.present(frame);

        if reconfigure_after_present {
            self.gpu.surface.configure(&self.gpu.device, &self.gpu.surface_config);
        }

        Ok(())
    }
}

struct App {
    state: Option<EngineState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match EngineState::new(event_loop) {
            Ok(state) => {
                state.window.request_redraw();
                self.state = Some(state);
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

        state.editor.handle_window_event(&state.window, &event);

        match event {
            WindowEvent::CloseRequested if is_main_window => event_loop.exit(),
            WindowEvent::Resized(size) if is_main_window => state.gpu.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } if is_main_window => {
                let PhysicalSize { width, height } = state.window.inner_size();
                state.gpu.resize(width, height);
            }
            WindowEvent::RedrawRequested if is_main_window => {
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
    gecko_core::diagnostics::init();
    tracing::info!(tracy = cfg!(feature = "tracy"), "initializing...");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App { state: None };
    event_loop.run_app(&mut app)?;

    Ok(())
}
