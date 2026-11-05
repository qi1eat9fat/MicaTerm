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
//!
//! 写入都是「改一份副本 → 先写临时文件再改名 → 成功才换进内存」：写失败时保险库原样不变。

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

use crate::secrets::{SecretRef, SecretStore, SecretsLocked};

pub use crypto::{DataKey, KdfParams};
pub use file::VAULT_FORMAT;
pub use keychain::{DataKeyStore, InMemoryDataKeyStore, SharedDataKeyStore};

use file::{KdfFile, SealedFile, VaultFile};

/// 一条秘密最长多少字节：最多是一个私钥文件。
pub const MAX_SECRET_BYTES: usize = 64 * 1024;

/// 保险库现在处在哪种状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultStatus {
    /// 还没有 `vault.json`：第一次要保存秘密时由界面问要不要设主密码，再 [`Vault::set_up`]。
    Absent,
    /// 没设主密码，本机钥匙串里有数据密钥：用起来无感，不会锁定，也没有锁定入口。
    Unprotected,
    /// 没设主密码，本机钥匙串里却没有数据密钥：保险库是在另一台电脑上建的，这里打不开。
    /// 读存过的秘密返回 [`VaultError::KeyLost`]；用户确认后 [`Vault::reset`]，不重置就现场询问密码。
    KeyLost,
    /// 设了主密码，还没解锁：读写存过的秘密返回 [`SecretsLocked`]，调用方弹解锁框，再 [`Vault::unlock`]。
    Locked,
    /// 设了主密码，已解锁。`remembered`：本机钥匙串里有数据密钥（「在本机记住」），下次启动自动解锁。
    Unlocked { remembered: bool },
}

impl VaultStatus {
    pub fn has_master_password(self) -> bool {
        matches!(self, Self::Locked | Self::Unlocked { .. })
    }
}

/// 保险库操作的失败。作为 `anyhow` 链上的叶子错误返回，调用方用 `downcast_ref` 认出它。
/// 锁着时的失败是 [`SecretsLocked`]，不在这里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// 还没建保险库（[`VaultStatus::Absent`]）：先问用户要不要设主密码，再 [`Vault::set_up`]。
    NotSetUp,
    /// 已经有保险库了，不能再 `set_up`。
    AlreadySetUp,
    /// 没设主密码的保险库在这台电脑上打不开（[`VaultStatus::KeyLost`]）。
    KeyLost,
    /// 主密码不对。
    WrongPassword,
    /// 这个保险库没设主密码：不能解锁、锁定或更换主密码（先 [`Vault::set_master_password`]）。
    NoMasterPassword,
    /// 这个保险库已经设了主密码：要换请用 [`Vault::change_master_password`]。
    HasMasterPassword,
    /// 秘密超过 [`MAX_SECRET_BYTES`]。
    TooLong,
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
            Self::WrongPassword => f.write_str("wrong master password"),
            Self::NoMasterPassword => f.write_str("the vault has no master password"),
            Self::HasMasterPassword => f.write_str("the vault already has a master password"),
            Self::TooLong => f.write_str("the secret is too long"),
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
    /// 本机钥匙串里有没有这个保险库的数据密钥。
    remembered: bool,
}

/// 用主密码包裹好的数据密钥，和写进文件的派生参数。
struct Wrapping {
    kdf: KdfFile,
    kek: Zeroizing<[u8; 32]>,
}

