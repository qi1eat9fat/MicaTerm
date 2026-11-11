// 参考 since2006/shell-rs @ b2a307a: src/ssh/route_tests.rs 改写
// 裁剪：去掉 Telnet、老服务器的密钥交换、工作线程的运行时提前退出（QiShell 的连接都建在共享运行时上）；
// 跳板改用 qishell-ssh 的 `TestServer`（它也能转发 direct-tcpip 通道）。

//! 跳板和代理（工单 15）：对着进程内的几台 russh 测试服务器和代理真的登录。
//! 只连本机回环地址，不碰用户的主机和保险库。

use std::sync::{Arc, Mutex};

use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer, closed_port};
use qishell_ssh::{
    AgentLocation, ConnectionPromptKind, HostLogin, JumpLogin, LoginMethod, LoginRoute, ProxyKind,
    ProxyLogin, SecretRef, SshConnector,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

use super::tests::{Answerer, Secrets, Terminal, trusting};
use super::{
    ConnectionTester as _, LoginTest, RemoteTarget, SshConnectionTester, SshRemoteTerminals,
};

/// 几台测试服务器、代理，和它们共用的运行时、临时的 `known_hosts`。
struct Lab {
    runtime: tokio::runtime::Runtime,
    directory: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        Self {
            runtime: tokio::runtime::Runtime::new().unwrap(),
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn server(&self) -> TestServer {
        self.runtime
            .block_on(TestServer::start(std::env::temp_dir()))
            .unwrap()
    }

    fn connector(&self, secrets: Arc<Secrets>) -> SshConnector {
        SshConnector::new(self.directory.path().join("known_hosts"), secrets)
            .with_agent(AgentLocation::None)
            .with_default_key_dir(None)
    }

    fn tester(&self, secrets: Arc<Secrets>) -> SshConnectionTester {
        SshConnectionTester::with_connector(self.connector(secrets))
    }

    /// 一个代理：替连接转到它被要求去的地方，记下去了哪里。`auth` 是 SOCKS5 要的用户名和密码。
    fn relay(&self, kind: ProxyKind, auth: Option<(&'static str, &'static str)>) -> Relay {
        let listener = self
            .runtime
            .block_on(TcpListener::bind(("127.0.0.1", 0)))
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let targets = Arc::new(Mutex::new(Vec::new()));
        let seen = targets.clone();
        self.runtime.spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let _ = match kind {
                        ProxyKind::Http => relay_http(socket, seen).await,
                        ProxyKind::Socks5 => relay_socks5(socket, auth, seen).await,
                    };
                });
            }
        });
        Relay { port, targets }
    }
}

struct Relay {
    port: u16,
    targets: Arc<Mutex<Vec<String>>>,
}

impl Relay {
    fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

async fn relay_http(mut socket: TcpStream, seen: Arc<Mutex<Vec<String>>>) -> anyhow::Result<()> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(socket.read_u8().await?);
    }
    let head = String::from_utf8(head)?;
    let target = head
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    seen.lock().unwrap().push(target.clone());
    let mut upstream = TcpStream::connect(target.as_str()).await?;
    socket
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    tokio::io::copy_bidirectional(&mut socket, &mut upstream).await?;
    Ok(())
}

async fn relay_socks5(
    mut socket: TcpStream,
    auth: Option<(&'static str, &'static str)>,
    seen: Arc<Mutex<Vec<String>>>,
) -> anyhow::Result<()> {
    let mut greeting = [0u8; 2];
    socket.read_exact(&mut greeting).await?;
    let mut methods = vec![0u8; usize::from(greeting[1])];
    socket.read_exact(&mut methods).await?;
    match auth {
        None => socket.write_all(&[5, 0]).await?,
        Some((user, password)) => {
            socket.write_all(&[5, 2]).await?;
            let mut header = [0u8; 2];
            socket.read_exact(&mut header).await?;
            let mut given_user = vec![0u8; usize::from(header[1])];
            socket.read_exact(&mut given_user).await?;
            let mut given_password = vec![0u8; usize::from(socket.read_u8().await?)];
            socket.read_exact(&mut given_password).await?;
            if given_user != user.as_bytes() || given_password != password.as_bytes() {
                socket.write_all(&[1, 1]).await?;
                return Ok(());
            }
            socket.write_all(&[1, 0]).await?;
        }
    }
    let mut request = [0u8; 4];
    socket.read_exact(&mut request).await?;
    let host = match request[3] {
        1 => {
            let mut address = [0u8; 4];
            socket.read_exact(&mut address).await?;
            std::net::Ipv4Addr::from(address).to_string()
        }
        3 => {
            let mut name = vec![0u8; usize::from(socket.read_u8().await?)];
            socket.read_exact(&mut name).await?;
            String::from_utf8(name)?
        }
        other => anyhow::bail!("address type {other}"),
    };
    let port = socket.read_u16().await?;
    seen.lock().unwrap().push(format!("{host}:{port}"));
    let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)).await else {
        socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    tokio::io::copy_bidirectional(&mut socket, &mut upstream).await?;
    Ok(())
}

