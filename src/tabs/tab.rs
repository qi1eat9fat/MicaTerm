//! 标签轨道上的一项：终端标签、SFTP 标签或编辑器标签（术语表）。
//!
//! 窗口壳的标签列表是 `TabList<Tab>`。卡片的处理器一律先 `as_terminal`：在 SFTP、编辑器标签里
//! 拿到 `None`，所以它们不能分屏、不能放大，也没有焦点卡片，不用另写判断。

use gpui_kit::{AnyElement, App, Entity, FocusHandle, IntoElement as _};

use super::{TabId, TabSnapshot, TerminalTab};
use crate::card::StatusDot;
use crate::editor::EditorTab;
use crate::sftp_tab::SftpTab;

/// 一个标签。
pub enum Tab {
    Terminal(TerminalTab),
    Sftp(Entity<SftpTab>),
    Editor(Entity<EditorTab>),
}

impl Tab {
    pub fn as_terminal(&self) -> Option<&TerminalTab> {
        match self {
            Self::Terminal(tab) => Some(tab),
            _ => None,
        }
    }

    pub fn as_terminal_mut(&mut self) -> Option<&mut TerminalTab> {
        match self {
            Self::Terminal(tab) => Some(tab),
            _ => None,
        }
    }

    pub fn as_sftp(&self) -> Option<&Entity<SftpTab>> {
        match self {
            Self::Sftp(tab) => Some(tab),
            _ => None,
        }
    }

    pub fn as_editor(&self) -> Option<&Entity<EditorTab>> {
        match self {
            Self::Editor(tab) => Some(tab),
            _ => None,
        }
    }

    /// 标签轨道要画的快照：渲染函数不读实体。
    pub fn snapshot(&self, id: TabId, active: bool, cx: &App) -> TabSnapshot {
        match self {
            Self::Terminal(tab) => {
                let card = tab.focused_card().read(cx);
                TabSnapshot {
                    id,
                    name: card.name(),
                    directory: card.directory_label().into(),
                    active,
                    tiles: tab.tiles(cx),
                    dot: tab.dot(cx),
                }
            }
            Self::Sftp(tab) => {
                let tab = tab.read(cx);
                TabSnapshot {
                    id,
                    name: tab.rail_name(),
                    directory: tab.rail_directory(cx),
                    active,
                    tiles: vec![tab.rail_tile()],
                    dot: tab.rail_dot(),
                }
            }
            Self::Editor(tab) => {
                let tab = tab.read(cx);
                TabSnapshot {
                    id,
                    name: tab.rail_name(),
                    directory: tab.rail_directory(),
                    active,
                    tiles: vec![tab.rail_tile()],
                    dot: tab.rail_dot(),
                }
            }
        }
    }

    /// 标签上的状态点。
    pub fn dot(&self, cx: &App) -> StatusDot {
        match self {
            Self::Terminal(tab) => tab.dot(cx),
            Self::Sftp(tab) => tab.read(cx).rail_dot(),
            Self::Editor(tab) => tab.read(cx).rail_dot(),
        }
    }

    /// 标签上叠放的图块。
    pub fn tiles(&self, cx: &App) -> Vec<crate::programs::Program> {
        match self {
            Self::Terminal(tab) => tab.tiles(cx),
            Self::Sftp(tab) => vec![tab.read(cx).rail_tile()],
            Self::Editor(tab) => vec![tab.read(cx).rail_tile()],
        }
    }

    /// 标签区里画的内容。
    pub fn content(&self, _: &App) -> AnyElement {
        match self {
            Self::Terminal(tab) => tab.content(),
            Self::Sftp(tab) => tab.clone().into_any_element(),
            Self::Editor(tab) => tab.clone().into_any_element(),
        }
    }

    /// 标签到前面来时焦点给谁：终端标签是焦点卡片，SFTP 标签是上次用的那一侧的文件列表。
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Terminal(tab) => tab.focused_card().read(cx).focus_handle(),
            Self::Sftp(tab) => tab.read(cx).focus_target(cx),
            Self::Editor(tab) => tab.read(cx).focus_target(cx),
        }
    }
}