impl Vault {
    /// 读 `path` 的保险库（没有也行），并从钥匙串取数据密钥。`params` 是以后设、换主密码时用的
    /// Argon2id 开销（生产用 [`KdfParams::DEFAULT`]），从不取文件里写的（文件可能被改弱）。
    ///
    /// 没设主密码时钥匙串读不出来是错误（不然会被当成「丢了」）；设了主密码时读不出来只是
    /// 不能自动解锁，照样打开，状态是锁着。钥匙串交来的密钥和文件对不上时当作没有。
    pub fn open(path: PathBuf, keychain: SharedDataKeyStore, params: KdfParams) -> Result<Self> {
        let file = file::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let key = match &file {
            None => None,
            Some(file) => {
                let kept = keychain.get(&file.vault_id);
                let kept = match file.kdf {
                    None => kept.context("reading the vault's key from the keychain")?,
                    Some(_) => kept.ok().flatten(),
                };
                kept.filter(|key| key_checks(file, key))
            }
        };
        let remembered = key.is_some();
        Ok(Self {
            path,
            keychain,
            params,
            state: Mutex::new(State {
                file,
                key,
                remembered,
            }),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn status(&self) -> VaultStatus {
        let state = self.state();
        let Some(file) = &state.file else {
            return VaultStatus::Absent;
        };
        match (file.kdf.is_some(), state.key.is_some()) {
            (false, true) => VaultStatus::Unprotected,
            (false, false) => VaultStatus::KeyLost,
            (true, false) => VaultStatus::Locked,
            (true, true) => VaultStatus::Unlocked {
                remembered: state.remembered,
            },
        }
    }

    /// 保险库 id（钥匙串里的键）；还没建保险库时为 `None`。重置后换新。
    pub fn vault_id(&self) -> Option<String> {
        self.state().file.as_ref().map(|file| file.vault_id.clone())
    }

    /// 这条秘密存过没有。不用解锁：条目名是明文。界面据此显示「已保存，保险库锁定中」，
    /// 连接前据此决定要不要先弹解锁框。
    pub fn contains(&self, secret: &SecretRef) -> bool {
        self.state()
            .file
            .as_ref()
            .is_some_and(|file| file.secrets.contains_key(&secret.account()))
    }

    /// 第一次保存秘密前建保险库。`master` 为 `None` 是「不设置」：数据密钥只进本机钥匙串
    /// （`remember` 不起作用）；有主密码时 `remember` 是「在本机记住」。
    pub fn set_up(&self, master: Option<&str>, remember: bool) -> Result<()> {
        if self.state().file.is_some() {
            return Err(VaultError::AlreadySetUp.into());
        }
        let wrapping = master.map(|password| self.wrapping(password)).transpose()?;
        let id_bytes = crypto::random_array::<16>()?;
        let vault_id = hex::encode(&id_bytes);
        let key = DataKey::random()?;
        let check = crypto::seal(key.as_bytes(), &[], &check_aad(&id_bytes))?;
        let mut file = VaultFile {
            format: VAULT_FORMAT,
            vault_id: vault_id.clone(),
            kdf: None,
            wrapped_key: None,
            key_check: SealedFile::of(&check),
            secrets: Default::default(),
            rest: Default::default(),
        };
        if let Some(wrapping) = wrapping {
            wrap(&mut file, &id_bytes, &key, wrapping)?;
        }
        let protected = file.kdf.is_some();
        // 没有主密码时钥匙串是唯一的一份：先存进钥匙串，存成了才写文件。
        let remember = remember || !protected;
        if !protected {
            self.keychain
                .set(&vault_id, &key)
                .context("keeping the vault's key in the keychain")?;
        }

        let mut state = self.state();
        if state.file.is_some() {
            return Err(VaultError::AlreadySetUp.into());
        }
        if let Err(error) = file::write(&self.path, &file) {
            if !protected {
                let _ = self.keychain.delete(&vault_id);
            }
            return Err(error).context("writing the vault");
        }
        state.file = Some(file);
        state.key = Some(key.clone());
        state.remembered = !protected;
        drop(state);
        if protected && remember {
            self.remember(&vault_id, &key, true)?;
        }
        Ok(())
    }

    /// 用主密码解锁。`remember` 是解锁卡片上的「在本机记住」：勾上就把数据密钥存进本机钥匙串，
    /// 不勾就删掉以前记住的那份。主密码不对时是 [`VaultError::WrongPassword`]。
    ///
    /// 钥匙串改不了时保险库照样已经解锁，返回的错误只说没记住（或没忘掉）。
    pub fn unlock(&self, password: &str, remember: bool) -> Result<()> {
        let (vault_id, key) = self.open_with(password)?;
        {
            let mut state = self.state();
            let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
            // 派生期间文件被重置或换了主密码：那这把钥匙就不是现在的了。
            if file.vault_id != vault_id || !key_checks(file, &key) {
                return Err(VaultError::WrongPassword.into());
            }
            state.key = Some(key.clone());
        }
        self.remember(&vault_id, &key, remember)
    }

    /// 手动锁定：丢掉内存里的数据密钥。钥匙串不动（记住了的，下次启动照样自动解锁）。
    pub fn lock(&self) -> Result<()> {
        let mut state = self.state();
        let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
        if file.kdf.is_none() {
            return Err(VaultError::NoMasterPassword.into());
        }
        state.key = None;
        Ok(())
    }

    /// 给没设主密码的保险库补设主密码：只给同一把数据密钥加一层包裹，已存的秘密不重新加密。
    /// `remember` 为 false 时把钥匙串里的那份删掉（之后只有主密码能打开）。
    pub fn set_master_password(&self, password: &str, remember: bool) -> Result<()> {
        let wrapping = self.wrapping(password)?;
        let mut state = self.state();
        let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
        if file.kdf.is_some() {
            return Err(VaultError::HasMasterPassword.into());
        }
        let key = state.key.clone().ok_or(VaultError::KeyLost)?;
        let id_bytes = file.vault_id_bytes()?;
        let vault_id = file.vault_id.clone();
        self.commit(&mut state, |file| wrap(file, &id_bytes, &key, wrapping))?;
        drop(state);
        self.remember(&vault_id, &key, remember)
    }

    /// 更换主密码：先核对现在的主密码，再用新的重新包裹同一把数据密钥，已存的秘密不重新加密。
    /// 锁着也能换（核对通过后就是解锁的）。「在本机记住」不变：钥匙串里存的是数据密钥，它没变。
    pub fn change_master_password(&self, current: &str, new: &str) -> Result<()> {
        let (vault_id, key) = self.open_with(current)?;
        let wrapping = self.wrapping(new)?;
        let mut state = self.state();
        let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
        if file.vault_id != vault_id || !key_checks(file, &key) {
            return Err(VaultError::WrongPassword.into());
        }
        let id_bytes = file.vault_id_bytes()?;
        self.commit(&mut state, |file| wrap(file, &id_bytes, &key, wrapping))?;
        state.key = Some(key);
        Ok(())
    }

    /// 重置保险库：清空所有秘密，删掉 `vault.json` 和本机钥匙串里的数据密钥，回到
    /// [`VaultStatus::Absent`]。主机和凭据不在这里，不受影响。
    ///
    /// 钥匙串删不掉时保险库照样已经重置，返回的错误只说旧的数据密钥还留在钥匙串里。
    pub fn reset(&self) -> Result<()> {
        let mut state = self.state();
        let Some(file) = &state.file else {
            return Ok(());
        };
        let vault_id = file.vault_id.clone();
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error).context("removing the vault");
            }
            _ => {}
        }
        state.file = None;
        state.key = None;
        state.remembered = false;
        drop(state);
        self.keychain
            .delete(&vault_id)
            .context("removing the old vault's key from the keychain")
    }

