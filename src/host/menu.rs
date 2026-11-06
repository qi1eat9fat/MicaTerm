// 参考 since2006/shell-rs @ b2a307a: src/host/host_panel.rs 的 `host_menu` 重写
// 裁剪：去掉「复制 ID」；按全部界面原型的顺序加上「在右侧 / 下方分屏打开」。

//! 主机的右键菜单（原型「主机右键菜单」）：连接 / 在右侧分屏打开 / 在下方分屏打开 /
//! 打开 SFTP / 编辑… / 创建副本 / 复制 IP（域名时叫「复制主机名」）/ 删除。
//!
//! 菜单项全部按 spec 的顺序写好；处理器还没接上的项用下面的开关藏起来，后面的工单接上处理器后
//! 只要把开关改成 `true`。菜单派发的 action 一律送到窗口壳的焦点（`target`）。

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::Host;

use crate::app::{
    ConnectHost, ConnectHostDown, ConnectHostRight, CopyHostAddress, DeleteHost, DuplicateHost,
    EditHost, OpenHostSftp,
};
use crate::i18n::t;

/// 「连接」：工单 12 接上了 `ConnectHost` 的处理器。
pub const SHOW_CONNECT: bool = true;
/// 「在右侧 / 下方分屏打开」：工单 16 接上了 `ConnectHostRight`、`ConnectHostDown` 的处理器。
pub const SHOW_SPLIT_OPEN: bool = true;
/// 「打开 SFTP」：工单 20 接上了 `OpenHostSftp` 的处理器。
pub const SHOW_OPEN_SFTP: bool = true;

/// 一台主机的右键菜单，按打开时的主机建。
pub fn build_host_menu(
    host: &Host,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let id = host.id;
    let copy_address = if host.address_is_ip() {
        t!("host.menu.copy_ip")
    } else {
        t!("host.menu.copy_host_name")
    };
    let target = target.clone();
    PopupMenu::build(window, cx, move |menu, _, _| {
        menu.action_context(target)
            .when(SHOW_CONNECT, |menu| {
                menu.menu(t!("host.menu.connect"), Box::new(ConnectHost(id)))
            })
            .when(SHOW_SPLIT_OPEN, |menu| {
                menu.menu(t!("host.menu.split_right"), Box::new(ConnectHostRight(id)))
                    .menu(t!("host.menu.split_down"), Box::new(ConnectHostDown(id)))
            })
            .when(SHOW_CONNECT || SHOW_SPLIT_OPEN, |menu| menu.separator())
            .when(SHOW_OPEN_SFTP, |menu| {
                menu.menu(t!("host.menu.open_sftp"), Box::new(OpenHostSftp(id)))
                    .separator()
            })
            .menu(t!("host.menu.edit"), Box::new(EditHost(id)))
            .menu(t!("host.menu.duplicate"), Box::new(DuplicateHost(id)))
            .menu(copy_address, Box::new(CopyHostAddress(id)))
            .separator()
            .menu(t!("host.menu.delete"), Box::new(DeleteHost(id)))
    })
}
