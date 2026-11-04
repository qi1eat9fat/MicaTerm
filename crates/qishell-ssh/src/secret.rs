// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/secret.rs
// 裁剪：去掉同步用的秘密（`SyncKeyring`、`SyncLogin`）。
// 改造：私钥进保险库：加 `PrivateKey`，口令从按文件路径存改成按凭据的 `keychain_id` 存。
//! Which secret a connection needs, and where it reads the saved ones.
//! Each app keeps its secrets in its own platform's keychain; the
//! connection only ever asks for one by its [`SecretRef`].

use std::sync::Arc;

use anyhow::Result;
use zeroize::Zeroizing;

/// The identity of one secret.
///
/// A login password belongs to the user and endpoint it logs in to, not to
/// a saved host: renaming or copying a host keeps it, and hosts of the same
/// user and endpoint share it. A key credential's private key and its
/// passphrase belong to the credential, so one key is asked about once. A
/// password credential's password belongs to the credential, shared by the
/// hosts that use it. A proxy's password belongs to the proxy's address and
/// user, shared by the hosts behind it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SecretRef {
    /// The login password of an SSH endpoint.
    Password {
        user: String,
        host: String,
        port: u16,
    },
    /// A key credential's private key, the key file's text (OpenSSH, PEM or
    /// PuTTY), by the credential's `keychain_id`.
    PrivateKey { keychain_id: String },
    /// The passphrase of a key credential's private key, by the credential's
    /// `keychain_id`.
    Passphrase { keychain_id: String },
    /// A password credential's password, by the credential's random
    /// `keychain_id` that is never reused.
    Credential { keychain_id: String },
    /// The password of an HTTP or SOCKS5 proxy.
    Proxy {
        user: String,
        host: String,
        port: u16,
    },
    /// The password of a connection that is not saved (an external or quick
    /// connection), by that temporary host's id. Only ever held in memory,
    /// never in a keychain.
    Temporary { host: u64 },
}

impl SecretRef {
    pub fn password(user: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self::Password {
            user: user.into(),
            host: host.into(),
            port,
        }
    }

    pub fn private_key(keychain_id: impl Into<String>) -> Self {
        Self::PrivateKey {
            keychain_id: keychain_id.into(),
        }
    }

    pub fn passphrase(keychain_id: impl Into<String>) -> Self {
        Self::Passphrase {
            keychain_id: keychain_id.into(),
        }
    }

    pub fn credential(keychain_id: impl Into<String>) -> Self {
        Self::Credential {
            keychain_id: keychain_id.into(),
        }
    }

    pub fn proxy(user: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self::Proxy {
            user: user.into(),
            host: host.into(),
            port,
        }
    }

    pub fn temporary(host: u64) -> Self {
        Self::Temporary { host }
    }

    /// The keychain entry's account name. Its prefix says the kind, readable
    /// as it is in Keychain Access. Saved secrets are found by it, so it
    /// never changes.
    pub fn account(&self) -> String {
        match self {
            Self::Password { user, host, port } => format!("password:{user}@{host}:{port}"),
            Self::PrivateKey { keychain_id } => format!("private-key:{keychain_id}"),
            Self::Passphrase { keychain_id } => format!("passphrase:{keychain_id}"),
            Self::Credential { keychain_id } => format!("credential:{keychain_id}"),
            Self::Proxy { user, host, port } => format!("proxy:{user}@{host}:{port}"),
            Self::Temporary { host } => format!("temporary:{host}"),
        }
    }
}

/// Where a connection reads the secrets saved for it.
///
/// It may block (a keychain may ask the user for access), so a connection
/// reads only what the server asks for, and never on an async worker: see
/// [`read_secret`].
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

    #[test]
    fn password_accounts_name_the_endpoint() {
        let secret = SecretRef::password("root", "10.0.1.12", 22);
        assert_eq!(secret.account(), "password:root@10.0.1.12:22");
    }

    #[test]
    fn private_key_and_passphrase_accounts_name_the_keychain_id() {
        assert_eq!(
            SecretRef::private_key("Jwg5rHvXCxw89paM").account(),
            "private-key:Jwg5rHvXCxw89paM"
        );
        assert_eq!(
            SecretRef::passphrase("Jwg5rHvXCxw89paM").account(),
            "passphrase:Jwg5rHvXCxw89paM"
        );
    }

    #[test]
    fn credential_accounts_name_the_keychain_id() {
        let secret = SecretRef::credential("Jwg5rHvXCxw89paM");
        assert_eq!(secret.account(), "credential:Jwg5rHvXCxw89paM");
    }

    #[test]
    fn proxy_accounts_name_the_proxy_and_its_user() {
        let secret = SecretRef::proxy("me", "127.0.0.1", 7890);
        assert_eq!(secret.account(), "proxy:me@127.0.0.1:7890");
    }

    #[test]
    fn other_accounts_are_fixed() {
        assert_eq!(SecretRef::temporary(7).account(), "temporary:7");
    }

    #[test]
    fn accounts_of_different_kinds_never_collide() {
        let accounts = [
            SecretRef::password("root", "10.0.1.12", 22),
            SecretRef::private_key("root@10.0.1.12:22"),
            SecretRef::passphrase("root@10.0.1.12:22"),
            SecretRef::credential("root@10.0.1.12:22"),
            SecretRef::proxy("root", "10.0.1.12", 22),
            SecretRef::temporary(22),
        ]
        .map(|secret| secret.account());
        for (ix, account) in accounts.iter().enumerate() {
            assert!(!accounts[ix + 1..].contains(account), "{account}");
        }
    }
}
