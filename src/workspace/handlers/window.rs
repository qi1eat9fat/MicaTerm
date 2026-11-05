//! 窗口壳的处理器：主机栏、右侧栏。新建主机在 `hosts`（工单 11），快速连接在 `quick_connect`（工单 18）。

use gpui_kit::*;

use crate::app::{OpenSftp, ShowCredentials, ToggleHostSidebar};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_window_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_toggle_host_sidebar))
        .on_action(cx.listener(Workspace::on_open_sftp))
        .on_action(cx.listener(Workspace::on_show_credentials))
}

impl Workspace {
    fn on_toggle_host_sidebar(
        &mut self,
        _: &ToggleHostSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host_sidebar_open = !self.host_sidebar_open;
        cx.notify();
    }

    /// SFTP 标签在 M3 接上。
    fn on_open_sftp(&mut self, _: &OpenSftp, _: &mut Window, _: &mut Context<Self>) {}

    /// 凭据面板在 M2（工单 14）接上。
    fn on_show_credentials(&mut self, _: &ShowCredentials, _: &mut Window, _: &mut Context<Self>) {}
}
