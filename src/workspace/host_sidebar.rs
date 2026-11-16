//! 主机栏（26 号 `.side`，宽 232，可以收起）：
//!
//! - 顶部一行高 32，和标签栏对齐：标志、MicaTerm、新建主机按钮、收起主机栏按钮（2026-11-16 用户加）；
//!   这一行的空白处能拖动窗口。
//! - 下面是常驻的搜索框和平铺主机列表，是 `crate::host::HostList`（工单 11），一直到窗口底。
//!   保险库的锁和设置按钮都在标题栏右侧。
//!
//! 文字一律 400（用户定：侧栏不加粗）。收起主机栏用顶部的按钮或 Ctrl+Shift+B，收起后标签栏最左边出现「展开主机栏」。

use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::app::{
    CatalogIcon, MARK_ICON, NewHost, PRODUCT_NAME, SERVER_PLUS_ICON, ToggleHostSidebar,
};
use crate::host::HostList;
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::icon_button::IconButton;

/// macOS 上给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(68.);

/// `list` 是搜索框加主机列表。
pub(super) fn render_host_sidebar(
    list: Entity<HostList>,
    target: &FocusHandle,
    tokens: &Tokens,
    scale: TypeScale,
) -> impl IntoElement {
    div()
        .id("host-sidebar")
        .test_support()
        .aria_label(t!("workspace.host_sidebar.title"))
        .flex()
        .flex_col()
        .flex_none()
        .w(size::SIDEBAR_WIDTH)
        .h_full()
        .px(px(8.))
        .text_size(scale.ui)
        .text_color(tokens.label)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .h(size::TAB_BAR)
                .pl(if cfg!(target_os = "macos") {
                    TRAFFIC_LIGHTS_WIDTH
                } else {
                    px(8.)
                })
                .child(
                    div()
                        .id("host-sidebar-drag")
                        .flex()
                        .flex_1()
                        .h_full()
                        .items_center()
                        .gap(px(8.))
                        .text_color(tokens.text)
                        .window_control_area(WindowControlArea::Drag)
                        .child(Icon::default().path(MARK_ICON).size(px(18.)))
                        .child(PRODUCT_NAME),
                )
                .child(
                    IconButton::new(
                        "host-sidebar-new-host",
                        // 服务器加号（铅笔像「编辑」，光加号又和旁边标签栏的「新建标签」撞，用户定）。
                        Icon::default().path(SERVER_PLUS_ICON),
                        t!("workspace.host_sidebar.new_host"),
                    )
                    // 不再常亮底色（2026-11-16 用户：旁边加了收起按钮后像一直选中着）：和收起按钮一样，悬停才有底色。
                    .size(size::BAR_BUTTON, size::BAR_ICON)
                    .tooltip(t!("workspace.host_sidebar.new_host"))
                    .dispatch(Box::new(NewHost), target, tokens),
                )
                // 收起主机栏：和标签栏最左边的「展开主机栏」是一对。
                .child(
                    IconButton::new(
                        "collapse-host-sidebar",
                        CatalogIcon::PanelLeftClose,
                        t!("workspace.host_sidebar.collapse"),
                    )
                    .size(size::BAR_BUTTON, size::BAR_ICON)
                    .tooltip(t!("workspace.host_sidebar.collapse"))
                    .dispatch(Box::new(ToggleHostSidebar), target, tokens),
                ),
        )
        .child(list)
}
