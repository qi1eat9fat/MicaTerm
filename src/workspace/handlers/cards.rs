//! 卡片的处理器（工单 02 建，05 补全分屏、放大、焦点移动、拖动间隙）。

use gpui_kit::*;
use qishell_terminal::LocalLaunch;

use crate::app::CloseCard;
use crate::card::{CardId, TerminalCard};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_cards_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_close_card))
}

impl Workspace {
    /// 新建一张本地卡片（还没放进任何标签）：分配 id，用注入的本地终端传输起 shell。
    /// 新标签和分屏（工单 05）都经这里建卡片。
    pub(in crate::workspace) fn open_local_card(
        &mut self,
        launch: LocalLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalCard> {
        let id = CardId(self.next_card_id);
        self.next_card_id += 1;
        let provider = self.services.local_terminals.clone();
        let target = self.focus_handle.clone();
        cx.new(|cx| TerminalCard::local(id, launch, provider, target, window, cx))
    }

    /// 关掉一张卡片和它的 shell。02 一个标签只有一张卡片，所以连标签一起关；
    /// 工单 05 改成从分屏树里摘掉这张卡片，树空了才关标签。
    fn on_close_card(&mut self, action: &CloseCard, window: &mut Window, cx: &mut Context<Self>) {
        let CloseCard(card) = *action;
        if let Some(tab) = self.tabs.find(|tab| tab.contains(card, cx)) {
            self.close_tab(tab, window, cx);
        }
    }
}
