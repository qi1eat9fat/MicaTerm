//! 标签轨道上的一项：终端标签、SFTP 标签、编辑器标签、新建 / 编辑主机的表单页（术语表；
//! 26 号「新建主机」：表单在标签页里打开，不再是居中的对话框），或设置（26 号「设置」）。
//!
//! 窗口壳的标签列表是 `TabList<Tab>`。卡片的处理器一律先 `as_terminal`：在 SFTP、编辑器标签里
//! 拿到 `None`，所以它们不能分屏、不能放大，也没有焦点卡片，不用另写判断。

use gpui_kit::{AnyElement, App, Entity, FocusHandle, IntoElement as _, SharedString};

use super::{TabId, TabLeading, TabSnapshot, TerminalTab};
use crate::app::CatalogIcon;
use crate::card::StatusDot;
use crate::editor::EditorTab;
use crate::settings::SettingsTab;
use crate::sftp_tab::SftpTab;

/// 一个标签。
pub enum Tab {
    Terminal(TerminalTab),
    Sftp(Entity<SftpTab>),
    Editor(Entity<EditorTab>),
    /// 设置：一个窗口只有一个。
    Settings(Entity<SettingsTab>),
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

    pub fn as_settings(&self) -> Option<&Entity<SettingsTab>> {
        match self {
            Self::Settings(settings) => Some(settings),
            _ => None,
        }
    }

    /// 标签轨道要画的快照：渲染函数不读实体。
    pub fn snapshot(&self, id: TabId, active: bool, cx: &App) -> TabSnapshot {
        match self {
            Self::Terminal(tab) => {
                let focused = tab.focused_card();
                let card = focused.read(cx);
                // 焦点卡片排第一，其余按分屏里的顺序（2026-11-14 用户定：图块叠放）。
                let members: Vec<_> = std::iter::once(focused)
                    .chain(tab.cards().filter(|other| *other != focused))
                    .map(|member| member.read(cx))
                    .collect();
                TabSnapshot {
                    id,
                    name: card.name(),
                    directory: card.directory_label().into(),
                    active,
                    leading: TabLeading::Tiles(
                        members.iter().map(|member| member.tile_program()).collect(),
                    ),
                    members: members.iter().map(|member| member.name()).collect(),
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
                    leading: TabLeading::Icon(CatalogIcon::Folder),
                    members: Vec::new(),
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
                    leading: TabLeading::Icon(CatalogIcon::File),
                    members: Vec::new(),
                    dot: tab.rail_dot(),
                }
            }
            // 26 号设置标签前是齿轮。
            Self::Settings(settings) => TabSnapshot {
                id,
                name: settings.read(cx).tab_name(),
                directory: SharedString::default(),
                active,
                leading: TabLeading::Icon(CatalogIcon::Settings),
                members: Vec::new(),
                dot: StatusDot::None,
            },
        }
    }

    /// 标签上的状态点。
    pub fn dot(&self, cx: &App) -> StatusDot {
        match self {
            Self::Terminal(tab) => tab.dot(cx),
            Self::Sftp(tab) => tab.read(cx).rail_dot(),
            Self::Editor(tab) => tab.read(cx).rail_dot(),
            Self::Settings(_) => StatusDot::None,
        }
    }

    /// 标签代表的程序：终端标签是焦点卡片的（标签前那一块画的就是它），SFTP、编辑器是自己的；
    /// 设置没有。
    pub fn tile(&self, cx: &App) -> Option<crate::programs::Program> {
        match self {
            Self::Terminal(tab) => Some(tab.focused_card().read(cx).tile_program()),
            Self::Sftp(tab) => Some(tab.read(cx).rail_tile()),
            Self::Editor(tab) => Some(tab.read(cx).rail_tile()),
            Self::Settings(_) => None,
        }
    }

    /// 标签区里画的内容。
    pub fn content(&self, _: &App) -> AnyElement {
        match self {
            Self::Terminal(tab) => tab.content(),
            Self::Sftp(tab) => tab.clone().into_any_element(),
            Self::Editor(tab) => tab.clone().into_any_element(),
            Self::Settings(settings) => settings.clone().into_any_element(),
        }
    }

    /// 标签到前面来时焦点给谁：终端标签是焦点卡片，SFTP 标签是上次用的那一侧的文件列表。
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Terminal(tab) => tab.focused_card().read(cx).focus_handle(),
            Self::Sftp(tab) => tab.read(cx).focus_target(cx),
            Self::Editor(tab) => tab.read(cx).focus_target(cx),
            Self::Settings(settings) => settings.read(cx).focus_target(),
        }
    }
}
