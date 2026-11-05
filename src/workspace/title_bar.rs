//! 主区域顶部的标题栏（高 44）：从左到右是「展开主机栏」（主机栏收起时）、标签轨道、
//! 「+」（新建本地终端标签）、「⌄」（快速连接）、拖动区、窗口按钮。
//!
//! 不用 gpui-component 的 `TitleBar`：它把子元素整块当作拖动区。这里只给 `title-drag`
//! 那块空白标 `WindowControlArea::Drag`，标签轨道和按钮都不在拖动区里。
//! 标签轨道（`tab-rail`）里的东西由 `tabs::TabRail` 画好传进来。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::icon_button::IconButton;
use super::quick_connect::QuickConnectAnchor;
use super::window_buttons::render_window_buttons;
use crate::app::{CatalogIcon, NewLocalTerminalTab, QuickConnect, ToggleHostSidebar};
use crate::i18n::t;
use crate::theme::{GAP, TITLE_BAR_HEIGHT, Tokens};

/// macOS 上主机栏收起时，给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(72.);

/// `quick_connect_open`：快速连接开着，「+ ⌄」显示成按下的底色（原型 `.plusgrp.open`）；
/// `anchor` 记下这组按钮画在哪里，弹层挂在它下面。
pub(super) fn render_title_bar(
    host_sidebar_open: bool,
    quick_connect_open: bool,
    anchor: &Rc<QuickConnectAnchor>,
    tabs: Option<impl IntoElement>,
    target: &FocusHandle,
    tokens: &Tokens,
    window: &Window,
) -> impl IntoElement {
    div()
        .id("title-bar")
        .test_support()
        .flex()
        .flex_none()
        .items_center()
        .h(TITLE_BAR_HEIGHT)
        .pl(px(4.))
        .pr(px(6.))
        .gap(px(10.))
        .when(!host_sidebar_open, |bar| {
            bar.pl(if cfg!(target_os = "macos") {
                TRAFFIC_LIGHTS_WIDTH
            } else {
                GAP + px(2.)
            })
            .child(
                IconButton::new(
                    "expand-host-sidebar",
                    CatalogIcon::PanelLeftOpen,
                    t!("workspace.host_sidebar.expand"),
                )
                .dispatch(Box::new(ToggleHostSidebar), target, tokens),
            )
        })
        .child(
            div()
                .id("tab-rail")
                .test_support()
                .flex()
                .items_center()
                .min_w_0()
                .children(tabs),
        )
        .child(
            div()
                .id("new-tab-group")
                .test_support()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .rounded(px(10.))
                .when(quick_connect_open, |group| group.bg(tokens.pressed))
                .child(record_bounds(anchor.clone(), |anchor| &anchor.group))
                .child(
                    IconButton::new(
                        "new-tab",
                        CatalogIcon::Plus,
                        t!("workspace.title_bar.new_local_terminal"),
                    )
                    .dispatch(Box::new(NewLocalTerminalTab), target, tokens),
                )
                .child(
                    div()
                        .relative()
                        .child(record_bounds(anchor.clone(), |anchor| &anchor.chevron))
                        .child(
                            IconButton::new(
                                "quick-connect",
                                CatalogIcon::ChevronDown,
                                t!("workspace.title_bar.quick_connect"),
                            )
                            .size(px(16.), px(10.))
                            .dispatch(
                                Box::new(QuickConnect),
                                target,
                                tokens,
                            ),
                        ),
                ),
        )
        .child(
            // 拖动区：Windows 上按住拖动窗口，双击最大化或还原，都由系统完成。
            div()
                .id("title-drag")
                .test_support()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(render_window_buttons(tokens, window))
}

/// 铺满父元素、不画东西的一块：每帧把父元素的位置记进 `anchor` 的某一格。
fn record_bounds(
    anchor: Rc<QuickConnectAnchor>,
    cell: fn(&QuickConnectAnchor) -> &Cell<Option<Bounds<Pixels>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, _, _| cell(&anchor).set(Some(bounds)),
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
