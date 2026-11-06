//! SFTP 标签的处理器（工单 20）：打开的三个入口（右侧栏「文件」、Ctrl+Shift+U、终端和主机的右键菜单），
//! 和发给 SFTP 标签的命令。命令本身在 `SftpTab::execute`，这里只找到标签转过去。
//!
//! 规则（spec 第 6 节）：焦点卡片是远程卡片时，打开它所在主机的 SFTP 标签，已有就切过去；是本地卡片时
//! 「文件」变灰。没有焦点卡片时：SFTP 标签不动；远程编辑器切到它的 SFTP 标签；本地编辑器和开始卡片变灰。
//! 主机右键开被右键的主机，初始目录在焦点卡片正好是这台主机时用它的目录，否则用主目录。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `open_explorer`、`new_explorer_panel`

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use crate::app::{
    OpenCardSftp, OpenHostSftp, OpenSftp, SftpAction, SftpShortcut, ToggleHiddenFiles,
};
use crate::backend::RemoteTarget;
use crate::card::TerminalCard;
use crate::host::{Host, HostId};
use crate::i18n::t;
use crate::sftp_tab::{SftpTab, SftpTabEvent, SftpTabId, SftpTabOptions, reported_directory};
use crate::tabs::{Tab, TabId};
use crate::workspace::{FilesButton, Workspace};

pub(in crate::workspace) fn bind_sftp_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_open_sftp))
        .on_action(cx.listener(Workspace::on_open_host_sftp))
        .on_action(cx.listener(Workspace::on_open_card_sftp))
        .on_action(cx.listener(Workspace::on_sftp_action))
        .on_action(cx.listener(Workspace::on_sftp_shortcut))
        .on_action(cx.listener(Workspace::on_toggle_hidden_files))
}

impl Workspace {
    /// 右侧栏「文件」现在是什么样：`OpenSftp` 按同一个规则做事。
    pub fn files_button(&self, cx: &App) -> FilesButton {
        match self.tabs.active_content() {
            None => FilesButton::Unavailable(t!("sftp_tab.files.no_remote_files")),
            Some(Tab::Terminal(tab)) => {
                if tab.focused_card().read(cx).remote_host().is_some() {
                    FilesButton::Open
                } else {
                    FilesButton::Unavailable(t!("sftp_tab.files.local_card"))
                }
            }
            Some(Tab::Sftp(_)) => FilesButton::Current,
            Some(Tab::Editor(editor)) => {
                if editor.read(cx).sftp().is_some() {
                    FilesButton::Open
                } else {
                    FilesButton::Unavailable(t!("sftp_tab.files.no_remote_files"))
                }
            }
        }
    }

    /// 右侧栏「文件」、Ctrl+Shift+U。
    fn on_open_sftp(&mut self, _: &OpenSftp, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.active_content() {
            Some(Tab::Terminal(tab)) => {
                let card = tab.focused_card().clone();
                if card.read(cx).remote_host().is_some() {
                    self.open_sftp_of_card(&card, window, cx);
                    return;
                }
            }
            Some(Tab::Sftp(_)) => return,
            Some(Tab::Editor(editor)) => {
                let sftp = editor.read(cx).sftp().map(|sftp| sftp.entity_id());
                let tab = sftp.and_then(|sftp| {
                    self.tabs
                        .find(|tab| tab.as_sftp().is_some_and(|tab| tab.entity_id() == sftp))
                });
                if let Some(tab) = tab {
                    self.activate_tab(tab, window, cx);
                    return;
                }
            }
            None => {}
        }
        // 按键也要有个交代：图标是灰的，悬停才看得到为什么。
        if let FilesButton::Unavailable(reason) = self.files_button(cx) {
            window.push_notification(Notification::info(reason), cx);
        }
    }

    /// 终端右键菜单「打开 SFTP」：这张卡片所在的主机。
    fn on_open_card_sftp(
        &mut self,
        &OpenCardSftp(card): &OpenCardSftp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(card) = self.card(card, cx) {
            self.open_sftp_of_card(&card, window, cx);
        }
    }

    /// 主机右键菜单「打开 SFTP」：被右键的主机。焦点卡片正好是这台主机才用它的目录。
    fn on_open_host_sftp(
        &mut self,
        &OpenHostSftp(id): &OpenHostSftp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.hosts.read(cx).host(id).cloned() else {
            return;
        };
        let target = self.remote_target(&host, cx);
        let start = self.active_terminal_tab().and_then(|tab| {
            let card = tab.focused_card().read(cx);
            (card.remote_host() == Some(id))
                .then(|| card.remote_start_directory().map(reported_directory))
                .flatten()
        });
        self.open_sftp_tab(host, target, start, window, cx);
    }

