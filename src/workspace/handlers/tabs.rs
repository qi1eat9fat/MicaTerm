//! 标签的处理器（工单 02、04）。

use gpui_kit::*;
use qishell_terminal::{LocalLaunch, LocalShell};

use crate::app::NewLocalTerminalTab;
use crate::tabs::{TabId, TerminalTab};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_tabs_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_new_local_terminal_tab))
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
            card.update(cx, |card, cx| card.close(cx));
        }
        let focus = match self.tabs.active_content() {
            Some(tab) => tab.focused_card().read(cx).focus_handle(),
            None => self.start_card_focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }
}
