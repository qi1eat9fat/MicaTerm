//! 设置的处理器（工单 07）。

use gpui_kit::*;

use crate::app::OpenSettings;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_settings_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_open_settings))
}

impl Workspace {
    /// 设置对话框在工单 07 接上。
    fn on_open_settings(&mut self, _: &OpenSettings, _: &mut Window, _: &mut Context<Self>) {}
}
