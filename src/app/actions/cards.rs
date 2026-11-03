//! 卡片的 action（工单 02 建，05 补全分屏、放大、焦点移动等）。

use gpui_kit::Action;

use crate::card::CardId;

/// 关闭这张卡片和它的 shell（卡片头部的 ×）。标签里没有别的卡片了，标签一起关。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseCard(pub CardId);
