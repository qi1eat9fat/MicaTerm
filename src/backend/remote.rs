// 参考 since2006/shell-rs @ b2a307a: src/terminal/transport.rs 的 `RemoteTerminalTransportProvider`
// 和 src/ssh/transport.rs 的 `SshTerminalTransportProvider` 重写：
// 改造：连接不再一张卡片一条，而是按主机放进连接表共用（每张卡片一个 shell 通道）；
// 连接问答、延迟、系统不走终端的事件通道，而是连接表直接发给窗口壳（它们属于连接，不属于卡片）。

//! 远程终端的边界：窗口壳经 [`RemoteTerminals`] 给远程卡片要传输工厂，再从 [`RemoteTerminals::events`]
//! 收连接的事件（问答、连上、延迟、系统），用 [`RemoteTerminals::respond`] 回答问题。
//! 生产实现是 [`SshRemoteTerminals`]，UI 测试注入假的。

use std::sync::Arc;
use std::time::Duration;

use micaterm_hosts::HostId;
use micaterm_ssh::{
    ConnectionPromptKind, ConnectionPromptReply, HostLogin, Latency, ProbedHost,
    SharedSecretSource, SshConnector,
};
use micaterm_terminal::SharedTerminalTransportFactory;

use super::pool::ConnectionPool;
use super::transport::RemoteTransportFactory;

/// 一个远程终端在 backend 里的编号。窗口壳用卡片的 id（`CardId.0`），收到事件时据此找卡片。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteTerminalId(pub u64);

/// 连接表里的一条连接（一次 SSH 登录）。重连是一条新的连接，编号不复用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

/// 一个问题：哪条连接问的，问的第几个。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PromptId {
    pub connection: ConnectionId,
    pub request: u64,
}

/// 远程终端连到哪里：哪台主机（`HostId.0`），怎么登录（主机数据在 UI 线程上解析好的，
/// 含凭据和跳板、代理的路线）。登录一样的终端共用一条连接；编辑主机改了登录，下次连接就是新的一条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteTarget {
    pub host: u64,
    pub login: HostLogin,
    /// 登录后另开 exec 通道探测系统（工单 34，主机的 `probe_os`）。不算登录的一部分：
    /// 只差这一项的终端照样共用一条连接（连接只在第一个 shell 通道开好时探测一次）。
    pub probe_os: bool,
}

impl RemoteTarget {
    /// 连 `host`，用 `login` 登录，登录后探测系统。
    pub fn new(host: u64, login: HostLogin) -> Self {
        Self {
            host,
            login,
            probe_os: true,
        }
    }

    /// 登录后探不探测系统。
    pub fn with_probe_os(mut self, probe_os: bool) -> Self {
        self.probe_os = probe_os;
        self
    }

    /// 能不能和 `other` 共用一条连接：同一台主机、同样的登录（探不探测不算）。
    pub fn shares_connection_with(&self, other: &RemoteTarget) -> bool {
        self.host == other.host && self.login == other.login
    }
}

/// 连接要问用户的一件事。属于一条连接：同一条连接上有几张卡片也只问一次。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotePrompt {
    pub id: PromptId,
    /// 问的时候在等这条连接的终端，窗口壳拿第一张还开着的卡片的主机名做副标题。
    pub terminals: Vec<RemoteTerminalId>,
    pub kind: ConnectionPromptKind,
}

/// 连接告诉窗口壳的事。连上、断开以外的状态（正在连接、已断开和原因）看终端自己的生命周期。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteEvent {
    /// 要问用户（主机密钥、密码、验证码）。
    Prompt(RemotePrompt),
    /// 终端开好了 shell 通道，在连接 `connection` 上。复用连接时也发：工单 16 据此知道哪些卡片共用一条连接。
    Attached {
        terminal: RemoteTerminalId,
        connection: ConnectionId,
    },
    /// 终端所在连接的一次往返延迟（每 5 秒一次，兼作保活）。
    Latency {
        terminal: RemoteTerminalId,
        latency: Latency,
    },
    /// 终端所在连接探测出了主机的系统和 `uname -n`（每条连接探测一次）。
    Probed {
        terminal: RemoteTerminalId,
        host: ProbedHost,
    },
    /// 连接 `connection` 不要了（等它的终端都放手了）：它还没答的问题都作废，窗口壳关掉问答框、
    /// 不再问排着的。
    PromptsWithdrawn { connection: ConnectionId },
    /// 探测系统失败了（工单 34）：窗口壳把主机 `host` 记成不再探测；连接断了
    /// （[`ProbeFailure::ConnectionLost`]）时连接上的终端 `terminals` 都断了，原地重连。
    /// 每条连接最多发一次。
    ProbeFailed {
        host: HostId,
        terminals: Vec<RemoteTerminalId>,
        failure: ProbeFailure,
    },
}

