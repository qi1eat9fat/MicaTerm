//! 编辑器的语法颜色（26 号「编辑器」）：和终端同一套配色，从当前的终端主题取——
//! 关键字蓝、函数青、类型品红、数字和常量黄、字符串绿、注释用亮黑（淡）；其余（变量、标点、运算符）
//! 是终端的前景色。行号栏、当前行、编辑器底色仍是界面的颜色（`component::component_theme`）。
//!
//! gpui-component 的 `ThemeStyle` 字段不公开，只能从主题 JSON 读，所以这里拼一段 JSON 再解出来。

use gpui_kit::Hsla;
use gpui_kit::component::highlighter::{HighlightTheme, SyntaxColors};
use micaterm_terminal::TerminalTheme;
use serde_json::{Map, Value, json};

/// ANSI 色号（`TerminalTheme::indexed`）。
const GREEN: u8 = 2;
const YELLOW: u8 = 3;
const BLUE: u8 = 4;
const MAGENTA: u8 = 5;
const CYAN: u8 = 6;
const BRIGHT_BLACK: u8 = 8;

/// 每种语法记号用哪个 ANSI 色。没列出的用前景色。
const SYNTAX: &[(&str, u8)] = &[
    ("keyword", BLUE),
    ("preproc", BLUE),
    ("tag", BLUE),
    ("function", CYAN),
    ("constructor", CYAN),
    ("type", MAGENTA),
    ("enum", MAGENTA),
    ("variant", MAGENTA),
    ("attribute", MAGENTA),
    ("label", MAGENTA),
    ("number", YELLOW),
    ("boolean", YELLOW),
    ("constant", YELLOW),
    ("string", GREEN),
    ("string.escape", GREEN),
    ("string.regex", GREEN),
    ("string.special", GREEN),
    ("string.special.symbol", GREEN),
    ("text.literal", GREEN),
    ("link_uri", GREEN),
    ("comment", BRIGHT_BLACK),
    ("comment_doc", BRIGHT_BLACK),
];

/// `#rrggbbaa`。
fn hex(color: Hsla) -> String {
    let rgba = color.to_rgb();
    let byte = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b),
        byte(rgba.a)
    )
}

/// 终端主题 `terminal` 的语法颜色。
pub fn syntax_colors(terminal: &TerminalTheme) -> SyntaxColors {
    let mut map = Map::new();
    for (name, index) in SYNTAX {
        map.insert(
            (*name).into(),
            json!({ "color": hex(terminal.indexed(*index)) }),
        );
    }
    serde_json::from_value(Value::Object(map)).unwrap_or_default()
}

/// 在 `base`（界面主题里的那份）上换成终端主题的语法颜色和前景色。
pub fn code_highlight(base: &HighlightTheme, terminal: &TerminalTheme) -> HighlightTheme {
    let mut theme = base.clone();
    theme.style.syntax = syntax_colors(terminal);
    theme.style.editor_foreground = Some(terminal.foreground());
    theme
}

#[cfg(test)]
mod tests {
    use micaterm_terminal::TerminalTheme;

    use super::{hex, syntax_colors};

    #[test]
    fn keywords_strings_and_comments_take_the_terminal_colors() {
        let theme = &TerminalTheme::all()[0];
        let syntax = syntax_colors(theme);
        let color = |name: &str| syntax.style(name).and_then(|style| style.color).map(hex);
        assert_eq!(color("keyword"), Some(hex(theme.indexed(4))));
        assert_eq!(color("string"), Some(hex(theme.indexed(2))));
        assert_eq!(color("number"), Some(hex(theme.indexed(3))));
        assert_eq!(color("comment"), Some(hex(theme.indexed(8))));
        assert_eq!(color("variable"), None);
    }
}
