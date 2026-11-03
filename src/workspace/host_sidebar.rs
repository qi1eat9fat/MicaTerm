//! 主机栏（宽 236，可以收起）：顶部是标志和收起按钮，下面是「主机 N」标题加「+」、
//! 主机列表，底部是设置按钮。
//!
//! M1 还没有主机：列表处显示「还没有主机」的引导。搜索框、平铺主机列表、拖动排序、
//! 「锁定保险库」按钮在 M2（工单 11、13）加上。

use gpui_kit::*;

use super::icon_button::IconButton;
use super::start_card::logo;
use crate::app::{CatalogIcon, NewHost, OpenSettings, PRODUCT_NAME, ToggleHostSidebar};
use crate::i18n::t;
use crate::theme::{GAP, HOST_SIDEBAR_WIDTH, TITLE_BAR_HEIGHT, Tokens};

/// macOS 上给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(68.);

pub(super) fn render_host_sidebar(target: &FocusHandle, tokens: &Tokens) -> impl IntoElement {
    div()
        .id("host-sidebar")
        .test_support()
        .aria_label(t!("workspace.host_sidebar.title"))
        .flex()
        .flex_col()
        .flex_none()
        .w(HOST_SIDEBAR_WIDTH)
        .h_full()
        .px(GAP)
        .pb(GAP)
        .child(
            // 顶部一行和标题栏同高；标志那一块也能拖动窗口。
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .h(TITLE_BAR_HEIGHT)
                .pl(if cfg!(target_os = "macos") {
                    TRAFFIC_LIGHTS_WIDTH
                } else {
                    px(4.)
                })
                .child(
                    div()
                        .id("host-sidebar-drag")
                        .flex()
                        .flex_1()
                        .h_full()
                        .items_center()
                        .gap(px(7.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::BOLD)
                        .opacity(0.8)
                        .window_control_area(WindowControlArea::Drag)
                        .child(logo(px(18.), px(5.)))
                        .child(PRODUCT_NAME),
                )
                .child(
                    IconButton::new(
                        "collapse-host-sidebar",
                        CatalogIcon::PanelLeftClose,
                        t!("workspace.host_sidebar.collapse"),
                    )
                    .dispatch(Box::new(ToggleHostSidebar), target, tokens),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .pt(px(4.))
                .pb(GAP)
                .pl(GAP)
                .pr(px(2.))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t!("workspace.host_sidebar.title"))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child("0"),
                )
                .child(
                    IconButton::new(
                        "host-sidebar-new-host",
                        CatalogIcon::Plus,
                        t!("workspace.host_sidebar.new_host"),
                    )
                    .size(px(22.), px(7.))
                    .dispatch(Box::new(NewHost), target, tokens),
                ),
        )
        .child(
            div()
                .id("host-list")
                .test_support()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .items_center()
                .pt(px(40.))
                .px(px(14.))
                .text_size(px(13.))
                .text_color(tokens.faint)
                .child(t!("workspace.host_sidebar.empty"))
                .child(
                    div()
                        .text_size(px(11.5))
                        .child(t!("workspace.host_sidebar.empty_hint")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .pt(px(6.))
                .px(px(2.))
                .child(
                    IconButton::new(
                        "open-settings",
                        CatalogIcon::Settings,
                        t!("workspace.host_sidebar.settings"),
                    )
                    .dispatch(Box::new(OpenSettings), target, tokens),
                ),
        )
}
