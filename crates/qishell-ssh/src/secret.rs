// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/secret.rs
// 裁剪：去掉临时连接（`Temporary`）和同步（`SyncKeyring`、`SyncLogin`）的秘密。
// 改造：QiShell 唯一的一份 `SecretRef`（qishell-hosts 重新导出它）：主机自己的登录密码按主机 id 存
// （2026-11-04 用户决定，副本不带密码）；私钥进保险库，加 `PrivateKey`，口令按凭据存；
// 凭据的条目名字段叫 `secret_id`；`account()` 是明文条目名，保险库直接拿它当键；`Debug` 只打印条目名。
//! Which secret a connection needs, and where it reads the saved ones.
//! QiShell keeps every secret in its vault; the connection only ever asks
//! for one by its [`SecretRef`].

use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use zeroize::Zeroizing;

/// The identity of one secret, as the database refers to it: the secret
/// itself is only ever in the vault.
///
/// 主机自己的登录密码属于这台主机，按主机 id 存：改名、改地址都还在，「创建副本」不带它，
/// 多台主机共用一个密码请用凭据。A password credential's password, a key
/// credential's private key and its passphrase belong to the credential,
/// shared by the hosts that use it, by the credential's random `secret_id`
/// that is never reused. A proxy's password belongs to the proxy's address
/// and user, shared by the hosts behind it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum SecretRef {
    /// 一台保存的主机自己的登录密码，按主机 id（qishell-hosts 的 `HostId`）。
    Password { host: u64 },
    /// A password credential's password.
    Credential { secret_id: String },
    /// 密钥凭据的私钥正文（OpenSSH、PEM 或 PuTTY 文本）。没有 `keys/` 目录，私钥只在保险库里。
    PrivateKey { secret_id: String },
    /// 密钥凭据的私钥口令。
    Passphrase { secret_id: String },
    /// The password of an HTTP or SOCKS5 proxy.
    Proxy {
        user: String,
        host: String,
        port: u16,
    },
}

impl SecretRef {
    pub fn password(host: u64) -> Self {
        Self::Password { host }
    }

    pub fn credential(secret_id: impl Into<String>) -> Self {
        Self::Credential {
            secret_id: secret_id.into(),
        }
    }

    pub fn private_key(secret_id: impl Into<String>) -> Self {
        Self::PrivateKey {
            secret_id: secret_id.into(),
        }
    }

    pub fn passphrase(secret_id: impl Into<String>) -> Self {
        Self::Passphrase {
            secret_id: secret_id.into(),
        }
    }

    pub fn proxy(user: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self::Proxy {
            user: user.into(),
            host: host.into(),
            port,
        }
    }

    /// The entry's name in the vault, in plain text. Its prefix says the
    /// kind. Saved secrets are found by it, so it never changes: the vault
    /// file keys each secret by it.
    pub fn account(&self) -> String {
        match self {
            Self::Password { host } => format!("password:host-{host}"),
            Self::Credential { secret_id } => format!("credential:{secret_id}"),
            Self::PrivateKey { secret_id } => format!("private-key:{secret_id}"),
            Self::Passphrase { secret_id } => format!("passphrase:{secret_id}"),
            Self::Proxy { user, host, port } => format!("proxy:{user}@{host}:{port}"),
        }
    }

    /// 是不是代理的密码：它按代理的地址和用户名被多台主机共用，最后一台用它的主机没了才删。
    /// 主机自己的密码跟着主机走，凭据的几种跟着凭据走。
    pub fn is_shared_by_hosts(&self) -> bool {
        matches!(self, Self::Proxy { .. })
    }
}

/// 只写条目名：`SecretRef` 本身不含秘密，但日志里也不必展开字段。
impl fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SecretRef({})", self.account())
    }
}

/// Where a connection reads the secrets saved for it.
///
/// It may block (the vault reads a file, and may ask the system keychain),
/// so a connection reads only what the server asks for, and never on an
/// async worker: see [`read_secret`].
pub trait SecretSource: Send + Sync + 'static {
    /// The saved secret, or `Ok(None)` when none is saved.
    fn secret(&self, secret: &SecretRef) -> Result<Option<Zeroizing<String>>>;
}

pub type SharedSecretSource = Arc<dyn SecretSource>;

/// Read a saved secret off the async workers, for what the connection can
/// ask for instead (a password, a passphrase). A source that errors, is
/// locked, or holds an empty value counts as nothing saved: the connection
/// then falls back to asking, which is always better than refusing to
/// connect.
pub async fn read_secret(
    secrets: &SharedSecretSource,
    secret: &SecretRef,
) -> Option<Zeroizing<String>> {
    let (secrets, secret) = (secrets.clone(), secret.clone());
    tokio::task::spawn_blocking(move || secrets.secret(&secret))
        .await
        .ok()?
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

/// Read a saved secret off the async workers, for what nobody can be asked
/// for instead (a private key): the source's own error, such as a locked
/// vault's, comes out as it is, for the caller to recognise. An empty value
/// counts as nothing saved.
pub(crate) async fn read_secret_or_fail(
    secrets: &SharedSecretSource,
    secret: &SecretRef,
) -> Result<Option<Zeroizing<String>>> {
    let (secrets, secret) = (secrets.clone(), secret.clone());
    let value = tokio::task::spawn_blocking(move || secrets.secret(&secret)).await??;
    Ok(value.filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::SecretRef;

    /// 主机自己的密码按主机 id 存：同一端点的两台主机（比如副本）各有各的。
    #[test]
    fn password_accounts_name_the_host() {
        let secret = SecretRef::password(7);
        assert_eq!(secret.account(), "password:host-7");
        assert_ne!(secret, SecretRef::password(8));
        assert!(!secret.is_shared_by_hosts());
    }

    #[test]
    fn a_credential_s_secrets_are_told_apart_by_kind() {
        let accounts = [
            SecretRef::credential("Jwg5rHvXCxw89paM"),
            SecretRef::private_key("Jwg5rHvXCxw89paM"),
            SecretRef::passphrase("Jwg5rHvXCxw89paM"),
        ]
        .map(|secret| secret.account());
        assert_eq!(
            accounts,
            [
                "credential:Jwg5rHvXCxw89paM",
                "private-key:Jwg5rHvXCxw89paM",
                "passphrase:Jwg5rHvXCxw89paM",
            ]
        );
    }

    #[test]
    fn proxy_accounts_name_the_proxy() {
        let secret = SecretRef::proxy("me", "127.0.0.1", 7890);
        assert_eq!(secret.account(), "proxy:me@127.0.0.1:7890");
        assert!(secret.is_shared_by_hosts());
        assert!(!SecretRef::credential("x").is_shared_by_hosts());
    }

    #[test]
    fn accounts_of_different_kinds_never_collide() {
        let accounts = [
            SecretRef::password(22),
            SecretRef::credential("host-22"),
            SecretRef::private_key("host-22"),
            SecretRef::passphrase("host-22"),
            SecretRef::proxy("host", "22", 22),
        ]
        .map(|secret| secret.account());
        for (ix, account) in accounts.iter().enumerate() {
            assert!(!accounts[ix + 1..].contains(account), "{account}");
        }
    }

    /// 日志里只出现条目名。
    #[test]
    fn debug_shows_only_the_account() {
        assert_eq!(
            format!("{:?}", SecretRef::proxy("me", "127.0.0.1", 7890)),
            "SecretRef(proxy:me@127.0.0.1:7890)"
        );
    }
}
