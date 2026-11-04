//! 标签的处理器：新建（02），切换、三种关闭（04），关闭在前的整个标签（macOS 的 ⇧⌘W）。

use gpui_kit::*;
use qishell_terminal::{LocalLaunch, LocalShell};

use gpui_kit::component::menu::PopupMenu;

use crate::app::{
    ActivateTab, CloseActiveTab, CloseOtherTabs, CloseTab, CloseTabsToRight, NewLocalTerminalTab,
    NextTab, OpenTabMenu, PreviousTab, SwitchToTab,
};
use crate::tabs::{TabId, TerminalTab, build_tab_menu};
use crate::workspace::Workspace;

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
        self.open_tab(TerminalTab::new(card, subscriptions, cx), window, cx);
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
        self.close_tab(id, window, cx);
    }

    fn on_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.active() {
            self.close_tab(id, window, cx);
        }
    }

    /// 只留下 `id`，它到前面来。
    fn on_close_other_tabs(
        &mut self,
        &CloseOtherTabs(id): &CloseOtherTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let others = self.tabs.others(id);
        if others.is_empty() {
            return;
        }
        self.tabs.activate(id);
        for other in others {
            self.close_tab(other, window, cx);
        }
    }

    /// 关掉 `id` 右边的标签；在前的那个在其中时，`id` 到前面来。
    fn on_close_tabs_to_right(
        &mut self,
        &CloseTabsToRight(id): &CloseTabsToRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let right = self.tabs.right_of(id);
        if right.is_empty() {
            return;
        }
        if self
            .tabs
            .active()
            .is_some_and(|active| right.contains(&active))
        {
            self.tabs.activate(id);
        }
        for tab in right {
            self.close_tab(tab, window, cx);
        }
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
    fn activate_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(id) {
            self.tab_rail.request_reveal();
            self.focus_active_tab(window, cx);
        }
    }

    /// 焦点给在前标签的焦点卡片；没有标签时给开始卡片（快捷键才有焦点路径可走）。
    /// 不动轨道的滚动位置：在前的标签换了，轨道自己会把它滚进视野。
    fn focus_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = match self.tabs.active_content() {
            Some(tab) => tab.focused_card().read(cx).focus_handle(),
            None => self.start_card_focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// 把标签加在最后、放到前面，焦点给它的焦点卡片。
    pub(in crate::workspace) fn open_tab(
        &mut self,
        tab: TerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TabId {
        let focus = tab.focused_card().read(cx).focus_handle();
        let id = self.tabs.push(tab);
        window.focus(&focus, cx);
        cx.notify();
        id
    }

    /// 关掉一个标签和它全部卡片的 shell；左边的标签到前面来，没有标签了就回到开始卡片。
    pub(in crate::workspace) fn close_tab(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.remove(id) else {
            return;
        };
        for card in tab.cards() {
            self.retire_card(card, cx);
        }
        self.focus_active_tab(window, cx);
    }
}
