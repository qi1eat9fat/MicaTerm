//! 系统钥匙串里的数据密钥：以保险库 id 为键。真正的实现在应用里（`keyring`），
//! 本 crate 只定接口，测试用 [`InMemoryDataKeyStore`]，不碰真实钥匙串。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;

use super::DataKey;

/// 存数据密钥的地方：系统钥匙串。
///
/// 没设主密码的保险库，数据密钥只在这里；设了主密码、又选了「在本机记住」时，
/// 这里也有一份，用来自动解锁。存的永远是数据密钥，不是主密码。
///
/// 每个方法都可能阻塞（macOS 上可能弹出授权框），只能在后台线程上调用。
pub trait DataKeyStore: Send + Sync + 'static {
    /// 保险库 `vault_id` 的数据密钥；没有时是 `Ok(None)`，不是错误。
    fn get(&self, vault_id: &str) -> Result<Option<DataKey>>;
    fn set(&self, vault_id: &str, key: &DataKey) -> Result<()>;
    /// 本来就没有也算成功。
    fn delete(&self, vault_id: &str) -> Result<()>;
}

pub type SharedDataKeyStore = Arc<dyn DataKeyStore>;

/// 进程内的钥匙串，给测试用。换一个新的空实例，就是「拷到了另一台电脑」。
#[derive(Default)]
pub struct InMemoryDataKeyStore {
    keys: Mutex<HashMap<String, DataKey>>,
}

impl InMemoryDataKeyStore {
    fn keys(&self) -> MutexGuard<'_, HashMap<String, DataKey>> {
        self.keys.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// 测试断言用：这台「电脑」记着哪几个保险库的数据密钥。
    pub fn vault_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.keys().keys().cloned().collect();
        ids.sort();
        ids
    }
}

impl DataKeyStore for InMemoryDataKeyStore {
    fn get(&self, vault_id: &str) -> Result<Option<DataKey>> {
        Ok(self.keys().get(vault_id).cloned())
    }

    fn set(&self, vault_id: &str, key: &DataKey) -> Result<()> {
        self.keys().insert(vault_id.to_string(), key.clone());
        Ok(())
    }

    fn delete(&self, vault_id: &str) -> Result<()> {
        self.keys().remove(vault_id);
        Ok(())
    }
}
