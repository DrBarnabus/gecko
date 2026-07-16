use dear_imgui_rs::{Context, StyleColor, TreeLineMode};

const ZINC_950: [f32; 3] = [0.035, 0.035, 0.043];
const ZINC_900: [f32; 3] = [0.094, 0.094, 0.106];
const ZINC_850: [f32; 3] = [0.122, 0.122, 0.137];
const ZINC_800: [f32; 3] = [0.153, 0.153, 0.165];
const ZINC_700: [f32; 3] = [0.247, 0.247, 0.275];
const ZINC_600: [f32; 3] = [0.322, 0.322, 0.357];
const ZINC_500: [f32; 3] = [0.443, 0.443, 0.478];
const ZINC_400: [f32; 3] = [0.631, 0.631, 0.667];
const ZINC_200: [f32; 3] = [0.894, 0.894, 0.906];
const ZINC_100: [f32; 3] = [0.957, 0.957, 0.961];

const ORANGE_600: [f32; 3] = [0.918, 0.345, 0.047];
const ORANGE_500: [f32; 3] = [0.976, 0.451, 0.086];
const ORANGE_400: [f32; 3] = [0.984, 0.573, 0.235];

fn opaque(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn alpha(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

pub fn set_style(imgui: &mut Context) {
    let style = imgui.style_mut();

    style.set_window_rounding(0.0);
    style.set_child_rounding(0.0);
    style.set_frame_rounding(2.0);
    style.set_popup_rounding(0.0);
    style.set_grab_rounding(2.0);
    style.set_tab_rounding(2.0);
    style.set_scrollbar_rounding(0.0);

    style.set_window_border_size(1.0);
    style.set_child_border_size(0.0);
    style.set_popup_border_size(1.0);
    style.set_frame_border_size(0.0);
    style.set_window_title_align([0.0, 0.5]);
    style.set_separator_text_align([0.0, 0.5]);
    style.set_window_padding([10.0, 10.0]);
    style.set_frame_padding([8.0, 4.0]);
    style.set_item_spacing([8.0, 6.0]);
    style.set_item_inner_spacing([6.0, 4.0]);
    style.set_indent_spacing(24.0);
    style.set_cell_padding([6.0, 4.0]);
    style.set_disabled_alpha(0.50);
    style.set_scrollbar_size(12.0);
    style.set_grab_min_size(10.0);
    style.set_tab_bar_overline_size(2.0);
    style.set_tree_lines_mode(TreeLineMode::TO_NODES);
    style.set_tree_lines_size(1.0);

    style.set_color(StyleColor::Text, opaque(ZINC_100));
    style.set_color(StyleColor::TextDisabled, opaque(ZINC_500));
    style.set_color(StyleColor::TextSelectedBg, alpha(ORANGE_400, 0.35));
    style.set_color(StyleColor::TextLink, opaque(ORANGE_400));

    style.set_color(StyleColor::WindowBg, opaque(ZINC_900));
    style.set_color(StyleColor::ChildBg, [0.0, 0.0, 0.0, 0.0]);
    style.set_color(StyleColor::PopupBg, alpha(ZINC_950, 0.98));
    style.set_color(StyleColor::Border, opaque(ZINC_700));
    style.set_color(StyleColor::BorderShadow, [0.0, 0.0, 0.0, 0.0]);

    style.set_color(StyleColor::FrameBg, opaque(ZINC_800));
    style.set_color(StyleColor::FrameBgHovered, opaque(ZINC_600));
    style.set_color(StyleColor::FrameBgActive, opaque(ZINC_500));

    style.set_color(StyleColor::TitleBg, opaque(ZINC_950));
    style.set_color(StyleColor::TitleBgActive, opaque(ZINC_850));
    style.set_color(StyleColor::TitleBgCollapsed, opaque(ZINC_950));
    style.set_color(StyleColor::MenuBarBg, opaque(ZINC_900));

    style.set_color(StyleColor::ScrollbarBg, [0.0, 0.0, 0.0, 0.0]);
    style.set_color(StyleColor::ScrollbarGrab, opaque(ZINC_700));
    style.set_color(StyleColor::ScrollbarGrabHovered, opaque(ZINC_500));
    style.set_color(StyleColor::ScrollbarGrabActive, opaque(ORANGE_400));

    style.set_color(StyleColor::CheckMark, opaque(ORANGE_400));
    style.set_color(StyleColor::CheckboxSelectedBg, opaque(ZINC_800));
    style.set_color(StyleColor::SliderGrab, opaque(ORANGE_500));
    style.set_color(StyleColor::SliderGrabActive, opaque(ORANGE_400));

    style.set_color(StyleColor::Button, opaque(ZINC_800));
    style.set_color(StyleColor::ButtonHovered, alpha(ORANGE_400, 0.30));
    style.set_color(StyleColor::ButtonActive, alpha(ORANGE_400, 0.55));

    style.set_color(StyleColor::Header, alpha(ORANGE_400, 0.22));
    style.set_color(StyleColor::HeaderHovered, alpha(ORANGE_400, 0.45));
    style.set_color(StyleColor::HeaderActive, alpha(ORANGE_400, 0.65));

    style.set_color(StyleColor::Separator, opaque(ZINC_700));
    style.set_color(StyleColor::SeparatorHovered, opaque(ORANGE_500));
    style.set_color(StyleColor::SeparatorActive, opaque(ORANGE_400));

    style.set_color(StyleColor::ResizeGrip, alpha(ZINC_600, 0.5));
    style.set_color(StyleColor::ResizeGripHovered, opaque(ORANGE_500));
    style.set_color(StyleColor::ResizeGripActive, opaque(ORANGE_400));

    style.set_color(StyleColor::Tab, opaque(ZINC_800));
    style.set_color(StyleColor::TabHovered, alpha(ORANGE_400, 0.40));
    style.set_color(StyleColor::TabSelected, opaque(ZINC_700));
    style.set_color(StyleColor::TabSelectedOverline, opaque(ORANGE_400));
    style.set_color(StyleColor::TabDimmed, opaque(ZINC_900));
    style.set_color(StyleColor::TabDimmedSelected, opaque(ZINC_800));
    style.set_color(StyleColor::TabDimmedSelectedOverline, alpha(ORANGE_600, 0.6));

    style.set_color(StyleColor::DockingPreview, alpha(ORANGE_400, 0.70));
    style.set_color(StyleColor::DockingEmptyBg, opaque(ZINC_950));

    style.set_color(StyleColor::PlotLines, opaque(ZINC_400));
    style.set_color(StyleColor::PlotLinesHovered, opaque(ORANGE_400));
    style.set_color(StyleColor::PlotHistogram, opaque(ORANGE_500));
    style.set_color(StyleColor::PlotHistogramHovered, opaque(ORANGE_400));

    style.set_color(StyleColor::TableHeaderBg, opaque(ZINC_800));
    style.set_color(StyleColor::TableBorderStrong, opaque(ZINC_700));
    style.set_color(StyleColor::TableBorderLight, opaque(ZINC_800));
    style.set_color(StyleColor::TableRowBg, [0.0, 0.0, 0.0, 0.0]);
    style.set_color(StyleColor::TableRowBgAlt, alpha(ZINC_200, 0.03));

    style.set_color(StyleColor::TreeLines, opaque(ZINC_700));
    style.set_color(StyleColor::InputTextCursor, opaque(ORANGE_400));
    style.set_color(StyleColor::DragDropTarget, opaque(ORANGE_400));
    style.set_color(StyleColor::NavCursor, opaque(ORANGE_400));
    style.set_color(StyleColor::NavWindowingHighlight, alpha(ORANGE_400, 0.70));
    style.set_color(StyleColor::NavWindowingDimBg, [0.0, 0.0, 0.0, 0.45]);
    style.set_color(StyleColor::ModalWindowDimBg, [0.0, 0.0, 0.0, 0.55]);
}
