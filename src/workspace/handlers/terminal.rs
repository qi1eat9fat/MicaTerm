//! 终端的处理器：找到焦点卡片里的终端，转发复制、粘贴、查找、清屏、字号等（工单 03 填写）。

use gpui_kit::*;

use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_terminal_actions(
    root: Stateful<Div>,
    _cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root
}