    /// 打开这张远程卡片所在主机的 SFTP 标签：用卡片打开时的主机和登录，从卡片知道的目录开始。
    fn open_sftp_of_card(
        &mut self,
        card: &Entity<TerminalCard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (host, target, start) = {
            let card = card.read(cx);
            let (Some(host), Some(target)) = (card.remote_host_snapshot(), card.remote_target())
            else {
                return;
            };
            (
                host.clone(),
                target.clone(),
                card.remote_start_directory().map(reported_directory),
            )
        };
        self.open_sftp_tab(host, target, start, window, cx);
    }

    /// 这台主机已经有 SFTP 标签就切过去，否则新开一个，自己登录一条连接。
    /// 保险库锁着时先解锁：这条连接总要新登录，不管这台主机有没有连着的卡片。
    fn open_sftp_tab(
        &mut self,
        host: Host,
        target: RemoteTarget,
        start: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.sftp_tab_of_host(host.id, cx) {
            self.activate_tab(tab, window, cx);
            return;
        }
        self.with_new_login_secrets(&target.clone(), window, cx, move |this, window, cx| {
            // 等解锁时可能已经开了一个。
            if let Some(tab) = this.sftp_tab_of_host(host.id, cx) {
                this.activate_tab(tab, window, cx);
                return;
            }
            let id = SftpTabId(this.next_sftp_id);
            this.next_sftp_id += 1;
            let options = SftpTabOptions {
                id,
                host,
                login: target.login,
                transports: this.services.sftp.clone(),
                local_files: this.services.local_files.clone(),
                local_directory: this.settings.read(cx).settings().sftp.local_directory,
                start_directory: start,
                dispatch: this.focus_handle.clone(),
            };
            let tab = cx.new(|cx| SftpTab::new(options, window, cx));
            cx.subscribe_in(&tab, window, Self::on_sftp_tab_event)
                .detach();
            this.open_tab(Tab::Sftp(tab), window, cx);
        });
    }

    fn on_sftp_tab_event(
        &mut self,
        tab: &Entity<SftpTab>,
        event: &SftpTabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SftpTabEvent::Changed => cx.notify(),
            SftpTabEvent::Prompt { generation, prompt } => {
                let id = tab.read(cx).id();
                self.ask_for_sftp_tab(id, *generation, prompt.clone(), window, cx);
            }
        }
    }

    /// 主机 `host` 的 SFTP 标签。
    fn sftp_tab_of_host(&self, host: HostId, cx: &App) -> Option<TabId> {
        self.tabs.find(|tab| {
            tab.as_sftp()
                .is_some_and(|tab| tab.read(cx).host_id() == host)
        })
    }

    /// id 为 `id` 的 SFTP 标签。
    pub(in crate::workspace) fn sftp_tab_by_id(
        &self,
        id: SftpTabId,
        cx: &App,
    ) -> Option<Entity<SftpTab>> {
        self.tabs
            .iter()
            .find_map(|(_, tab)| tab.as_sftp().filter(|tab| tab.read(cx).id() == id).cloned())
    }

    /// 标签 `tab` 的 SFTP 标签实体，测试用。
    pub fn sftp_tab(&self, tab: TabId) -> Option<Entity<SftpTab>> {
        self.tabs.get(tab).and_then(Tab::as_sftp).cloned()
    }

    /// 在前的 SFTP 标签，测试用。
    pub fn active_sftp_tab(&self) -> Option<Entity<SftpTab>> {
        self.tabs.active_content().and_then(Tab::as_sftp).cloned()
    }

    /// 菜单、拖放、对话框发来的命令。对话框比连接活得久时，它发的作废。
    fn on_sftp_action(&mut self, action: &SftpAction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.sftp_tab_by_id(action.tab(), cx) else {
            return;
        };
        if action
            .generation()
            .is_some_and(|generation| generation != tab.read(cx).generation())
        {
            return;
        }
        tab.update(cx, |tab, cx| tab.execute(action.command(), window, cx));
    }

    /// 文件列表的按键：发给在前的 SFTP 标签。
    fn on_sftp_shortcut(
        &mut self,
        SftpShortcut(command): &SftpShortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_sftp_tab() {
            tab.update(cx, |tab, cx| tab.execute(command, window, cx));
        }
    }

    /// 显示隐藏文件：写进设置（经 `apply` 设全局量），所有 SFTP 标签的这一侧都跟着变。
    fn on_toggle_hidden_files(
        &mut self,
        &ToggleHiddenFiles(side): &ToggleHiddenFiles,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.update(cx, |store, cx| {
            store.update(|settings| settings.sftp.show_hidden.toggle(side), cx)
        });
    }

    /// 开一个编辑器标签（工单 23 从 `sftp_tab::file_open` 接过来）。
    pub fn open_editor_tab(
        &mut self,
        editor: Entity<crate::editor::EditorTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TabId {
        self.open_tab(Tab::Editor(editor), window, cx)
    }
}
