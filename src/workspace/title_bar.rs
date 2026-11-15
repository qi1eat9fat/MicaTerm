//! 主区域顶部的标题栏，也就是 26 号的标签栏（高 32、比内容暗一档、下边一条细线）：从左到右是
//! 「展开主机栏」（主机栏收起时）、标签、「+」（新建本地终端标签）、拖动区、
//! 深浅切换、保险库锁（设了主密码时）、设置，最后是窗口按钮。分屏按钮在每张卡片自己的状态栏上（2026-11-14 分屏重做）。
//!
//! 不用 gpui-component 的 `TitleBar`：它把子元素整块当作拖动区。这里只给 `title-drag`
//! 那块空白标 `WindowControlArea::Drag`，标签轨道和按钮都不在拖动区里。
//! 标签轨道（`tab-rail`）里的东西由 `tabs::TabRail` 画。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::window_buttons::render_window_buttons;
use crate::ui::icon_button::IconButton;
use gpui_kit::component::ActiveTheme as _;

use crate::app::{
    CatalogIcon, LockVault, NewLocalTerminalTab, OpenSettings, ToggleAppearance, ToggleHostSidebar,
    UnlockVault,
};
use crate::i18n::t;
use crate::tabs::{TabRail, TabSnapshot};
use crate::theme::{Tokens, size};
use crate::vault::VaultStatus;

/// macOS 上主机栏收起时，给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(72.);

/// 画标题栏要的窗口壳状态。
pub(super) struct TitleBarState<'a> {
    /// 主机栏开着：不画「展开主机栏」。
    pub host_sidebar_open: bool,
    /// 标签轨道：画 `tabs`，也接住拖着的标签（工单 29）。
    pub rail: &'a TabRail,
    pub tabs: Vec<TabSnapshot>,
    /// 保险库的状态（没有保险库时是 `None`），决定画不画锁、画哪一把。
    pub vault: Option<VaultStatus>,
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
        vault,
    } = state;
    let dark = cx.theme().is_dark();
    let tabs = rail.render(tabs, target, tokens, window, cx);
    let bar_button = |id: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id, icon, label.clone())
            .size(size::BAR_BUTTON, size::BAR_ICON)
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
        // 下边线画在最底下：在前的标签有内容底色，盖住它，和下面的内容连成一片（26 号 `.tab` 的 margin-bottom: -1px）。
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(size::LINE)
                .bg(tokens.line),
        )
        .when(!host_sidebar_open, |bar| {
            bar.when(cfg!(target_os = "macos"), |bar| {
                bar.pl(TRAFFIC_LIGHTS_WIDTH)
            })
            .child(
                div().flex_none().px(px(4.)).child(
                    IconButton::new(
                        "expand-host-sidebar",
                        CatalogIcon::PanelLeftOpen,
                        t!("workspace.host_sidebar.expand"),
                    )
                    .size(size::BAR_BUTTON, size::BAR_ICON)
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
                .ml(px(6.))
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
        .child(
            // 26 号 `.tools`：深浅切换、保险库锁、设置。
            div()
                .id("title-tools")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .px(px(6.))
                .child(
                    // 浅色时显示月亮（点了切到深色），深色时显示太阳。
                    if dark {
                        bar_button(
                            "toggle-appearance",
                            CatalogIcon::Sun,
                            t!("workspace.title_bar.to_light"),
                        )
                    } else {
                        bar_button(
                            "toggle-appearance",
                            CatalogIcon::Moon,
                            t!("workspace.title_bar.to_dark"),
                        )
                    }
                    .dispatch(Box::new(ToggleAppearance), target, tokens),
                )
                .children(vault_button(vault).map(|(id, icon, label, action)| {
                    bar_button(id, icon, label).dispatch(action, target, tokens)
                }))
                .child(
                    bar_button(
                        "open-settings",
                        CatalogIcon::Settings,
                        t!("workspace.title_bar.settings"),
                    )
                    .dispatch(Box::new(OpenSettings), target, tokens),
                ),
        )
        .child(render_window_buttons(tokens, window))
}

/// 保险库锁：解锁着显示打开的锁，点了立即锁定；锁着显示锁，点了弹解锁框。
/// 没设主密码（或者没有保险库）时不画：没有锁定这回事。
fn vault_button(
    vault: Option<VaultStatus>,
) -> Option<(&'static str, CatalogIcon, SharedString, Box<dyn Action>)> {
    match vault? {
        VaultStatus::Unlocked { .. } => Some((
            "lock-vault",
            CatalogIcon::LockOpen,
            t!("vault.title_bar.lock"),
            Box::new(LockVault),
        )),
        VaultStatus::Locked => Some((
            "unlock-vault",
            CatalogIcon::Lock,
            t!("vault.title_bar.unlock"),
            Box::new(UnlockVault),
        )),
        _ => None,
    }
}
