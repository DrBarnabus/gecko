pub mod ui;
pub mod viewport;

use std::sync::Arc;

use anyhow::Result;
use dear_imgui_rs::{
    Context, DockLayout, DockLayoutApply, DockLayoutError, DockNodeFlags, DockSplit, DockspaceTarget, Id, StyleVar, Ui,
    WindowClass,
};
use dear_imgui_wgpu::{GammaMode, WgpuInitInfo, WgpuRenderer};
use dear_imgui_winit::{HiDpiMode, WinitPlatform};
use gecko_core::diagnostics::LogBuffer;
use gecko_renderer::surface::Surface;
use gecko_rhi::Rhi;
use gecko_runtime::Scene;
use winit::{
    event::WindowEvent,
    window::{Window, WindowId},
};

#[cfg(feature = "multi-viewport")]
use dear_imgui_winit::multi_viewport as winit_mvp;

#[cfg(feature = "multi-viewport")]
pub use winit_mvp::set_event_loop_for_frame;

use crate::{ui::console::Console, viewport::Viewport};

pub struct Editor {
    pub imgui: Context,
    platform: WinitPlatform,
    pub renderer: WgpuRenderer,
    viewports_enabled: bool,

    pub console: Console,
    pub viewport: Viewport,

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
    pub fn new(rhi: &Rhi, surface: &Surface, window: &Arc<Window>, log_buffer: Arc<LogBuffer>) -> Result<Self> {
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

        ui::fonts::load_fonts(&mut imgui);

        let init_info = WgpuInitInfo::new(rhi.device(), rhi.queue(), surface.format())
            .with_instance(rhi.instance())
            .with_adapter(rhi.adapter());

        let mut renderer = WgpuRenderer::new(init_info, &mut imgui)?;
        renderer.set_gamma_mode(GammaMode::Auto);

        ui::theme::set_style(&mut imgui);

        let viewport = Viewport::new(rhi.device(), &mut renderer, surface.format());

        let mut editor = Self {
            imgui,
            platform,
            renderer,
            viewports_enabled,

            console: Console::new(log_buffer),
            viewport,

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

    pub fn begin_frame_maintenance(&mut self, rhi: &Rhi, format: wgpu::TextureFormat) {
        self.viewport.apply_resize(rhi.device(), &mut self.renderer, format);
    }

    #[tracing::instrument(skip_all)]
    pub fn render(
        &mut self,
        window: &Window,
        scene: &mut Scene,
        render_pass: &mut wgpu::RenderPass<'_>,
        framebuffer_width: u32,
        framebuffer_height: u32,
    ) -> Result<()> {
        self.platform.prepare_frame(window, &mut self.imgui);

        let viewport_texture_id = self.viewport.texture_id;
        let mut viewport_desired_size = self.viewport.desired;

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

        let copy_request = self.console.render(ui);
        inspector_render(ui, scene);
        hierarchy_render(ui, scene);

        let game_class = WindowClass::default().dock_node_flags_override_set(DockNodeFlags::AUTO_HIDE_TAB_BAR);
        ui.set_next_window_class(&game_class);
        let padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        ui.window("Game").build(|| {
            let [width, height] = ui.content_region_avail();
            viewport_desired_size = (width.max(1.0) as u32, height.max(1.0) as u32);

            ui.image(viewport_texture_id, [width.max(1.0), height.max(1.0)]);
        });
        padding.pop();

        self.renderer.new_frame()?;
        self.renderer.render_context_with_fb_size(
            &mut self.imgui,
            render_pass,
            framebuffer_width,
            framebuffer_height,
        )?;

        if let Some(text) = copy_request {
            self.imgui.set_clipboard_text(text);
        }

        self.viewport.desired = viewport_desired_size;

        Ok(())
    }

    #[tracing::instrument(skip_all)]
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

fn hierarchy_render(ui: &Ui, scene: &mut Scene) {
    ui.window("Hierarchy").build(|| {
        let mut clicked = None;

        for (idx, cube) in scene.cubes.iter().enumerate() {
            let selected = scene.selected == Some(idx);

            if ui.selectable_config(&cube.name).selected(selected).build() {
                clicked = Some(idx);
            }
        }

        if let Some(idx) = clicked {
            if scene.selected != Some(idx) {
                tracing::debug!(entity = %scene.cubes[idx].name, "selected");
            }

            scene.selected = Some(idx);
        }

        if ui.is_window_hovered() && ui.is_mouse_clicked(dear_imgui_rs::MouseButton::Right) {
            scene.selected = None;
        }
    });
}

fn inspector_render(ui: &Ui, scene: &mut Scene) {
    ui.window("Inspector").build(|| match scene.selected {
        Some(idx) => {
            let cube = &mut scene.cubes[idx];

            ui.text(&cube.name);
            ui.separator();

            ui.text("Position");
            for (axis, value) in ["##px", "##py", "##pz"].iter().zip(cube.position.iter_mut()) {
                ui.set_next_item_width(90.0);
                ui.drag_float_config(axis).speed(0.05).build(ui, value);
                ui.same_line();
            }
            ui.new_line();

            ui.set_next_item_width(120.0);
            ui.drag_float_config("Scale").speed(0.02).build(ui, &mut cube.scale);
            ui.set_next_item_width(120.0);
            ui.drag_float_config("Spin").speed(0.02).build(ui, &mut cube.spin_speed);
            ui.color_edit3("Color", &mut cube.color);
        }
        None => {
            ui.text("Camera");
            ui.separator();

            ui.set_next_item_width(140.0);
            ui.slider_config("Yaw", -3.2, 3.2).build(&mut scene.camera.yaw);
            ui.set_next_item_width(140.0);
            ui.slider_config("Pitch", 0.05, 1.45).build(&mut scene.camera.pitch);
            ui.set_next_item_width(140.0);
            ui.slider_config("Distance", 2.0, 25.0)
                .build(&mut scene.camera.distance);

            ui.checkbox("Show grid", &mut scene.show_grid);
            ui.separator();

            ui.text_disabled("Select a cube in the Hierarchy");
        }
    });
}
