//! 窗口壳里的远程卡片（工单 12）：按主机建远程卡片、开标签，收连接表的事件转给卡片和问答队列。
//!
//! 扩展点（后续工单只加入口，不改这里的连接逻辑）：
//! - `open_remote_card(host, ..)`：按主机建一张远程卡片（还没放进标签）。登录一样的卡片自动共用一条连接
//!   （连接表在 backend）；`open_remote_card_as(host, target, ..)` 按给定的快照和登录建；
//! - 分屏（工单 16）：`split_remote_card` 用源卡片的快照和登录（复用它的连接），`split_open_host`
//!   是主机右键「在右侧 / 下方分屏打开」，都经 `place_beside_focused_card` 放进标签；
//!   `open_cards_by_host` 是主机栏的绿色数字；
//! - `connect_host(host, ..)`：在新标签里连接一台主机，工单 18 的快速连接、最近连接直接调它
//!   （或派发 `ConnectHost`）；
//! - `remote_target(host, ..)`：主机怎么连（含凭据和跳板、代理的路线，`HostData::login_of`），工单 15 不用改这里；
//! - `on_remote_event`：连接表的事件分发，工单 17 的系统徽章在 `RemoteUpdate::Probed` 之后照常生效；
//! - `secrets`：连接前取秘密的钩子（工单 13、33）：要用保险库里的秘密而保险库锁着时，卡片照常建出来、
//!   放进标签，先「等待解锁保险库」，解锁后原地接着连（`wait_for_vault` → `vault_answered`）。

mod prompts;
mod secrets;

use std::collections::HashMap;

use gpui_kit::*;

pub(in crate::workspace) use prompts::PromptQueue;

use crate::backend::{RemoteEvent, RemoteTarget, RemoteTerminalId};
use crate::card::{CardId, RemoteLaunch, RemoteUpdate, TerminalCard, VaultHold};
use crate::host::{Host, HostId};
use crate::split::Axis;
use crate::tabs::{Tab, TabId, TerminalTab};
use crate::workspace::Workspace;

