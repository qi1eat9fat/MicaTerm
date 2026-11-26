//! 主机的处理器（工单 11）：新建、编辑、创建副本、复制地址、删除、拖动排序、聚焦主机搜索。
//!
//! 连接类的 action：`ConnectHost`（12，在新标签里连接）在这里，`OpenHostSftp` 在 `sftp`（20）。
//! 把主机开在卡片旁边走分屏的新卡片（`handlers::cards`）。

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use crate::app::{
    ConnectHost, CopyHostAddress, DeleteHost, DuplicateHost, EditHost, FocusHostSearch, MoveHost,
    NewHost,
};
use crate::host::{open_delete_host_dialog, open_host_dialog};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_hosts_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_new_host))
        .on_action(cx.listener(Workspace::on_edit_host))
        .on_action(cx.listener(Workspace::on_duplicate_host))
        .on_action(cx.listener(Workspace::on_copy_host_address))
        .on_action(cx.listener(Workspace::on_delete_host))
        .on_action(cx.listener(Workspace::on_move_host))
        .on_action(cx.listener(Workspace::on_focus_host_search))
        .on_action(cx.listener(Workspace::on_connect_host))
}

impl Workspace {
    /// 在新标签里连接主机（双击、回车、右键菜单「连接」）：已经连着就复用连接，不重新登录。
    fn on_connect_host(
        &mut self,
        &ConnectHost(id): &ConnectHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.connect_host(id, window, cx);
    }

    /// 新建主机（主机栏顶部的新建按钮、Ctrl+Shift+N、新标签页的「新建主机」）：弹出模态对话框。
    fn on_new_host(&mut self, _: &NewHost, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        open_host_dialog(
            self.hosts.clone(),
            None,
            self.services.connection_tester.clone(),
            window,
            cx,
        );
    }

    fn on_edit_host(
        &mut self,
        &EditHost(id): &EditHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || self.hosts.read(cx).host(id).is_none() {
            return;
        }
        open_host_dialog(
            self.hosts.clone(),
            Some(id),
            self.services.connection_tester.clone(),
            window,
            cx,
        );
    }

    /// 创建副本：名称后加「 副本」，不带密码，插在原主机后面；主机栏自动选上它。
    fn on_duplicate_host(
        &mut self,
        &DuplicateHost(id): &DuplicateHost,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hosts.update(cx, |store, cx| store.duplicate(id, cx));
    }

    /// 复制 IP / 复制主机名：把地址放进剪贴板。
    fn on_copy_host_address(
        &mut self,
        &CopyHostAddress(id): &CopyHostAddress,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.hosts.read(cx).host(id) {
            cx.write_to_clipboard(ClipboardItem::new_string(host.address.to_string()));
        }
    }

    /// 删除主机：先确认。已经打开的卡片不受影响。
    fn on_delete_host(
        &mut self,
        &DeleteHost(id): &DeleteHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        open_delete_host_dialog(self.hosts.clone(), id, window, cx);
    }

    /// 拖动排序：放到另一台主机的前面或后面。
    fn on_move_host(
        &mut self,
        &MoveHost { host, drop }: &MoveHost,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hosts
            .update(cx, |store, cx| store.move_host(host, drop, cx));
    }

    /// 聚焦主机搜索（Ctrl+Shift+H）：主机栏收着时先展开。
    fn on_focus_host_search(
        &mut self,
        _: &FocusHostSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.host_sidebar_open {
            self.host_sidebar_open = true;
            cx.notify();
        }
        self.host_list
            .update(cx, |list, cx| list.focus_search(window, cx));
    }
}