    /// 用 `password` 打开数据密钥（不改状态）。Argon2 在锁外跑，不挡住别的读写。
    fn open_with(&self, password: &str) -> Result<(String, DataKey)> {
        let (vault_id, id_bytes, kdf, wrapped) = {
            let state = self.state();
            let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
            let kdf = file.kdf.clone().ok_or(VaultError::NoMasterPassword)?;
            let wrapped = file
                .wrapped_key
                .clone()
                .ok_or_else(|| VaultError::Damaged("no wrapped key".into()))?;
            (file.vault_id.clone(), file.vault_id_bytes()?, kdf, wrapped)
        };
        let (params, salt) = kdf.params()?;
        let kek = crypto::derive(password, &salt, params)?;
        let plain = crypto::open(&kek, &wrapped.sealed()?, &key_aad(&id_bytes))
            .ok_or(VaultError::WrongPassword)?;
        let bytes: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::Damaged("bad key length".into()))?;
        Ok((vault_id, DataKey::from_bytes(bytes)))
    }

    /// 新主密码的派生：新的随机 salt，开销用构造时给的。
    fn wrapping(&self, password: &str) -> Result<Wrapping> {
        let salt = crypto::random_array::<16>()?;
        let kek = crypto::derive(password, &salt, self.params)?;
        Ok(Wrapping {
            kdf: KdfFile::new(self.params, &salt),
            kek,
        })
    }

    /// 让钥匙串里有（`remember`）或没有这把数据密钥，并记下结果。
    fn remember(&self, vault_id: &str, key: &DataKey, remember: bool) -> Result<()> {
        let result = if remember {
            self.keychain
                .set(vault_id, key)
                .context("keeping the vault's key in the keychain")
        } else {
            self.keychain
                .delete(vault_id)
                .context("removing the vault's key from the keychain")
        };
        if result.is_ok() {
            self.state().remembered = remember;
        }
        result
    }

    /// 改 `state.file` 的一份副本、写盘，写成了才换进内存：写失败时保险库原样不变。
    fn commit(
        &self,
        state: &mut State,
        change: impl FnOnce(&mut VaultFile) -> Result<(), VaultError>,
    ) -> Result<()> {
        let mut file = state.file.clone().ok_or(VaultError::NotSetUp)?;
        change(&mut file)?;
        file::write(&self.path, &file).context("writing the vault")?;
        state.file = Some(file);
        Ok(())
    }
}

