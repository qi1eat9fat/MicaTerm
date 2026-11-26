//! gpui-component 的主题（对话框、下拉菜单、输入框、通知、开关……用的颜色和字体），全部从 `Tokens` 映射：
//! 不继承 gpui-component 默认主题里的任何颜色字段，避免第三方默认色在通知、链接、编辑器状态等处漏出。
//! 界面颜色不跟终端主题走；强调色是 `Tokens::brand`。

use std::rc::Rc;

use gpui_kit::component::highlighter::StatusColors;
use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, SharedString};
use serde_json::json;

use super::{INTERFACE_FONT_SIZE, MONO_FONT_FAMILY, Tokens, UI_FONT_FAMILY, on_color, size};

/// 浅色界面主题的名字：主题里是这个名字，就说明已经换上了 MicaTerm 的颜色。
pub const LIGHT_THEME_NAME: &str = "MicaTerm Light";
/// 深色界面主题的名字。
pub const DARK_THEME_NAME: &str = "MicaTerm Dark";

/// `mode` 的界面主题名。
pub fn component_theme_name(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => LIGHT_THEME_NAME,
        ThemeMode::Dark => DARK_THEME_NAME,
    }
}

/// `mode` 的界面主题：所有组件颜色均由 `Tokens` 单一数据源映射生成。
pub fn component_theme(mode: ThemeMode, cx: &App) -> Rc<ThemeConfig> {
    let registry = ThemeRegistry::global(cx);
    let (default, tokens) = match mode {
        ThemeMode::Light => (registry.default_light_theme(), Tokens::light()),
        ThemeMode::Dark => (registry.default_dark_theme(), Tokens::dark()),
    };
    let mut config = ThemeConfig::clone(default);
    config.name = component_theme_name(mode).into();
    config.mode = mode;
    config.is_default = false;
    // 界面字号由设置定（`settings::apply`），主题不带字号，免得换主题时把它改回去。
    config.font_size = None;
    // 界面字体 Instrument Sans；等宽字号与界面正文基准对齐（12px）。
    config.font_family = Some(SharedString::new_static(UI_FONT_FAMILY));
    config.mono_font_family = Some(SharedString::new_static(MONO_FONT_FAMILY));
    config.mono_font_size = Some(INTERFACE_FONT_SIZE);
    config.radius = Some(f32::from(size::RADIUS_MD) as usize);
    config.radius_lg = Some(f32::from(size::RADIUS_LG) as usize);
    // 编辑器：保留语法表占位（随后由 `code_highlight` 换成当前终端主题），底色、行号、不可见字符和诊断状态色全从 Tokens 来。
    let mut highlight = config.highlight.take().unwrap_or_default();
    highlight.editor_background = Some(tokens.content);
    highlight.editor_gutter_background = Some(tokens.content);
    highlight.editor_foreground = Some(tokens.text);
    highlight.editor_active_line = Some(tokens.hover);
    highlight.editor_line_number = Some(tokens.faint);
    highlight.editor_active_line_number = Some(tokens.text2);
    highlight.editor_invisible = Some(tokens.faint);
    highlight.status = status_colors(&tokens);
    config.highlight = Some(highlight);

    // 清空默认主题里的 shadcn 颜色，全部从 `Tokens` 显式映射或基于其回退推导。
    config.colors = Default::default();
    let hex = |color: Hsla| Some(hex(color));
    let clear = Some(SharedString::new_static("#00000000"));
    let colors = &mut config.colors;

    // 对话框、浮层、菜单：面板色底、正文色字、外圈 `line_strong`。
    colors.background = hex(tokens.popup);
    colors.foreground = hex(tokens.text);
    colors.border = hex(tokens.line_strong);
    colors.popover = hex(tokens.popup);
    colors.popover_foreground = hex(tokens.text);
    colors.accordion = hex(tokens.popup);
    colors.group_box = hex(tokens.popup);
    colors.group_box_foreground = hex(tokens.text);
    colors.group_box_title_foreground = hex(tokens.text2);
    colors.description_list_label = hex(tokens.input);
    colors.description_list_label_foreground = hex(tokens.text2);

    // 次要文字与输入框底。
    colors.muted = hex(tokens.input);
    colors.muted_foreground = hex(tokens.text2);

    // 强调色：主按钮、开关打开、焦点环、链接、进度条、拖放框。
    colors.primary = hex(tokens.brand);
    colors.primary_hover = hex(tokens.brand_hover);
    colors.primary_active = hex(tokens.brand_hover);
    colors.primary_foreground = hex(tokens.on_brand);
    colors.button_primary = hex(tokens.brand);
    colors.button_primary_hover = hex(tokens.brand_hover);
    colors.button_primary_active = hex(tokens.brand_hover);
    colors.button_primary_foreground = hex(tokens.on_brand);
    colors.ring = hex(tokens.brand);
    colors.caret = hex(tokens.text);
    colors.link = hex(tokens.brand);
    colors.link_hover = hex(tokens.brand_hover);
    colors.link_active = hex(tokens.brand_hover);
    colors.progress_bar = hex(tokens.brand);
    colors.slider_bar = hex(tokens.brand);
    colors.slider_thumb = hex(tokens.switch_knob);
    colors.drag_border = hex(tokens.brand_ring);
    colors.drop_target = hex(tokens.brand_soft);

    // 语义状态色（通知、诊断、状态按钮）：全部收口到 `Tokens`。
    colors.success = hex(tokens.ok);
    colors.success_foreground = hex(on_color(tokens.ok));
    colors.warning = hex(tokens.warn);
    colors.warning_foreground = hex(on_color(tokens.warn));
    colors.info = hex(tokens.brand);
    colors.info_foreground = hex(tokens.on_brand);
    colors.danger = hex(tokens.err);
    colors.danger_foreground = hex(on_color(tokens.err));

    // 菜单项、列表行的悬停和选中；选区用强调色的淡底。
    colors.accent = hex(tokens.hover);
    colors.accent_foreground = hex(tokens.text);
    colors.list = clear.clone();
    colors.list_even = clear.clone();
    colors.list_head = clear.clone();
    colors.list_hover = hex(tokens.hover);
    colors.list_active = hex(tokens.sel);
    colors.list_active_border = hex(tokens.line_strong);
    colors.selection = hex(tokens.brand_soft);
    colors.skeleton = hex(tokens.hover);

    // 次按钮：透明/悬停中性底。
    colors.secondary = hex(tokens.hover);
    colors.secondary_hover = hex(tokens.sel);
    colors.secondary_active = hex(tokens.sel);
    colors.secondary_foreground = hex(tokens.text);

    // 控件描边、开关、滚动条、窗口框架与遮罩。
    colors.input = hex(tokens.line_strong);
    colors.switch = hex(tokens.line_strong);
    colors.switch_thumb = hex(tokens.switch_knob);
    colors.scrollbar = clear.clone();
    colors.scrollbar_thumb = hex(tokens.scrollbar);
    colors.scrollbar_thumb_hover = hex(tokens.scrollbar);
    colors.overlay = hex(tokens.backdrop);
    colors.window_border = hex(tokens.line);

    // 侧栏、标签栏、标题栏、状态栏（若有组件读这些角色，也与 Tokens 一致）。
    colors.sidebar = hex(tokens.side);
    colors.sidebar_foreground = hex(tokens.text);
    colors.sidebar_border = hex(tokens.line);
    colors.sidebar_accent = hex(tokens.hover);
    colors.sidebar_accent_foreground = hex(tokens.text);
    colors.sidebar_primary = hex(tokens.brand);
    colors.sidebar_primary_foreground = hex(tokens.on_brand);
    colors.tab = clear.clone();
    colors.tab_foreground = hex(tokens.text2);
    colors.tab_active = hex(tokens.tab_active);
    colors.tab_active_foreground = hex(tokens.text);
    colors.tab_bar = hex(tokens.tabbar);
    colors.tab_bar_segmented = hex(tokens.input);
    colors.title_bar = hex(tokens.tabbar);
    colors.title_bar_border = hex(tokens.line);
    colors.status_bar = hex(tokens.tabbar);
    colors.status_bar_border = hex(tokens.line);

    // 文件列表：表格本身透明，透出内容区的底；悬停和选中只画在名称格上。
    colors.table = clear.clone();
    colors.table_head = clear.clone();
    colors.table_foot = clear.clone();
    colors.table_even = clear.clone();
    colors.table_hover = clear.clone();
    colors.table_row_border = clear;
    colors.table_active = hex(tokens.sel);
    colors.table_active_border = hex(tokens.line_strong);
    colors.table_head_foreground = hex(tokens.faint);
    colors.table_foot_foreground = hex(tokens.faint);
    Rc::new(config)
}

