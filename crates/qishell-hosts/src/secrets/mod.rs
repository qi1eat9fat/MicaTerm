// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/secrets/mod.rs
// 裁剪：去掉临时连接的 `TemporarySecretStore` 和交给 SSH 连接的 `KeychainSource`（接 SSH 登录时再加）；
// `SecretRef` 改为本 crate 自己的（见 secret_ref.rs）；加上保险库锁着时的 `SecretsLocked`。

//! 秘密的读写接口和不落地的几种实现。真正的实现是数据目录里的保险库（工单 10），
//! 它实现 [`SecretStore`]。
//!
//! 数据库里永远不出现密码、口令或私钥，只存 [`SecretRef`] 指向的条目名；`database.rs`
//! 的 `schema_never_contains_secret_columns` 守着这条不变量。

mod memory;
mod secret_ref;

use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use zeroize::Zeroizing;

pub use memory::{InMemorySecretStore, NoSecretStore, SecretsUnavailable};
pub use secret_ref::SecretRef;

/// 秘密的读写：生产里是保险库，测试里是 [`InMemorySecretStore`]。
///
/// 每个方法都可能阻塞（保险库要读写文件，「在本机记住」时还要碰系统钥匙串，
/// macOS 上可能弹出授权框），所以只能在后台执行器上调用，**永远不要在 UI 线程上调用**。
pub trait SecretStore: Send + Sync + 'static {
    /// 读一条秘密。没存过是 `Ok(None)`，不是错误。保险库锁着时返回
    /// [`SecretsLocked`]，调用方据此弹出解锁框。
    fn get(&self, secret: &SecretRef) -> Result<Option<Zeroizing<String>>>;
    fn set(&self, secret: &SecretRef, value: &str) -> Result<()>;
    /// 删一条秘密。条目本来就不存在也算成功。
    fn delete(&self, secret: &SecretRef) -> Result<()>;
    /// 这台机器上能不能存秘密。为 false 时 `set` 一定失败。
    fn is_available(&self) -> bool;
}

pub type SharedSecretStore = Arc<dyn SecretStore>;

/// 保险库锁着、读写不了秘密时的失败。作为 `anyhow` 链上的叶子错误返回，
/// 调用方用 `downcast_ref::<SecretsLocked>()` 认出它（[`crate::SecretError::of`] 就是这样做的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretsLocked;

/// For logs; the app words it for the user.
impl fmt::Display for SecretsLocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the vault is locked")
    }
}

impl std::error::Error for SecretsLocked {}

#[cfg(test)]
mod tests {
    use super::{InMemorySecretStore, NoSecretStore, SecretRef, SecretStore};

    #[test]
    fn the_memory_store_round_trips() {
        let store = InMemorySecretStore::default();
        let secret = SecretRef::password(1);
        assert!(store.get(&secret).unwrap().is_none());

        store.set(&secret, "hunter2").unwrap();
        assert_eq!(
            store.get(&secret).unwrap().as_deref().map(String::as_str),
            Some("hunter2")
        );

        store.set(&secret, "hunter3").unwrap();
        assert_eq!(
            store.get(&secret).unwrap().as_deref().map(String::as_str),
            Some("hunter3")
        );

        store.delete(&secret).unwrap();
        assert!(store.get(&secret).unwrap().is_none());
    }

    #[test]
    fn deleting_a_missing_secret_succeeds() {
        let store = InMemorySecretStore::default();
        let secret = SecretRef::passphrase("Jwg5rHvXCxw89paM");
        assert!(store.delete(&secret).is_ok());
    }

    #[test]
    fn the_memory_store_keys_by_account() {
        let store = InMemorySecretStore::default();
        let one = SecretRef::password(1);
        let other = SecretRef::password(2);
        store.set(&one, "hunter2").unwrap();
        assert!(store.get(&other).unwrap().is_none());
    }

    #[test]
    fn the_disabled_store_reads_empty_and_refuses_writes() {
        let store = NoSecretStore;
        let secret = SecretRef::password(1);
        assert!(!store.is_available());
        assert!(store.get(&secret).unwrap().is_none());
        assert!(store.set(&secret, "hunter2").is_err());
        assert!(store.delete(&secret).is_ok());
    }
}