/// 用 `wrapping` 的包裹密钥包好 `key`，连同派生参数写进 `file`。
fn wrap(
    file: &mut VaultFile,
    vault_id: &[u8; 16],
    key: &DataKey,
    wrapping: Wrapping,
) -> Result<(), VaultError> {
    let sealed = crypto::seal(&wrapping.kek, key.as_bytes(), &key_aad(vault_id))?;
    file.kdf = Some(wrapping.kdf);
    file.wrapped_key = Some(SealedFile::of(&sealed));
    Ok(())
}

/// 钥匙串交来的（或主密码解出的）数据密钥是不是这个保险库的。
fn key_checks(file: &VaultFile, key: &DataKey) -> bool {
    let (Ok(vault_id), Ok(check)) = (file.vault_id_bytes(), file.key_check.sealed()) else {
        return false;
    };
    crypto::open(key.as_bytes(), &check, &check_aad(&vault_id)).is_some()
}

/// 包裹的数据密钥绑在它的保险库上。
fn key_aad(vault_id: &[u8; 16]) -> Vec<u8> {
    [b"qishell-vault key".as_slice(), vault_id].concat()
}

fn check_aad(vault_id: &[u8; 16]) -> Vec<u8> {
    [b"qishell-vault check".as_slice(), vault_id].concat()
}

/// 把一条秘密的密文绑在它的保险库和条目名上：挪到别的条目、别的保险库都打不开。
fn secret_aad(vault_id: &[u8; 16], account: &str) -> Vec<u8> {
    [
        b"qishell-vault secret".as_slice(),
        vault_id,
        account.as_bytes(),
    ]
    .concat()
}

/// 手上没有数据密钥时为什么：锁着，还是丢了。
fn missing_key(file: &VaultFile) -> anyhow::Error {
    match file.kdf {
        Some(_) => SecretsLocked.into(),
        None => VaultError::KeyLost.into(),
    }
}

/// 读写秘密。没存过的秘密不用解锁就答「没有」；存过的，锁着时返回 [`SecretsLocked`]，
/// 打不开（[`VaultStatus::KeyLost`]）时返回 [`VaultError::KeyLost`]。还没建保险库时写入返回
/// [`VaultError::NotSetUp`]。删除不需要数据密钥，锁着也能删。
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
        let key = state.key.as_ref().ok_or_else(|| missing_key(file))?;
        let vault_id = file.vault_id_bytes()?;
        let plain = crypto::open(
            key.as_bytes(),
            &entry.sealed()?,
            &secret_aad(&vault_id, &account),
        )
        .ok_or_else(|| VaultError::Damaged(format!("{account} does not open")))?;
        let text = String::from_utf8(plain.to_vec())
            .map_err(|_| VaultError::Damaged(format!("{account} is not text")))?;
        Ok(Some(Zeroizing::new(text)))
    }

    fn set(&self, secret: &SecretRef, value: &str) -> Result<()> {
        if value.len() > MAX_SECRET_BYTES {
            return Err(VaultError::TooLong.into());
        }
        let mut state = self.state();
        let file = state.file.as_ref().ok_or(VaultError::NotSetUp)?;
        let key = state.key.as_ref().ok_or_else(|| missing_key(file))?;
        let account = secret.account();
        let sealed = crypto::seal(
            key.as_bytes(),
            value.as_bytes(),
            &secret_aad(&file.vault_id_bytes()?, &account),
        )?;
        self.commit(&mut state, |file| {
            file.secrets.insert(account, SealedFile::of(&sealed));
            Ok(())
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
            Ok(())
        })
    }

    fn is_available(&self) -> bool {
        true
    }
}
