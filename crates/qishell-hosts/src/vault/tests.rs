// 保险库的行为测试：只经公开接口（`Vault` 的方法和它实现的 `SecretStore`）和磁盘上的 `vault.json`。
// 钥匙串用 `InMemoryDataKeyStore`；换一个新的空实例就是「拷到了另一台电脑」。

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use super::{InMemoryDataKeyStore, KdfParams, Vault, VaultError, VaultStatus};
use crate::secrets::{SecretRef, SecretStore};

struct Computer {
    keychain: Arc<InMemoryDataKeyStore>,
}

impl Computer {
    fn new() -> Self {
        Self {
            keychain: Arc::new(InMemoryDataKeyStore::default()),
        }
    }

    fn open(&self, path: &PathBuf) -> Vault {
        Vault::open(path.clone(), self.keychain.clone(), KdfParams::FAST_FOR_TESTS).unwrap()
    }
}

fn data_dir() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    (dir, path)
}

fn read(vault: &Vault, secret: &SecretRef) -> Option<String> {
    vault
        .get(secret)
        .unwrap()
        .map(|value| value.as_str().to_string())
}

#[test]
fn without_a_master_password_secrets_open_on_this_computer() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    assert_eq!(vault.status(), VaultStatus::Absent);

    vault.set_up(None, false).unwrap();
    assert_eq!(vault.status(), VaultStatus::Unprotected);
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    assert_eq!(read(&vault, &SecretRef::password(1)).as_deref(), Some("hunter2"));

    // 下次启动：钥匙串里有数据密钥，不用解锁。
    let again = computer.open(&path);
    assert_eq!(again.status(), VaultStatus::Unprotected);
    assert_eq!(read(&again, &SecretRef::password(1)).as_deref(), Some("hunter2"));
    assert_eq!(read(&again, &SecretRef::password(2)), None);

    // 秘密进了 vault.json，但不是明文。
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("password:host-1"));
    assert!(!written.contains("hunter2"));
}

fn error_of(result: anyhow::Result<impl Sized>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => error,
    }
}

fn vault_error(result: anyhow::Result<impl Sized>) -> VaultError {
    error_of(result)
        .downcast_ref::<VaultError>()
        .cloned()
        .expect("a vault error")
}

/// 没设主密码的保险库拷到另一台电脑：那里的钥匙串没有数据密钥，存过的秘密打不开，
/// 但能认出来（界面据此提示「在另一台电脑上创建、没设主密码」），没存过的照常是「没有」。
#[test]
fn without_its_key_in_the_keychain_an_unprotected_vault_is_lost() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();

    let elsewhere = Computer::new().open(&path);
    assert_eq!(elsewhere.status(), VaultStatus::KeyLost);
    assert!(elsewhere.contains(&SecretRef::password(1)));
    assert_eq!(
        vault_error(elsewhere.get(&SecretRef::password(1))),
        VaultError::KeyLost
    );
    assert_eq!(read(&elsewhere, &SecretRef::password(2)), None);
    assert_eq!(
        vault_error(elsewhere.set(&SecretRef::password(2), "x")),
        VaultError::KeyLost
    );
}