/// 系统探测怎么失败的（工单 34）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFailure {
    /// 探测通道打不开（主机拒开第二个通道、拒跑命令），连接还在。
    Refused,
    /// 开探测通道或探测进行中整条连接断了（有的堡垒机再开一个通道就断开）。
    ConnectionLost,
}

/// 远程终端的传输从哪里来。生产是 [`SshRemoteTerminals`]，测试注入假的。
pub trait RemoteTerminals: Send + Sync + 'static {
    /// 远程终端 `terminal` 连 `target` 的传输工厂：每次建传输（打开、Enter 原地重连）都在 `target`
    /// 的连接上开一个 shell 通道；还没有活着的连接就新登录一次。
    fn factory_for(
        &self,
        terminal: RemoteTerminalId,
        target: &RemoteTarget,
    ) -> SharedTerminalTransportFactory;

    /// 连接的事件。窗口壳打开时取一次，一直收。
    fn events(&self) -> async_channel::Receiver<RemoteEvent>;

    /// 回答问题 `prompt`。连接已经不等了（取消、关了卡片）就什么都不做。
    fn respond(&self, prompt: PromptId, reply: ConnectionPromptReply);

    /// 现在给 `target` 建终端会不会复用已有的连接（连着的或正在连的）：会就用不到秘密，
    /// 窗口壳不必先解锁保险库。单通道的主机每次都新建连接，不算。
    fn reuses_connection(&self, target: &RemoteTarget) -> bool;
}

pub type SharedRemoteTerminals = Arc<dyn RemoteTerminals>;

/// 生产环境的远程终端：经 SSH 连接表登录，主机密钥记在数据目录的 `known_hosts`，
/// 秘密从 `secrets` 读（保险库锁着时读不到，连接改成现场询问密码）。
pub struct SshRemoteTerminals {
    pool: Arc<ConnectionPool>,
    events: async_channel::Receiver<RemoteEvent>,
}

impl SshRemoteTerminals {
    pub fn new(known_hosts: impl Into<std::path::PathBuf>, secrets: SharedSecretSource) -> Self {
        Self::with_connector(SshConnector::new(known_hosts, secrets))
    }

    /// 用给定的连接器（测试里指定 Agent、默认私钥目录）。
    pub fn with_connector(connector: SshConnector) -> Self {
        Self::with_probe_timeout(connector, super::pool::PROBE_TIMEOUT)
    }

    /// 用给定的连接器，系统探测最多等 `probe_timeout`（测试里缩短）。
    pub(super) fn with_probe_timeout(connector: SshConnector, probe_timeout: Duration) -> Self {
        let (sender, events) = async_channel::unbounded();
        Self {
            pool: ConnectionPool::new(super::runtime::shared(), connector, sender, probe_timeout),
            events,
        }
    }

    /// 连接表里现在有几条连接，测试用。
    pub fn connection_count(&self) -> usize {
        self.pool.connection_count()
    }
}

impl RemoteTerminals for SshRemoteTerminals {
    fn factory_for(
        &self,
        terminal: RemoteTerminalId,
        target: &RemoteTarget,
    ) -> SharedTerminalTransportFactory {
        Arc::new(RemoteTransportFactory::new(
            self.pool.clone(),
            terminal,
            target.clone(),
        ))
    }

    fn events(&self) -> async_channel::Receiver<RemoteEvent> {
        self.events.clone()
    }

    fn respond(&self, prompt: PromptId, reply: ConnectionPromptReply) {
        self.pool.respond(prompt, reply);
    }

    fn reuses_connection(&self, target: &RemoteTarget) -> bool {
        self.pool.reuses_connection(target)
    }
}
