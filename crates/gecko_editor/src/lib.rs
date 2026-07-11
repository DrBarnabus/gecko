use std::sync::Arc;

use anyhow::Result;
use dear_imgui_rs::{
    Context, DockLayout, DockLayoutApply, DockLayoutError, DockNodeFlags, DockSplit, DockspaceTarget, Id, Ui,
    WindowClass,
};
use dear_imgui_wgpu::{GammaMode, WgpuInitInfo, WgpuRenderer};
use dear_imgui_winit::{HiDpiMode, WinitPlatform};
use gecko_renderer::gpu::Gpu;
use winit::{event::WindowEvent, window::Window};

pub struct Editor {
    pub imgui: Context,
    platform: WinitPlatform,
    pub renderer: WgpuRenderer,
    quit_requested: bool,
}

impl Editor {
    pub fn new(gpu: &Gpu, window: &Arc<Window>) -> Result<Self> {
        let mut imgui = Context::create();
        imgui.set_ini_filename(None::<String>).ok();

        let io = imgui.io_mut();
        let mut flags = io.config_flags();
        flags.insert(dear_imgui_rs::ConfigFlags::DOCKING_ENABLE);
        io.set_config_flags(flags);

        let mut platform = WinitPlatform::new(&mut imgui);
        platform.attach_window(window, HiDpiMode::Default, &mut imgui);

        let init_info = WgpuInitInfo::new(gpu.device.clone(), gpu.queue.clone(), gpu.surface_config.format)
            .with_instance(gpu.instance.clone())
            .with_adapter(gpu.adapter.clone());

        let mut renderer = WgpuRenderer::new(init_info, &mut imgui)?;
        renderer.set_gamma_mode(GammaMode::Auto);

        let editor = Self {
            imgui,
            platform,
            renderer,
            quit_requested: false,
        };

        Ok(editor)
    }

    pub fn handle_window_event(&mut self, window: &Arc<Window>, event: &WindowEvent) {
        self.platform.handle_window_event(&mut self.imgui, window, event);
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
