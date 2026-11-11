//! 标签的处理器：新建（02），切换、三种关闭（04），关闭在前的整个标签（macOS 的 ⇧⌘W），拖动排序（29）。

use gpui_kit::*;
use qishell_terminal::{LocalLaunch, LocalShell};

use gpui_kit::component::menu::PopupMenu;

use crate::app::{
    ActivateTab, CloseActiveTab, CloseOtherTabs, CloseTab, CloseTabsToRight, MoveTab,
    NewLocalTerminalTab, NextTab, OpenTabMenu, PreviousTab, SwitchToTab,
};
use crate::editor::EditorTab;
use crate::i18n::t;
use crate::tabs::{Tab, TabId, TerminalTab, build_tab_menu};
use crate::ui::DialogLayer;
use crate::ui::confirm::{
    Choice, ChoiceButton, Confirm, ConfirmLine, open_choice_dialog, open_confirm_dialog,
};
use crate::workspace::Workspace;

/// 关闭标签前要问的事。
struct CloseQuestion {
    /// 还有传输没完成的 SFTP 标签。
    transfers: Vec<Unfinished>,
    /// 有未保存修改的文件。
    unsaved: Vec<SharedString>,
}

impl CloseQuestion {
    fn is_empty(&self) -> bool {
        self.transfers.is_empty() && self.unsaved.is_empty()
    }

    /// 框里的话（原型「关闭 SFTP 标签：还有传输」）。
    fn lines(&self) -> Vec<ConfirmLine> {
        let mut lines: Vec<_> = self
            .transfers
            .iter()
            .map(|tab| {
                ConfirmLine::Text(t!(
                    "sftp_tab.close.transfers",
                    name = tab.name,
                    count = tab.count,
                    detail = tab.detail
                ))
            })
            .collect();
        if !self.transfers.is_empty() {
            lines.push(ConfirmLine::Text(t!("sftp_tab.close.transfers_note")));
        }
        lines.extend(
            self.unsaved
                .iter()
                .map(|name| ConfirmLine::Text(t!("sftp_tab.close.unsaved", name = name))),
        );
        lines
    }
}

/// 一个还有传输没完成的 SFTP 标签。
struct Unfinished {
    /// 主机名。
    name: SharedString,
    /// 几批。
    count: usize,
    /// 括号里的：正在传的文件和百分比（原型「关闭 SFTP 标签」）。
    detail: SharedString,
}

/// 写回结束后接着做什么。
#[derive(Clone)]
enum AfterSaves {
    Close {
        ids: Vec<TabId>,
        activate: Option<TabId>,
    },
    Quit,
}

/// 关标签或退出在等写回结束：写回是截断重写，中止了文件就只剩一半。开着一个只能「不等了」的框。
pub(in crate::workspace) struct SaveWait {
    then: AfterSaves,
    layer: DialogLayer,
}

pub(in crate::workspace) fn bind_tabs_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_new_local_terminal_tab))
        .on_action(cx.listener(Workspace::on_next_tab))
        .on_action(cx.listener(Workspace::on_previous_tab))
        .on_action(cx.listener(Workspace::on_switch_to_tab))
        .on_action(cx.listener(Workspace::on_activate_tab))
        .on_action(cx.listener(Workspace::on_close_tab))
        .on_action(cx.listener(Workspace::on_close_active_tab))
        .on_action(cx.listener(Workspace::on_close_other_tabs))
        .on_action(cx.listener(Workspace::on_close_tabs_to_right))
        .on_action(cx.listener(Workspace::on_open_tab_menu))
        .on_action(cx.listener(Workspace::on_move_tab))
}

