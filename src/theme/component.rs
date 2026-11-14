//! gpui-component 的主题（对话框、下拉菜单、输入框、开关……用的颜色和字体），从 26 号原型的 token 映射过来：
//! 从 gpui-component 默认的浅色或深色主题出发，换掉和原型对得上的那些颜色，
//! 其余（悬停、按下这类）由 gpui-component 自己从这些推出来。界面颜色不跟终端主题走。

use std::rc::Rc;

use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, SharedString};

use super::{MONO_FONT_FAMILY, Tokens, UI_FONT_FAMILY, size};

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

/// `mode` 的界面主题：gpui-component 的默认主题换上 26 号 `.win.light` / `.win.dark` 的颜色。
pub fn component_theme(mode: ThemeMode, cx: &App) -> Rc<ThemeConfig> {
    let registry = ThemeRegistry::global(cx);
    let (default, tokens) = match mode {
        ThemeMode::Light => (registry.default_light_theme(), Tokens::light()),
        ThemeMode::Dark => (registry.default_dark_theme(), Tokens::dark()),
    };
    let mut config = ThemeConfig::clone(default);
    config.name = component_theme_name(mode).into();
    config.is_default = false;
    // 界面字号由设置定（`settings::apply`），主题不带字号，免得换主题时把它改回去。
    config.font_size = None;
    // 界面字体 HarmonyOS Sans SC；代码（编辑器）和终端同一种等宽字体 Maple Mono，13（26 号 `.ed-code`）。
    config.font_family = Some(SharedString::new_static(UI_FONT_FAMILY));
    config.mono_font_family = Some(SharedString::new_static(MONO_FONT_FAMILY));
    config.mono_font_size = Some(13.);
    config.radius = Some(f32::from(size::RADIUS) as usize);
    config.radius_lg = Some(f32::from(size::RADIUS_LG) as usize);
    // 编辑器（26 号 `.ed`）：语法颜色用默认主题的，正文、行号栏和当前行换成内容区的颜色。
    // 不换的话行号栏是默认主题的底色，深色下和内容区差一大截。
    let mut highlight = config.highlight.take().unwrap_or_default();
    highlight.editor_background = Some(tokens.content);
    highlight.editor_gutter_background = Some(tokens.content);
    highlight.editor_foreground = Some(tokens.text);
    highlight.editor_active_line = Some(tokens.hover);
    highlight.editor_line_number = Some(tokens.faint);
    highlight.editor_active_line_number = Some(tokens.text2);
    config.highlight = Some(highlight);
    let hex = |color: Hsla| Some(hex(color));
    let colors = &mut config.colors;
    // 对话框、浮层、菜单：浅色同内容底，深色 `#202124`；外圈 `--line-strong`。
    colors.background = hex(tokens.popup);
    colors.foreground = hex(tokens.text);
    colors.border = hex(tokens.line_strong);
    colors.popover = hex(tokens.popup);
    colors.popover_foreground = hex(tokens.text);
    // 次要文字和底：`--text2`、输入框底。
    colors.muted = hex(tokens.input);
    colors.muted_foreground = hex(tokens.text2);
    // 没有强调色（用户定）：主按钮是浅灰底 `--sel` + 正文色字，主次靠位置区分；
    // 焦点环、开关打开、选中都用正文色。
    colors.primary = hex(tokens.text);
    colors.primary_hover = hex(tokens.text);
    colors.primary_active = hex(tokens.text);
    colors.primary_foreground = hex(tokens.content);
    colors.button_primary = hex(tokens.sel);
    colors.button_primary_hover = hex(tokens.sel);
    colors.button_primary_active = hex(tokens.sel);
    colors.button_primary_foreground = hex(tokens.text);
    colors.ring = hex(tokens.line_strong);
    colors.caret = hex(tokens.text);
    // 菜单项、列表行的悬停和选中：`--hover`、`--sel`。
    colors.accent = hex(tokens.hover);
    colors.accent_foreground = hex(tokens.text);
    colors.list_hover = hex(tokens.hover);
    colors.list_active = hex(tokens.sel);
    colors.selection = hex(tokens.sel);
    // 次按钮：透明底描边（描边是 `border`），悬停 `--hover`。
    colors.secondary = hex(tokens.hover);
    colors.secondary_hover = hex(tokens.sel);
    colors.secondary_active = hex(tokens.sel);
    colors.secondary_foreground = hex(tokens.text);
    // 输入框描边 `--line-strong`；开关关着时的底用 `--line-strong`，开着是正文色（`primary`）。
    colors.input = hex(tokens.line_strong);
    colors.switch = hex(tokens.line_strong);
    colors.scrollbar_thumb = hex(tokens.scrollbar);
    colors.scrollbar_thumb_hover = hex(tokens.scrollbar);
    // 弹窗不压暗背景；错误红 `--err`。
    colors.overlay = hex(gpui_kit::transparent_black());
    colors.danger = hex(tokens.err);
    colors.group_box = hex(tokens.tabbar);
    // 文件列表：表格本身透明，透出内容区的底；悬停和选中只画在名称格上（工单 10 重做）。
    let clear = Some(SharedString::new_static("#00000000"));
    colors.table = clear.clone();
    colors.table_head = clear.clone();
    colors.table_even = clear.clone();
    colors.table_hover = clear.clone();
    colors.table_row_border = clear;
    colors.table_active = hex(tokens.sel);
    colors.table_active_border = hex(tokens.line_strong);
    colors.table_head_foreground = hex(tokens.faint);
    Rc::new(config)
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
    use gpui_kit::rgba;

    use super::hex;

    #[test]
    fn colors_are_written_as_hex_with_alpha() {
        assert_eq!(hex(rgba(0x4f5bd58c).into()).as_ref(), "#4f5bd58c");
        assert_eq!(hex(rgba(0xffffffff).into()).as_ref(), "#ffffffff");
    }
}
