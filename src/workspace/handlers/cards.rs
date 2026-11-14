//! 卡片的处理器（工单 02 建，05 补全分屏、放大、焦点移动、拖动间隙）。
//!
//! 分屏树在终端标签里（`TerminalTab`），这里只找到标签、调它的方法，再把焦点交给该聚焦的卡片。
//! 分屏（2026-11-14 重做）：先在卡片旁边放一张占位的新卡片（`CardPicker`），在里面选开什么——
//! 再开一个（复制这张卡片）、本地终端、某台主机——选好了撤掉占位，把真卡片分到同一个位置。

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;
use micaterm_terminal::{LocalLaunch, LocalShell};

use crate::app::{
    CloseCard, CloseFocusedCard, FocusCard, MoveCardToNewTab, MoveDivider, ResetDivider,
    ResizeDivider, SplitCard, SplitDown, SplitRight, ToggleZoomCard, ZoomCard,
};
use crate::card::{
    CardEvent, CardId, CardPicker, PickerChoice, PickerEvent, PickerSource, TerminalCard,
    next_picker_id,
};
use crate::host::HostMark;
use crate::split::{Axis, Closed};
use crate::tabs::{Tab, TabId, TerminalTab};
use crate::ui::focus_after_dialogs;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_cards_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_close_card))
        .on_action(cx.listener(Workspace::on_close_focused_card))
        .on_action(cx.listener(Workspace::on_split_right))
        .on_action(cx.listener(Workspace::on_split_down))
        .on_action(cx.listener(Workspace::on_split_card))
        .on_action(cx.listener(Workspace::on_move_card_to_new_tab))
        .on_action(cx.listener(Workspace::on_toggle_zoom_card))
        .on_action(cx.listener(Workspace::on_zoom_card))
        .on_action(cx.listener(Workspace::on_focus_card))
        .on_action(cx.listener(Workspace::on_move_divider))
        .on_action(cx.listener(Workspace::on_reset_divider))
        .on_action(cx.listener(Workspace::on_resize_divider))
}

