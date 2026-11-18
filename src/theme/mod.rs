//! 视觉常量：颜色 token（`tokens`）、尺寸（`size`）、字号（`TypeScale`）和字体。
//!
//! 颜色取自 Ayu 主题（见 `tokens`）；尺寸和字号仍按 26 号逐屏原型的换算表（见 `size`、`type_scale`）。
//! 规则：业务代码里不写颜色、尺寸和字号数字，都从这里取。

pub mod code_colors;
mod component;
pub mod size;
mod tokens;
mod type_scale;

pub use component::{DARK_THEME_NAME, LIGHT_THEME_NAME, component_theme, component_theme_name};
pub use tokens::Tokens;
pub use type_scale::{INTERFACE_FONT_SIZE, TypeScale, ui_text};

use gpui_kit::{FontWeight, Pixels};

/// 界面字体：中英文都用 HarmonyOS Sans SC（用户定）。本机有 300 / 400 / 500 / 700，没有 600。
pub const UI_FONT_FAMILY: &str = "HarmonyOS Sans SC";
/// 等宽字体：终端、编辑器、快捷键、等宽小字，Maple Mono 的 CN 版自带中文（用户定）。
pub const MONO_FONT_FAMILY: &str = micaterm_terminal::DEFAULT_FONT_FAMILY;

/// 字重只用两种：正文 400，标题和强调 500。主机栏一律 400。
/// HarmonyOS Sans SC 没有 600，写 600 会落到 Bold。
pub const WEIGHT_NORMAL: FontWeight = FontWeight::NORMAL;
pub const WEIGHT_MEDIUM: FontWeight = FontWeight::MEDIUM;

/// 同一行里两段不同字体、字号的文字（如 12 的界面字 + 11.5 的 Maple Mono 快捷键）都设成行高
/// `line_height`、盒子居中时，要把 `to` 那段往下挪多少，两者的基线才对齐（ui-lessons 第四节第 3 条：
/// 按字形度量算，不写死像素）。GPUI 把字形盒在行高里上下居中，基线在 `(行高 − 上伸 − 下伸) / 2 + 上伸`。
pub fn baseline_shift(
    window: &gpui_kit::Window,
    from: (&'static str, Pixels),
    to: (&'static str, Pixels),
    line_height: Pixels,
) -> Pixels {
    let text = window.text_system();
    let baseline = |(family, size): (&'static str, Pixels)| {
        let id = text.resolve_font(&gpui_kit::font(family));
        text.baseline_offset(id, size, line_height)
    };
    baseline(from) - baseline(to)
}
