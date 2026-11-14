// 借自 since2006/shell-rs @ b2a307a 的 MIT 代码：crates/shellrs-sync/src/vault.rs
// （`KdfParams`、`derive`、`wrap`）和 crates/shellrs-sync/src/secret.rs（`seal`、`open`）。
// 改造：没有密钥代数；随机数直接取 `getrandom`；整组加密改成每条秘密一个 nonce，
// AAD 把密文绑在保险库 id 和条目名上。

//! 保险库的密码学：主密码经 Argon2id 派生包裹密钥（KEK），KEK 只用来包裹随机的数据密钥；
//! 每条秘密用数据密钥各自加密（XChaCha20-Poly1305，每条一个随机 nonce）。

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version as ArgonVersion};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use unicode_normalization::UnicodeNormalization as _;
use zeroize::Zeroizing;

use super::{VaultError, hex};

/// Argon2id 的开销，写在 `vault.json` 里，以后可以调高。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub passes: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// 64 MiB、3 遍：普通电脑上约一秒。
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        passes: 3,
        parallelism: 1,
    };
    /// 保险库最多能要求的开销：被改过的文件不能让程序耗尽内存或空转。
    pub const MAX: Self = Self {
        memory_kib: 256 * 1024,
        passes: 10,
        parallelism: 4,
    };
    /// Argon2 允许的最小开销，只给测试用：不用等它。
    pub const FAST_FOR_TESTS: Self = Self {
        memory_kib: 8,
        passes: 1,
        parallelism: 1,
    };

    fn argon2(self) -> Result<Argon2<'static>, VaultError> {
        let within = self.memory_kib <= Self::MAX.memory_kib
            && (1..=Self::MAX.passes).contains(&self.passes)
            && (1..=Self::MAX.parallelism).contains(&self.parallelism);
        if !within {
            return Err(VaultError::Damaged("key derivation out of bounds".into()));
        }
        let params = Params::new(self.memory_kib, self.passes, self.parallelism, Some(32))
            .map_err(|error| VaultError::Damaged(error.to_string()))?;
        Ok(Argon2::new(
            Algorithm::Argon2id,
            ArgonVersion::V0x13,
            params,
        ))
    }

    /// 在跑之前先检查：越界的参数直接拒绝。
    pub(crate) fn check(self) -> Result<(), VaultError> {
        self.argon2().map(drop)
    }
}

/// 保险库的数据密钥：随机的 32 字节。`Debug` 不显示内容，drop 时清零。
#[derive(Clone, PartialEq, Eq)]
pub struct DataKey(Zeroizing<[u8; 32]>);

impl DataKey {
    pub(crate) fn random() -> Result<Self, VaultError> {
        Ok(Self(Zeroizing::new(random_array()?)))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 存进系统钥匙串的样子：64 个小写十六进制字符。
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(&*self.0))
    }

    /// [`DataKey::to_hex`] 的反向；不是 64 个十六进制字符时为 `None`。
    pub fn from_hex(text: &str) -> Option<Self> {
        hex::decode_array::<32>(text).map(Self::from_bytes)
    }
}

impl fmt::Debug for DataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DataKey(…)")
    }
}

/// 一段密文和它的 nonce。
pub(crate) struct Sealed {
    pub nonce: [u8; 24],
    pub sealed: Vec<u8>,
}

pub(crate) fn random_array<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| VaultError::Random)?;
    Ok(out)
}

/// 主密码派生出的包裹密钥。
pub(crate) fn derive(
    password: &str,
    salt: &[u8; 16],
    params: KdfParams,
) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    // Mac 和 Windows 上打出的同一个密码，带重音的字母可能组合方式不同；NFC 之后字节相同。
    let password = Zeroizing::new(password.nfc().collect::<String>());
    let mut kek = Zeroizing::new([0u8; 32]);
    params
        .argon2()?
        .hash_password_into(password.as_bytes(), salt, &mut *kek)
        .map_err(|error| VaultError::Damaged(error.to_string()))?;
    Ok(kek)
}

/// 用 `key` 加密 `plain`，密文绑在 `aad` 上。
pub(crate) fn seal(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Result<Sealed, VaultError> {
    let nonce: [u8; 24] = random_array()?;
    let sealed = XChaCha20Poly1305::new_from_slice(key)
        .expect("32-byte key")
        .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad })
        .map_err(|_| VaultError::Damaged("cannot seal".into()))?;
    Ok(Sealed { nonce, sealed })
}

/// [`seal`] 的反向；密钥不对、密文或 AAD 被改过时为 `None`。
pub(crate) fn open(key: &[u8; 32], sealed: &Sealed, aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    XChaCha20Poly1305::new_from_slice(key)
        .expect("32-byte key")
        .decrypt(
            &XNonce::from(sealed.nonce),
            Payload {
                msg: &sealed.sealed,
                aad,
            },
        )
        .ok()
        .map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::{KdfParams, derive, hex};

    /// 派生就是格式的一部分：同样的密码、salt 和开销，每个版本都得到同样的密钥。
    /// 期望值是 shellrs-sync 用参考 C 实现（argon2-cffi）核对过的，不是本 crate 自己算的。
    #[test]
    fn the_key_derivation_is_pinned() {
        let kek = derive("correct horse", &[7; 16], KdfParams::FAST_FOR_TESTS).unwrap();
        assert_eq!(
            hex::encode(&*kek),
            "77c7165d162cfa442dd959d139711790f84c091401080df1b8cbdb1855ccf11e"
        );
    }

    #[test]
    fn a_password_typed_composed_or_decomposed_derives_the_same_key() {
        let params = KdfParams::FAST_FOR_TESTS;
        assert_eq!(
            *derive("caf\u{e9}", &[1; 16], params).unwrap(),
            *derive("cafe\u{301}", &[1; 16], params).unwrap()
        );
    }
}
