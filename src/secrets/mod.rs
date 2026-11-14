//! 秘密：数据目录里的保险库（核心在 `micaterm_hosts::vault`）和应用自己的系统钥匙串实现。
//! 钥匙串里只放保险库的数据密钥，密码、口令、私钥都在 `vault.json` 里（ADR 0003）。

mod keychain;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use micaterm_hosts::{KdfParams, Vault};

pub use keychain::{KeychainDataKeyStore, SERVICE, account};

/// 打开数据目录里的保险库（没有 `vault.json` 也行，状态是 `Absent`），数据密钥从系统钥匙串取。
///
/// 会阻塞（读文件、读钥匙串，macOS 上可能弹授权框）：只在后台线程上调用。
pub fn open_vault() -> Result<Arc<Vault>> {
    let path = crate::app::vault_path().context("finding the data folder")?;
    let vault = Vault::open(path, Arc::new(KeychainDataKeyStore), KdfParams::DEFAULT)?;
    Ok(Arc::new(vault))
}
