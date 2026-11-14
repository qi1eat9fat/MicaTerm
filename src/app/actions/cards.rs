//! 卡片的 action（工单 02 建，05 补全分屏、放大、焦点移动等）。

use gpui_kit::Action;

use crate::card::CardId;
use crate::split::{Axis, Direction, NodeId};

/// 关闭这张卡片和它的 shell（终端右键菜单「关闭」）。标签里没有别的卡片了，标签一起关。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct CloseCard(pub CardId);

/// 把焦点卡片向右分屏（Ctrl+Shift+D）：右边出现一张新卡片，在里面选开什么（再开一个、本地终端、主机）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SplitRight;

/// 把焦点卡片向下分屏（Ctrl+Shift+E），同 [`SplitRight`]。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SplitDown;

/// 卡片状态栏上的「向右分屏」「向下分屏」：分的是这张卡片（先让它成为焦点卡片）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SplitCard {
    pub card: CardId,
    pub axis: Axis,
}

/// 终端右键菜单「移到新标签」：把这张卡片从分屏里拆出去，单独成一个标签。标签里只有它时什么都不做。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct MoveCardToNewTab(pub CardId);

/// 关闭焦点卡片（Ctrl+Shift+W）。标签里没有别的卡片了，标签一起关。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct CloseFocusedCard;

/// 放大或还原焦点卡片（Ctrl+Shift+Enter）。只改显示，不改分屏树的结构。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ToggleZoomCard;

/// 终端右键菜单「放大」「还原」、双击状态栏、状态栏上的「还原」：放大或还原这张卡片。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ZoomCard(pub CardId);

/// 焦点移到那一边紧挨着的卡片（Alt+方向键）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct FocusCard(pub Direction);

/// 把焦点卡片最近的那条同方向的分隔线往那一边推（Alt+Shift+方向键）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct MoveDivider(pub Direction);

/// 双击间隙：这条分隔线恢复成一半。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ResetDivider(pub NodeId);

/// 拖完间隙：这条分隔线的比例变成 `ratio`（限制在 0.08–0.92）。由 resizable 组件在松开鼠标时派发。
#[derive(Action, Clone, Copy, Debug, PartialEq)]
#[action(namespace = micaterm, no_json)]
pub struct ResizeDivider {
    pub node: NodeId,
    pub ratio: f32,
}
