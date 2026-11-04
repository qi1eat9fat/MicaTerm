//! 卡片的 action（工单 02 建，05 补全分屏、放大、焦点移动等）。

use gpui_kit::Action;

use crate::card::CardId;
use crate::split::{Axis, Direction, NodeId};

/// 关闭这张卡片和它的 shell（卡片头部的 ×）。标签里没有别的卡片了，标签一起关。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseCard(pub CardId);

/// 把焦点卡片向右分屏（Ctrl+Shift+D）。本地卡片在同一个目录里开新的本地终端。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct SplitRight;

/// 把焦点卡片向下分屏（Ctrl+Shift+E）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct SplitDown;

/// 卡片头部的「向右分屏」「向下分屏」：分的是按钮所在的那张卡片。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct SplitCard(pub CardId, pub Axis);

/// 关闭焦点卡片（Ctrl+Shift+W）。标签里没有别的卡片了，标签一起关。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseFocusedCard;

/// 放大或还原焦点卡片（Ctrl+Shift+Enter）。只改显示，不改分屏树的结构。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ToggleZoomCard;

/// 卡片头部的「放大」按钮、双击头部：放大或还原这张卡片。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ZoomCard(pub CardId);

/// 焦点移到那一边紧挨着的卡片（Alt+方向键）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct FocusCard(pub Direction);

/// 把焦点卡片最近的那条同方向的分隔线往那一边推（Alt+Shift+方向键）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct MoveDivider(pub Direction);

/// 双击间隙：这条分隔线恢复成一半。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ResetDivider(pub NodeId);

/// 拖完间隙：这条分隔线的比例变成 `ratio`（限制在 0.08–0.92）。由 resizable 组件在松开鼠标时派发。
#[derive(Action, Clone, Copy, Debug, PartialEq)]
#[action(namespace = qishell, no_json)]
pub struct ResizeDivider {
    pub node: NodeId,
    pub ratio: f32,
}
