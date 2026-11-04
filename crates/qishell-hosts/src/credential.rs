// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/credential.rs
// 裁剪：私钥不再是本机文件，`key_path` 和「ShellRS 自己保管的密钥文件」去掉，私钥正文和口令
// 都按凭据的 `secret_id` 存进保险库；数据库里改存不算秘密的公钥（锁着时也能显示算法、复制公钥）；
// 去掉同步用的 `edited_ms`；`keychain_id` 改名 `secret_id`。

//! Saved logins that hosts share: a user name with a password, a private key
//! or the SSH agent. They live with the hosts because hosts refer to them
//! and they are stored in the same database; the credential panel that lists
//! and edits them depends on this one, never the reverse.

use gpui_shared_string::SharedString;

use crate::secrets::SecretRef;

use super::{AuthKind, SecretId};

/// Stable identity of a credential. Never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CredentialId(pub u64);

/// What a credential logs in with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CredentialKind {
    /// A password kept in the vault under the credential's own entry.
    #[default]
    Password,
    /// 一把私钥：私钥正文和口令（如果有）都在保险库里，按凭据的 `secret_id` 存。
    /// 「本机文件」只在保存时读一次，之后和原文件再无关系。
    Key,
    /// Whatever keys the SSH agent holds.
    Agent,
}

impl CredentialKind {
    /// Every kind, in the order the form lists them.
    pub const ALL: [CredentialKind; 3] = [
        CredentialKind::Password,
        CredentialKind::Key,
        CredentialKind::Agent,
    ];

    /// The stored spelling. Kept apart from the label the app shows, so
    /// translating the UI cannot rewrite what is already in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialKind::Password => "password",
            CredentialKind::Key => "key",
            CredentialKind::Agent => "agent",
        }
    }

    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "password" => Some(CredentialKind::Password),
            "key" => Some(CredentialKind::Key),
            "agent" => Some(CredentialKind::Agent),
            _ => None,
        }
    }

    /// How a host that used a credential of this kind logs in once the
    /// credential is deleted: the nearest it gets on its own. A password
    /// credential's host is asked for its password; a key's or the agent's
    /// tries the agent and the default keys.
    pub fn without_credential(self) -> AuthKind {
        match self {
            CredentialKind::Password => AuthKind::Password,
            CredentialKind::Key | CredentialKind::Agent => AuthKind::NoPassword,
        }
    }
}

/// Why a draft cannot be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialDraftError {
    Name,
}

/// The user name a login falls back to when none is given, as in the host
/// form.
pub const DEFAULT_USER: &str = "root";

/// The values the credential form commits. Never holds a secret: those go
/// straight to the vault.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CredentialDraft {
    pub name: SharedString,
    pub kind: CredentialKind,
    pub user: SharedString,
    /// 密钥凭据的公钥，OpenSSH 的一行 `ssh-ed25519 AAAA… 注释`。不是秘密，存在数据库里，
    /// 保险库锁着时也能显示算法、复制公钥。只有 [`CredentialKind::Key`] 才有；
    /// 读不出公钥的私钥（老格式的加密私钥）没有。
    pub public_key: Option<SharedString>,
}

impl CredentialDraft {
    pub fn new(
        name: impl Into<SharedString>,
        kind: CredentialKind,
        user: impl Into<SharedString>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            user: user.into(),
            public_key: None,
        }
    }

    pub fn with_public_key(mut self, line: impl Into<SharedString>) -> Self {
        let line = line.into();
        self.public_key = (!line.trim().is_empty()).then_some(line);
        self
    }

    /// The draft with surrounding blanks removed, an empty user name taken
    /// as [`DEFAULT_USER`] and a public key kept only by a key credential,
    /// or the first thing wrong with it.
    pub fn validated(mut self) -> Result<Self, CredentialDraftError> {
        self.name = self.name.trim().to_string().into();
        if self.name.is_empty() {
            return Err(CredentialDraftError::Name);
        }
        let user = self.user.trim();
        self.user = if user.is_empty() {
            DEFAULT_USER.into()
        } else {
            user.to_string().into()
        };
        self.public_key = match self.kind {
            CredentialKind::Key => self
                .public_key
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .map(Into::into),
            CredentialKind::Password | CredentialKind::Agent => None,
        };
        Ok(self)
    }
}