impl Workspace {
    /// 一直收连接表的事件，交给 `on_remote_event`。窗口壳建好时调一次。
    pub(in crate::workspace) fn listen_for_remote_events(
        services: &crate::workspace::WorkspaceServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let events = services.remote_terminals.events();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.on_remote_event(event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// 连接表说的一件事：问题进问答队列；连上、延迟、系统转给那张卡片。
    fn on_remote_event(&mut self, event: RemoteEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (terminal, update) = match event {
            RemoteEvent::Prompt(prompt) => {
                self.prompts.push(prompt);
                self.show_next_prompt(window, cx);
                return;
            }
            RemoteEvent::PromptsWithdrawn { connection } => {
                self.withdraw_prompts(connection, window, cx);
                return;
            }
            RemoteEvent::Attached {
                terminal,
                connection,
            } => (terminal, RemoteUpdate::Attached(connection)),
            RemoteEvent::Latency { terminal, latency } => {
                (terminal, RemoteUpdate::Latency(latency))
            }
            RemoteEvent::Probed { terminal, host } => (terminal, RemoteUpdate::Probed(host)),
        };
        let Some(card) = self.card(card_of(terminal), cx) else {
            return;
        };
        let os = card.update(cx, |card, cx| card.apply_remote(update, cx));
        let (Some(os), Some(host)) = (os, card.read(cx).remote_host()) else {
            return;
        };
        // 探测出的系统记进主机，主机栏的徽章跟着变（主机已经删了就不记）。
        self.hosts.update(cx, |store, cx| {
            if store.host(host).is_some_and(|host| host.os != Some(os)) {
                store.set_host_os(host, Some(os), cx);
            }
        });
    }

    /// 远程卡片连上了（每次重连都算）：记进最近连接，失败的连接不算。工单 18 的最近连接据此排序。
    pub(in crate::workspace) fn on_remote_card_connected(
        &mut self,
        card: &Entity<TerminalCard>,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = card.read(cx).remote_host() else {
            return;
        };
        self.hosts.update(cx, |store, cx| {
            if store.host(host).is_some() {
                store.record_connection(host, cx);
            }
        });
    }

    /// 主机 `host` 怎么连：主机数据在 UI 线程上解析好的登录（凭据、跳板、代理）。
    pub(in crate::workspace) fn remote_target(&self, host: &Host, cx: &App) -> RemoteTarget {
        RemoteTarget {
            host: host.id.0,
            login: self.hosts.read(cx).login_of(host),
        }
    }

    /// 按主机新建一张远程卡片（还没放进任何标签）：分配 id，在这台主机的连接上开一个 shell 通道
    /// （已经连着就复用连接，不再登录）。没有这台主机时是 `None`。新标签和分屏都经这里建远程卡片。
    pub(in crate::workspace) fn open_remote_card(
        &mut self,
        host: HostId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalCard>> {
        let host = self.hosts.read(cx).host(host)?.clone();
        let target = self.remote_target(&host, cx);
        Some(self.open_remote_card_as(host, target, window, cx))
    }

    /// 按给定的主机快照和登录新建一张远程卡片（还没放进任何标签）。分屏一张远程卡片时用它的快照和登录，
    /// 和它在同一条连接上开 shell 通道（主机后来被编辑、删除也一样）。
    /// 要用保险库里的秘密而保险库锁着时（工单 33），卡片先「等待解锁保险库」、不连，解锁框等这次更新
    /// 结束（调用方把卡片放进标签以后）再弹，解开后在这张卡片里接着连。
    pub(in crate::workspace) fn open_remote_card_as(
        &mut self,
        host: Host,
        target: RemoteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalCard> {
        let id = CardId(self.next_card_id);
        self.next_card_id += 1;
        let focus = self.focus_handle.clone();
        if self.waits_for_vault(&target, false, cx) {
            let card = cx.new(|cx| {
                TerminalCard::remote_waiting_for_vault(id, host, target, focus, window, cx)
            });
            self.watch_notices(&card, window, cx);
            self.wait_for_vault(id, window, cx);
            return card;
        }
        let factory = self
            .services
            .remote_terminals
            .factory_for(terminal_of(id), &target);
        let launch = RemoteLaunch {
            host,
            target,
            factory,
        };
        let card = cx.new(|cx| TerminalCard::remote(id, launch, focus, window, cx));
        // 终端的通知和响铃（工单 03）。
        self.watch_notices(&card, window, cx);
        card
    }

    /// 分屏一张远程卡片（工单 16）：新卡片用 `source` 的主机快照和登录，复用它的连接、新开 shell 通道，
    /// 不重新登录；连接只给一个通道时 backend 自动改成新建一条连接。放进 `tab`，挨着它的焦点卡片。
    pub(in crate::workspace) fn split_remote_card(
        &mut self,
        tab: TabId,
        source: &Entity<TerminalCard>,
        axis: Axis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (host, target) = {
            let source = source.read(cx);
            let (Some(host), Some(target)) =
                (source.remote_host_snapshot(), source.remote_target())
            else {
                return;
            };
            (host.clone(), target.clone())
        };
        let card = self.open_remote_card_as(host, target, window, cx);
        self.place_beside_focused_card(tab, axis, card, window, cx);
    }

    /// 主机右键菜单「在右侧 / 下方分屏打开」（工单 16）：把主机 `host` 开在在前标签的焦点卡片旁边，
    /// 本地卡片、远程卡片旁边都行；已经连着就复用连接。在前的不是终端标签（开始卡片、SFTP 标签、
    /// 编辑器标签）时开在新标签里。
    pub(in crate::workspace) fn split_open_host(
        &mut self,
        host: HostId,
        axis: Axis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .active()
            .filter(|_| self.active_terminal_tab().is_some())
        else {
            self.connect_host(host, window, cx);
            return;
        };
        let Some(card) = self.open_remote_card(host, window, cx) else {
            return;
        };
        self.place_beside_focused_card(tab, axis, card, window, cx);
    }

    /// 每台主机开着几张远程卡片（不管连没连着、走哪条连接），主机栏右侧的绿色数字。
    pub(in crate::workspace) fn open_cards_by_host(&self, cx: &App) -> HashMap<HostId, usize> {
        let mut counts = HashMap::new();
        for tab in self.terminal_tabs() {
            for card in tab.cards() {
                if let Some(host) = card.read(cx).remote_host() {
                    *counts.entry(host).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    /// 主机 `host` 开着几张远程卡片，测试用。
    pub fn open_cards_of(&self, host: HostId, cx: &App) -> usize {
        self.open_cards_by_host(cx)
            .get(&host)
            .copied()
            .unwrap_or_default()
    }

    /// 在新标签里连接主机 `host`，标签里只有这一张远程卡片（双击、回车、右键菜单「连接」）。
    /// 这台主机已经连着时复用连接、新开 shell 通道，不重新登录（2026-11-04 用户决定）。
    pub(in crate::workspace) fn connect_host(
        &mut self,
        host: HostId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(card) = self.open_remote_card(host, window, cx) else {
            return;
        };
        let subscriptions = self.card_subscriptions(&card, window, cx);
        self.open_tab(
            Tab::Terminal(TerminalTab::new(card, subscriptions, cx)),
            window,
            cx,
        );
    }

    /// 远程卡片 `card` 原地重连：主机还在就按它现在的样子（编辑过的设置这次生效），
    /// 删了就按卡片打开时的样子。要等保险库时（工单 33）卡片原地「等待解锁保险库」，取消过解锁的
    /// 卡片这次运行取消过也再问；已经在等的不再排一次。
    pub(in crate::workspace) fn reconnect_card(
        &mut self,
        card: CardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entity) = self.card(card, cx) else {
            return;
        };
        let (host, target, hold) = {
            let card = entity.read(cx);
            let (Some(snapshot), Some(target)) =
                (card.remote_host_snapshot(), card.remote_target())
            else {
                return;
            };
            let (host, target) = match self.hosts.read(cx).host(snapshot.id) {
                Some(host) => (host.clone(), self.remote_target(host, cx)),
                None => (snapshot.clone(), target.clone()),
            };
            (host, target, card.vault_hold())
        };
        match hold {
            Some(VaultHold::Waiting) => {}
            _ if self.waits_for_vault(&target, hold == Some(VaultHold::Declined), cx) => {
                entity.update(cx, |card, cx| card.wait_for_vault(host, target, cx));
                self.wait_for_vault(card, window, cx);
            }
            _ => {
                let factory = self
                    .services
                    .remote_terminals
                    .factory_for(terminal_of(card), &target);
                entity.update(cx, |card, cx| card.reconnect(host, target, factory, cx));
            }
        }
    }
}

/// 卡片在连接表里的编号就是卡片 id。
fn terminal_of(card: CardId) -> RemoteTerminalId {
    RemoteTerminalId(card.0)
}

fn card_of(terminal: RemoteTerminalId) -> CardId {
    CardId(terminal.0)
}