impl Workspace {
    /// 新开一个终端标签，里面一张本地卡片，跑设置里的 shell（没写就自动检测），焦点给它。
    fn on_new_local_terminal_tab(
        &mut self,
        _: &NewLocalTerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shell = self
            .settings
            .read(cx)
            .settings()
            .local_terminal
            .shell
            .unwrap_or_default();
        let launch = LocalLaunch::new(LocalShell::new(&shell));
        let card = self.open_local_card(launch, window, cx);
        let subscriptions = self.card_subscriptions(&card, window, cx);
        self.open_tab(
            Tab::Terminal(TerminalTab::new(card, subscriptions, cx)),
            window,
            cx,
        );
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.step(1).is_some() {
            self.tab_rail.request_reveal();
            self.focus_active_tab(window, cx);
        }
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.step(-1).is_some() {
            self.tab_rail.request_reveal();
            self.focus_active_tab(window, cx);
        }
    }

    /// 第 n 个标签到前面来（9 是最后一个）；没有那个位置时不动。
    fn on_switch_to_tab(
        &mut self,
        &SwitchToTab(position): &SwitchToTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.nth(position) {
            self.activate_tab(id, window, cx);
        }
    }

    fn on_activate_tab(
        &mut self,
        &ActivateTab(id): &ActivateTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_tab(id, window, cx);
    }

    fn on_close_tab(
        &mut self,
        &CloseTab(id): &CloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_close_tabs(vec![id], None, window, cx);
    }

