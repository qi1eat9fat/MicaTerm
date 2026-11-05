//! 远程卡片（工单 12）的夹具：假的远程终端（不连网络，记下每次给哪台主机建了传输、问题怎么答的）
//! 和假的连接测试器。
// 参考 since2006/shell-rs @ b2a307a: tests/ui/support/terminal.rs 的 `RecordingRemoteProvider`
// 和 tests/ui/support/connection.rs 的 `FakeConnectionTester` 重写。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt as _;
use futures::future::BoxFuture;
use qishell::backend::{
    ConnectionId, ConnectionPromptKind, ConnectionPromptReply, ConnectionTester, LoginTest,
    PromptId, RemoteEvent, RemotePrompt, RemoteTarget, RemoteTerminalId, RemoteTerminals,
    TrustAsker,
};
use qishell_terminal::SharedTerminalTransportFactory;

use super::terminal::FakeTerminalFactory;

/// 问题得到的回答（秘密写成明文，测试才看得见）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answered {
    Trust,
    Answers(Vec<String>),
    Cancel,
}

/// 假的远程终端：每个终端都用同一个假传输工厂（假 shell，按开卡片的顺序数第几次启动）；
/// 记下每次给谁建了传输、连哪台主机；测试替连接表发问题和事件。
pub struct FakeRemoteTerminals {
    pub factory: Arc<FakeTerminalFactory>,
    targets: Mutex<Vec<(RemoteTerminalId, RemoteTarget)>>,
    replies: Mutex<Vec<(PromptId, Answered)>>,
    sender: async_channel::Sender<RemoteEvent>,
    receiver: async_channel::Receiver<RemoteEvent>,
    next_request: AtomicU64,
}

impl FakeRemoteTerminals {
    pub fn new(factory: FakeTerminalFactory) -> Self {
        let (sender, receiver) = async_channel::unbounded();
        Self {
            factory: Arc::new(factory),
            targets: Mutex::default(),
            replies: Mutex::default(),
            sender,
            receiver,
            next_request: AtomicU64::new(1),
        }
    }

    /// 每次建传输工厂（打开、重连）给的终端和主机，按顺序。
    pub fn targets(&self) -> Vec<(RemoteTerminalId, RemoteTarget)> {
        self.targets
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// 替连接表问一个问题：是终端 `terminal`（卡片 id）所在的连接问的。
    pub fn ask(&self, terminal: u64, kind: ConnectionPromptKind) -> PromptId {
        let id = PromptId {
            connection: ConnectionId(terminal),
            request: self.next_request.fetch_add(1, Ordering::SeqCst),
        };
        self.emit(RemoteEvent::Prompt(RemotePrompt {
            id,
            terminals: vec![RemoteTerminalId(terminal)],
            kind,
        }));
        id
    }

    /// 替连接表发一个事件（连上、延迟、系统）。
    pub fn emit(&self, event: RemoteEvent) {
        self.sender.try_send(event).expect("the workspace listens");
    }

    /// 问题得到的回答，按回答的顺序。
    pub fn replies(&self) -> Vec<(PromptId, Answered)> {
        self.replies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl Default for FakeRemoteTerminals {
    fn default() -> Self {
        Self::new(FakeTerminalFactory::default())
    }
}

impl RemoteTerminals for FakeRemoteTerminals {
    fn factory_for(
        &self,
        terminal: RemoteTerminalId,
        target: &RemoteTarget,
    ) -> SharedTerminalTransportFactory {
        self.targets
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((terminal, target.clone()));
        self.factory.clone()
    }

    fn events(&self) -> async_channel::Receiver<RemoteEvent> {
        self.receiver.clone()
    }

    fn respond(&self, prompt: PromptId, reply: ConnectionPromptReply) {
        let answered = match reply {
            ConnectionPromptReply::TrustAndSave => Answered::Trust,
            ConnectionPromptReply::Answers(answers) => Answered::Answers(
                answers
                    .iter()
                    .map(|answer| answer.expose().to_string())
                    .collect(),
            ),
            ConnectionPromptReply::Cancel => Answered::Cancel,
        };
        self.replies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((prompt, answered));
    }

    /// 假的连接不会断：给同一个登录建过终端，就算有能复用的连接。
    fn reuses_connection(&self, target: &RemoteTarget) -> bool {
        self.targets
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .any(|(_, seen)| seen == target)
    }
}

/// 一次测试的登录：地址、端口、用户名，和表单里的密码。
pub type TestedLogin = (String, u16, String, Option<String>);

/// 假的连接测试器：不连网络，按给定的结果回答；可以要求先确认主机密钥。
pub struct FakeConnectionTester {
    outcome: Result<(), String>,
    asks_trust: bool,
    /// 每次测试的登录：地址、端口、用户名，和表单里的密码。
    requests: Mutex<Vec<TestedLogin>>,
    trust_answers: Arc<Mutex<Vec<bool>>>,
}

impl FakeConnectionTester {
    pub fn succeeding() -> Self {
        Self {
            outcome: Ok(()),
            asks_trust: false,
            requests: Mutex::default(),
            trust_answers: Arc::default(),
        }
    }

    pub fn failing(reason: &str) -> Self {
        Self {
            outcome: Err(reason.into()),
            ..Self::succeeding()
        }
    }

    /// 先问一次主机密钥（第一次见到的），信了才算成功。
    pub fn asking_trust() -> Self {
        Self {
            asks_trust: true,
            ..Self::succeeding()
        }
    }

    pub fn requests(&self) -> Vec<TestedLogin> {
        self.requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn trust_answers(&self) -> Vec<bool> {
        self.trust_answers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl ConnectionTester for FakeConnectionTester {
    fn test(
        &self,
        request: LoginTest,
        trust: TrustAsker,
    ) -> BoxFuture<'static, Result<(), String>> {
        let login = request.login();
        self.requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((
                login.host.clone(),
                login.port,
                login.user.clone(),
                request.password().map(str::to_string),
            ));
        let outcome = self.outcome.clone();
        if !self.asks_trust {
            return async move { outcome }.boxed();
        }
        let ConnectionPromptKind::UnknownHost(question) = ConnectionPromptKind::unknown_host(
            login.host.clone(),
            login.port,
            "ssh-ed25519",
            "SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8",
        ) else {
            unreachable!("an unknown host stays one");
        };
        let answers = self.trust_answers.clone();
        async move {
            let trusted = trust(question).await;
            answers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(trusted);
            if trusted {
                outcome
            } else {
                Err("未信任该主机的密钥".into())
            }
        }
        .boxed()
    }
}
