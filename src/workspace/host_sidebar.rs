//! 主机栏（宽 236，可以收起）：顶部是标志和收起按钮，下面是「主机 N」标题加「+」、
//! 主机列表，底部是设置按钮。
//!
//! 搜索框和平铺主机列表是 `crate::host::HostList`（工单 11），这里只画外框。
//! 扩展点：底部「锁定保险库」按钮（工单 13）加在设置按钮后面。

use gpui_kit::*;

use super::icon_button::IconButton;
use super::start_card::logo;
use crate::app::{CatalogIcon, NewHost, OpenSettings, PRODUCT_NAME, ToggleHostSidebar};
use crate::host::HostList;
use crate::i18n::t;
use crate::theme::{GAP, HOST_SIDEBAR_WIDTH, TITLE_BAR_HEIGHT, Tokens};

/// macOS 上给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(68.);

/// `count` 是保存的主机数（「主机 N」），`list` 是搜索框加主机列表。
pub(super) fn render_host_sidebar(
    count: usize,
    list: Entity<HostList>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
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
                        .id("host-count")
                        .test_support()
                        .aria_label(count.to_string())
                        .flex_1()
                        .text_size(px(12.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(count.to_string()),
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
        .child(list)
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
