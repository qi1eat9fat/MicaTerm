//! 窗口壳的处理器：主机栏、右侧栏。新建主机在 `hosts`（工单 11），快速连接在 `quick_connect`（工单 18），凭据面板在 `credentials`（工单 14）。

use gpui_kit::*;

use crate::app::ToggleHostSidebar;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_window_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_toggle_host_sidebar))
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
}
