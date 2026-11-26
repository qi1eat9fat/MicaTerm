//! 主机栏（宽 240，可以收起成 44）：照空壳原型的侧栏。
//!
//! - 顶部一行高 28（和标签栏同高）：标志和 MicaTerm，空白处能拖动窗口；收起到 44 宽时只留标志。
//! - 下面是常驻的搜索行（搜索框 + 新建主机的加号）和平铺主机列表，是 `crate::host::HostList`。
//! - 底部一行：深浅切换、保险库的锁（没设主密码时变灰）、设置，最右是收起主机栏。
//!   收起后只在中间留展开按钮；macOS 收起时整条侧栏让给红绿灯，展开按钮在标签栏最左边。

use gpui_kit::*;

use crate::app::{
    CatalogIcon, LOGO_IMAGE, LockVault, OpenSettings, PRODUCT_NAME, ToggleAppearance,
    ToggleHostSidebar, UnlockVault,
};
use crate::host::HostList;
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, TypeScale, WEIGHT_SEMIBOLD, size};
use crate::ui::icon_button::IconButton;
use crate::vault::VaultStatus;

/// `list` 是搜索框加主机列表。`vault` 是保险库的状态，决定画不画锁、画哪一把。
pub(super) fn render_host_sidebar(
    list: Entity<HostList>,
    vault: Option<VaultStatus>,
    dark: bool,
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
        .text_size(scale.body)
        .text_color(tokens.label)
        .child(render_top(false, tokens, scale))
        .child(list)
        .child(render_footer(false, vault, dark, target, tokens))
}

/// 收起后的主机栏：只有标志和收起按钮，列表和底部工具都不画。
pub(super) fn render_collapsed_host_sidebar(
    vault: Option<VaultStatus>,
    dark: bool,
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
        .w(size::SIDEBAR_COLLAPSED)
        .h_full()
        .text_size(scale.body)
        .text_color(tokens.label)
        .child(render_top(true, tokens, scale))
        .child(render_footer(true, vault, dark, target, tokens))
}

/// 顶部一行：标志和名字；空白处能拖动窗口。`collapsed` 时只留居中的标志。
fn render_top(collapsed: bool, tokens: &Tokens, scale: TypeScale) -> Div {
    let logo = img(LOGO_IMAGE).size(size::ICON_LG).flex_none();
    if collapsed {
        return div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(size::TAB_BAR)
            .text_color(tokens.text)
            .child(
                div()
                    .id("host-sidebar-drag")
                    .flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .window_control_area(WindowControlArea::Drag)
                    .child(logo),
            );
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::TAB_BAR)
        .pl(if cfg!(target_os = "macos") {
            size::TRAFFIC_LIGHTS_WIDTH
        } else {
            px(10.)
        })
        .pr(space::SPACE_12)
        .child(
            div()
                .id("host-sidebar-drag")
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .gap(space::SPACE_8)
                .text_color(tokens.text)
                .window_control_area(WindowControlArea::Drag)
                .child(logo)
                .child(
                    div()
                        .text_size(scale.sm)
                        .font_weight(WEIGHT_SEMIBOLD)
                        .child(PRODUCT_NAME),
                ),
        )
}

/// 底部一行：深浅切换、保险库的锁、设置，最右是收起。收起后只有居中的收起按钮。
fn render_footer(
    collapsed: bool,
    vault: Option<VaultStatus>,
    dark: bool,
    target: &FocusHandle,
    tokens: &Tokens,
) -> Div {
    let button = |id: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id, icon, label.clone())
            .sm()
            .tooltip(label)
    };
    let (toggle_id, toggle_icon, toggle_label) = if collapsed {
        (
            "expand-host-sidebar",
            CatalogIcon::PanelRight,
            t!("workspace.host_sidebar.expand"),
        )
    } else {
        (
            "collapse-host-sidebar",
            CatalogIcon::PanelLeft,
            t!("workspace.host_sidebar.collapse"),
        )
    };
    let toggle = button(toggle_id, toggle_icon, toggle_label).dispatch(
        Box::new(ToggleHostSidebar),
        target,
        tokens,
    );
    // 顶上补一条与右侧状态栏等厚的透明 1px 边，保证 Taffy border-box 下按钮中心与状态栏完全同高。
    let mut footer = div()
        .flex()
        .flex_none()
        .items_center()
        .mt_auto()
        .h(size::STATUS_BAR)
        .border_t(size::LINE)
        .border_color(transparent_black())
        .px(space::SPACE_6)
        .text_color(tokens.st_dim);
    if collapsed {
        return footer.justify_center().child(toggle);
    }
    let theme = button(
        "toggle-appearance",
        if dark {
            CatalogIcon::Sun
        } else {
            CatalogIcon::Moon
        },
        if dark {
            t!("workspace.title_bar.to_light")
        } else {
            t!("workspace.title_bar.to_dark")
        },
    )
    .dispatch(Box::new(ToggleAppearance), target, tokens);
    let settings = button(
        "open-settings",
        CatalogIcon::Settings,
        t!("workspace.title_bar.settings"),
    )
    .dispatch(Box::new(OpenSettings), target, tokens);
    // 顺序：深浅切换、保险库的锁、设置（锁夹在中间，2026-11-24 用户定）。
    let (id, icon, label, action, enabled) = vault_button(vault);
    footer = footer
        .child(theme)
        .child(
            button(id, icon, label)
                .dimmed(!enabled)
                .dispatch(action, target, tokens),
        )
        .child(settings);
    footer.child(div().flex().items_center().ml_auto().child(toggle))
}

/// 保险库锁：解锁着显示打开的锁，点了立即锁定；锁着显示锁，点了弹解锁框。
/// 没设主密码、没有保险库或保险库打不开时也画同一把锁，但变灰、点了没反应，悬停说为什么。
/// 返回 `(id, 图标, 文案, action, 可用)`。
fn vault_button(
    vault: Option<VaultStatus>,
) -> (
    &'static str,
    CatalogIcon,
    SharedString,
    Box<dyn Action>,
    bool,
) {
    match vault {
        Some(VaultStatus::Unlocked { .. }) => (
            "lock-vault",
            CatalogIcon::LockOpen,
            t!("vault.title_bar.lock"),
            Box::new(LockVault),
            true,
        ),
        Some(VaultStatus::Locked) => (
            "unlock-vault",
            CatalogIcon::Lock,
            t!("vault.title_bar.unlock"),
            Box::new(UnlockVault),
            true,
        ),
        Some(VaultStatus::KeyLost) => (
            "vault-lock-unavailable",
            CatalogIcon::Lock,
            t!("vault.title_bar.key_lost"),
            Box::new(UnlockVault),
            false,
        ),
        _ => (
            "vault-lock-unavailable",
            CatalogIcon::Lock,
            t!("vault.title_bar.no_master_password"),
            Box::new(LockVault),
            false,
        ),
    }
}
