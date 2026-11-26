//! 对着进程内的 russh 测试服务器真的登录：
//! - 密码：主机自己的密码按主机 id 存在保险库里，没保存的登录现场询问；
//! - 私钥：私钥是保险库里的一条秘密，经 `SecretSource` 读进内存再解析，不再按文件路径读。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use tokio::sync::{mpsc, watch};
use zeroize::Zeroizing;

use crate::testing::{TEST_PASSWORD, TEST_USER, TestServer};
use crate::{
    AuthError, AuthenticationRequest, ConnectionPromptKind, ConnectionPromptReply,
    ConnectionSecret, HostLogin, LoginMethod, PrivateKeySource, SavedKey, SecretRef, SecretSource,
    SshConnectionConfig, SshConnector, SshPrompts, find_cause,
};

/// 内存里的保险库。
#[derive(Default)]
struct Vault(Mutex<HashMap<SecretRef, String>>);

impl Vault {
    fn with(entries: &[(SecretRef, &str)]) -> Arc<Self> {
        let vault = Self::default();
        for (secret, value) in entries {
            vault
                .0
                .lock()
                .unwrap()
                .insert(secret.clone(), value.to_string());
        }
        Arc::new(vault)
    }
}

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

/// 锁着的保险库：读什么都报「已锁定」。
struct LockedVault;

#[derive(Debug)]
struct VaultLocked;

impl std::fmt::Display for VaultLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the vault is locked")
    }
}

impl std::error::Error for VaultLocked {}

impl SecretSource for LockedVault {
    fn secret(&self, _: &SecretRef) -> Result<Option<Zeroizing<String>>> {
        Err(VaultLocked.into())
    }
}

fn random_key() -> PrivateKey {
    PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap()
}

fn openssh_text(key: &PrivateKey) -> String {
    key.to_openssh(LineEnding::LF).unwrap().to_string()
}

fn key_login(port: u16, key: &SavedKey) -> HostLogin {
    let mut login = HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Key);
    login.key = Some(key.clone());
    login
}

/// 登录一次：信任主机密钥，问密码或口令时答 `answer`（没有就取消）。
/// 返回结果和登录过程中问过的认证问题。
async fn connect(
    login: &HostLogin,
    secrets: Arc<dyn SecretSource>,
    answer: Option<&str>,
) -> (Result<()>, Vec<AuthenticationRequest>) {
    let directory = tempfile::tempdir().unwrap();
    let connector =
        SshConnector::new(directory.path().join("known_hosts"), secrets).with_default_key_dir(None);
    let (prompts, mut asked) = mpsc::unbounded_channel();
    let (_shutdown, shutdown) = watch::channel(false);
    let broker = Arc::new(SshPrompts::new(
        Arc::new(move |prompt| prompts.send(prompt).is_ok()),
        shutdown,
    ));
    let answerer = {
        // 弱引用：连接用完后 broker 就该没了，好让提问的通道关上。
        let broker = Arc::downgrade(&broker);
        let answer = answer.map(str::to_string);
        tokio::spawn(async move {
            let mut requests = Vec::new();
            while let Some(prompt) = asked.recv().await {
                let reply = match prompt.kind() {
                    ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                    ConnectionPromptKind::Authentication(question) => {
                        requests.push(question.request().clone());
                        match &answer {
                            Some(answer) => {
                                ConnectionPromptReply::Answers(vec![ConnectionSecret::new(
                                    answer.clone(),
                                )])
                            }
                            None => ConnectionPromptReply::Cancel,
                        }
                    }
                    _ => ConnectionPromptReply::Cancel,
                };
                if let Some(broker) = broker.upgrade() {
                    broker.respond(prompt.request_id(), reply);
                }
            }
            requests
        })
    };
    let result = connector
        .connect(&SshConnectionConfig::from(login), broker)
        .await
        .map(|_| ());
    // 连接用完即弃：提问的发送端随 broker 一起没了，回答的任务就结束。
    let requests = answerer.await.unwrap();
    (result, requests)
}

fn password_login(port: u16) -> HostLogin {
    HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Password)
}

#[tokio::test]
async fn a_hosts_saved_password_logs_in_without_asking() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let login = password_login(server.port).with_password(SecretRef::password(7));
    let vault = Vault::with(&[(SecretRef::password(7), TEST_PASSWORD)]);

    let (result, asked) = connect(&login, vault, None).await;

    result.unwrap();
    assert!(asked.is_empty(), "{asked:?}");
}

/// 另一台主机（比如副本）的密码不算这台的：按主机 id 存，同一个 `user@host:port` 也不共用。
#[tokio::test]
async fn another_hosts_password_is_not_this_ones() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let login = password_login(server.port).with_password(SecretRef::password(8));
    let vault = Vault::with(&[(SecretRef::password(7), TEST_PASSWORD)]);

    let (result, asked) = connect(&login, vault, Some(TEST_PASSWORD)).await;

    result.unwrap();
    assert_eq!(
        asked,
        [AuthenticationRequest::Password {
            saved_rejected: false
        }]
    );
}

/// 没保存的登录（快速连接）没有密码条目，直接问。
#[tokio::test]
async fn a_login_with_nothing_saved_asks_for_the_password() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();

    let (result, asked) = connect(
        &password_login(server.port),
        Vault::with(&[]),
        Some(TEST_PASSWORD),
    )
    .await;

    result.unwrap();
    assert_eq!(
        asked,
        [AuthenticationRequest::Password {
            saved_rejected: false
        }]
    );
}

