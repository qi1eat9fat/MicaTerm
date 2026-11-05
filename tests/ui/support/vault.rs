//! 保险库界面（工单 13）的夹具：一个真的保险库（`qishell_hosts::Vault`），放在临时目录里，钥匙串是
//! 进程内的（不碰系统钥匙串），Argon2 用最小开销。能直接造出未设、没设主密码、已锁定、已解锁、
//! 打不开（另一台电脑上建的）几种状态。

use std::sync::Arc;

use qishell::workspace::WorkspaceServices;
use qishell_hosts::SecretStore as _;
pub use qishell_hosts::vault::InMemoryDataKeyStore;
pub use qishell_hosts::{KdfParams, SecretRef, Vault, VaultStatus};
use tempfile::TempDir;

use super::fake_services;

/// 测试用的主密码。
pub const MASTER: &str = "correct horse";

/// 临时目录里的保险库和这台「电脑」的钥匙串。测试拿着它（目录随它删掉）。
pub struct TestVault {
    pub vault: Arc<Vault>,
    pub keychain: Arc<InMemoryDataKeyStore>,
    dir: TempDir,
}

impl TestVault {
    fn open_in(dir: TempDir, keychain: Arc<InMemoryDataKeyStore>) -> Self {
        let vault = Vault::open(
            dir.path().join("vault.json"),
            keychain.clone(),
            KdfParams::FAST_FOR_TESTS,
        )
        .expect("the vault opens");
        Self {
            vault: Arc::new(vault),
            keychain,
            dir,
        }
    }

    /// 还没有 `vault.json`：第一次保存秘密时问要不要设主密码。
    pub fn absent() -> Self {
        Self::open_in(
            tempfile::tempdir().expect("a temporary folder"),
            Arc::default(),
        )
    }

    /// 没设主密码，数据密钥在本机钥匙串里。
    pub fn unprotected() -> Self {
        let vault = Self::absent();
        vault.vault.set_up(None, false).unwrap();
        vault
    }

    /// 设了主密码 [`MASTER`]、已解锁、没记在本机。
    pub fn unlocked() -> Self {
        let vault = Self::absent();
        vault.vault.set_up(Some(MASTER), false).unwrap();
        vault
    }

    /// 存一条秘密（保险库得能写）。
    pub fn with_secret(self, secret: &SecretRef, value: &str) -> Self {
        self.vault.set(secret, value).unwrap();
        self
    }

    /// 锁上（设了主密码的）。
    pub fn locked(self) -> Self {
        self.vault.lock().unwrap();
        self
    }

    /// 拷到另一台电脑：同一个 `vault.json`，钥匙串是空的。没设主密码的保险库在那里打不开。
    pub fn on_another_computer(self) -> Self {
        let Self { dir, .. } = self;
        Self::open_in(dir, Arc::default())
    }

    /// 再从磁盘读一次（同一台电脑）：看写进去的东西。
    pub fn reopened(&self) -> Vault {
        Vault::open(
            self.dir.path().join("vault.json"),
            self.keychain.clone(),
            KdfParams::FAST_FOR_TESTS,
        )
        .expect("the vault opens")
    }

    /// 保险库里这条秘密的明文；锁着、没存都是 `None`。
    pub fn read(&self, secret: &SecretRef) -> Option<String> {
        self.vault
            .get(secret)
            .ok()
            .flatten()
            .map(|value| value.as_str().to_string())
    }

    pub fn status(&self) -> VaultStatus {
        self.vault.status()
    }

    /// 注入窗口壳的服务：主机数据和连接都读写这个保险库。
    pub fn services(&self) -> WorkspaceServices {
        WorkspaceServices {
            secrets: self.vault.clone(),
            vault: Some(self.vault.clone()),
            ..fake_services()
        }
    }
}
