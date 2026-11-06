//! 编辑器标签的 action（工单 23）：发给在前的编辑器标签。

use gpui_kit::Action;

/// 保存在前的编辑器标签（Ctrl+S、头部的「保存」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct SaveEditor;

/// 重新读在前的编辑器标签的文件（头部的「重新加载」），有未保存的修改先问。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ReloadEditor;
