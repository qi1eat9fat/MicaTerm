//! 私钥登录：私钥是保险库里的一条秘密，经 `SecretSource` 读进内存再解析，
//! 不再按文件路径读。对着进程内的 russh 测试服务器真的登录一次。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use tokio::sync::{mpsc, watch};
use zeroize::Zeroizing;

use crate::testing::{TEST_USER, TestServer};
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

/// 登录一次：信任主机密钥，问口令时答 `passphrase`（没有就取消）。
/// 返回结果和登录过程中问过的认证问题。
async fn connect(
    login: &HostLogin,
    secrets: Arc<dyn SecretSource>,
    passphrase: Option<&str>,
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
        let passphrase = passphrase.map(str::to_string);
        tokio::spawn(async move {
            let mut requests = Vec::new();
            while let Some(prompt) = asked.recv().await {
                let reply = match prompt.kind() {
                    ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                    ConnectionPromptKind::Authentication(question) => {
                        requests.push(question.request().clone());
                        match &passphrase {
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
