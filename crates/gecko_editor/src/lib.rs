use std::sync::Arc;

use anyhow::Result;
use dear_imgui_rs::{
    Context, DockLayout, DockLayoutApply, DockLayoutError, DockNodeFlags, DockSplit, DockspaceTarget, Id, Ui,
    WindowClass,
};
use dear_imgui_wgpu::{GammaMode, WgpuInitInfo, WgpuRenderer};
use dear_imgui_winit::{HiDpiMode, WinitPlatform};
use gecko_renderer::gpu::Gpu;
use winit::{
    event::WindowEvent,
    window::{Window, WindowId},
};

#[cfg(feature = "multi-viewport")]
use dear_imgui_winit::multi_viewport as winit_mvp;

#[cfg(feature = "multi-viewport")]
pub use winit_mvp::set_event_loop_for_frame;

pub struct Editor {
    pub imgui: Context,
    platform: WinitPlatform,
    pub renderer: WgpuRenderer,
    viewports_enabled: bool,

    quit_requested: bool,
}

impl Drop for Editor {
    fn drop(&mut self) {
        #[cfg(feature = "multi-viewport")]
        if self.viewports_enabled {
            winit_mvp::shutdown_multi_viewport_support(&mut self.imgui);
        }
    }
}

impl Editor {
    pub fn new(gpu: &Gpu, window: &Arc<Window>) -> Result<Self> {
        let viewports_enabled = cfg!(feature = "multi-viewport")
            && cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux"));

        let mut imgui = Context::create();
        imgui.set_ini_filename(None::<String>).ok();

        let io = imgui.io_mut();
        let mut flags = io.config_flags();

        #[cfg(feature = "multi-viewport")]
        if viewports_enabled {
            flags.insert(dear_imgui_rs::ConfigFlags::VIEWPORTS_ENABLE);
        }

        flags.insert(dear_imgui_rs::ConfigFlags::DOCKING_ENABLE);

        io.set_config_flags(flags);

        let mut platform = WinitPlatform::new(&mut imgui);
        platform.attach_window(window, HiDpiMode::Default, &mut imgui);

        let init_info = WgpuInitInfo::new(gpu.device.clone(), gpu.queue.clone(), gpu.surface_config.format)
            .with_instance(gpu.instance.clone())
            .with_adapter(gpu.adapter.clone());

        let mut renderer = WgpuRenderer::new(init_info, &mut imgui)?;
        renderer.set_gamma_mode(GammaMode::Auto);

        let mut editor = Self {
            imgui,
            platform,
            renderer,
            viewports_enabled,

            quit_requested: false,
        };

        #[cfg(feature = "multi-viewport")]
        if editor.viewports_enabled {
            winit_mvp::init_multi_viewport_support(&mut editor.imgui, window);
        }

        Ok(editor)
    }

    #[cfg(feature = "multi-viewport")]
    pub fn install_viewport_callbacks(&mut self) -> Result<()> {
        if self.viewports_enabled {
            unsafe { dear_imgui_wgpu::multi_viewport::enable(&mut self.renderer, &mut self.imgui)? };
        }

        Ok(())
    }

    pub fn handle_window_event(&mut self, window: &Arc<Window>, window_id: WindowId, event: &WindowEvent) {
        #[cfg(feature = "multi-viewport")]
        {
            let full: winit::event::Event<()> = winit::event::Event::WindowEvent {
                window_id,
                event: event.clone(),
            };

            let _ = winit_mvp::handle_event_with_multi_viewport(&mut self.platform, &mut self.imgui, window, &full);
        }

        #[cfg(not(feature = "multi-viewport"))]
        {
            let _ = window_id;
            self.platform.handle_window_event(&mut self.imgui, window, event);
        }
    }

    pub fn wants_quit(&self) -> bool {
        self.quit_requested
    }

    pub fn render(
        &mut self,
        window: &Window,
        render_pass: &mut wgpu::RenderPass<'_>,
        framebuffer_width: u32,
        framebuffer_height: u32,
    ) -> Result<()> {
        self.platform.prepare_frame(window, &mut self.imgui);

        let ui = self.imgui.frame();

        build_dock_layout(ui)?;

        let mut quit_requested = false;

        if let Some(_bar) = ui.begin_main_menu_bar() {
            ui.menu("File", || {
                if ui.menu_item("Quit") {
                    quit_requested = true;
                }
            });
        }

        if quit_requested {
            self.quit_requested = true;
        }

        ui.window("Hierarchy").build(|| ui.text("Hierarchy"));
        ui.window("Inspector").build(|| ui.text("Inspector"));
        ui.window("Console").build(|| ui.text("Console"));

        let game_class = WindowClass::default().dock_node_flags_override_set(DockNodeFlags::AUTO_HIDE_TAB_BAR);
        ui.set_next_window_class(&game_class);
        ui.window("Game").build(|| ui.text("Game"));

        self.renderer.new_frame()?;
        self.renderer.render_context_with_fb_size(
            &mut self.imgui,
            render_pass,
            framebuffer_width,
            framebuffer_height,
        )?;

        Ok(())
    }

    pub fn update_platform_windows(&mut self) {
        #[cfg(feature = "multi-viewport")]
        if self.viewports_enabled {
            self.imgui.update_platform_windows();
            self.imgui.render_platform_windows_default();
        }
    }
}

fn build_dock_layout(ui: &Ui) -> Result<Id, DockLayoutError> {
    let layout = DockLayout::split(
        DockSplit::Down,
        0.25,
        DockLayout::tabs(["Console"]),
        DockLayout::split(
            DockSplit::Right,
            0.25,
            DockLayout::split(
                DockSplit::Down,
                0.5,
                DockLayout::tabs(["Inspector"]),
                DockLayout::tabs(["Hierarchy"]),
            ),
            DockLayout::tabs(["Game"]),
        ),
    );

    let dockspace_id = ui.get_id("MainDockSpace");
    let main_viewport = ui.main_viewport();

    let target = DockspaceTarget::new(dockspace_id, main_viewport.work_pos(), main_viewport.work_size())?
        .flags(DockNodeFlags::PASSTHRU_CENTRAL_NODE);

    ui.dockspace_over_main_viewport_with_layout(&target, &layout, DockLayoutApply::IfMissing)
}
