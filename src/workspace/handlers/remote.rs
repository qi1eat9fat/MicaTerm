//! 远程卡片的处理器（工单 12）：原地重连（断开后按 Enter、右键「重新连接」）、断开连接、复制 IP。
//! 建卡片、连接、问答在 `workspace/remote/`；在新标签里连接主机（`ConnectHost`）在 `hosts`。

use gpui_kit::*;

use crate::app::{CopyCardAddress, DisconnectCard, ReconnectCard};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_remote_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_reconnect_card))
        .on_action(cx.listener(Workspace::on_disconnect_card))
        .on_action(cx.listener(Workspace::on_copy_card_address))
}

impl Workspace {
    fn on_reconnect_card(
        &mut self,
        &ReconnectCard(card): &ReconnectCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reconnect_card(card, window, cx);
    }

    fn on_disconnect_card(
        &mut self,
        &DisconnectCard(card): &DisconnectCard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(card) = self.card(card, cx) {
            card.update(cx, |card, cx| card.disconnect(cx));
        }
    }

    /// 复制卡片打开时那台主机的地址。
    fn on_copy_card_address(
        &mut self,
        &CopyCardAddress(card): &CopyCardAddress,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let address = self.card(card, cx).and_then(|card| {
            card.read(cx)
                .remote_host_snapshot()
                .map(|host| host.address.to_string())
        });
        if let Some(address) = address {
            cx.write_to_clipboard(ClipboardItem::new_string(address));
        }
    }
}
