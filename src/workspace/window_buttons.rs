//! 窗口按钮：照空壳的三个彩色圆点（最小化黄、最大化绿、关闭红），每个 28 宽、高同标签栏。
//! 左边一条 1 × 20 的细线把入口图标和它们隔开。
//!
//! - Windows：只标 `window_control_area(Min / Max / Close)`，按下和抬起由平台层在非客户区完成
//!   （最大化按钮还会弹出贴靠布局）。**不要**挂 `stop_propagation` 或点击处理器，
//!   否则平台层记不下按下的按钮，按钮就失效了。
//! - Linux：没有平台层的按钮区，自己处理点击。
//! - macOS：用系统的红绿灯，不画。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, size};
use crate::ui::icon_button::TextTooltip;

/// 一个窗口按钮的宽度。
const CAPTION_WIDTH: Pixels = px(22.);
/// 圆点的直径。
const DOT: Pixels = px(10.);

#[derive(Clone, Copy)]
enum WindowButton {
    Minimize,
    Maximize { restore: bool },
    Close,
}

impl WindowButton {
    fn id(self) -> &'static str {
        match self {
            Self::Minimize => "window-min",
            Self::Maximize { .. } => "window-max",
            Self::Close => "window-close",
        }
    }

    fn area(self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Maximize { .. } => WindowControlArea::Max,
            Self::Close => WindowControlArea::Close,
        }
    }

    fn color(self, tokens: &Tokens) -> Hsla {
        match self {
            Self::Minimize => tokens.window_min,
            Self::Maximize { .. } => tokens.window_max,
            Self::Close => tokens.window_close,
        }
    }

    fn label(self) -> SharedString {
        match self {
            Self::Minimize => t!("workspace.window.minimize"),
            Self::Maximize { restore: false } => t!("workspace.window.maximize"),
            Self::Maximize { restore: true } => t!("workspace.window.restore"),
            Self::Close => t!("workspace.window.close"),
        }
    }

    fn render(self, tokens: &Tokens) -> impl IntoElement + use<> {
        let label = self.label();
        let button = div()
            .id(self.id())
            .test_support()
            .aria_label(label.clone())
            .tooltip(move |_, cx| TextTooltip::view(label.clone(), cx))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(CAPTION_WIDTH)
            .h_full()
            .rounded(size::RADIUS_SM)
            .cursor_pointer()
            .hover(move |style| style.bg(tokens.hover))
            .window_control_area(self.area())
            .child(div().size(DOT).rounded_full().bg(self.color(tokens)));
        button.when(cfg!(target_os = "linux"), |button| {
            button
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match self {
                        Self::Minimize => window.minimize_window(),
                        Self::Maximize { .. } => window.zoom_window(),
                        Self::Close => {
                            if !crate::app::quit_held_back(window, cx) {
                                window.remove_window();
                            }
                        }
                    }
                })
        })
    }
}

/// 三个窗口按钮，左边一条细线。macOS 上什么都不画（用系统的红绿灯）。
pub(super) fn render_window_buttons(tokens: &Tokens, window: &Window) -> impl IntoElement {
    let buttons = [
        WindowButton::Minimize,
        WindowButton::Maximize {
            restore: window.is_maximized(),
        },
        WindowButton::Close,
    ];
    div()
        .flex()
        .flex_none()
        .h_full()
        .when(!cfg!(target_os = "macos"), |row| {
            // 分隔线到图标也要约 6 的可见空隙：左边去掉 Agent 按钮的 4 内边距留 2，
            // 右边最小化圆点自己的内边距（23 的按钮、11 的点）已经够 6，不再留。
            row.child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h_full()
                    .ml(space::SPACE_2)
                    .bg(tokens.line),
            )
            .children(buttons.map(|button| button.render(tokens)))
        })
}