impl Workspace {
    /// 新建一张本地卡片（还没放进任何标签）：分配 id，用注入的本地终端传输起 shell。
    /// 新标签和分屏都经这里建卡片。
    pub(in crate::workspace) fn open_local_card(
        &mut self,
        launch: LocalLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalCard> {
        let id = CardId(self.next_card_id);
        self.next_card_id += 1;
        let provider = self.services.local_terminals.clone();
        let card = cx.new(|cx| TerminalCard::local(id, launch, provider, window, cx));
        // 终端的通知和响铃（工单 03）。
        self.watch_notices(&card, window, cx);
        card
    }

    /// 窗口壳对一张卡片的订阅，交给标签跟着卡片走：名称、目录、图块、状态点变了标签重画；
    /// 程序结束或响铃时用户看不到它就亮橙点；
    /// 卡片里的终端拿到焦点（点了它），它就成了标签的焦点卡片。
    pub(in crate::workspace) fn card_subscriptions(
        &mut self,
        card: &Entity<TerminalCard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let id = card.read(cx).id();
        let focus = card.read(cx).focus_handle();
        vec![
            cx.subscribe(card, |this, card, event: &CardEvent, cx| match event {
                CardEvent::Changed => cx.notify(),
                CardEvent::WantsAttention => this.on_card_wants_attention(&card, cx),
            }),
            cx.on_focus_in(&focus, window, move |this, _, cx| {
                this.on_card_focused(id, cx)
            }),
        ]
    }

    /// 焦点进了卡片 `card`：记进它所在标签的分屏树，标签名跟着变。
    fn on_card_focused(&mut self, card: CardId, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_of(card, cx) else {
            return;
        };
        if let Some(tab) = self.terminal_tab_mut(tab) {
            tab.focus_card(card, cx);
        }
        cx.notify();
    }

    /// `card` 在哪个标签里。
    fn tab_of(&self, card: CardId, cx: &App) -> Option<TabId> {
        self.tabs
            .find(|tab| tab.as_terminal().is_some_and(|tab| tab.contains(card, cx)))
    }

    /// `id` 这个标签，是终端标签时。
    pub(in crate::workspace) fn terminal_tab_mut(&mut self, id: TabId) -> Option<&mut TerminalTab> {
        self.tabs.get_mut(id).and_then(Tab::as_terminal_mut)
    }

    /// 把焦点交给在前那个标签的焦点卡片。
    fn focus_active_card(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_terminal_tab() {
            let focus = tab.focused_card().read(cx).focus_handle();
            window.focus(&focus, cx);
        }
    }

    /// 关掉一张卡片和它的 shell：从分屏树里摘掉，兄弟占据它的位置；是标签里最后一张就关掉标签。
    fn on_close_card(&mut self, action: &CloseCard, window: &mut Window, cx: &mut Context<Self>) {
        let CloseCard(card) = *action;
        self.close_card(card, window, cx);
    }

    fn on_close_focused_card(
        &mut self,
        _: &CloseFocusedCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // SFTP 标签和编辑器标签没有卡片：关的是整个标签（spec 第 8 节）。
        match self.tabs.active_content() {
            Some(Tab::Terminal(tab)) => {
                let card = tab.focused_card().read(cx).id();
                self.close_card(card, window, cx);
            }
            Some(_) => {
                if let Some(id) = self.tabs.active() {
                    self.request_close_tabs(vec![id], None, window, cx);
                }
            }
            None => {}
        }
    }

    fn close_card(&mut self, card: CardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tab_of(card, cx) else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(id) else {
            return;
        };
        match tab.close_card(card, cx) {
            (Closed::LastCard, _) => self.close_tab(id, window, cx),
            (Closed::Merged, Some(entity)) => {
                self.retire_card(&entity, cx);
                if self.tabs.active() == Some(id) {
                    self.focus_active_card(window, cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    /// 卡片离开了窗口壳（关卡片、关标签）：关掉它的 shell，丢掉它的通知限频记录。
    pub(in crate::workspace) fn retire_card(
        &mut self,
        card: &Entity<TerminalCard>,
        cx: &mut Context<Self>,
    ) {
        let id = card.read(cx).id();
        self.notice_times.forget(id);
        card.update(cx, |card, cx| card.close(cx));
    }

    fn on_split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        self.open_card_picker(Axis::Horizontal, window, cx);
    }

    fn on_split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.open_card_picker(Axis::Vertical, window, cx);
    }

    /// 卡片顶行的分屏按钮：分的是那张卡片。
    fn on_split_card(&mut self, action: &SplitCard, window: &mut Window, cx: &mut Context<Self>) {
        let SplitCard { card, axis } = *action;
        if self.focus_card_in_its_tab(card, cx) {
            self.open_card_picker(axis, window, cx);
        }
    }

    /// 在前那个标签的焦点卡片旁边放一张占位的新卡片，焦点给它的搜索框。
    /// 弹窗（解锁保险库、连接时的问答）开着时不分：新卡片会抢走弹窗的键盘。
    fn open_card_picker(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let Some(source) = self
            .tabs
            .get(tab)
            .and_then(Tab::as_terminal)
            .map(|tab| tab.focused_card().clone())
        else {
            return;
        };
        let source = {
            let card = source.read(cx);
            PickerSource {
                name: card.name(),
                mark: card.remote_host_snapshot().map(HostMark::of),
            }
        };
        let id = next_picker_id();
        let hosts = self.hosts.clone();
        let picker = cx.new(|cx| CardPicker::new(id, axis, source, hosts, window, cx));
        let subscription = cx.subscribe_in(&picker, window, move |this, _, event, window, cx| {
            this.on_picker_event(tab, id, *event, window, cx)
        });
        if let Some(terminal) = self.terminal_tab_mut(tab) {
            terminal.open_picker(axis, picker.clone(), subscription, cx);
        }
        picker.update(cx, |picker, cx| picker.focus(window, cx));
        cx.notify();
    }

    /// 新卡片里选好了或者撤销了：撤掉占位，选好了就把真卡片分到同一个位置。
    fn on_picker_event(
        &mut self,
        tab: TabId,
        picker: CardId,
        event: PickerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(terminal) = self.terminal_tab_mut(tab) else {
            return;
        };
        let choice = match event {
            PickerEvent::Dismissed { refocus } => {
                if terminal.take_picker(picker, cx).is_some()
                    && refocus
                    && self.tabs.active() == Some(tab)
                {
                    self.focus_active_card(window, cx);
                }
                cx.notify();
                return;
            }
            PickerEvent::Chosen(choice) => choice,
        };
        // 选好了：占位那一格留给真卡片（焦点卡片还是分屏的那张，下面照旧「分」在它旁边，落进这一格）。
        let Some(slot) = terminal.choose_picker(picker, cx) else {
            return;
        };
        let axis = slot.view.read(cx).axis();
        match choice {
            PickerChoice::Duplicate => self.split_focused_card(axis, window, cx),
            PickerChoice::Local => {
                let launch = LocalLaunch::new(self.local_shell(cx));
                let card = self.open_local_card(launch, window, cx);
                self.place_beside_focused_card(tab, axis, card, window, cx);
            }
            PickerChoice::Host(host) => {
                if let Some(card) = self.open_remote_card(host, window, cx) {
                    self.place_beside_focused_card(tab, axis, card, window, cx);
                }
            }
        }
        // 没开出卡片（主机刚被删了）：那一格收回，焦点回到分屏的那张。
        if self
            .terminal_tab_mut(tab)
            .is_some_and(|terminal| terminal.release_reserved(cx))
        {
            self.focus_active_card(window, cx);
        }
        cx.notify();
    }

    /// 设置里的 shell（没写就自动检测）。
    pub(in crate::workspace) fn local_shell(&self, cx: &App) -> LocalShell {
        LocalShell::new(
            &self
                .settings
                .read(cx)
                .settings()
                .local_terminal
                .shell
                .unwrap_or_default(),
        )
    }

    /// 右键菜单「移到新标签」：把卡片从分屏里拆出去，单独成一个标签（在前）。标签里只有它时什么都不做。
    fn on_move_card_to_new_tab(
        &mut self,
        action: &MoveCardToNewTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let MoveCardToNewTab(card) = *action;
        let Some(id) = self.tab_of(card, cx) else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(id) else {
            return;
        };
        let (Closed::Merged, Some(entity)) = tab.close_card(card, cx) else {
            return;
        };
        entity.update(cx, |card, cx| card.set_place(true, false, true, cx));
        let subscriptions = self.card_subscriptions(&entity, window, cx);
        self.open_tab(
            Tab::Terminal(TerminalTab::new(entity, subscriptions, cx)),
            window,
            cx,
        );
    }

    /// 让 `card` 成为它所在标签的焦点卡片；只对在前的标签有效。
    fn focus_card_in_its_tab(&mut self, card: CardId, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.tabs.active() else {
            return false;
        };
        self.terminal_tab_mut(id)
            .is_some_and(|tab| tab.focus_card(card, cx))
    }

    /// 分在前那个标签的焦点卡片，按卡片类型分开：本地卡片在同一个目录里、用同一个 shell 开新的本地终端；
    /// 远程卡片复用同一条连接、新开 shell 通道（工单 16，见 `split_remote_card`）。
    fn split_focused_card(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.active() else {
            return;
        };
        let Some(tab) = self.tabs.get(id).and_then(Tab::as_terminal) else {
            return;
        };
        let source = tab.focused_card().clone();
        if source.read(cx).remote_host().is_some() {
            self.split_remote_card(id, &source, axis, window, cx);
            return;
        }
        let source = source.read(cx);
        let shell = source
            .local_shell()
            .cloned()
            .unwrap_or_else(|| self.local_shell(cx));
        let launch = LocalLaunch {
            shell,
            directory: source.directory(),
        };
        let card = self.open_local_card(launch, window, cx);
        self.place_beside_focused_card(id, axis, card, window, cx);
    }

    /// 把新卡片 `card` 放进标签 `tab`，在它焦点卡片的右边或下面，焦点给新卡片（对话框开着时等它关了再给）。
    /// 标签已经不在了（等解锁时关掉了）就关掉新卡片的 shell。
    pub(in crate::workspace) fn place_beside_focused_card(
        &mut self,
        tab: TabId,
        axis: Axis,
        card: Entity<TerminalCard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.get(tab).and_then(Tab::as_terminal).is_none() {
            self.retire_card(&card, cx);
            return;
        }
        let subscriptions = self.card_subscriptions(&card, window, cx);
        let focus = card.read(cx).focus_handle();
        if let Some(tab) = self.terminal_tab_mut(tab) {
            tab.split(axis, card, subscriptions, cx);
        }
        // 问答框、解锁框开着时不抢它们的焦点，框关了再给（工单 32）。
        focus_after_dialogs(&focus, window, cx);
        cx.notify();
    }

    fn on_toggle_zoom_card(
        &mut self,
        _: &ToggleZoomCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.active()
            && let Some(tab) = self.terminal_tab_mut(id)
            && tab.toggle_zoom(cx)
        {
            self.focus_active_card(window, cx);
            cx.notify();
        }
    }

    /// 放大按钮、双击头部：放大的是那张卡片；已经放大着就还原。
    fn on_zoom_card(&mut self, action: &ZoomCard, window: &mut Window, cx: &mut Context<Self>) {
        let ZoomCard(card) = *action;
        if self.focus_card_in_its_tab(card, cx) {
            self.on_toggle_zoom_card(&ToggleZoomCard, window, cx);
        }
    }

    fn on_focus_card(&mut self, action: &FocusCard, window: &mut Window, cx: &mut Context<Self>) {
        let FocusCard(direction) = *action;
        let Some(id) = self.tabs.active() else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(id) else {
            return;
        };
        if let Some(card) = tab.move_focus(direction, cx) {
            let focus = card.read(cx).focus_handle();
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    fn on_move_divider(
        &mut self,
        action: &MoveDivider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let MoveDivider(direction) = *action;
        if let Some(id) = self.tabs.active()
            && let Some(tab) = self.terminal_tab_mut(id)
        {
            tab.move_divider(direction, window, cx);
            cx.notify();
        }
    }

    fn on_reset_divider(
        &mut self,
        action: &ResetDivider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ResetDivider(node) = *action;
        if let Some(id) = self.tabs.active()
            && let Some(tab) = self.terminal_tab_mut(id)
        {
            tab.reset_divider(node, window, cx);
            cx.notify();
        }
    }

    fn on_resize_divider(
        &mut self,
        action: &ResizeDivider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ResizeDivider { node, ratio } = *action;
        if let Some(id) = self.tabs.active()
            && let Some(tab) = self.terminal_tab_mut(id)
        {
            tab.resize_divider(node, ratio, window, cx);
            cx.notify();
        }
    }
}
