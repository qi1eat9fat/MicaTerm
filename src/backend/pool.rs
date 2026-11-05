//! 连接表：按主机持有连接，一条连接上开多个 shell 通道（spec 第 3、10 节「分屏复用连接」，
//! 2026-11-04 用户决定双击已经连着的主机也复用）。
//!
//! - 登录一样（[`RemoteTarget`] 相等）的终端共用一条活着的连接；正在连的时候，后来的终端等同一个结果，
//!   主机密钥、密码、验证码只问一次；
//! - 连接的问题经 `events` 发给窗口壳，带上在等它的终端；
//! - 第一个 shell 通道开好以后，这条连接探测一次系统，并且每 5 秒量一次延迟，结果发给它上面的每个终端；
//! - 最后一个终端放手时才断开连接；还在连的时候都放手了，就取消（问题随之作废）；
//! - 服务器只给一个通道（复用时开不出 shell 通道）：这台主机记一个「单通道」标记（本次运行有效），
//!   之后它的终端都新建连接。

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use qishell_ssh::{
    ConnectionPrompt, ConnectionPromptReply, HostOsProbe, LATENCY_INTERVAL, Latency, ProbeOutcome,
    ProbedHost, SshConnectionConfig, SshConnector, SshHandle, SshPrompts, open_probe, round_trip,
};
use russh::ChannelMsg;
use tokio::runtime::Handle;
use tokio::sync::watch;

use super::describe::describe_login_error;
use super::remote::{
    ConnectionId, PromptId, RemoteEvent, RemotePrompt, RemoteTarget, RemoteTerminalId,
};
use crate::i18n::t;

/// 探测系统最多等多久：主机不回答也只是没有徽章。
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 挂断时最多等多久。
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// 一条连接现在怎么样。
#[derive(Clone)]
pub(super) enum Link {
    /// 还在连（可能在等用户回答）。
    Pending,
    /// 登录好了。
    Up(Arc<SshHandle>),
    /// 没连上，已经写成给用户看的原因。
    Down(String),
}

pub(super) struct ConnectionPool {
    runtime: Handle,
    connector: SshConnector,
    events: async_channel::Sender<RemoteEvent>,
    state: Mutex<PoolState>,
    me: Weak<ConnectionPool>,
}

#[derive(Default)]
struct PoolState {
    next_id: u64,
    entries: Vec<Entry>,
    /// 只给一个通道的主机（本次运行有效）：它们的终端每次都新建连接。
    single_channel: Vec<RemoteTarget>,
}

struct Entry {
    id: ConnectionId,
    target: RemoteTarget,
    /// 在用（或在等）这条连接的终端。
    terminals: Vec<RemoteTerminalId>,
    link: watch::Receiver<Link>,
    broker: Arc<SshPrompts>,
    shutdown: watch::Sender<bool>,
    /// 新建时就不给别人复用（单通道的主机）。
    exclusive: bool,
    /// 探测和量延迟开始了没有。
    monitored: bool,
    probed: Option<ProbedHost>,
    latency: Option<Latency>,
}

impl Entry {
    /// 还能不能给新来的终端用：没连失败、没断。
    fn reusable(&self) -> bool {
        if self.exclusive {
            return false;
        }
        match &*self.link.borrow() {
            Link::Pending => true,
            Link::Up(handle) => !handle.is_closed(),
            Link::Down(_) => false,
        }
    }
}

/// 一个终端借着的一条连接。丢掉它就是放手：最后一个放手时连接断开。
pub(super) struct Lease {
    pool: Arc<ConnectionPool>,
    connection: ConnectionId,
    terminal: RemoteTerminalId,
    link: watch::Receiver<Link>,
    /// 借的是已经有的连接（不是为它新登录的）。
    reused: bool,
}

impl Lease {
    /// 等连接登录好。没连上时是给用户看的原因。
    pub(super) async fn ready(&mut self) -> Result<Arc<SshHandle>, String> {
        let link = self
            .link
            .wait_for(|link| !matches!(link, Link::Pending))
            .await
            .map(|link| link.clone())
            .unwrap_or_else(|_| Link::Down(t!("ssh.connect.cancelled").into()));
        match link {
            Link::Up(handle) => Ok(handle),
            Link::Down(reason) => Err(reason),
            Link::Pending => unreachable!("waited for it to settle"),
        }
    }

    pub(super) fn reused(&self) -> bool {
        self.reused
    }

    /// shell 通道开好了：告诉窗口壳终端在哪条连接上，补发这条连接已经知道的延迟和系统，
    /// 第一次时开始探测和量延迟。
    pub(super) fn shell_opened(&self, handle: &Arc<SshHandle>) {
        self.pool.opened(self.connection, self.terminal, handle);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.pool.release(self.connection, self.terminal);
    }
}

