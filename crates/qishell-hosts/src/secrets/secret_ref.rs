// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/secret.rs（MIT，版权声明同本 crate 的 LICENSE）
// 裁剪：只拿 `SecretRef`；去掉临时连接和同步的变体，口令改按凭据存，加上保险库里的私钥；
// 主机的登录密码改按主机 id 单独存（用户 2026-11-04 决定，不再按 user@host:port 共用）。
// 上游的 `SecretRef` 在 ssh crate 里，hosts 再导出它。qishell-ssh（工单 08）和本 crate 并行拷入，
// 这里先放一份 hosts 自己的；两边合入后由接 SSH 登录的工单（12）统一成一份。

use std::fmt;

/// The identity of one secret, as the database refers to it: the secret
/// itself is only ever in the vault (保险库).
///
/// 主机自己的登录密码属于这台主机，按主机 id 存：改名、改地址都还在，「创建副本」不带它，
/// 多台主机共用一个密码请用凭据。A password credential's password, a key
/// credential's private key and its passphrase belong to the credential,
/// shared by the hosts that use it, by the credential's random `secret_id`
/// that is never reused. A proxy's password belongs to the proxy's address
/// and user, shared by the hosts behind it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum SecretRef {
    /// 一台保存的主机自己的登录密码，按主机 id（`HostId`）。
    Password { host: u64 },
    /// A password credential's password.
    Credential { secret_id: String },
    /// 密钥凭据的私钥正文（OpenSSH 或 PEM 文本）。没有 `keys/` 目录，私钥只在保险库里。
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

    /// The entry's name in the store. Its prefix says the kind. Saved
    /// secrets are found by it, so it never changes: the vault file keys
    /// each secret by it.
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

#[cfg(test)]
mod tests {
    use super::SecretRef;

    #[test]
    fn password_accounts_name_the_host() {
        let secret = SecretRef::password(7);
        assert_eq!(secret.account(), "password:host-7");
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
}
