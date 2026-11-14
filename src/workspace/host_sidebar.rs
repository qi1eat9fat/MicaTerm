//! 主机栏（26 号 `.side`，宽 232，可以收起）：
//!
//! - 顶部一行高 32，和标签栏对齐：标志、MicaTerm、新建主机按钮；这一行的空白处能拖动窗口。
//! - 下面是常驻的搜索框和平铺主机列表，是 `crate::host::HostList`（工单 11）。
//! - 底部一行高 28，和右边窗格的状态栏对齐：只放保险库状态（「保险库已解锁 / 已锁定」，点它锁定或解锁）；
//!   没设主密码（或没有保险库）时这一行是空的。设置按钮在标题栏右侧。
//!
//! 文字一律 400（用户定：侧栏不加粗）。收起主机栏用 Ctrl+Shift+B，收起后标签栏最左边出现「展开主机栏」。

use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::app::{
    CatalogIcon, LockVault, MARK_ICON, NewHost, PRODUCT_NAME, SERVER_PLUS_ICON, UnlockVault,
};
use crate::host::HostList;
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::icon_button::IconButton;
use crate::vault::VaultStatus;

/// macOS 上给红绿灯让出的宽度。
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(68.);

/// `list` 是搜索框加主机列表，`vault` 是保险库的状态（没有保险库时是 `None`）。
pub(super) fn render_host_sidebar(
    list: Entity<HostList>,
    vault: Option<VaultStatus>,
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
                    .size(size::BAR_BUTTON, size::BAR_ICON)
                    .radius(size::RADIUS_LG)
                    .primary(true)
                    .tooltip(t!("workspace.host_sidebar.new_host"))
                    .dispatch(Box::new(NewHost), target, tokens),
                ),
        )
        .child(list)
        .child(render_vault_row(vault, target, tokens))
}

/// 底部保险库一行（26 号 `.side-foot`）：和状态栏同高 28、同样贴窗口底边；上面留一条透明的 1px，
/// 让文字中线和状态栏（有 1px 上边线）对齐。解锁着点了立即锁定；锁着点了弹解锁框。
/// 没设主密码（或者没有保险库）时是空的：没有锁定这回事。
fn render_vault_row(
    vault: Option<VaultStatus>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let row = div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::STATUS_BAR)
        .border_t(size::LINE)
        .border_color(transparent_black())
        .pl(px(8.));
    let (id, icon, text, label, action): (_, _, _, _, Box<dyn Action>) = match vault {
        Some(VaultStatus::Unlocked { .. }) => (
            "lock-vault",
            CatalogIcon::LockOpen,
            t!("vault.sidebar.unlocked"),
            t!("vault.sidebar.lock"),
            Box::new(LockVault),
        ),
        Some(VaultStatus::Locked) => (
            "unlock-vault",
            CatalogIcon::Lock,
            t!("vault.sidebar.locked"),
            t!("vault.sidebar.unlock"),
            Box::new(UnlockVault),
        ),
        _ => return row,
    };
    let target = target.clone();
    let text_color = tokens.text;
    row.child(
        div()
            .id(id)
            .test_support()
            .aria_label(label)
            .flex()
            .items_center()
            .gap(px(6.))
            .text_color(tokens.text2)
            .cursor_pointer()
            .hover(move |style| style.text_color(text_color))
            .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
            // 锁身实心在下、锁梁细在上，几何居中看着偏低：上移 1px（26 号 `.side-foot .lk svg`）。
            .child(
                div()
                    .flex_none()
                    .relative()
                    .top(px(-1.))
                    .child(Icon::new(icon).size(px(13.))),
            )
            .child(text),
    )
}
