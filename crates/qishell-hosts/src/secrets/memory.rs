// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/secrets/memory.rs
// 裁剪：去掉给 SSH 连接读的 `SecretSource` 实现（接 SSH 登录时再加）；「钥匙串」的说法改成保险库。

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use anyhow::Result;
use zeroize::Zeroizing;

use super::{SecretRef, SecretStore};

/// 进程内的秘密存储，给测试用：行为和保险库解锁时一致，但什么都不落地。
#[derive(Default)]
pub struct InMemorySecretStore {
    entries: Mutex<HashMap<String, Zeroizing<String>>>,
}

impl InMemorySecretStore {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Zeroizing<String>>> {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// 测试断言用：当前存了几条。
    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for InMemorySecretStore {
    fn get(&self, secret: &SecretRef) -> Result<Option<Zeroizing<String>>> {
        Ok(self.entries().get(&secret.account()).cloned())
    }

    fn set(&self, secret: &SecretRef, value: &str) -> Result<()> {
        self.entries()
            .insert(secret.account(), Zeroizing::new(value.to_string()));
        Ok(())
    }

    fn delete(&self, secret: &SecretRef) -> Result<()> {
        self.entries().remove(&secret.account());
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// 没有地方存秘密时写秘密的失败。
#[derive(Debug)]
pub struct SecretsUnavailable;

/// For logs; the app words it for the user.
impl fmt::Display for SecretsUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("there is nowhere to keep secrets")
    }
}

impl std::error::Error for SecretsUnavailable {}

/// 什么都不存：读永远是空，写明确报错。是 `HostData` 的默认值，所以单元测试
/// 不会碰到任何秘密存储。
pub struct NoSecretStore;

impl SecretStore for NoSecretStore {
    fn get(&self, _: &SecretRef) -> Result<Option<Zeroizing<String>>> {
        Ok(None)
    }

    fn set(&self, _: &SecretRef, _: &str) -> Result<()> {
        Err(SecretsUnavailable.into())
    }

    fn delete(&self, _: &SecretRef) -> Result<()> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        false
    }
}