#[tokio::test]
async fn a_key_kept_in_the_vault_logs_in() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let key = random_key();
    server.authorize(key.public_key().clone());
    let saved = SavedKey::new("deploy-key");
    let vault = Vault::with(&[(saved.private_key(), &openssh_text(&key))]);

    let (result, asked) = connect(&key_login(server.port, &saved), vault, None).await;

    result.unwrap();
    assert!(asked.is_empty(), "{asked:?}");
}

#[tokio::test]
async fn a_key_the_server_does_not_know_is_rejected() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let saved = SavedKey::new("stranger");
    let vault = Vault::with(&[(saved.private_key(), &openssh_text(&random_key()))]);

    let (result, _) = connect(&key_login(server.port, &saved), vault, None).await;

    let error = result.unwrap_err();
    assert_eq!(
        find_cause::<AuthError>(&error),
        Some(&AuthError::KeyRejected)
    );
}

#[tokio::test]
async fn an_encrypted_key_is_unlocked_with_its_saved_passphrase() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let key = random_key();
    server.authorize(key.public_key().clone());
    let encrypted = key
        .encrypt(&mut russh::keys::key::safe_rng(), "correct horse")
        .unwrap();
    let saved = SavedKey::new("encrypted");
    let vault = Vault::with(&[
        (saved.private_key(), &openssh_text(&encrypted)),
        (saved.passphrase(), "correct horse"),
    ]);

    let (result, asked) = connect(&key_login(server.port, &saved), vault, None).await;

    result.unwrap();
    assert!(asked.is_empty(), "{asked:?}");
}

#[tokio::test]
async fn an_encrypted_key_without_a_saved_passphrase_asks_for_it() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let key = random_key();
    server.authorize(key.public_key().clone());
    let encrypted = key
        .encrypt(&mut russh::keys::key::safe_rng(), "correct horse")
        .unwrap();
    let saved = SavedKey::new("encrypted");
    let vault = Vault::with(&[(saved.private_key(), &openssh_text(&encrypted))]);

    let login = key_login(server.port, &saved);
    let (result, asked) = connect(&login, vault, Some("correct horse")).await;

    result.unwrap();
    assert_eq!(
        asked,
        [AuthenticationRequest::Passphrase {
            key: PrivateKeySource::Saved(saved),
            saved_rejected: false,
        }]
    );
}

#[tokio::test]
async fn a_key_missing_from_the_vault_says_so() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let saved = SavedKey::new("lost");

    let (result, _) = connect(&key_login(server.port, &saved), Vault::with(&[]), None).await;

    let error = result.unwrap_err();
    assert_eq!(
        find_cause::<AuthError>(&error),
        Some(&AuthError::KeyMissing)
    );
}

#[tokio::test]
async fn text_that_is_not_a_key_is_unreadable() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let saved = SavedKey::new("garbage");
    let vault = Vault::with(&[(saved.private_key(), "not a private key")]);

    let (result, _) = connect(&key_login(server.port, &saved), vault, None).await;

    let error = result.unwrap_err();
    assert_eq!(
        find_cause::<AuthError>(&error),
        Some(&AuthError::KeyUnreadable {
            key: PrivateKeySource::Saved(saved),
        })
    );
}

/// 保险库锁着时，读私钥的错误原样沿链传上去，调用方能认出来并弹出解锁框。
#[tokio::test]
async fn a_locked_vault_fails_the_login_with_its_own_error() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let saved = SavedKey::new("deploy-key");

    let (result, _) = connect(&key_login(server.port, &saved), Arc::new(LockedVault), None).await;

    let error = result.unwrap_err();
    assert!(
        error.chain().any(|cause| cause.is::<VaultLocked>()),
        "{error:?}"
    );
}

#[tokio::test]
async fn open_shell_advertises_truecolor_env() {
    let server = TestServer::start(std::env::temp_dir()).await.unwrap();
    let key = random_key();
    server.authorize(key.public_key().clone());
    let saved = SavedKey::new("env-check");
    let vault = Vault::with(&[(saved.private_key(), &openssh_text(&key))]);

    let directory = tempfile::tempdir().unwrap();
    let connector =
        SshConnector::new(directory.path().join("known_hosts"), vault).with_default_key_dir(None);
    let (_shutdown, shutdown) = watch::channel(false);
    let broker_slot: Arc<std::sync::Mutex<Option<std::sync::Weak<SshPrompts>>>> =
        Arc::default();
    let reply_slot = broker_slot.clone();
    let broker = Arc::new(SshPrompts::new(
        Arc::new(move |prompt| {
            let reply = match prompt.kind() {
                ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                _ => ConnectionPromptReply::Cancel,
            };
            if let Some(broker) = reply_slot
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
    *broker_slot.lock().unwrap() = Some(Arc::downgrade(&broker));

    let (handle, _) = connector
        .connect(
            &SshConnectionConfig::from(&key_login(server.port, &saved)),
            broker,
        )
        .await
        .unwrap();
    let mut channel = crate::open_shell(
        &handle,
        "xterm-256color",
        crate::PtySize {
            columns: 80,
            rows: 24,
            width: 0,
            height: 0,
        },
    )
    .await
    .unwrap();
    // 等服务器按序处理完 pty_request、env_request、shell_request 后发回的两个 Success。
    assert!(matches!(channel.wait().await, Some(russh::ChannelMsg::Success)));
    assert!(matches!(channel.wait().await, Some(russh::ChannelMsg::Success)));
    assert!(
        server
            .seen()
            .env
            .contains(&("COLORTERM".to_string(), "truecolor".to_string())),
        "{:?}",
        server.seen().env
    );
}
