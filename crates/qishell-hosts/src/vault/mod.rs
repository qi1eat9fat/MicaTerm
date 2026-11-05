//! 保险库：数据目录里的 `vault.json`，存放密码、口令和私钥（ADR 0003、spec §4）。
//!
//! 随机的数据密钥加密每一条秘密（每条一个 nonce），所以设主密码、换主密码只是重新包裹这把
//! 数据密钥，已存的秘密不重新加密。数据密钥在哪儿：
//! - 没设主密码：只在本机钥匙串（[`DataKeyStore`]，以保险库 id 为键）。拷到别的电脑，钥匙串里
//!   没有，秘密视为丢失（[`VaultStatus::KeyLost`]）。
//! - 设了主密码：主密码经 Argon2id 包裹它写在文件里；选了「在本机记住」时钥匙串里也有一份，
//!   启动时自动解锁。
//!
//! [`Vault`] 实现 [`SecretStore`]。每个方法都会阻塞（读写文件、碰钥匙串、跑 Argon2），
//! **只能在后台线程上调用**。两个进程同时开同一个数据目录会互相覆盖 `vault.json`，v1 不处理。

mod crypto;
mod file;
mod hex;
mod keychain;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context as _, Result};
use zeroize::Zeroizing;

use crate::secrets::{SecretRef, SecretStore};

pub use crypto::{DataKey, KdfParams};
pub use file::VAULT_FORMAT;
pub use keychain::{DataKeyStore, InMemoryDataKeyStore, SharedDataKeyStore};

use file::{SealedFile, VaultFile};

/// 保险库现在处在哪种状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultStatus {
    /// 还没有 `vault.json`：第一次要保存秘密时由界面问要不要设主密码，再 [`Vault::set_up`]。
    Absent,
    /// 没设主密码，本机钥匙串里有数据密钥：用起来无感，不会锁定。
    Unprotected,
    /// 没设主密码，本机钥匙串里却没有数据密钥：保险库是在另一台电脑上建的，这里打不开。
    /// 读存过的秘密返回 [`VaultError::KeyLost`]；用户确认后 [`Vault::reset`]，不重置就现场询问密码。
    KeyLost,
}

/// 保险库操作的失败。作为 `anyhow` 链上的叶子错误返回，调用方用 `downcast_ref` 认出它。
/// 锁着时的失败是 [`crate::secrets::SecretsLocked`]，不在这里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// 还没建保险库（[`VaultStatus::Absent`]）：先问用户要不要设主密码，再 [`Vault::set_up`]。
    NotSetUp,
    /// 已经有保险库了，不能再 `set_up`。
    AlreadySetUp,
    /// 没设主密码的保险库在这台电脑上打不开（[`VaultStatus::KeyLost`]）。
    KeyLost,
    /// 文件坏了或被改过。
    Damaged(String),
    /// 文件是更新版本的 QiShell 写的（格式号是这个）。
    TooNew(u32),
    /// 系统取不到随机数。
    Random,
}

/// For logs; the app words it for the user.
impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSetUp => f.write_str("the vault is not set up"),
            Self::AlreadySetUp => f.write_str("the vault is already set up"),
            Self::KeyLost => f.write_str("the vault's key is not on this computer"),
            Self::Damaged(why) => write!(f, "the vault is damaged: {why}"),
            Self::TooNew(format) => write!(f, "the vault is in a newer format ({format})"),
            Self::Random => f.write_str("no random numbers"),
        }
    }
}

impl std::error::Error for VaultError {}

/// 数据目录里的保险库。见模块文档。
pub struct Vault {
    path: PathBuf,
    keychain: SharedDataKeyStore,
    params: KdfParams,
    state: Mutex<State>,
}

struct State {
    file: Option<VaultFile>,
    /// 手上的数据密钥；没有就是锁着或丢了。
    key: Option<DataKey>,
}

