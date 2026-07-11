use std::sync::Arc;

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

struct EngineState {
    window: Arc<Window>,
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

        tracing::info!("initialized");

        Ok(Self { window })
    }

    fn redraw(&mut self) -> Result<()> {
        let _frame = tracing::debug_span!("frame").entered();

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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        let is_main_window = window_id == state.window.id();

        match event {
            WindowEvent::CloseRequested if is_main_window => event_loop.exit(),
            WindowEvent::Resized(_size) if is_main_window => {}
            WindowEvent::ScaleFactorChanged { .. } if is_main_window => {}
            WindowEvent::RedrawRequested if is_main_window => {
                if let Err(e) = state.redraw() {
                    tracing::error!(error = ?e, "failed to redraw");
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