/// 主机 `id` 的密码登录，密码按主机 id 存在保险库里。
fn login(id: u64, host: &str, port: u16) -> HostLogin {
    HostLogin::new(host, port, TEST_USER, LoginMethod::Password)
        .with_password(SecretRef::password(id))
}

/// 名叫 `name` 的跳板主机（主机 id 是 `id`）。
fn hop(id: u64, name: &str, port: u16) -> JumpLogin {
    JumpLogin::Host {
        name: name.into(),
        login: Box::new(login(id, "127.0.0.1", port)),
    }
}

fn target(login: HostLogin) -> RemoteTarget {
    RemoteTarget::new(1, login)
}

fn saved(entries: &[(SecretRef, &str)]) -> Arc<Secrets> {
    let secrets = Secrets::default();
    secrets.0.lock().unwrap().extend(
        entries
            .iter()
            .map(|(secret, value)| (secret.clone(), value.to_string())),
    );
    Arc::new(secrets)
}

fn proxy(kind: ProxyKind, port: u16, user: Option<&str>) -> LoginRoute {
    LoginRoute::Proxy(ProxyLogin {
        kind,
        host: "127.0.0.1".into(),
        port,
        user: user.map(str::to_string),
    })
}

/// 问题是在哪台跳板主机上问的（`None` 是要连的主机自己）。
fn asked_at(kind: &ConnectionPromptKind) -> (&'static str, Option<String>) {
    match kind {
        ConnectionPromptKind::UnknownHost(question) => {
            ("主机密钥", question.jump_host().map(str::to_string))
        }
        ConnectionPromptKind::Authentication(question) => {
            ("密码", question.jump_host().map(str::to_string))
        }
        ConnectionPromptKind::HostKeyChanged(question) => {
            ("密钥变了", question.jump_host().map(str::to_string))
        }
    }
}

#[test]
fn a_host_behind_two_jump_hosts_is_reached_through_them_in_order() {
    let lab = Lab::new();
    let (first, second, host) = (lab.server(), lab.server(), lab.server());
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        lab.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));
    let route = LoginRoute::Jump(vec![
        hop(2, "阿里云99", first.port),
        hop(3, "禅道", second.port),
    ]);

    let terminal = Terminal::open(
        &remote,
        1,
        &target(login(1, "127.0.0.1", host.port).with_route(route)),
    );
    terminal.expect_started();
    terminal.echo("hello\n");

    // 每一跳打开去下一跳的通道。
    assert_eq!(
        first.seen().forwards,
        [("127.0.0.1".to_string(), u32::from(second.port))]
    );
    assert_eq!(
        second.seen().forwards,
        [("127.0.0.1".to_string(), u32::from(host.port))]
    );
    // 跳板主机的问题都写明是哪一台。
    let at = |name: &str| Some(name.to_string());
    assert_eq!(
        answers.prompts().iter().map(asked_at).collect::<Vec<_>>(),
        [
            ("主机密钥", at("阿里云99")),
            ("密码", at("阿里云99")),
            ("主机密钥", at("禅道")),
            ("密码", at("禅道")),
            ("主机密钥", None),
            ("密码", None),
        ]
    );
    terminal.close();
}

#[test]
fn jump_hosts_log_in_with_their_own_saved_passwords() {
    let lab = Lab::new();
    let (jump, host) = (lab.server(), lab.server());
    let secrets = saved(&[
        (SecretRef::password(1), TEST_PASSWORD),
        (SecretRef::password(2), TEST_PASSWORD),
    ]);
    let remote = Arc::new(SshRemoteTerminals::with_connector(lab.connector(secrets)));
    let answers = Answerer::start(&remote, None);
    let route = LoginRoute::Jump(vec![hop(2, "跳板", jump.port)]);

    let terminal = Terminal::open(
        &remote,
        1,
        &target(login(1, "127.0.0.1", host.port).with_route(route)),
    );
    terminal.expect_started();

    let asked = answers.prompts();
    assert!(
        asked
            .iter()
            .all(|kind| matches!(kind, ConnectionPromptKind::UnknownHost(_))),
        "{asked:?}"
    );
    terminal.close();
}

#[test]
fn a_deleted_jump_host_fails_before_anything_is_connected() {
    let lab = Lab::new();
    let jump = lab.server();
    let remote = Arc::new(SshRemoteTerminals::with_connector(
        lab.connector(Arc::default()),
    ));
    let answers = Answerer::start(&remote, Some(TEST_PASSWORD));
    let route = LoginRoute::Jump(vec![hop(2, "阿里云99", jump.port), JumpLogin::Deleted]);

    let error = Terminal::open(
        &remote,
        1,
        &target(login(1, "10.0.0.5", 22).with_route(route)),
    )
    .finish()
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "第 2 台跳板主机已被删除，请编辑这台主机的连接方式"
    );
    assert!(answers.prompts().is_empty(), "什么都没连，什么都不问");
    assert!(jump.seen().forwards.is_empty());
}

