// 参考 since2006/shell-rs @ b2a307a: src/host/host_panel.rs 的 `host_menu` 重写
// 裁剪：去掉「复制 ID」。

//! 主机的右键菜单（原型「主机右键菜单」）：连接 / 打开 SFTP / 编辑… / 创建副本 /
//! 复制 IP（域名时叫「复制主机名」）/ 删除。分屏打开主机不在这里（2026-11-14 分屏重做）：
//! 在卡片上分屏，再在新卡片里选主机。
//!
//! 样式是共用的 `micaterm_ui::Menu`（26 号「主机右键」），删除是红字。菜单派发的 action 一律送到
//! 窗口壳的焦点（`target`）。

use gpui_kit::*;
use micaterm_ui::Menu;

use micaterm_hosts::Host;

use crate::app::{ConnectHost, CopyHostAddress, DeleteHost, DuplicateHost, EditHost, OpenHostSftp};
use crate::i18n::t;

/// 一台主机的右键菜单，按打开时的主机建。
pub fn build_host_menu(
    host: &Host,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Menu> {
    let id = host.id;
    let copy_address = if host.address_is_ip() {
        t!("host.menu.copy_ip")
    } else {
        t!("host.menu.copy_host_name")
    };
    let target = target.clone();
    Menu::build(window, cx, move |menu, _, _| {
        menu.action_context(target)
            .menu(t!("host.menu.connect"), Box::new(ConnectHost(id)))
            // 回车由主机列表自己处理（选中一行后回车连接），没有绑定可查。
            .keys("Enter")
            .separator()
            .menu(t!("host.menu.open_sftp"), Box::new(OpenHostSftp(id)))
            .separator()
            .menu(t!("host.menu.edit"), Box::new(EditHost(id)))
            .menu(t!("host.menu.duplicate"), Box::new(DuplicateHost(id)))
            .menu(copy_address, Box::new(CopyHostAddress(id)))
            .separator()
            .danger(t!("host.menu.delete"), Box::new(DeleteHost(id)), false)
    })
}
