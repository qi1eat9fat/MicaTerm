//! 系统探测失败后自动停用（工单 34）：有的堡垒机再开一个通道就断掉整条连接，登录后另开 exec 通道
//! 探测系统会把 shell 一起弄断。连接表说探测失败了：主机记成不再探测（写进 hosts 数据库），
//! 提示一句；连接断了的卡片原地重连，这次不探测。用户可以在主机对话框里重新打开。

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use crate::backend::RemoteTerminalId;
use crate::host::HostId;
use crate::i18n::t;
use crate::workspace::Workspace;

impl Workspace {
    /// 连接表说主机 `host` 的探测失败了；`lost` 时连接断了，`terminals` 上的卡片原地重连。
    pub(in crate::workspace) fn on_probe_failed(
        &mut self,
        host: u64,
        terminals: Vec<RemoteTerminalId>,
        lost: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = HostId(host);
        let cards: Vec<_> = terminals
            .into_iter()
            .filter_map(|terminal| self.card(super::card_of(terminal), cx))
            .collect();
        // 卡片记着的主机快照也不再探测：主机删了，重连就按快照，不能再探测一次又断掉。
        for card in &cards {
            card.update(cx, |card, _| card.stop_probing());
        }
        let saved = self.hosts.read(cx).host(host).map(|host| host.name.clone());
        let changed = self
            .hosts
            .update(cx, |store, cx| store.set_host_probe(host, false, cx));
        let name = saved.or_else(|| {
            cards
                .first()
                .and_then(|card| card.read(cx).remote_host_snapshot())
                .map(|host| host.name.clone())
        });
        // 同一台主机的几条连接（单通道的主机一张卡片一条）都失败时只提示一次。
        if let Some(name) = name
            && (changed || self.hosts.read(cx).host(host).is_none())
        {
            let message = if lost {
                t!("ssh.probe.disabled_reconnecting", host = name)
            } else {
                t!("ssh.probe.disabled", host = name)
            };
            window.push_notification(Notification::warning(message), cx);
        }
        if lost {
            for card in cards {
                let id = card.read(cx).id();
                self.reconnect_card(id, window, cx);
            }
        }
    }
}
