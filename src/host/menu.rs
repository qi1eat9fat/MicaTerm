// 参考 since2006/shell-rs @ b2a307a: src/host/host_panel.rs 的 `host_menu` 重写
// 裁剪：去掉「复制 ID」；按全部界面原型的顺序加上「在右侧 / 下方分屏打开」。

//! 主机的右键菜单（原型「主机右键菜单」）：连接 / 在右侧分屏打开 / 在下方分屏打开 /
//! 打开 SFTP / 编辑… / 创建副本 / 复制 IP（域名时叫「复制主机名」）/ 删除。
//!
//! 菜单派发的 action 一律送到窗口壳的焦点（`target`）。

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use qishell_hosts::Host;

use crate::app::{
    ConnectHost, ConnectHostDown, ConnectHostRight, CopyHostAddress, DeleteHost, DuplicateHost,
    EditHost, OpenHostSftp,
};
use crate::i18n::t;

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
            .menu(t!("host.menu.connect"), Box::new(ConnectHost(id)))
            .menu(t!("host.menu.split_right"), Box::new(ConnectHostRight(id)))
            .menu(t!("host.menu.split_down"), Box::new(ConnectHostDown(id)))
            .separator()
            .menu(t!("host.menu.open_sftp"), Box::new(OpenHostSftp(id)))
            .separator()
            .menu(t!("host.menu.edit"), Box::new(EditHost(id)))
            .menu(t!("host.menu.duplicate"), Box::new(DuplicateHost(id)))
            .menu(copy_address, Box::new(CopyHostAddress(id)))
            .separator()
            .menu(t!("host.menu.delete"), Box::new(DeleteHost(id)))
    })
}
