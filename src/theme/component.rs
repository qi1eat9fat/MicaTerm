//! gpui-component 的主题（对话框、下拉菜单、输入框、开关……用的颜色），从原型的 token 映射过来：
//! 从 gpui-component 默认的浅色或深色主题出发，换掉和原型对得上的那些颜色，
//! 其余（悬停、按下这类）由 gpui-component 自己从这些推出来。界面颜色不跟终端主题走。

use std::rc::Rc;

use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, SharedString};

use super::Tokens;

/// 浅色界面主题的名字：主题里是这个名字，就说明已经换上了 QiShell 的颜色。
pub const LIGHT_THEME_NAME: &str = "QiShell Light";
/// 深色界面主题的名字。
pub const DARK_THEME_NAME: &str = "QiShell Dark";

/// `mode` 的界面主题名。
pub fn component_theme_name(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => LIGHT_THEME_NAME,
        ThemeMode::Dark => DARK_THEME_NAME,
    }
}

/// `mode` 的界面主题：gpui-component 的默认主题换上原型 `.win.light` / `.win.dark` 的颜色。
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
    // 代码（编辑器）的字体：随包的 JetBrains Mono，12.5（原型 `.ed.mono`）。
    config.mono_font_family = Some(SharedString::new_static("JetBrains Mono"));
    config.mono_font_size = Some(12.5);
    // 编辑器（原型 `.ed`）：语法颜色用默认主题的，正文、行号栏和当前行换成卡片的颜色。
    // 不换的话行号栏是默认主题的底色，深色下和卡片差一大截。
    let mut highlight = config.highlight.take().unwrap_or_default();
    highlight.editor_background = Some(tokens.card_focused);
    highlight.editor_gutter_background = Some(tokens.card_focused);
    highlight.editor_foreground = Some(tokens.terminal);
    highlight.editor_active_line = Some(tokens.hover);
    highlight.editor_line_number = Some(tokens.faint);
    highlight.editor_active_line_number = Some(tokens.text);
    config.highlight = Some(highlight);
    let hex = |color: Hsla| Some(hex(color));
    let colors = &mut config.colors;
    // 对话框、浮层：`--panel`、`--panel-b`。
    colors.background = hex(tokens.panel);
    colors.foreground = hex(tokens.text);
    colors.border = hex(tokens.panel_border);
    colors.popover = hex(tokens.panel);
    colors.popover_foreground = hex(tokens.text);
    // 次要文字和底：`--faint`、`--field`。
    colors.muted = hex(tokens.field);
    colors.muted_foreground = hex(tokens.faint);
    // 强调色：主按钮、焦点环、开关打开时、光标。
    colors.primary = hex(tokens.accent);
    colors.primary_hover = hex(tokens.accent);
    colors.primary_active = hex(tokens.accent);
    colors.primary_foreground = hex(tokens.on_accent);
    colors.button_primary = hex(tokens.accent);
    colors.button_primary_hover = hex(tokens.accent);
    colors.button_primary_active = hex(tokens.accent);
    colors.button_primary_foreground = hex(tokens.on_accent);
    colors.ring = hex(tokens.accent);
    colors.caret = hex(tokens.text);
    // 菜单项、列表行的悬停和选中：`--hover`、`--sel`。
    colors.accent = hex(tokens.hover);
    colors.accent_foreground = hex(tokens.text);
    colors.list_hover = hex(tokens.hover);
    colors.list_active = hex(tokens.selection);
    colors.selection = hex(tokens.selection);
    // 普通按钮：`.btn` 是 `--hover` 底，悬停 `--pressed`。
    colors.secondary = hex(tokens.hover);
    colors.secondary_hover = hex(tokens.pressed);
    colors.secondary_active = hex(tokens.pressed);
    colors.secondary_foreground = hex(tokens.text);
    // 输入框描边 `--field-b`；开关关着时的底用分隔线色。
    colors.input = hex(tokens.field_border);
    colors.switch = hex(tokens.separator);
    colors.scrollbar_thumb = hex(tokens.scrollbar);
    colors.scrollbar_thumb_hover = hex(tokens.scrollbar);
    // 对话框遮罩 `--scrim`、错误红 `--bad`。
    colors.overlay = hex(tokens.scrim);
    colors.danger = hex(tokens.bad);
    colors.group_box = hex(tokens.group);
    // 文件列表（原型 `.ftab`）：表格本身透明，透出卡片的底；悬停和选中只画在名称格上。
    let clear = Some(SharedString::new_static("#00000000"));
    colors.table = clear.clone();
    colors.table_head = clear.clone();
    colors.table_even = clear.clone();
    colors.table_hover = clear.clone();
    colors.table_row_border = clear;
    colors.table_active = hex(tokens.selection_solid);
    colors.table_active_border = hex(tokens.accent);
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
