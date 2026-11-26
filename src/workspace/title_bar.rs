//! 主区域顶部的标签栏（高 36、和内容同底、下边一条细线）：从左到右是
//! 「展开主机栏」（主机栏收起时）、标签、「+」（新建本地终端标签）、拖动区、
//! 右侧面板的入口（文件、片段、凭据、Agent），最后是三色窗口圆点。
//! 深浅切换、设置和保险库的锁在主机栏底部。
//!
//! 不用 gpui-component 的 `TitleBar`：它把子元素整块当作拖动区。这里只给 `title-drag`
//! 那块空白标 `WindowControlArea::Drag`，标签轨道和按钮都不在拖动区里。
//! 标签轨道（`tab-rail`）里的东西由 `tabs::TabRail` 画。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::right_sidebar::{AgentRail, FilesButton, RightPanel, render_panel_tools};
use super::window_buttons::render_window_buttons;
use crate::ui::icon_button::IconButton;

use crate::app::{CatalogIcon, NewLocalTerminalTab, ToggleHostSidebar};
use crate::i18n::t;
use crate::tabs::{TabRail, TabSnapshot};
use crate::theme::space;
use crate::theme::{Tokens, size};

/// 画标题栏要的窗口壳状态。
pub(super) struct TitleBarState<'a> {
    /// 主机栏开着：不画「展开主机栏」。
    pub host_sidebar_open: bool,
    /// 标签轨道：画 `tabs`，也接住拖着的标签（工单 29）。
    pub rail: &'a TabRail,
    pub tabs: Vec<TabSnapshot>,
    /// 右侧展开的面板，决定入口图标按下的样子。
    pub right_panel: Option<RightPanel>,
    /// 「文件」入口现在能不能点。
    pub files: FilesButton,
    /// Agent 入口上的点和悬停提示。
    pub agent: AgentRail,
}

/// 点击和菜单都经 `target`（窗口壳的焦点）派发。
/// 拖着的标签由整条标题栏接住（工单 29）：松开在轨道右端以外（`+`、拖动区）放到最后，
/// 左端以外（「展开主机栏」）放到第一个。
pub(super) fn render_title_bar(
    state: TitleBarState<'_>,
    target: &FocusHandle,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let TitleBarState {
        host_sidebar_open,
        rail,
        tabs,
        right_panel,
        files,
        agent,
    } = state;
    let tabs = rail.render(tabs, target, tokens, window, cx);
    let bar_button = |id: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id, icon, label.clone())
            .sm()
            .tooltip(label)
    };
    div()
        .id("title-bar")
        .map(|bar| rail.accept_drops(bar, target))
        .test_support()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(size::TAB_BAR)
        .bg(tokens.tabbar)
        // 下边线画在最底下，和下面的内容分开。
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(size::LINE)
                .bg(tokens.line),
        )
        // macOS 上主机栏收起时整条侧栏都不画，给红绿灯让位，展开按钮放在标签轨道左边；
        // Windows、Linux 收起后还留 40 宽的侧栏，展开按钮在它底部。
        .when(!host_sidebar_open && cfg!(target_os = "macos"), |bar| {
            bar.pl(size::TRAFFIC_LIGHTS_WIDTH).child(
                div().flex_none().px(space::SPACE_4).child(
                    IconButton::new(
                        "expand-host-sidebar",
                        CatalogIcon::PanelRight,
                        t!("workspace.host_sidebar.expand"),
                    )
                    .sm()
                    .dispatch(Box::new(ToggleHostSidebar), target, tokens),
                ),
            )
        })
        .child(
            div()
                .id("tab-rail")
                .test_support()
                .flex()
                .h_full()
                .min_w_0()
                .children(tabs),
        )
        .child(
            div()
                .id("new-tab-group")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                // 可见空隙取钥匙和 Agent 之间（约 11）的一半：6 里去掉按钮自己的 4 内边距。
                .ml(space::SPACE_2)
                .child(
                    bar_button(
                        "new-tab",
                        CatalogIcon::Plus,
                        t!("workspace.title_bar.new_local_terminal"),
                    )
                    .dispatch(Box::new(NewLocalTerminalTab), target, tokens),
                ),
        )
        .child(
            // 拖动区：Windows 上按住拖动窗口，双击最大化或还原，都由系统完成。
            // 不挂任何处理器（拖着的标签由外面的标题栏接住），免得影响系统拖窗口。
            div()
                .id("title-drag")
                .test_support()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(render_panel_tools(
            right_panel,
            files,
            agent,
            target,
            tokens,
        ))
        .child(render_window_buttons(tokens, window))
}