/// 编辑器诊断与状态色：`StatusColors` 字段不公开，经 serde 从 `Tokens` 映射。
fn status_colors(tokens: &Tokens) -> StatusColors {
    serde_json::from_value(json!({
        "error": hex(tokens.err),
        "error.background": hex(tokens.err_soft),
        "error.border": hex(tokens.err_line),
        "warning": hex(tokens.warn),
        "warning.background": hex(tokens.warn_soft),
        "warning.border": hex(tokens.warn_line),
        "info": hex(tokens.brand),
        "info.background": hex(tokens.brand_soft),
        "info.border": hex(tokens.brand_ring),
        "success": hex(tokens.ok),
        "success.background": hex(tokens.ok_soft),
        "success.border": hex(tokens.ok),
        "hint": hex(tokens.text2),
        "hint.background": hex(tokens.hover),
        "hint.border": hex(tokens.line_strong),
        "created": hex(tokens.ok),
        "modified": hex(tokens.warn),
        "deleted": hex(tokens.err),
    }))
    .unwrap_or_default()
}

/// `#rrggbbaa`：gpui-component 读主题颜色的写法。
fn hex(color: Hsla) -> SharedString {
    let rgba = color.to_rgb();
    let byte = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b),
        byte(rgba.a)
    )
    .into()
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{TestAppContext, rgba};

    use super::{component_theme, hex};
    use crate::theme::{Tokens, on_color};

    #[test]
    fn colors_are_written_as_hex_with_alpha() {
        assert_eq!(hex(rgba(0x4f5bd58c).into()).as_ref(), "#4f5bd58c");
        assert_eq!(hex(rgba(0xffffffff).into()).as_ref(), "#ffffffff");
    }

    /// 组件库里的状态色、链接色和面板色都必须来自 `Tokens`，不漏出默认主题的 shadcn 颜色。
    #[gpui_kit::test]
    fn component_theme_maps_all_semantic_roles_from_tokens(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            for (mode, tokens) in [
                (ThemeMode::Light, Tokens::light()),
                (ThemeMode::Dark, Tokens::dark()),
            ] {
                let config = component_theme(mode, cx);
                let mut theme = Theme::default();
                theme.apply_config(&config);
                let approx = |actual: gpui_kit::Hsla, expected: gpui_kit::Hsla| {
                    assert_eq!(hex(actual), hex(expected));
                };
                approx(theme.background, tokens.popup);
                approx(theme.foreground, tokens.text);
                approx(theme.primary, tokens.brand);
                approx(theme.primary_foreground, tokens.on_brand);
                approx(theme.success, tokens.ok);
                approx(theme.success_foreground, on_color(tokens.ok));
                approx(theme.warning, tokens.warn);
                approx(theme.warning_foreground, on_color(tokens.warn));
                approx(theme.info, tokens.brand);
                approx(theme.info_foreground, tokens.on_brand);
                approx(theme.danger, tokens.err);
                approx(theme.danger_foreground, on_color(tokens.err));
                approx(theme.link, tokens.brand);
                approx(theme.link_hover, tokens.brand_hover);
                approx(theme.drag_border, tokens.brand_ring);
                approx(theme.drop_target, tokens.brand_soft);
                approx(theme.switch_thumb, tokens.switch_knob);
                approx(theme.highlight_theme.style.status.error(cx), tokens.err);
                approx(theme.highlight_theme.style.status.warning(cx), tokens.warn);
                approx(theme.highlight_theme.style.status.info(cx), tokens.brand);
                approx(theme.highlight_theme.style.status.success(cx), tokens.ok);
            }
        });
    }
}