#[test]
fn a_jump_host_that_cannot_reach_the_next_says_so() {
    let lab = Lab::new();
    let jump = lab.server();
    let closed = closed_port();
    let tester = lab.tester(saved(&[(SecretRef::password(2), TEST_PASSWORD)]));
    let route = LoginRoute::Jump(vec![hop(2, "阿里云99", jump.port)]);
    let request = LoginTest::new(
        login(1, "127.0.0.1", closed).with_route(route),
        TEST_PASSWORD,
    );

    assert_eq!(
        futures::executor::block_on(tester.test(request, trusting(true))),
        Err(format!(
            "跳板主机「阿里云99」无法连接到 127.0.0.1:{closed}：连接失败"
        ))
    );
}

/// 测试连接时表单里只有这台主机自己的密码，跳板主机用它自己存的。
#[test]
fn a_connection_test_through_a_jump_host_uses_the_jump_hosts_saved_password() {
    let lab = Lab::new();
    let (jump, host) = (lab.server(), lab.server());
    let route = LoginRoute::Jump(vec![hop(2, "阿里云99", jump.port)]);
    let request = || {
        LoginTest::new(
            login(1, "127.0.0.1", host.port).with_route(route.clone()),
            TEST_PASSWORD,
        )
    };

    let unsaved = lab.tester(Arc::default());
    assert_eq!(
        futures::executor::block_on(unsaved.test(request(), trusting(true))),
        Err("跳板主机「阿里云99」：未填写密码".into())
    );
    let saved = lab.tester(saved(&[(SecretRef::password(2), TEST_PASSWORD)]));
    assert_eq!(
        futures::executor::block_on(saved.test(request(), trusting(true))),
        Ok(())
    );
}

#[test]
fn a_host_behind_a_socks5_proxy_is_reached_by_name_with_the_proxys_saved_password() {
    let lab = Lab::new();
    let host = lab.server();
    let relay = lab.relay(ProxyKind::Socks5, Some(("me", "proxy-password")));
    let secrets = saved(&[
        (SecretRef::password(1), TEST_PASSWORD),
        (
            SecretRef::proxy("me", "127.0.0.1", relay.port),
            "proxy-password",
        ),
    ]);
    let remote = Arc::new(SshRemoteTerminals::with_connector(lab.connector(secrets)));
    let _answers = Answerer::start(&remote, None);
    let route = proxy(ProxyKind::Socks5, relay.port, Some("me"));

    // 名字交给代理去解析。
    let terminal = Terminal::open(
        &remote,
        1,
        &target(login(1, "localhost", host.port).with_route(route)),
    );
    terminal.expect_started();
    terminal.echo("through the proxy\n");

    assert_eq!(relay.targets(), [format!("localhost:{}", host.port)]);
    terminal.close();
}

/// 测试连接用表单里填的代理密码，存没存过都一样。
#[test]
fn a_connection_test_uses_the_proxy_password_typed_in_the_form() {
    let lab = Lab::new();
    let host = lab.server();
    let relay = lab.relay(ProxyKind::Socks5, Some(("me", "proxy-password")));
    let saved_wrong = saved(&[(SecretRef::proxy("me", "127.0.0.1", relay.port), "stale")]);
    let tester = lab.tester(saved_wrong);
    let request = |typed: &str| {
        LoginTest::new(
            login(1, "127.0.0.1", host.port).with_route(proxy(
                ProxyKind::Socks5,
                relay.port,
                Some("me"),
            )),
            TEST_PASSWORD,
        )
        .with_proxy_password(typed)
    };

    assert_eq!(
        futures::executor::block_on(tester.test(request("wrong"), trusting(true))),
        Err("代理服务器拒绝了用户名或密码".into())
    );
    assert_eq!(
        futures::executor::block_on(tester.test(request("proxy-password"), trusting(true))),
        Ok(())
    );
}

#[test]
fn a_host_behind_an_http_proxy_is_reached_through_it() {
    let lab = Lab::new();
    let host = lab.server();
    let relay = lab.relay(ProxyKind::Http, None);
    let tester = lab.tester(Arc::default());
    let request = LoginTest::new(
        login(1, "127.0.0.1", host.port).with_route(proxy(ProxyKind::Http, relay.port, None)),
        TEST_PASSWORD,
    );

    assert_eq!(
        futures::executor::block_on(tester.test(request, trusting(true))),
        Ok(())
    );
    assert_eq!(relay.targets(), [format!("127.0.0.1:{}", host.port)]);
}

#[test]
fn a_proxy_that_is_not_there_is_named() {
    let lab = Lab::new();
    let port = closed_port();
    let tester = lab.tester(Arc::default());
    let request = LoginTest::new(
        login(1, "10.0.0.5", 22).with_route(proxy(ProxyKind::Socks5, port, None)),
        TEST_PASSWORD,
    );

    assert_eq!(
        futures::executor::block_on(tester.test(request, trusting(true))),
        Err(format!(
            "无法连接代理服务器 127.0.0.1:{port}：连接被拒绝，该端口上没有服务在监听"
        ))
    );
}
