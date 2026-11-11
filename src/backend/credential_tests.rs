//! 用凭据登录（工单 14）：主机数据里的凭据（私钥在保险库里）经连接表对着进程内的 russh 测试服务器
//! 真的登录。不碰真实的 Agent 和 `~/.ssh`（「无密码」的默认私钥放在临时目录里）。

use std::sync::Arc;

use qishell_hosts::secrets::{InMemorySecretStore, SharedSecretStore, StoreSource};
use qishell_hosts::{
    AuthKind, CredentialDraft, CredentialKind, GeneratedKey, HostData, HostDraft, HostId,
    KeyAlgorithm, SecretStore as _,
};
use qishell_ssh::testing::{TEST_USER, TestServer};
use qishell_ssh::{AgentLocation, ConnectionPromptKind, SshConnector};
use russh::keys::PublicKey;

use super::tests::{Answerer, Terminal};
use super::{RemoteTarget, SshRemoteTerminals};

/// 测试服务器、它的运行时和临时目录（`known_hosts`、「无密码」的默认私钥都在里面）。
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

    /// 连接读 `secrets` 里的秘密，「无密码」在临时目录里找默认私钥。
    fn remote(&self, secrets: SharedSecretStore) -> Arc<SshRemoteTerminals> {
        let connector = SshConnector::new(
            self.directory.path().join("known_hosts"),
            StoreSource::shared(secrets),
        )
        .with_agent(AgentLocation::None)
        .with_default_key_dir(Some(self.directory.path().to_path_buf()));
        Arc::new(SshRemoteTerminals::with_connector(connector))
    }

    /// 让服务器认 `key`。
    fn authorize(&self, key: &GeneratedKey) {
        self.server
            .authorize(PublicKey::from_openssh(&key.public_key_line("")).unwrap());
    }
}

/// 一台指向测试服务器的主机，id 是 1。
fn host_at(port: u16, auth: AuthKind) -> HostDraft {
    HostDraft::new("test", "127.0.0.1", port, TEST_USER, auth)
}

fn target(data: &HostData) -> RemoteTarget {
    RemoteTarget::new(1, data.login(HostId(1)).unwrap())
}

/// 连上以后只问过主机密钥，没问过密码或口令。
fn asked_only_for_the_host_key(answers: &Answerer) {
    let prompts = answers.prompts();
    assert!(
        prompts
            .iter()
            .all(|kind| matches!(kind, ConnectionPromptKind::UnknownHost(_))),
        "{prompts:?}"
    );
}

#[test]
fn a_host_using_a_key_credential_logs_in_with_the_key_kept_in_the_vault() {
    let fixture = Fixture::start();
    let key = GeneratedKey::generate(KeyAlgorithm::Ed25519).unwrap();
    fixture.authorize(&key);
    let secrets = Arc::new(InMemorySecretStore::default());
    let mut data = HostData::empty().with_secrets(secrets.clone());
    let credential = data.insert_credential_unnotified(
        CredentialDraft::new("部署密钥", CredentialKind::Key, "someone-else")
            .with_public_key(key.public_key_line("")),
    );
    // 私钥用口令加密：口令和私钥一起存在保险库里，登录时都不问。
    let saved = data.credential(credential).unwrap().clone();
    secrets
        .set(
            &saved.private_key_secret(),
            &key.encode("部署密钥", "correct horse").unwrap(),
        )
        .unwrap();
    secrets
        .set(&saved.passphrase_secret(), "correct horse")
        .unwrap();
    // 用户名跟着凭据走：先把凭据的用户名改成测试服务器认的那个。
    data.update_credential_unnotified(
        credential,
        CredentialDraft::new("部署密钥", CredentialKind::Key, TEST_USER),
    );
    data.insert_unnotified(
        host_at(fixture.server.port, AuthKind::Password).with_credential(credential),
    );

    let remote = fixture.remote(secrets);
    let answers = Answerer::start(&remote, None);
    let terminal = Terminal::open(&remote, 1, &target(&data));
    terminal.expect_started();
    terminal.echo("hello\n");
    asked_only_for_the_host_key(&answers);
    terminal.close();
}

#[test]
fn a_host_without_a_password_logs_in_with_a_default_key() {
    let fixture = Fixture::start();
    let key = GeneratedKey::generate(KeyAlgorithm::Ed25519).unwrap();
    fixture.authorize(&key);
    std::fs::write(
        fixture.directory.path().join("id_ed25519"),
        key.encode("", "").unwrap().as_str(),
    )
    .unwrap();
    let mut data = HostData::empty();
    data.insert_unnotified(host_at(fixture.server.port, AuthKind::NoPassword));

    let remote = fixture.remote(Arc::new(InMemorySecretStore::default()));
    let answers = Answerer::start(&remote, None);
    let terminal = Terminal::open(&remote, 1, &target(&data));
    terminal.expect_started();
    asked_only_for_the_host_key(&answers);
    terminal.close();
}

#[test]
fn a_host_using_a_password_credential_logs_in_with_the_credentials_password() {
    let fixture = Fixture::start();
    let secrets = Arc::new(InMemorySecretStore::default());
    let mut data = HostData::empty().with_secrets(secrets.clone());
    let credential = data.insert_credential_unnotified(CredentialDraft::new(
        "运维密码",
        CredentialKind::Password,
        TEST_USER,
    ));
    let saved = data.credential(credential).unwrap().clone();
    secrets
        .set(
            &saved.password_secret(),
            qishell_ssh::testing::TEST_PASSWORD,
        )
        .unwrap();
    data.insert_unnotified(
        host_at(fixture.server.port, AuthKind::Password).with_credential(credential),
    );

    let remote = fixture.remote(secrets);
    let answers = Answerer::start(&remote, None);
    let terminal = Terminal::open(&remote, 1, &target(&data));
    terminal.expect_started();
    asked_only_for_the_host_key(&answers);
    terminal.close();
}
