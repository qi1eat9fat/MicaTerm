//! 外部 CLI 的 `exec`（Agent CLI 工单 02）对着进程内的测试服务器：没人可问，所以陌生主机、
//! 没存的密码当场失败并说明是哪一种；信任过、存了密码的主机跑完命令交回输出和退出码。

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::testing::{TEST_PASSWORD, TEST_USER, TestServer};
use crate::{
    ConnectionPromptKind, ConnectionPromptReply, ExecErrorKind, ExecExit, ExecStream, HostLogin,
    LoginMethod, SecretRef, SecretSource, SshConnectionConfig, SshConnector, SshPrompts,
    run_command,
};

/// 内存里的保险库。
#[derive(Default)]
struct Vault(Mutex<HashMap<SecretRef, String>>);

impl SecretSource for Vault {
    fn secret(&self, secret: &SecretRef) -> Result<Option<Zeroizing<String>>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(secret)
            .cloned()
            .map(Zeroizing::new))
    }
}

fn connector(known_hosts: &Path, saved_password: bool) -> SshConnector {
    let vault = Vault::default();
    if saved_password {
        vault
            .0
            .lock()
            .unwrap()
            .insert(SecretRef::password(7), TEST_PASSWORD.into());
    }
    SshConnector::new(known_hosts, Arc::new(vault)).with_default_key_dir(None)
}

fn login(port: u16) -> HostLogin {
    HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Password)
        .with_password(SecretRef::password(7))
}

/// 照用户在 MicaTerm 里连过一次那样，先信任并记下主机密钥。
async fn trust(connector: &SshConnector, login: &HostLogin) {
    let (_shutdown, shutdown) = watch::channel(false);
    let broker: Arc<std::sync::Mutex<Option<std::sync::Weak<SshPrompts>>>> = Arc::default();
    let answer = broker.clone();
    let prompts = Arc::new(SshPrompts::new(
        Arc::new(move |prompt: crate::ConnectionPrompt| {
            let reply = match prompt.kind() {
                ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                _ => ConnectionPromptReply::Cancel,
            };
            if let Some(broker) = answer
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|weak| weak.upgrade())
            {
                broker.respond(prompt.request_id(), reply);
            }
            true
        }),
        shutdown,
    ));
    *broker.lock().unwrap() = Some(Arc::downgrade(&prompts));
    connector
        .connect(&SshConnectionConfig::from(login), prompts)
        .await
        .unwrap();
}

/// 在阻塞线程上跑一条命令：输出收进 `(stdout, stderr)`。
async fn exec(
    connector: SshConnector,
    login: HostLogin,
    command: &'static str,
) -> (Result<ExecExit, crate::ExecError>, Vec<u8>, Vec<u8>) {
    tokio::task::spawn_blocking(move || {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let result = run_command(
            &connector,
            &SshConnectionConfig::from(&login),
            command,
            &mut |stream, bytes| {
                match stream {
                    ExecStream::Stdout => stdout.extend_from_slice(bytes),
                    ExecStream::Stderr => stderr.extend_from_slice(bytes),
                }
                Ok(())
            },
        );
        (result, stdout, stderr)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_host_never_trusted_fails_at_once_and_says_so() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let connector = connector(&dir.path().join("known_hosts"), true);

    let (result, stdout, _) = exec(connector, login(server.port), "uname -a").await;

    assert_eq!(result.unwrap_err().kind, ExecErrorKind::HostKeyUnknown);
    assert!(stdout.is_empty());
    assert!(server.seen().commands.is_empty());
}

#[tokio::test]
async fn a_trusted_host_with_a_saved_password_runs_the_command() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    server.reply("uname -a", "Linux test 6.8\n");
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    trust(&connector(&known_hosts, true), &login(server.port)).await;

    let (result, stdout, stderr) = exec(
        connector(&known_hosts, true),
        login(server.port),
        "uname -a",
    )
    .await;

    assert_eq!(result.unwrap(), ExecExit::Code(0));
    assert_eq!(stdout, b"Linux test 6.8\n");
    assert!(stderr.is_empty());
    assert_eq!(server.seen().commands, ["uname -a"]);
}

#[tokio::test]
async fn a_password_nobody_saved_is_a_missing_credential_not_a_question() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    trust(&connector(&known_hosts, true), &login(server.port)).await;

    let (result, ..) = exec(connector(&known_hosts, false), login(server.port), "id").await;

    assert_eq!(result.unwrap_err().kind, ExecErrorKind::MissingCredential);
}

#[tokio::test]
async fn output_nobody_reads_abandons_the_command() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    trust(&connector(&known_hosts, true), &login(server.port)).await;

    let connector = connector(&known_hosts, true);
    let login = login(server.port);
    let result = tokio::task::spawn_blocking(move || {
        run_command(
            &connector,
            &SshConnectionConfig::from(&login),
            "yes",
            &mut |_, _| Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        )
    })
    .await
    .unwrap();

    assert_eq!(result.unwrap_err().kind, ExecErrorKind::Aborted);
}
