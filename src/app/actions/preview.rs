//! 预览里图片的 action（工单 24）：焦点在预览里时按键才到这里，由预览自己处理。

use gpui_kit::Action;

/// 放大一档。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ZoomPreviewIn;

/// 缩小一档。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ZoomPreviewOut;

/// 适合窗口。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct FitPreview;

/// 原图（100%）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ActualSizePreview;