impl Vault {
    /// 读 `path` 的保险库（没有也行），并从钥匙串取数据密钥。`params` 是以后设、换主密码时用的
    /// Argon2id 开销（生产用 [`KdfParams::DEFAULT`]），从不取文件里写的（文件可能被改弱）。
    pub fn open(path: PathBuf, keychain: SharedDataKeyStore, params: KdfParams) -> Result<Self> {
        let file = file::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let key = match &file {
            None => None,
            Some(file) => keychain
                .get(&file.vault_id)
                .context("reading the vault's key from the keychain")?
                .filter(|key| key_checks(file, key)),
        };
        Ok(Self {
            path,
            keychain,
            params,
            state: Mutex::new(State { file, key }),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn status(&self) -> VaultStatus {
        let state = self.state();
        match (&state.file, &state.key) {
            (None, _) => VaultStatus::Absent,
            (Some(_), Some(_)) => VaultStatus::Unprotected,
            (Some(_), None) => VaultStatus::KeyLost,
        }
    }

    /// 这条秘密存过没有。不用解锁：条目名是明文。界面据此显示「已保存，保险库锁定中」，
    /// 连接前据此决定要不要先弹解锁框。
    pub fn contains(&self, secret: &SecretRef) -> bool {
        self.state()
            .file
            .as_ref()
            .is_some_and(|file| file.secrets.contains_key(&secret.account()))
    }

    /// 第一次保存秘密前建保险库。`master` 为 `None` 是「不设置」：数据密钥只进本机钥匙串。
    pub fn set_up(&self, master: Option<&str>, remember: bool) -> Result<()> {
        let _ = (master, remember);
        let mut state = self.state();
        if state.file.is_some() {
            return Err(VaultError::AlreadySetUp.into());
        }
        let vault_id = hex::encode(&crypto::random_array::<16>()?);
        let key = DataKey::random()?;
        let id_bytes = hex::decode_array::<16>(&vault_id).expect("hex");
        let check = crypto::seal(key.as_bytes(), &[], &check_aad(&id_bytes))?;
        let file = VaultFile {
            format: file::VAULT_FORMAT,
            vault_id: vault_id.clone(),
            kdf: None,
            wrapped_key: None,
            key_check: SealedFile::of(&check),
            secrets: Default::default(),
            rest: Default::default(),
        };
        self.keychain
            .set(&vault_id, &key)
            .context("keeping the vault's key in the keychain")?;
        if let Err(error) = file::write(&self.path, &file) {
            let _ = self.keychain.delete(&vault_id);
            return Err(error).context("writing the vault");
        }
        state.file = Some(file);
        state.key = Some(key);
        Ok(())
    }

    /// 改 `state.file` 的一份副本、写盘，写成了才换进内存：写失败时保险库原样不变。
    fn commit(&self, state: &mut State, change: impl FnOnce(&mut VaultFile)) -> Result<()> {
        let mut file = state.file.clone().ok_or(VaultError::NotSetUp)?;
        change(&mut file);
        file::write(&self.path, &file).context("writing the vault")?;
        state.file = Some(file);
        Ok(())
    }
}

/// 钥匙串交来的数据密钥是不是这个保险库的。
fn key_checks(file: &VaultFile, key: &DataKey) -> bool {
    let (Ok(vault_id), Ok(check)) = (file.vault_id_bytes(), file.key_check.sealed()) else {
        return false;
    };
    crypto::open(key.as_bytes(), &check, &check_aad(&vault_id)).is_some()
}

fn check_aad(vault_id: &[u8; 16]) -> Vec<u8> {
    [b"qishell-vault check".as_slice(), vault_id].concat()
}

/// 把一条秘密的密文绑在它的保险库和条目名上：挪到别的条目、别的保险库都打不开。
fn secret_aad(vault_id: &[u8; 16], account: &str) -> Vec<u8> {
    [b"qishell-vault secret".as_slice(), vault_id, account.as_bytes()].concat()
}

impl SecretStore for Vault {
    fn get(&self, secret: &SecretRef) -> Result<Option<Zeroizing<String>>> {
        let state = self.state();
        let Some(file) = &state.file else {
            return Ok(None);
        };
        let account = secret.account();
        let Some(entry) = file.secrets.get(&account) else {
            return Ok(None);
        };
        let key = state.key.as_ref().ok_or(VaultError::KeyLost)?;
        let vault_id = file.vault_id_bytes()?;
        let plain = crypto::open(key.as_bytes(), &entry.sealed()?, &secret_aad(&vault_id, &account))
            .ok_or_else(|| VaultError::Damaged(format!("{account} does not open")))?;
        let text = String::from_utf8(plain.to_vec())
            .map_err(|_| VaultError::Damaged(format!("{account} is not text")))?;
        Ok(Some(Zeroizing::new(text)))
    }

    fn set(&self, secret: &SecretRef, value: &str) -> Result<()> {
        let mut state = self.state();
        let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
        let key = state.key.as_ref().ok_or(VaultError::KeyLost)?;
        let account = secret.account();
        let sealed = crypto::seal(
            key.as_bytes(),
            value.as_bytes(),
            &secret_aad(&file.vault_id_bytes()?, &account),
        )?;
        self.commit(&mut state, |file| {
            file.secrets.insert(account, SealedFile::of(&sealed));
        })
    }

    fn delete(&self, secret: &SecretRef) -> Result<()> {
        let mut state = self.state();
        let account = secret.account();
        let held = state
            .file
            .as_ref()
            .is_some_and(|file| file.secrets.contains_key(&account));
        if !held {
            return Ok(());
        }
        self.commit(&mut state, |file| {
            file.secrets.remove(&account);
        })
    }

    fn is_available(&self) -> bool {
        true
    }
}
