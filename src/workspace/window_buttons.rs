//! 窗口按钮：最小化、最大化（或还原）、关闭。
//!
//! - Windows：只标 `window_control_area(Min / Max / Close)`，按下和抬起由平台层在非客户区完成
//!   （最大化按钮还会弹出贴靠布局）。**不要**挂 `stop_propagation` 或点击处理器，
//!   否则平台层记不下按下的按钮，按钮就失效了。
//! - Linux：没有平台层的按钮区，自己处理点击。
//! - macOS：用系统的红绿灯，不画。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::{Tokens, WINDOW_BUTTON, WINDOW_BUTTON_RADIUS};

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

    fn icon(self) -> CatalogIcon {
        match self {
            Self::Minimize => CatalogIcon::WindowMinimize,
            Self::Maximize { restore: false } => CatalogIcon::WindowMaximize,
            Self::Maximize { restore: true } => CatalogIcon::WindowRestore,
            Self::Close => CatalogIcon::WindowClose,
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
        let (hover_bg, hover_fg) = match self {
            Self::Close => (tokens.close_hover, tokens.close_hover_text),
            _ => (tokens.hover, tokens.text),
        };
        div()
            .id(self.id())
            .test_support()
            .aria_label(self.label())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(WINDOW_BUTTON)
            .rounded(WINDOW_BUTTON_RADIUS)
            .text_color(tokens.icon)
            .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
            .window_control_area(self.area())
            .when(cfg!(target_os = "linux"), |button| {
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
            .child(Icon::new(self.icon()).size(px(16.)))
    }
}

/// 三个窗口按钮，前面一条分隔竖线。macOS 上什么都不画（用系统的红绿灯）。
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
        .items_center()
        .gap(px(2.))
        .when(!cfg!(target_os = "macos"), |row| {
            row.child(div().w(px(1.)).h(px(16.)).mx(px(6.)).bg(tokens.separator))
                .children(buttons.map(|button| button.render(tokens)))
        })
}