/// A saved credential.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Credential {
    pub id: CredentialId,
    /// Names the credential's secrets in the vault. Random rather than
    /// derived from `id`, which can come back after a restart: a new
    /// credential must never find the secrets an old one left behind.
    pub secret_id: SecretId,
    pub name: SharedString,
    pub kind: CredentialKind,
    pub user: SharedString,
    /// See [`CredentialDraft::public_key`].
    pub public_key: Option<SharedString>,
    /// Position in the credential list.
    pub sort_order: i64,
}

impl Credential {
    /// A credential with a fresh secret id. The store makes sure it is not
    /// one another credential already has.
    pub fn new(id: CredentialId, draft: CredentialDraft) -> Self {
        Self {
            id,
            secret_id: SecretId::generate(),
            name: draft.name,
            kind: draft.kind,
            user: draft.user,
            public_key: draft.public_key,
            sort_order: 0,
        }
    }

    /// The editable fields, for pre-filling the credential form.
    pub fn draft(&self) -> CredentialDraft {
        CredentialDraft {
            name: self.name.clone(),
            kind: self.kind,
            user: self.user.clone(),
            public_key: self.public_key.clone(),
        }
    }

    /// Where a password credential keeps its password.
    pub fn password_secret(&self) -> SecretRef {
        SecretRef::credential(self.secret_id.as_str())
    }

    /// 密钥凭据的私钥正文在保险库里的条目。
    pub fn private_key_secret(&self) -> SecretRef {
        SecretRef::private_key(self.secret_id.as_str())
    }

    /// 密钥凭据的私钥口令在保险库里的条目。
    pub fn passphrase_secret(&self) -> SecretRef {
        SecretRef::passphrase(self.secret_id.as_str())
    }

    /// 这种凭据在保险库里有哪些条目：删除凭据、或改了类型时，它们跟着删。
    pub fn secrets(&self) -> Vec<SecretRef> {
        match self.kind {
            CredentialKind::Password => vec![self.password_secret()],
            CredentialKind::Key => vec![self.private_key_secret(), self.passphrase_secret()],
            CredentialKind::Agent => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_the_database_spelling() {
        for kind in CredentialKind::ALL {
            assert_eq!(CredentialKind::from_stored(kind.as_str()), Some(kind));
        }
        assert_eq!(CredentialKind::from_stored("otp"), None);
    }

    #[test]
    fn a_draft_needs_a_name() {
        let saved = CredentialDraft::new("  运维  ", CredentialKind::Password, "  ")
            .validated()
            .unwrap();
        assert_eq!(saved.name.as_ref(), "运维");
        assert_eq!(saved.user.as_ref(), DEFAULT_USER);

        assert_eq!(
            CredentialDraft::new(" ", CredentialKind::Password, "root").validated(),
            Err(CredentialDraftError::Name)
        );
        // 读不出公钥的私钥照样能存。
        let key = CredentialDraft::new("部署", CredentialKind::Key, "deploy")
            .validated()
            .unwrap();
        assert_eq!(key.public_key, None);
        let key = CredentialDraft::new("部署", CredentialKind::Key, "deploy")
            .with_public_key(" ssh-ed25519 AAAAC3Nza deploy@web \n")
            .validated()
            .unwrap();
        assert_eq!(
            key.public_key.as_deref(),
            Some("ssh-ed25519 AAAAC3Nza deploy@web")
        );
    }

    #[test]
    fn only_a_key_credential_keeps_a_public_key() {
        for kind in [CredentialKind::Password, CredentialKind::Agent] {
            let draft = CredentialDraft::new("x", kind, "root")
                .with_public_key("ssh-ed25519 AAAAC3Nza")
                .validated()
                .unwrap();
            assert_eq!(draft.public_key, None);
        }
    }

    #[test]
    fn each_credential_has_its_own_vault_entries() {
        let draft = CredentialDraft::new("运维", CredentialKind::Password, "root");
        let one = Credential::new(CredentialId(1), draft.clone());
        let other = Credential::new(CredentialId(1), draft);
        assert_ne!(one.password_secret(), other.password_secret());
        assert_eq!(
            one.password_secret().account(),
            format!("credential:{}", one.secret_id)
        );
    }

    #[test]
    fn a_credential_s_secrets_follow_its_kind() {
        let credential =
            |kind| Credential::new(CredentialId(1), CredentialDraft::new("x", kind, "root"));
        let password = credential(CredentialKind::Password);
        assert_eq!(password.secrets(), [password.password_secret()]);
        let key = credential(CredentialKind::Key);
        assert_eq!(
            key.secrets(),
            [key.private_key_secret(), key.passphrase_secret()]
        );
        assert!(credential(CredentialKind::Agent).secrets().is_empty());
    }
}
