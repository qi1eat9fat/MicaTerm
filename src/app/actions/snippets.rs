//! 命令片段的 action（工单 19）。展开、收起面板的 `ShowSnippets` 在 `window` 里（右侧栏图标）。

use gpui_kit::{Action, Pixels, Point};

use crate::host::SnippetId;

/// 新建命令片段：打开片段对话框。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct NewSnippet;

/// 编辑这个片段：打开片段对话框。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct EditSnippet(pub SnippetId);

/// 删除这个片段（先确认）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct DeleteSnippet(pub SnippetId);

/// 把这个片段的命令复制到剪贴板。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct CopySnippet(pub SnippetId);

/// 把片段的命令输入到焦点卡片的终端提示符后面（不自动执行）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct EnterSnippet(pub SnippetId);

/// 在 `position`（窗口坐标）为这个片段开右键菜单。
#[derive(Action, Clone, Copy, Debug, PartialEq)]
#[action(namespace = micaterm, no_json)]
pub struct OpenSnippetMenu {
    pub snippet: SnippetId,
    pub position: Point<Pixels>,
}

/// 收起面板底部「没有发出去」的提示。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct DismissSnippetNotice;
