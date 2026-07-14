use dear_imgui_rs::Context;

pub const UI_FONT_SIZE: f32 = 16.0;

const OPEN_SANS_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/OpenSans-Regular.ttf");

pub fn load_fonts(imgui: &mut Context) {
    let loaded = imgui
        .fonts()
        .add_font_from_memory_ttf(OPEN_SANS_REGULAR, UI_FONT_SIZE, None, None)
        .is_some();

    assert!(loaded, "failed to load bundled Open Sans UI font");
}
