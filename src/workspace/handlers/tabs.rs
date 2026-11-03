//! 标签的处理器（工单 02、04）。

use gpui_kit::*;

use crate::app::NewLocalTerminalTab;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_tabs_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_new_local_terminal_tab))
}

impl Workspace {
    /// 本地终端卡片在工单 02 接上。
    fn on_new_local_terminal_tab(
        &mut self,
        _: &NewLocalTerminalTab,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
}
