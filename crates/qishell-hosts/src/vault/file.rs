// 参考 since2006/shell-rs @ b2a307a: crates/shellrs-sync/src/vault.rs 的 `VaultFile`、`KdfFile`、
// `WrappedKey`、`read`（MIT）改写：只有一把数据密钥（没有代数）；没设主密码时 `kdf`、
// `wrapped_key` 为 null；秘密也放在这个文件里，每条各自加密。

//! `vault.json` 的样子和读写。
//!
//! ```json
//! {
//!   "format": 1,
//!   "vault_id": "<16 字节 hex>",
//!   "kdf": null | { "algorithm": "argon2id", "version": 19, "memory_kib", "passes", "parallelism", "salt" },
//!   "wrapped_key": null | { "nonce", "sealed" },
//!   "key_check": { "nonce", "sealed" },
//!   "secrets": { "<SecretRef::account()>": { "nonce", "sealed" } }
//! }
//! ```
//!
//! 条目名是明文（主机、用户名本来就明文存在同一目录的数据库里），所以锁着也能回答「这条存过没有」。
//! 这个版本不认识的字段放在 `rest` 里原样写回，新版本加的东西不会被旧版本改密码时丢掉。

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::crypto::{KdfParams, Sealed};
use super::{VaultError, hex};

/// 这个版本读写的格式。发布后只升不改：以后改格式要升它，读旧格式要加迁移。
pub const VAULT_FORMAT: u32 = 1;

type Map = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct VaultFile {
    pub format: u32,
    pub vault_id: String,
    pub kdf: Option<KdfFile>,
    pub wrapped_key: Option<SealedFile>,
    /// 用数据密钥加密的空内容：从钥匙串取回的数据密钥对不对，靠它核对。
    pub key_check: SealedFile,
    pub secrets: BTreeMap<String, SealedFile>,
    #[serde(flatten)]
    pub rest: Map,
}

/// 换主密码时整个换掉，未知字段也不留（它们描述的是旧 salt）。
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct KdfFile {
    pub algorithm: String,
    pub version: u32,
    pub memory_kib: u32,
    pub passes: u32,
    pub parallelism: u32,
    pub salt: String,
    #[serde(flatten)]
    pub rest: Map,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SealedFile {
    pub nonce: String,
    pub sealed: String,
}

impl SealedFile {
    pub fn of(sealed: &Sealed) -> Self {
        Self {
            nonce: hex::encode(&sealed.nonce),
            sealed: hex::encode(&sealed.sealed),
        }
    }

    pub fn sealed(&self) -> Result<Sealed, VaultError> {
        let nonce = hex::decode_array::<24>(&self.nonce).ok_or(bad("bad nonce"))?;
        let sealed = hex::decode(&self.sealed).ok_or(bad("bad ciphertext"))?;
        Ok(Sealed { nonce, sealed })
    }
}

impl KdfFile {
    pub fn new(params: KdfParams, salt: &[u8; 16]) -> Self {
        Self {
            algorithm: "argon2id".into(),
            version: 0x13,
            memory_kib: params.memory_kib,
            passes: params.passes,
            parallelism: params.parallelism,
            salt: hex::encode(salt),
            rest: Map::new(),
        }
    }

    /// 文件里写的开销和 salt；不认识的算法、越界的开销在跑之前就拒绝。
    pub fn params(&self) -> Result<(KdfParams, [u8; 16]), VaultError> {
        if self.algorithm != "argon2id" || self.version != 0x13 {
            return Err(bad("unknown key derivation"));
        }
        let salt = hex::decode_array(&self.salt).ok_or(bad("bad salt"))?;
        let params = KdfParams {
            memory_kib: self.memory_kib,
            passes: self.passes,
            parallelism: self.parallelism,
        };
        params.check()?;
        Ok((params, salt))
    }
}

impl VaultFile {
    pub fn vault_id_bytes(&self) -> Result<[u8; 16], VaultError> {
        hex::decode_array(&self.vault_id).ok_or(bad("bad vault id"))
    }
}

fn bad(why: &str) -> VaultError {
    VaultError::Damaged(why.into())
}

/// 读 `path`；文件不存在时为 `Ok(None)`。比这个版本新的格式报 [`VaultError::TooNew`]。
pub(crate) fn read(path: &Path) -> anyhow::Result<Option<VaultFile>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| VaultError::Damaged(error.to_string()))?;
    let format = value
        .get("format")
        .and_then(serde_json::Value::as_u64)
        .ok_or(bad("no format"))?;
    if format > u64::from(VAULT_FORMAT) {
        return Err(VaultError::TooNew(u32::try_from(format).unwrap_or(u32::MAX)).into());
    }
    let file: VaultFile =
        serde_json::from_value(value).map_err(|error| VaultError::Damaged(error.to_string()))?;
    file.vault_id_bytes()?;
    if let Some(kdf) = &file.kdf {
        kdf.params()?;
        if file.wrapped_key.is_none() {
            return Err(bad("a master password without a wrapped key").into());
        }
    }
    Ok(Some(file))
}

/// 一步换掉整个文件：先写同目录的临时文件、落盘，再改名盖过去。中途崩溃或写失败，
/// 原来的 `vault.json` 原样还在。
pub(crate) fn write(path: &Path, file: &VaultFile) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
    write_atomically(path, &bytes)
}

pub(crate) fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = temporary_path(path);
    let written = (|| {
        let mut out = fs::File::create(&temporary)?;
        out.write_all(bytes)?;
        out.sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    rename_over(&temporary, path)
}

/// Windows 上目标文件被杀毒软件、索引服务短暂占着时改名会 `PermissionDenied`：重试几次。
fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied && tries < 5 => {
                tries += 1;
                thread::sleep(Duration::from_millis(20 * tries));
            }
            Err(error) => {
                let _ = fs::remove_file(from);
                return Err(error);
            }
            Ok(()) => return Ok(()),
        }
    }
}
