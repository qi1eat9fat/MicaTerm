// 参考 since2006/shell-rs @ b2a307a: src/secrets/keychain.rs 改写：钥匙串里不再放密码和口令，
// 只放保险库的数据密钥（`DataKeyStore`），账户名 `vault:<保险库 id>`；service 改成 `micaterm`。

use anyhow::{Result, anyhow};
use keyring::{Entry, Error};
use micaterm_hosts::{DataKey, DataKeyStore};

use crate::i18n::t;

/// 钥匙串条目的 service 名。发布后不能改：改了就找不到已经记住的数据密钥。
pub const SERVICE: &str = "micaterm";

/// 保险库 `vault_id` 的数据密钥在钥匙串里的账户名。发布后不能改。
pub fn account(vault_id: &str) -> String {
    format!("vault:{vault_id}")
}

/// 真正的系统钥匙串：Windows 凭据管理器、macOS 钥匙串、Linux 的 Secret Service。
/// 数据密钥存成 64 个十六进制字符，远低于 Windows 凭据管理器一条 2560 字节的上限。
///
/// macOS 上未签名的开发构建每次重新编译后都会被当成另一个程序，弹一次系统授权框；
/// 那个框只卡住发起调用的线程，所以调用方必须待在后台线程上。
pub struct KeychainDataKeyStore;

impl KeychainDataKeyStore {
    /// 这台机器上有没有能用的系统钥匙串。第一次调用会初始化后端。
    pub fn is_available() -> bool {
        Entry::store_status().is_ok()
    }

    fn entry(vault_id: &str) -> Result<Entry> {
        Entry::new(SERVICE, &account(vault_id)).map_err(|error| describe(Action::Open, error))
    }
}

impl DataKeyStore for KeychainDataKeyStore {
    fn get(&self, vault_id: &str) -> Result<Option<DataKey>> {
        match Self::entry(vault_id)?.get_password() {
            // 不是数据密钥的样子（被别的程序改过）：等于没有，保险库会当作没记住。
            Ok(text) => Ok(DataKey::from_hex(&zeroize::Zeroizing::new(text))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(describe(Action::Read, error)),
        }
    }

    fn set(&self, vault_id: &str, key: &DataKey) -> Result<()> {
        Self::entry(vault_id)?
            .set_password(&key.to_hex())
            .map_err(|error| describe(Action::Write, error))
    }

    fn delete(&self, vault_id: &str) -> Result<()> {
        match Self::entry(vault_id)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(describe(Action::Delete, error)),
        }
    }
}

/// 把钥匙串错误说成界面语言的话。`keyring` 的 `Display` 从不打印秘密本身，但仍然只在
/// 兜底分支里用它，免得将来上游改了实现把内容带出来。
fn describe(action: Action, error: Error) -> anyhow::Error {
    let reason = match error {
        Error::NoDefaultStore => t!("secrets.keychain.no_store"),
        Error::NoStorageAccess(_) => t!("secrets.keychain.no_access"),
        Error::TooLong(_, limit) => t!("secrets.keychain.too_long", limit = limit),
        other => other.to_string().into(),
    };
    anyhow!(match action {
        Action::Open => t!("secrets.keychain.open_failed", reason = reason),
        Action::Read => t!("secrets.keychain.read_failed", reason = reason),
        Action::Write => t!("secrets.keychain.write_failed", reason = reason),
        Action::Delete => t!("secrets.keychain.delete_failed", reason = reason),
    })
}

/// 对钥匙串条目做了什么，给报错用。
#[derive(Clone, Copy)]
enum Action {
    Open,
    Read,
    Write,
    Delete,
}

#[cfg(test)]
mod tests {
    use micaterm_hosts::{DataKey, DataKeyStore};

    use super::{KeychainDataKeyStore, account};

    #[test]
    fn the_account_names_the_vault() {
        assert_eq!(
            account("00112233445566778899aabbccddeeff"),
            "vault:00112233445566778899aabbccddeeff"
        );
    }

    /// 真钥匙串的往返测试。会在当前用户的钥匙串里建一条临时条目（测完删掉），还可能弹出
    /// 系统授权框，所以默认不跑：`cargo test --lib -- --ignored keychain`。
    #[test]
    #[ignore = "读写真实的系统钥匙串"]
    fn the_system_keychain_round_trips_a_data_key() {
        assert!(
            KeychainDataKeyStore::is_available(),
            "这台机器上没有可用的系统钥匙串"
        );
        let store = KeychainDataKeyStore;
        let vault_id = "micaterm-test-0000000000000000";
        store.delete(vault_id).unwrap();
        assert!(store.get(vault_id).unwrap().is_none());

        let key = DataKey::from_bytes([7; 32]);
        store.set(vault_id, &key).unwrap();
        assert_eq!(store.get(vault_id).unwrap(), Some(key));

        store.delete(vault_id).unwrap();
        assert!(store.get(vault_id).unwrap().is_none());
        // 本来就没有也算删成功。
        store.delete(vault_id).unwrap();
    }

    /// 真钥匙串 + 真文件的保险库：不设主密码时数据密钥进本机钥匙串，重开能读，重置后条目删掉。
    #[test]
    #[ignore = "读写真实的系统钥匙串"]
    fn a_vault_without_a_master_password_keeps_its_key_in_the_system_keychain() {
        use std::sync::Arc;

        use micaterm_hosts::secrets::SecretStore as _;
        use micaterm_hosts::{KdfParams, SecretRef, Vault, VaultStatus};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        let open = || {
            Vault::open(
                path.clone(),
                Arc::new(KeychainDataKeyStore),
                KdfParams::FAST_FOR_TESTS,
            )
            .unwrap()
        };
        let vault = open();
        vault.set_up(None, false).unwrap();
        let vault_id = vault.vault_id().unwrap();
        vault.set(&SecretRef::password(1), "hunter2").unwrap();

        let again = open();
        assert_eq!(again.status(), VaultStatus::Unprotected);
        assert_eq!(
            again
                .get(&SecretRef::password(1))
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("hunter2")
        );

        again.reset().unwrap();
        assert!(KeychainDataKeyStore.get(&vault_id).unwrap().is_none());
    }
}
