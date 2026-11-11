//! 标签的 action：新建（02），切换、关闭（04），拖动排序（29）。

use gpui_kit::{Action, Pixels, Point};

use crate::tabs::{TabDrop, TabId};

gpui_kit::actions!(
    qishell,
    [
        /// 新建本地终端标签（Ctrl+Shift+T）：里面是一张本地卡片。
        NewLocalTerminalTab,
        /// 下一个标签（Ctrl+Tab、Ctrl+PageDown），过了最后一个回到第一个。
        NextTab,
        /// 上一个标签（Ctrl+Shift+Tab、Ctrl+PageUp），过了第一个回到最后一个。
        PreviousTab,
        /// 关闭在前的标签和它的全部卡片（macOS 的 ⇧⌘W；Windows 和 Linux 没有快捷键）。
        CloseActiveTab,
    ]
);

/// 切到第 n 个标签（Ctrl+Shift+1…9），从 1 数起；9 表示最后一个。没有这个位置时什么都不做。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct SwitchToTab(pub usize);

/// 把这个标签放到前面（点标签）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ActivateTab(pub TabId);

/// 关闭这个标签和它的全部卡片（标签上的 ×、右键菜单「关闭标签」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseTab(pub TabId);

/// 关闭这个标签以外的全部标签，留下的这个到前面来（右键菜单「关闭其他标签」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseOtherTabs(pub TabId);

/// 关闭这个标签右边的全部标签（右键菜单「关闭右侧的标签」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CloseTabsToRight(pub TabId);

/// 在 `position`（窗口坐标）开这个标签的右键菜单（右键标签）。
#[derive(Action, Clone, Copy, Debug, PartialEq)]
#[action(namespace = qishell, no_json)]
pub struct OpenTabMenu {
    pub tab: TabId,
    pub position: Point<Pixels>,
}

/// 把这个标签放到 `drop` 说的地方（某个标签前后、最前、最后），在前的标签不变（拖动排序，工单 29）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct MoveTab {
    pub tab: TabId,
    pub drop: TabDrop,
}
