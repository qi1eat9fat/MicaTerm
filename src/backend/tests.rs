//! backend 对着进程内的 russh 测试服务器真的登录：远程终端的传输、共享连接表、测试连接。
//! 测试不碰真实的 Agent、`~/.ssh` 和数据目录（`known_hosts` 在临时目录里）。

use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer, closed_port};
use qishell_ssh::{
    AgentLocation, AuthenticationRequest, ConnectionPromptKind, ConnectionPromptReply,
    ConnectionSecret, HostLogin, LoginMethod, PROBE_COMMAND, SecretRef, SecretSource, SshConnector,
};
use qishell_terminal::{TerminalSize, TerminalTransportCommand, TerminalTransportEvent};
use zeroize::Zeroizing;

use super::{
    ConnectionTester as _, LoginTest, RemoteEvent, RemoteTarget, RemoteTerminalId,
    RemoteTerminals as _, SshConnectionTester, SshRemoteTerminals, TrustAsker,
};

const WAIT: Duration = Duration::from_secs(10);

/// 内存里的秘密。
#[derive(Default)]
pub(super) struct Secrets(pub(super) Mutex<Vec<(SecretRef, String)>>);

impl SecretSource for Secrets {
    fn secret(&self, secret: &SecretRef) -> anyhow::Result<Option<Zeroizing<String>>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|(saved, _)| saved == secret)
            .map(|(_, value)| Zeroizing::new(value.clone())))
    }
}

/// 一台测试服务器和它旁边的一切：运行时、临时的 `known_hosts`。
struct Fixture {
    server: TestServer,
    directory: tempfile::TempDir,
    _runtime: tokio::runtime::Runtime,
}

impl Fixture {
    fn start() -> Self {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime
            .block_on(TestServer::start(std::env::temp_dir()))
            .unwrap();
        Self {
            server,
            directory: tempfile::tempdir().unwrap(),
            _runtime: runtime,
        }
    }

    fn connector(&self, secrets: Arc<Secrets>) -> SshConnector {
        SshConnector::new(self.directory.path().join("known_hosts"), secrets)
            .with_agent(AgentLocation::None)
            .with_default_key_dir(None)
    }

    fn target(&self) -> RemoteTarget {
        target(self.server.port)
    }
}

fn target(port: u16) -> RemoteTarget {
    RemoteTarget {
        host: 1,
        login: HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Password)
            .with_password(SecretRef::password(1)),
    }
}

/// 连接问的问题怎么答：信任主机密钥，问密码答 `password`（`None` 就取消）。记下问过的问题和别的事件。
pub(super) struct Answerer {
    seen: Arc<Mutex<Vec<RemoteEvent>>>,
    _thread: JoinHandle<()>,
}

impl Answerer {
    pub(super) fn start(remote: &Arc<SshRemoteTerminals>, password: Option<&'static str>) -> Self {
        Self::listen(remote, move |kind| {
            Some(match kind {
                ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                ConnectionPromptKind::Authentication(_) => match password {
                    Some(password) => {
                        ConnectionPromptReply::Answers(vec![ConnectionSecret::new(password)])
                    }
                    None => ConnectionPromptReply::Cancel,
                },
                ConnectionPromptKind::HostKeyChanged(_) => ConnectionPromptReply::Cancel,
            })
        })
    }

    /// 信任主机密钥，问密码时不答（像用户还没填）。
    fn leaving_passwords(remote: &Arc<SshRemoteTerminals>) -> Self {
        Self::listen(remote, |kind| match kind {
            ConnectionPromptKind::UnknownHost(_) => Some(ConnectionPromptReply::TrustAndSave),
            _ => None,
        })
    }

