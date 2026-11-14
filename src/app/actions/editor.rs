//! 编辑器标签的 action（工单 23）：发给在前的编辑器标签。

use gpui_kit::Action;

/// 保存在前的编辑器标签（Ctrl+S、头部的「保存」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SaveEditor;

/// 重新读在前的编辑器标签的文件（头部的「重新加载」），有未保存的修改先问。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ReloadEditor;

/// 编辑器查找浮卡（UI 重做工单 11）：下一个（Enter、F3）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct FindNextInEditor;

/// 上一个（Shift+Enter、Shift+F3）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct FindPreviousInEditor;

/// 关掉查找浮卡（Esc），焦点回到文字。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct DismissEditorFind;

/// 替换当前匹配，跳到下一个（替换框里 Enter）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ReplaceInEditor;

/// 全部替换（Ctrl+Alt+Enter）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ReplaceAllInEditor;
