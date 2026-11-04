//! 设置的处理器：打开设置对话框（工单 07）。

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use crate::app::OpenSettings;
use crate::settings::open_settings_dialog;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_settings_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_open_settings))
}

impl Workspace {
    /// 打开设置对话框（Ctrl+, 或主机栏底部的齿轮）。已经开着对话框时不再叠一个。
    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        open_settings_dialog(self.settings.clone(), window, cx);
    }
}
