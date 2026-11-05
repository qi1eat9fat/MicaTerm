//! 快速连接的处理器（工单 18）：「⌄」、Ctrl+Shift+O 开关弹层。弹层的开关、位置和焦点在 `workspace/quick_connect.rs`。

use gpui_kit::*;

use crate::app::QuickConnect;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_quick_connect_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_quick_connect))
}

impl Workspace {
    fn on_quick_connect(&mut self, _: &QuickConnect, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_quick_connect(window, cx);
    }
}