    /// 记下全部事件；问题按 `reply` 答，它给 `None` 就不答。
    fn listen(
        remote: &Arc<SshRemoteTerminals>,
        reply: impl Fn(&ConnectionPromptKind) -> Option<ConnectionPromptReply> + Send + 'static,
    ) -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let events = remote.events();
        let remote = Arc::downgrade(remote);
        let record = seen.clone();
        let thread = thread::spawn(move || {
            while let Ok(event) = events.recv_blocking() {
                record.lock().unwrap().push(event.clone());
                let RemoteEvent::Prompt(prompt) = event else {
                    continue;
                };
                let Some(reply) = reply(&prompt.kind) else {
                    continue;
                };
                let Some(remote) = remote.upgrade() else {
                    break;
                };
                remote.respond(prompt.id, reply);
            }
        });
        Self {
            seen,
            _thread: thread,
        }
    }

    /// 问过的问题，按顺序。
    pub(super) fn prompts(&self) -> Vec<ConnectionPromptKind> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                RemoteEvent::Prompt(prompt) => Some(prompt.kind.clone()),
                _ => None,
            })
            .collect()
    }

    fn auth_prompts(&self) -> usize {
        self.prompts()
            .iter()
            .filter(|kind| matches!(kind, ConnectionPromptKind::Authentication(_)))
            .count()
    }

    /// 等到有一个事件让 `found` 返回 `Some`。
    pub(super) fn wait_for<T>(&self, found: impl Fn(&RemoteEvent) -> Option<T>) -> T {
        let started = Instant::now();
        loop {
            if let Some(value) = self.seen.lock().unwrap().iter().find_map(&found) {
                return value;
            }
            assert!(started.elapsed() < WAIT, "等不到事件");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 在自己的线程上跑一个远程终端的传输，像终端引擎那样。
pub(super) struct Terminal {
    commands: mpsc::Sender<TerminalTransportCommand>,
    events: async_channel::Receiver<TerminalTransportEvent>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
}

impl Terminal {
    pub(super) fn open(remote: &SshRemoteTerminals, id: u64, target: &RemoteTarget) -> Self {
        let factory = remote.factory_for(RemoteTerminalId(id), target);
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = async_channel::unbounded();
        let transport = factory.create();
        let thread = thread::spawn(move || transport.run(TerminalSize::DEFAULT, receiver, sender));
        Self {
            commands,
            events,
            thread: Some(thread),
        }
    }

    /// 等下一个终端事件。
    fn next(&self) -> TerminalTransportEvent {
        let started = Instant::now();
        loop {
            match self.events.try_recv() {
                Ok(event) => return event,
                Err(async_channel::TryRecvError::Closed) => panic!("传输已经结束"),
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(started.elapsed() < WAIT, "等不到终端事件");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    pub(super) fn expect_started(&self) {
        match self.next() {
            TerminalTransportEvent::Started => {}
            other => panic!("没有连上：{other:?}"),
        }
    }

    /// 打一行字，等它回显回来。
    pub(super) fn echo(&self, text: &str) {
        self.commands
            .send(TerminalTransportCommand::Write(text.as_bytes().to_vec()))
            .unwrap();
        let mut output = Vec::new();
        while !String::from_utf8_lossy(&output).contains(text) {
            match self.next() {
                TerminalTransportEvent::Output(bytes) => output.extend(bytes),
                other => panic!("回显之前来了 {other:?}"),
            }
        }
    }

    /// 传输自己结束时的结果（连接失败是 `Err`）。
    pub(super) fn finish(mut self) -> anyhow::Result<()> {
        self.thread.take().unwrap().join().unwrap()
    }

    /// 像关卡片那样让传输停下，等它结束；返回它的结果。
    fn finish_after_shutdown(mut self) -> anyhow::Result<()> {
        let _ = self.commands.send(TerminalTransportCommand::Shutdown);
        self.thread.take().unwrap().join().unwrap()
    }

    pub(super) fn close(mut self) {
        let _ = self.commands.send(TerminalTransportCommand::Shutdown);
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_password_login_asks_for_the_key_and_the_password_then_opens_the_shell() {
    let fixture = Fixture::start();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let terminal = Terminal::open(&remote, 1, &fixture.target());
    terminal.expect_started();
    terminal.echo("hello\n");

    let prompts = answers.prompts();
    assert!(
        matches!(prompts[0], ConnectionPromptKind::UnknownHost(_)),
        "{prompts:?}"
    );
    assert!(
        matches!(
            &prompts[1],
            ConnectionPromptKind::Authentication(question)
                if question.request() == &AuthenticationRequest::Password { saved_rejected: false }
        ),
        "{prompts:?}"
    );
    terminal.close();
}

#[test]
fn a_saved_password_logs_in_without_asking_and_the_key_is_remembered() {
    let fixture = Fixture::start();
    let secrets = Arc::new(Secrets::default());
    secrets
        .0
        .lock()
        .unwrap()
        .push((SecretRef::password(1), TEST_PASSWORD.into()));
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(secrets),
    ));
    let answers = Answerer::start(&remote, None);

    Terminal::open(&remote, 1, &fixture.target()).expect_started_and_close();
    wait_until("连接断开", || remote.connection_count() == 0);
    // 第二次连接：主机密钥已经记在 known_hosts 里，什么都不问。
    Terminal::open(&remote, 2, &fixture.target()).expect_started_and_close();

    let prompts = answers.prompts();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(matches!(prompts[0], ConnectionPromptKind::UnknownHost(_)));
    let saved = std::fs::read_to_string(fixture.directory.path().join("known_hosts")).unwrap();
    assert!(
        saved.contains(&format!("[127.0.0.1]:{}", fixture.server.port)),
        "{saved}"
    );
}

impl Terminal {
    fn expect_started_and_close(self) {
        self.expect_started();
        self.close();
    }
}

/// 用户决定（2026-11-04）：已经连着的主机再开一张卡片，复用连接、新开 shell 通道，不重新登录。
#[test]
fn a_second_terminal_of_a_connected_host_shares_its_connection_without_logging_in_again() {
    let fixture = Fixture::start();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));
    assert!(!remote.reuses_connection(&fixture.target()));

    let first = Terminal::open(&remote, 1, &fixture.target());
    first.expect_started();
    // 连着了：再开一张不登录，窗口壳不必为它解锁保险库（M2 code review S2）。
    assert!(remote.reuses_connection(&fixture.target()));
    let second = Terminal::open(&remote, 2, &fixture.target());
    second.expect_started();

    assert_eq!(answers.auth_prompts(), 1, "密码只问一次");
    assert_eq!(remote.connection_count(), 1);
    let attached = |terminal: u64| {
        answers.wait_for(|event| match event {
            RemoteEvent::Attached {
                terminal: id,
                connection,
            } if *id == RemoteTerminalId(terminal) => Some(*connection),
            _ => None,
        })
    };
    assert_eq!(attached(1), attached(2));

    // 关掉第一张卡片，第二张的通道还在：连接跑在共享运行时上，不属于第一张卡片。
    first.close();
    second.echo("still here\n");
    assert_eq!(remote.connection_count(), 1);
    second.close();
    wait_until("最后一张卡片关了，连接断开", || {
        remote.connection_count() == 0
    });
    assert!(!remote.reuses_connection(&fixture.target()));
}

/// 只给一个通道的堡垒机：复用的连接开不出 shell 通道，自动改成新建一条连接；之后这台主机的终端直接新建。
#[test]
fn a_host_that_gives_one_channel_per_connection_gets_a_new_connection_per_terminal() {
    let fixture = Fixture::start();
    fixture.server.single_channel();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let first = Terminal::open(&remote, 1, &fixture.target());
    first.expect_started();
    let second = Terminal::open(&remote, 2, &fixture.target());
    second.expect_started();
    second.echo("on its own connection\n");

    assert_eq!(answers.auth_prompts(), 2, "回退时新登录了一次");
    assert_eq!(remote.connection_count(), 2);

    // 记下了「单通道」：第三张卡片不再先试着复用，直接新建（还是能用）。
    let third = Terminal::open(&remote, 3, &fixture.target());
    third.expect_started();
    third.echo("third\n");
    assert_eq!(answers.auth_prompts(), 3);
    assert_eq!(remote.connection_count(), 3);

    // 各连各的：关掉第一张，别的照常。
    first.close();
    second.echo("still mine\n");
    second.close();
    third.close();
    wait_until("都关了，连接都断开", || {
        remote.connection_count() == 0
    });
}

#[test]
fn terminals_that_open_while_the_host_is_connecting_wait_for_the_same_login() {
    let fixture = Fixture::start();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let first = Terminal::open(&remote, 1, &fixture.target());
    let second = Terminal::open(&remote, 2, &fixture.target());
    first.expect_started();
    second.expect_started();

    assert_eq!(answers.auth_prompts(), 1);
    assert_eq!(remote.connection_count(), 1);
    first.close();
    second.close();
}

/// 问密码的时候，等这条连接的卡片都关了：连接取消，窗口壳收到「问题作废」好关掉问答框
/// （M2 code review S3）。
#[test]
fn closing_the_last_terminal_while_asked_withdraws_the_question() {
    let fixture = Fixture::start();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::leaving_passwords(&remote);

    let terminal = Terminal::open(&remote, 1, &fixture.target());
    let asked = answers.wait_for(|event| match event {
        RemoteEvent::Prompt(prompt)
            if matches!(prompt.kind, ConnectionPromptKind::Authentication(_)) =>
        {
            Some(prompt.id.connection)
        }
        _ => None,
    });
    // 关掉卡片：传输结束（连接被取消，结果不管）。
    let _ = terminal.finish_after_shutdown();

    let withdrawn = answers.wait_for(|event| match event {
        RemoteEvent::PromptsWithdrawn { connection } => Some(*connection),
        _ => None,
    });
    assert_eq!(withdrawn, asked);
    wait_until("连接取消了", || remote.connection_count() == 0);
}

#[test]
fn the_connection_reports_the_hosts_system_and_its_latency() {
    let fixture = Fixture::start();
    fixture
        .server
        .reply(PROBE_COMMAND, "Linux\nweb-01\nID=ubuntu\n");
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let terminal = Terminal::open(&remote, 7, &fixture.target());
    terminal.expect_started();

    let host = answers.wait_for(|event| match event {
        RemoteEvent::Probed { terminal, host } if *terminal == RemoteTerminalId(7) => {
            Some(host.clone())
        }
        _ => None,
    });
    assert_eq!(host.os, "ubuntu");
    assert_eq!(host.hostname.as_deref(), Some("web-01"));
    answers.wait_for(|event| match event {
        RemoteEvent::Latency { terminal, .. } if *terminal == RemoteTerminalId(7) => Some(()),
        _ => None,
    });
    terminal.close();
}

#[test]
fn a_refused_connection_fails_the_terminal_with_the_reason_in_chinese() {
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        SshConnector::new(
            tempfile::tempdir().unwrap().path().join("known_hosts"),
            Arc::new(Secrets::default()),
        )
        .with_agent(AgentLocation::None)
        .with_default_key_dir(None),
    ));
    let error = Terminal::open(&remote, 1, &target(closed_port()))
        .finish()
        .unwrap_err();
    assert_eq!(error.to_string(), "连接被拒绝，该端口上没有服务在监听");
    assert_eq!(remote.connection_count(), 0, "连不上的连接不留在表里");
}

#[test]
fn a_password_the_server_refuses_every_time_fails_the_login() {
    let fixture = Fixture::start();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        fixture.connector(Arc::default()),
    ));
    let _answers = Answerer::start(&remote, Some("wrong"));
    let error = Terminal::open(&remote, 1, &fixture.target())
        .finish()
        .unwrap_err();
    assert!(error.to_string().starts_with("认证失败"), "{error}");
}

