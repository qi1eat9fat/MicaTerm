//! 字号：四档加一档次要小字（ui-lessons 第四节第 2 条；`small` 是 26 号的 `--fs-gh`），
//! 全部由界面字号换算出来。界面字号不让用户改（用户定，2026-11-14 去掉了设置里的这一项）：
//! Windows 上 12，其他平台 13。
//! 自绘部分和 gpui-component 控件读同一个值（控件读 `Theme::font_size`，即 `ui`）。
//!
//! 换算表就是 26 号原型的 `fs=12 / 13 / 14` 三档（14 档现在用不到，留着对照原型）：
//!
//! | 界面字号 | 界面 `ui` | 主机名 `side` | 等宽小字 `mono_small` | 页标题 `title` | 次要小字 `small` | 侧栏行高 |
//! |---|---|---|---|---|---|---|
//! | 12 | 12 | 13 | 11.5 | 14 | 12 | 28 |
//! | 13 | 13 | 13 | 12.5 | 14 | 12 | 30 |
//! | 14 | 14 | 14 | 13.5 | 14 | 13 | 30 |
//!
//! 页标题三档都是 14（26 号 `.hp-title`、`.stg-title` 写死 14）：字号大的档里它和正文一样大，靠 500 区分。
//!
//! 终端和编辑器的字号是终端自己的设置，不在这里。

use std::ops::RangeInclusive;

use gpui_kit::{App, Pixels, Rems, px, rems};

use gpui_kit::component::ActiveTheme as _;

/// 界面字号（像素）：Windows 上 12，即系统 9pt 正文（用户定）；其他平台 13。终端有自己的字号。
pub const INTERFACE_FONT_SIZE: f32 = if cfg!(windows) { 12.0 } else { 13.0 };

/// 换算表的三档。
const STEPS: RangeInclusive<f32> = 12.0..=14.0;

/// 一套字号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    /// 界面正文、标签、按钮、状态栏、菜单（Windows 上默认 12，即 9pt）。
    pub ui: Pixels,
    /// 侧栏主机名、片段名这类列表主文字。
    pub side: Pixels,
    /// 快捷键、等宽的小字（Maple Mono）。
    pub mono_small: Pixels,
    /// 页标题：新建主机、设置页。三档都是 14。
    pub title: Pixels,
    /// 次要小字：面板的分组标题（26 号 `--fs-gh`）、凭据行后面的说明。
    pub small: Pixels,
    /// 侧栏一行的高度。
    pub side_row: Pixels,
}

impl TypeScale {
    /// 界面字号是 `base` 时的一套字号。`base` 收进 12–14 并取整。
    pub fn from_base(base: f32) -> Self {
        let base = if base.is_finite() {
            base.round().clamp(*STEPS.start(), *STEPS.end())
        } else {
            INTERFACE_FONT_SIZE
        };
        Self {
            ui: px(base),
            side: px(base.max(13.)),
            mono_small: px(base - 0.5),
            title: px(14.),
            small: px(if base >= 14. { 13. } else { 12. }),
            side_row: px(if base <= 12. { 28. } else { 30. }),
        }
    }

    /// 当前的一套字号：界面字号就是 gpui-component 主题的 `font_size`（`settings::apply` 设的）。
    pub fn current(cx: &App) -> Self {
        Self::from_base(f32::from(cx.theme().font_size))
    }
}

/// 界面字号写成 rem：gpui-component 的 `Root` 把窗口的 rem 设成主题字号，也就是 `TypeScale::ui`。
/// 拿不到 `cx` 的小函数用它；不写字号的文字本来就是它，要盖过父元素的字号时才写。
pub fn ui_text() -> Rems {
    rems(1.)
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::TypeScale;

    #[test]
    fn the_12_step_matches_the_prototype() {
        let scale = TypeScale::from_base(12.);
        assert_eq!(scale.ui, px(12.));
        assert_eq!(scale.side, px(13.));
        assert_eq!(scale.mono_small, px(11.5));
        assert_eq!(scale.title, px(14.));
        assert_eq!(scale.small, px(12.));
        assert_eq!(scale.side_row, px(28.));
    }

    #[test]
    fn larger_steps_grow_the_text_sizes() {
        let (a, b, c) = (
            TypeScale::from_base(12.),
            TypeScale::from_base(13.),
            TypeScale::from_base(14.),
        );
        assert!(a.ui < b.ui && b.ui < c.ui);
        assert!(a.side <= b.side && b.side < c.side);
        assert_eq!((a.title, b.title, c.title), (px(14.), px(14.), px(14.)));
        assert_eq!(c.side, px(14.));
        assert_eq!((b.small, c.small), (px(12.), px(13.)));
    }

    #[test]
    fn bases_outside_the_three_steps_are_brought_back() {
        assert_eq!(TypeScale::from_base(20.), TypeScale::from_base(14.));
        assert_eq!(TypeScale::from_base(9.), TypeScale::from_base(12.));
        assert_eq!(TypeScale::from_base(12.4), TypeScale::from_base(12.));
    }
}
