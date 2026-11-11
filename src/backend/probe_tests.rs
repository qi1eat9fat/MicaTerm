//! 系统探测可关、失败后自动停用（工单 34）：连接表对着进程内的 russh 测试服务器真的登录，
//! 服务器模拟只给一个通道、或者再开通道就断开整条连接的堡垒机。

use std::sync::Arc;

use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer};
use qishell_ssh::{AgentLocation, HostLogin, LoginMethod, PROBE_COMMAND, SecretRef, SshConnector};

use super::tests::{Answerer, Terminal};
use super::{RemoteEvent, RemoteTarget, RemoteTerminalId, SshRemoteTerminals};

/// 测试服务器、它的运行时和临时的 `known_hosts`。
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
        server.reply(PROBE_COMMAND, "Linux\nweb-01\nID=ubuntu\n");
        Self {
            server,
            directory: tempfile::tempdir().unwrap(),
            _runtime: runtime,
        }
    }

    fn remote(&self) -> Arc<SshRemoteTerminals> {
        let connector = SshConnector::new(
            self.directory.path().join("known_hosts"),
            Arc::new(super::tests::Secrets::default()),
        )
        .with_agent(AgentLocation::None)
        .with_default_key_dir(None);
        Arc::new(SshRemoteTerminals::with_connector(connector))
    }

    /// 主机 7，登录后探不探测系统。
    fn target(&self, probe_os: bool) -> RemoteTarget {
        RemoteTarget::new(
            7,
            HostLogin::new(
                "127.0.0.1",
                self.server.port,
                TEST_USER,
                LoginMethod::Password,
            )
            .with_password(SecretRef::password(7)),
        )
        .with_probe_os(probe_os)
    }

    /// 服务器收到的探测命令有几条。
    fn probes(&self) -> usize {
        self.server
            .seen()
            .commands
            .iter()
            .filter(|command| command.as_str() == PROBE_COMMAND)
            .count()
    }
}

/// 等到连接报了一次延迟（探测在它之前，探测完了才量延迟）。
fn wait_for_latency(answers: &Answerer, terminal: u64) {
    answers.wait_for(|event| match event {
        RemoteEvent::Latency { terminal: of, .. } if *of == RemoteTerminalId(terminal) => Some(()),
        _ => None,
    });
}

fn probe_failed(answers: &Answerer) -> (u64, Vec<RemoteTerminalId>, bool) {
    answers.wait_for(|event| match event {
        RemoteEvent::ProbeFailed {
            host,
            terminals,
            lost,
        } => Some((*host, terminals.clone(), *lost)),
        _ => None,
    })
}

/// 连接说过探测的结果（成功或失败）没有。
fn probed(answers: &Answerer) -> bool {
    answers.events().iter().any(|event| {
        matches!(
            event,
            RemoteEvent::Probed { .. } | RemoteEvent::ProbeFailed { .. }
        )
    })
}

#[test]
fn a_host_that_does_not_probe_opens_no_exec_channel() {
    let fixture = Fixture::start();
    let remote = fixture.remote();
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let terminal = Terminal::open(&remote, 1, &fixture.target(false));
    terminal.expect_started();
    wait_for_latency(&answers, 1);
    terminal.echo("still here");

    assert_eq!(fixture.probes(), 0, "没开 exec 通道探测");
    assert!(!probed(&answers), "{:?}", answers.events());
    terminal.close();
}

#[test]
fn a_host_that_refuses_the_probe_channel_is_marked_and_keeps_its_shell() {
    let fixture = Fixture::start();
    fixture.server.single_channel();
    let remote = fixture.remote();
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let terminal = Terminal::open(&remote, 1, &fixture.target(true));
    terminal.expect_started();

    let (host, terminals, lost) = probe_failed(&answers);
    assert_eq!(host, 7);
    assert_eq!(terminals, [RemoteTerminalId(1)]);
    assert!(!lost, "通道被拒，连接还在");
    terminal.echo("still here");
    wait_for_latency(&answers, 1);
    terminal.close();
}

#[test]
fn a_bastion_that_drops_the_connection_on_the_probe_reports_it_lost_and_reconnects_without_probing()
{
    let fixture = Fixture::start();
    fixture.server.end_on_another_channel();
    let remote = fixture.remote();
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));

    let terminal = Terminal::open(&remote, 1, &fixture.target(true));
    terminal.expect_started();
    let (host, terminals, lost) = probe_failed(&answers);
    assert_eq!(
        (host, terminals, lost),
        (7, vec![RemoteTerminalId(1)], true)
    );
    let error = terminal.finish().expect_err("探测把 shell 一起弄断了");
    assert!(error.to_string().contains("断开"), "{error}");

    // 原地重连：这次不探测，shell 留得住。
    let terminal = Terminal::open(&remote, 1, &fixture.target(false));
    terminal.expect_started();
    terminal.echo("back again");
    wait_for_latency(&answers, 1);
    assert_eq!(fixture.server.connections(), 2);
    assert_eq!(
        fixture.probes(),
        0,
        "堡垒机在探测命令之前就断了，重连后也没再探测"
    );
    terminal.echo("still here");
    terminal.close();
}