pub(super) fn trusting(trusted: bool) -> TrustAsker {
    Arc::new(move |_| async move { trusted }.boxed())
}

#[test]
fn a_connection_test_logs_in_with_the_typed_password() {
    let fixture = Fixture::start();
    let tester = SshConnectionTester::with_connector(fixture.connector(Arc::default()));
    let login = fixture.target().login;

    let ok = futures::executor::block_on(
        tester.test(LoginTest::new(login.clone(), TEST_PASSWORD), trusting(true)),
    );
    assert_eq!(ok, Ok(()));
    let wrong =
        futures::executor::block_on(tester.test(LoginTest::new(login, "nope"), trusting(true)));
    assert_eq!(wrong, Err("用户名或密码错误".into()));
}

#[test]
fn a_connection_test_whose_host_key_is_declined_says_so() {
    let fixture = Fixture::start();
    let tester = SshConnectionTester::with_connector(fixture.connector(Arc::default()));
    let result = futures::executor::block_on(tester.test(
        LoginTest::new(fixture.target().login, TEST_PASSWORD),
        trusting(false),
    ));
    assert_eq!(result, Err("未信任该主机的密钥".into()));
}

#[test]
fn a_connection_test_with_the_password_left_empty_asks_for_it() {
    let fixture = Fixture::start();
    let tester = SshConnectionTester::with_connector(fixture.connector(Arc::default()));
    let result = futures::executor::block_on(
        tester.test(LoginTest::new(fixture.target().login, ""), trusting(true)),
    );
    assert_eq!(result, Err("未填写密码".into()));
}