    fn on_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.active() {
            self.request_close_tabs(vec![id], None, window, cx);
        }
    }

    /// 只留下 `id`（和它所属的 SFTP 标签），它到前面来。
    fn on_close_other_tabs(
        &mut self,
        &CloseOtherTabs(id): &CloseOtherTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let others = self.without_owner_of(id, self.tabs.others(id), cx);
        if others.is_empty() {
            return;
        }
        self.request_close_tabs(others, Some(id), window, cx);
    }

    /// 关掉 `id` 右边的标签；在前的那个在其中时，`id` 到前面来。
    fn on_close_tabs_to_right(
        &mut self,
        &CloseTabsToRight(id): &CloseTabsToRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let right = self.without_owner_of(id, self.tabs.right_of(id), cx);
        if right.is_empty() {
            return;
        }
        let activate = self
            .tabs
            .active()
            .is_some_and(|active| right.contains(&active))
            .then_some(id);
        self.request_close_tabs(right, activate, window, cx);
    }

    /// 开标签的右键菜单，焦点给菜单（上下键、回车、Esc）。菜单关了（选了一项、Esc、点别处），
    /// 焦点回到在前标签的焦点卡片；选的那项随后由上面的处理器执行。
    fn on_open_tab_menu(
        &mut self,
        &OpenTabMenu { tab, position }: &OpenTabMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.get(tab).is_none() {
            return;
        }
        let menu = build_tab_menu(tab, &self.focus_handle, window, cx);
        let dismiss = cx.subscribe_in(
            &menu,
            window,
            |this, menu: &Entity<PopupMenu>, _: &DismissEvent, window, cx| {
                this.tab_rail.close_menu(menu);
                this.focus_active_tab(window, cx);
            },
        );
        window.focus(&menu.focus_handle(cx), cx);
        self.tab_rail.open_menu(tab, position, menu, dismiss);
        cx.notify();
    }

    /// 让 `id` 到前面来并滚进视野，焦点给它的焦点卡片；没有这个标签时什么都不做。
    pub(in crate::workspace) fn activate_tab(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.activate(id) {
            self.tab_rail.request_reveal();
            self.focus_active_tab(window, cx);
        }
    }

    /// 焦点给在前标签的焦点卡片；没有标签时给开始卡片（快捷键才有焦点路径可走）。
    /// 不动轨道的滚动位置：在前的标签换了，轨道自己会把它滚进视野。
    pub(in crate::workspace) fn focus_active_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = match self.tabs.active_content() {
            Some(tab) => tab.focus_handle(cx),
            None => self.start_card_focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// 把标签加在最后、放到前面，焦点给它的焦点卡片（对话框开着时等它关了再给）。
    pub(in crate::workspace) fn open_tab(
        &mut self,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TabId {
        let focus = tab.focus_handle(cx);
        let id = self.tabs.push(tab);
        // 问答框、解锁框开着时不抢它们的焦点，框关了再给（工单 32）。
        crate::ui::focus_after_dialogs(&focus, window, cx);
        cx.notify();
        id
    }

    /// 关闭这些标签，用户要求关闭的都走这里：有未完成的传输、未保存的文件就先问一次（合成一个框），
    /// 确认后再逐个关；取消了什么都不变。终端标签不问。`activate` 是关之前先到前面来的标签
    /// （「关闭其他」「关闭右侧」）。
    pub(in crate::workspace) fn request_close_tabs(
        &mut self,
        ids: Vec<TabId>,
        activate: Option<TabId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saving = self.saving_editors(&ids, cx);
        if !saving.is_empty() {
            self.wait_for_saves(AfterSaves::Close { ids, activate }, saving, window, cx);
            return;
        }
        let question = CloseQuestion {
            transfers: self.unfinished_transfers(&ids, cx),
            unsaved: self.unsaved_editors(&ids, cx),
        };
        if question.is_empty() {
            self.close_tabs_now(ids, activate, window, cx);
            return;
        }
        // 只关一个有修改的编辑器：问「保存 / 不保存 / 取消」，它到前面来，看得见在问谁。
        if let [id] = ids[..]
            && let Some(editor) = self.tabs.get(id).and_then(Tab::as_editor).cloned()
        {
            self.activate_tab(id, window, cx);
            editor.update(cx, |editor, cx| editor.confirm_close(window, cx));
            return;
        }
        // 已经在问了：不再叠一个。
        if self
            .close_dialog
            .as_ref()
            .is_some_and(|layer| layer.is_open(window, cx))
        {
            return;
        }
        let layer = DialogLayer::new();
        self.close_dialog = Some(layer.clone());
        let workspace = cx.weak_entity();
        let sftp_only = ids.len() == 1;
        open_confirm_dialog(
            Confirm {
                id: "close-tabs",
                title: if sftp_only {
                    t!("sftp_tab.close.title_one")
                } else {
                    t!("sftp_tab.close.title_many")
                },
                lines: question.lines(),
                cancel: t!("sftp_tab.close.cancel"),
                confirm: t!("sftp_tab.close.confirm"),
            },
            layer,
            Box::new(move |_, window, cx| {
                // 等确认框关了再关标签：确认框关掉时焦点要还给原来的地方，那个地方可能正是要关的标签。
                window.defer(cx, move |window, cx| {
                    let _ = workspace.update(cx, |this, cx| {
                        this.close_tabs_now(ids, activate, window, cx)
                    });
                });
                None
            }),
            Box::new(|_, _| {}),
            window,
            cx,
        );
    }

    /// 关窗口、退出之前（工单 23）：有未完成的传输、未保存的文件就问一次（合成一个框），返回 true 表示拦下了，
    /// 用户同意时由框自己退出。
    pub fn ask_before_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ids = self.tabs.ids();
        let saving = self.saving_editors(&ids, cx);
        if !saving.is_empty() {
            self.wait_for_saves(AfterSaves::Quit, saving, window, cx);
            return true;
        }
        let transfers = self.unfinished_transfers(&ids, cx);
        let unsaved = self.unsaved_editors(&ids, cx);
        if transfers.is_empty() && unsaved.is_empty() {
            return false;
        }
        // 已经在问了：还是拦着，不再叠一个。
        if self
            .quit_dialog
            .as_ref()
            .is_some_and(|layer| layer.is_open(window, cx))
        {
            return true;
        }
        let mut lines: Vec<_> = transfers
            .iter()
            .map(|tab| {
                ConfirmLine::Text(t!(
                    "quit.transfers",
                    name = tab.name,
                    count = tab.count,
                    detail = tab.detail
                ))
            })
            .collect();
        if !transfers.is_empty() {
            lines.push(ConfirmLine::Text(t!("quit.transfers_note")));
        }
        lines.extend(
            unsaved
                .iter()
                .map(|name| ConfirmLine::Text(t!("quit.unsaved", name = name))),
        );
        let layer = DialogLayer::new();
        self.quit_dialog = Some(layer.clone());
        open_confirm_dialog(
            Confirm {
                id: "quit-confirm",
                title: t!("quit.title"),
                lines,
                cancel: t!("quit.cancel"),
                confirm: t!("quit.confirm"),
            },
            layer,
            Box::new(|_, _, cx| {
                cx.quit();
                None
            }),
            Box::new(|_, _| {}),
            window,
            cx,
        );
        true
    }

    /// 写回结束了才接着做 `then`，这期间开着一个框说在等谁。已经在等就只换要做的事（退出盖过关标签）。
    fn wait_for_saves(
        &mut self,
        then: AfterSaves,
        saving: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(wait) = &mut self.save_wait
            && wait.layer.is_open(window, cx)
        {
            if matches!(then, AfterSaves::Quit) {
                wait.then = then;
            }
            return;
        }
        let mut lines: Vec<_> = saving
            .iter()
            .map(|name| ConfirmLine::Text(t!("save_wait.saving", name = name)))
            .collect();
        lines.push(ConfirmLine::Text(match then {
            AfterSaves::Close { .. } => t!("save_wait.then_close"),
            AfterSaves::Quit => t!("save_wait.then_quit"),
        }));
        let layer = DialogLayer::new();
        self.save_wait = Some(SaveWait {
            then,
            layer: layer.clone(),
        });
        let workspace = cx.weak_entity();
        let focus = window.focused(cx);
        open_choice_dialog(
            Choice {
                id: "save-wait",
                title: t!("save_wait.title"),
                subtitle: None,
                lines,
                check: None,
                buttons: vec![ChoiceButton::new("cancel", t!("save_wait.cancel"))],
            },
            layer,
            Box::new(move |_, window, cx| {
                if let Some(focus) = &focus {
                    window.focus(focus, cx);
                }
                let _ = workspace.update(cx, |this, _| this.save_wait = None);
            }),
            window,
            cx,
        );
    }

    /// 一次写回结束了。等着的事接着走；写回失败就不走了：文件可能不完整，错误已经显示着，用户得看到。
    pub(in crate::workspace) fn on_save_ended(
        &mut self,
        saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(wait) = &self.save_wait else {
            return;
        };
        let scope = match &wait.then {
            AfterSaves::Close { ids, .. } => ids.clone(),
            AfterSaves::Quit => self.tabs.ids(),
        };
        if saved && !self.saving_editors(&scope, cx).is_empty() {
            return;
        }
        let Some(SaveWait { then, layer }) = self.save_wait.take() else {
            return;
        };
        layer.close(window, cx);
        if !saved {
            self.focus_active_tab(window, cx);
            return;
        }
        match then {
            AfterSaves::Close { ids, activate } => {
                self.request_close_tabs(ids, activate, window, cx)
            }
            AfterSaves::Quit => {
                if !self.ask_before_quit(window, cx) {
                    cx.quit();
                }
            }
        }
    }

    fn close_tabs_now(
        &mut self,
        ids: Vec<TabId>,
        activate: Option<TabId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = activate {
            self.tabs.activate(id);
        }
        for id in ids {
            self.close_tab(id, window, cx);
        }
    }

    /// 这些标签里还有传输没完成的 SFTP 标签。工单 22 填的是 `SftpTab::unfinished_transfers`。
    fn unfinished_transfers(&self, ids: &[TabId], cx: &App) -> Vec<Unfinished> {
        ids.iter()
            .filter_map(|id| self.tabs.get(*id)?.as_sftp())
            .map(|tab| tab.read(cx))
            .filter(|tab| tab.unfinished_transfers() > 0)
            .map(|tab| Unfinished {
                name: tab.rail_name(),
                count: tab.unfinished_transfers(),
                detail: match tab.transfer_queue().unfinished_detail() {
                    Some((file, Some(percent))) => {
                        t!("sftp_tab.close.detail", file = file, percent = percent)
                    }
                    Some((file, None)) => t!("sftp_tab.close.detail_file", file = file),
                    None => SharedString::default(),
                },
            })
            .collect()
    }

    /// 留下 `kept` 时不能关的去掉：远程编辑器所属的 SFTP 标签，关了会把它连带关掉。
    fn without_owner_of(&self, kept: TabId, ids: Vec<TabId>, cx: &App) -> Vec<TabId> {
        let owner = self
            .tabs
            .get(kept)
            .and_then(Tab::as_editor)
            .and_then(|editor| editor.read(cx).sftp())
            .map(|sftp| sftp.entity_id());
        let Some(owner) = owner else {
            return ids;
        };
        ids.into_iter()
            .filter(|id| {
                self.tabs
                    .get(*id)
                    .and_then(Tab::as_sftp)
                    .is_none_or(|tab| tab.entity_id() != owner)
            })
            .collect()
    }

    /// 关闭这些标签时跟着关、又有未保存修改的文件：编辑器标签自己，和这些 SFTP 标签打开的远程文件。
    /// 工单 23 填的是 `EditorTab::has_unsaved_changes`。
    pub(in crate::workspace) fn unsaved_editors(
        &self,
        ids: &[TabId],
        cx: &App,
    ) -> Vec<SharedString> {
        self.closing_editors(ids, cx)
            .filter(|editor| editor.has_unsaved_changes())
            .map(EditorTab::file_name)
            .collect()
    }

    /// 关闭这些标签时跟着关、又正在写回的文件。
    fn saving_editors(&self, ids: &[TabId], cx: &App) -> Vec<SharedString> {
        self.closing_editors(ids, cx)
            .filter(|editor| editor.is_saving())
            .map(EditorTab::file_name)
            .collect()
    }

    /// 关闭这些标签时跟着关的编辑器：它们自己，和这些 SFTP 标签打开的远程文件。
    fn closing_editors<'a>(
        &'a self,
        ids: &'a [TabId],
        cx: &'a App,
    ) -> impl Iterator<Item = &'a EditorTab> {
        let closing_sftp: Vec<_> = ids
            .iter()
            .filter_map(|id| self.tabs.get(*id)?.as_sftp())
            .map(Entity::entity_id)
            .collect();
        self.tabs
            .iter()
            .filter_map(|(id, tab)| Some((id, tab.as_editor()?.read(cx))))
            .filter(move |(id, editor)| {
                ids.contains(id)
                    || editor
                        .sftp()
                        .is_some_and(|sftp| closing_sftp.contains(&sftp.entity_id()))
            })
            .map(|(_, editor)| editor)
    }

    /// 关掉一个标签和它全部卡片的 shell；左边的标签到前面来，没有标签了就回到开始卡片。
    /// SFTP 标签的连接跟着断，它打开的远程编辑器标签一起关（本地文件的不属于它，继续开着）。
    /// 不问：要问的经 `request_close_tabs`。
    pub(in crate::workspace) fn close_tab(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.remove(id) else {
            return;
        };
        match &tab {
            Tab::Terminal(tab) => {
                for card in tab.cards() {
                    self.retire_card(card, cx);
                }
            }
            Tab::Sftp(sftp) => {
                let closed = sftp.read(cx).id();
                self.withdraw_sftp_prompts(closed, window, cx);
                let editors: Vec<TabId> = self
                    .tabs
                    .iter()
                    .filter(|(_, other)| {
                        other
                            .as_editor()
                            .and_then(|editor| editor.read(cx).sftp())
                            .is_some_and(|owner| owner.entity_id() == sftp.entity_id())
                    })
                    .map(|(id, _)| id)
                    .collect();
                for editor in editors {
                    self.close_tab(editor, window, cx);
                }
                sftp.update(cx, |sftp, cx| sftp.close(window, cx));
            }
            Tab::Editor(_) => {}
        }
        self.focus_active_tab(window, cx);
    }

    /// 拖动排序（工单 29）：只改顺序，在前的标签和焦点都不动。
    fn on_move_tab(
        &mut self,
        &MoveTab { tab, drop }: &MoveTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.move_to(tab, drop) {
            cx.notify();
        }
    }
}