impl ConnectionPool {
    pub(super) fn new(
        runtime: Handle,
        connector: SshConnector,
        events: async_channel::Sender<RemoteEvent>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            runtime,
            connector,
            events,
            state: Mutex::default(),
            me: me.clone(),
        })
    }

    pub(super) fn runtime(&self) -> &Handle {
        &self.runtime
    }

    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn me(&self) -> Arc<Self> {
        self.me.upgrade().expect("the pool is alive while used")
    }

    pub(super) fn connection_count(&self) -> usize {
        self.lock().entries.len()
    }

    /// 终端 `terminal` 借一条到 `target` 的连接：有能复用的就复用，否则新登录一次。
    /// `fresh` 时一定新登录，并且这条连接不给别人复用（单通道的主机）。
    pub(super) fn attach(
        &self,
        terminal: RemoteTerminalId,
        target: &RemoteTarget,
        fresh: bool,
    ) -> Lease {
        let mut state = self.lock();
        let single = fresh || state.single_channel.contains(target);
        if !single
            && let Some(entry) = state
                .entries
                .iter_mut()
                .find(|entry| &entry.target == target && entry.reusable())
        {
            entry.terminals.push(terminal);
            return Lease {
                pool: self.me(),
                connection: entry.id,
                terminal,
                link: entry.link.clone(),
                reused: true,
            };
        }
        state.next_id += 1;
        let id = ConnectionId(state.next_id);
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let broker = Arc::new(SshPrompts::new(self.prompt_sender(id), shutdown_receiver));
        let (link_sender, link) = watch::channel(Link::Pending);
        state.entries.push(Entry {
            id,
            target: target.clone(),
            terminals: vec![terminal],
            link: link.clone(),
            broker: broker.clone(),
            shutdown,
            exclusive: single,
            monitored: false,
            probed: None,
            latency: None,
        });
        drop(state);
        self.runtime
            .spawn(self.me().connect(id, target.clone(), broker, link_sender));
        Lease {
            pool: self.me(),
            connection: id,
            terminal,
            link,
            reused: false,
        }
    }

    /// 连接 `id` 的问题怎么送到窗口壳：带上在等它的终端。窗口壳不在了（发不出去）就当没人可问。
    fn prompt_sender(
        &self,
        id: ConnectionId,
    ) -> Arc<dyn Fn(ConnectionPrompt) -> bool + Send + Sync> {
        let pool = self.me.clone();
        Arc::new(move |prompt: ConnectionPrompt| {
            let Some(pool) = pool.upgrade() else {
                return false;
            };
            let terminals = pool.terminals_of(id);
            pool.events
                .try_send(RemoteEvent::Prompt(RemotePrompt {
                    id: PromptId {
                        connection: id,
                        request: prompt.request_id(),
                    },
                    terminals,
                    kind: prompt.kind().clone(),
                }))
                .is_ok()
        })
    }

    fn terminals_of(&self, id: ConnectionId) -> Vec<RemoteTerminalId> {
        self.lock()
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.terminals.clone())
            .unwrap_or_default()
    }

    /// 在共享运行时上登录一次，结果交给在等的终端。没人等了（都放手了）就挂断。
    async fn connect(
        self: Arc<Self>,
        id: ConnectionId,
        target: RemoteTarget,
        broker: Arc<SshPrompts>,
        link: watch::Sender<Link>,
    ) {
        let result = self
            .connector
            .connect(&SshConnectionConfig::from(&target.login), broker)
            .await;
        match result {
            Ok((handle, _fingerprint)) => {
                let handle = Arc::new(handle);
                let wanted = self.lock().entries.iter().any(|entry| entry.id == id);
                let _ = link.send(Link::Up(handle.clone()));
                if !wanted {
                    hang_up(&handle).await;
                }
            }
            Err(error) => {
                let _ = link.send(Link::Down(describe_login_error(&error)));
                // 连不上的连接不留在表里：下次（Enter 重连）重新登录。
                self.lock().entries.retain(|entry| entry.id != id);
            }
        }
    }

    pub(super) fn respond(&self, prompt: PromptId, reply: ConnectionPromptReply) {
        let broker = self
            .lock()
            .entries
            .iter()
            .find(|entry| entry.id == prompt.connection)
            .map(|entry| entry.broker.clone());
        if let Some(broker) = broker {
            broker.respond(prompt.request, reply);
        }
    }

    /// 复用的连接开不出 shell 通道：这台主机只给一个通道，之后都新建连接。
    pub(super) fn mark_single_channel(&self, target: &RemoteTarget) {
        let mut state = self.lock();
        if !state.single_channel.contains(target) {
            state.single_channel.push(target.clone());
        }
    }

    fn opened(&self, id: ConnectionId, terminal: RemoteTerminalId, handle: &Arc<SshHandle>) {
        let (probed, latency, start) = {
            let mut state = self.lock();
            let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
                return;
            };
            let start = !entry.monitored;
            entry.monitored = true;
            (entry.probed.clone(), entry.latency, start)
        };
        let _ = self.events.try_send(RemoteEvent::Attached {
            terminal,
            connection: id,
        });
        if let Some(host) = probed {
            let _ = self.events.try_send(RemoteEvent::Probed { terminal, host });
        }
        if let Some(latency) = latency {
            let _ = self
                .events
                .try_send(RemoteEvent::Latency { terminal, latency });
        }
        if start {
            self.runtime.spawn(self.me().monitor(id, handle.clone()));
        }
    }

    /// 连接 `id` 活着的时候：先探测一次系统，再每 5 秒量一次延迟，结果发给它上面的每个终端。
    async fn monitor(self: Arc<Self>, id: ConnectionId, handle: Arc<SshHandle>) {
        let mut shutdown = match self.lock().entries.iter().find(|entry| entry.id == id) {
            Some(entry) => entry.shutdown.subscribe(),
            None => return,
        };
        if let Ok(Some(host)) = tokio::time::timeout(PROBE_TIMEOUT, probe(&handle)).await {
            self.publish(
                id,
                |entry| entry.probed = Some(host.clone()),
                |terminal| RemoteEvent::Probed {
                    terminal,
                    host: host.clone(),
                },
            );
        }
        let mut ticker = tokio::time::interval(LATENCY_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = shutdown.changed() => return,
            }
            if handle.is_closed() {
                return;
            }
            let latency = round_trip(&handle).await;
            if !self.publish(
                id,
                |entry| entry.latency = Some(latency),
                |terminal| RemoteEvent::Latency { terminal, latency },
            ) {
                return;
            }
        }
    }

    /// 记进连接 `id`，再发给它上面的每个终端。连接已经不在了就返回 `false`。
    fn publish(
        &self,
        id: ConnectionId,
        record: impl FnOnce(&mut Entry),
        event: impl Fn(RemoteTerminalId) -> RemoteEvent,
    ) -> bool {
        let terminals = {
            let mut state = self.lock();
            let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
                return false;
            };
            record(entry);
            entry.terminals.clone()
        };
        for terminal in terminals {
            let _ = self.events.try_send(event(terminal));
        }
        true
    }

    /// 终端 `terminal` 不用连接 `id` 了。是最后一个就断开（还在连就取消）。
    fn release(&self, id: ConnectionId, terminal: RemoteTerminalId) {
        let removed = {
            let mut state = self.lock();
            let Some(index) = state.entries.iter().position(|entry| entry.id == id) else {
                return;
            };
            let entry = &mut state.entries[index];
            if let Some(at) = entry.terminals.iter().position(|held| *held == terminal) {
                entry.terminals.remove(at);
            }
            if !entry.terminals.is_empty() {
                return;
            }
            state.entries.remove(index)
        };
        let _ = removed.shutdown.send(true);
        removed.broker.cancel_all();
        // 问答框可能还开着：告诉窗口壳这条连接的问题都作废了。
        let _ = self
            .events
            .try_send(RemoteEvent::PromptsWithdrawn { connection: id });
        if let Link::Up(handle) = &*removed.link.borrow() {
            let handle = handle.clone();
            self.runtime.spawn(async move { hang_up(&handle).await });
        }
    }
}

/// 礼貌地挂断。
async fn hang_up(handle: &SshHandle) {
    let _ = tokio::time::timeout(
        DISCONNECT_TIMEOUT,
        handle.disconnect(
            russh::Disconnect::ByApplication,
            "QiShell closed the connection",
            "zh-CN",
        ),
    )
    .await;
}

/// 在自己的通道上探测主机的系统（先 POSIX，认不出再问 Windows）。主机不回答就是 `None`。
async fn probe(handle: &SshHandle) -> Option<ProbedHost> {
    let mut probe = HostOsProbe::new();
    while let Some(command) = probe.command() {
        let mut channel = open_probe(handle, Some(command)).await?;
        loop {
            match channel.wait().await {
                // stderr 不要：Windows 上只有 shell 抱怨 `uname` 不存在。
                Some(ChannelMsg::Data { data }) => probe.push(&data),
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                _ => {}
            }
        }
        match probe.finish() {
            ProbeOutcome::Detected(host) => return Some(host),
            ProbeOutcome::AskWindows => continue,
            ProbeOutcome::GaveUp => return None,
        }
    }
    None
}
