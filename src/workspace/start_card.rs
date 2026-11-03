//! 开始卡片：没有任何标签时（启动时，或关掉最后一个标签后）标签区显示的卡片。
//!
//! M1 还没有主机，所以只有欢迎语、「新建本地终端」「新建主机」两个按钮和快速连接的快捷键；
//! 最近连接列表在 M2（工单 18）加在两个按钮上面。

use gpui_kit::component::{Icon, kbd::Kbd};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{CatalogIcon, NewHost, NewLocalTerminalTab, QuickConnect};
use crate::i18n::t;
use crate::theme::{CARD_RADIUS, Tokens};

/// 标志的渐变：原型 `.logo-i` 的 #7c5cff → #2e9bd0。
pub(super) fn logo(size: Pixels, radius: Pixels) -> Div {
    div()
        .flex_none()
        .size(size)
        .rounded(radius)
        .bg(linear_gradient(
            135.,
            linear_color_stop(rgb(0x7c5cff), 0.),
            linear_color_stop(rgb(0x2e9bd0), 1.),
        ))
}

/// 开始卡片。`focus` 是它自己的焦点（没有标签时由它持有），按钮经 `target` 派发 action。
pub(super) fn render_start_card(
    focus: &FocusHandle,
    target: &FocusHandle,
    tokens: &Tokens,
    window: &Window,
) -> impl IntoElement {
    let quick_connect = Kbd::binding_for_action(&QuickConnect, None, window);
    div()
        .id("start-card")
        .test_support()
        .track_focus(focus)
        .aria_label(t!("workspace.start.title"))
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .rounded(CARD_RADIUS)
        .bg(tokens.card)
        .border_1()
        .border_color(tokens.card_border)
        .shadow(vec![BoxShadow {
            color: tokens.shadow,
            offset: point(px(0.), px(6.)),
            blur_radius: px(22.),
            spread_radius: px(-2.),
            inset: false,
        }])
        .child(
            div()
                .w(px(440.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .mb(px(22.))
                        .child(logo(px(48.), px(14.)).mb(px(12.)))
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .mb(px(6.))
                                .child(t!("workspace.start.title")),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(tokens.muted)
                                .child(t!("workspace.start.hint")),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .mb(px(14.))
                        .child(big_button(
                            "start-new-local-terminal",
                            CatalogIcon::Terminal,
                            (rgb(0x334155).into(), rgb(0x86efac).into()),
                            t!("workspace.start.new_local_terminal"),
                            t!("workspace.start.new_local_terminal_hint"),
                            Box::new(NewLocalTerminalTab),
                            target,
                            tokens,
                        ))
                        .child(big_button(
                            "start-new-host",
                            CatalogIcon::Plus,
                            (tokens.accent, tokens.on_accent),
                            t!("workspace.start.new_host"),
                            t!("workspace.start.new_host_hint"),
                            Box::new(NewHost),
                            target,
                            tokens,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(tokens.faint)
                        .child(t!("workspace.start.quick_connect"))
                        .when_some(quick_connect, |keys, kbd| keys.child(kbd)),
                ),
        )
}

/// 开始卡片上的大按钮：彩色图标块、名称和一行说明。
#[allow(clippy::too_many_arguments)]
fn big_button(
    id: &'static str,
    icon: CatalogIcon,
    (icon_bg, icon_fg): (Hsla, Hsla),
    title: SharedString,
    hint: SharedString,
    action: Box<dyn Action>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let target = target.clone();
    let hover = tokens.card_focused;
    div()
        .id(id)
        .test_support()
        .aria_label(title.clone())
        .flex()
        .flex_1()
        .items_center()
        .gap(px(12.))
        .h(px(64.))
        .px(px(16.))
        .rounded(CARD_RADIUS)
        .bg(tokens.card_focused)
        .border_1()
        .border_color(tokens.card_focused_border)
        .shadow(vec![BoxShadow {
            color: tokens.shadow_focused,
            offset: point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        }])
        .cursor_pointer()
        .hover(move |style| style.bg(hover.opacity(0.92)))
        .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.))
                .rounded(px(9.))
                .bg(icon_bg)
                .text_color(icon_fg)
                .child(Icon::new(icon).size(px(18.))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(title)
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(hint),
                ),
        )
}
