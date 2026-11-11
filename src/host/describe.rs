// 参考 since2006/shell-rs @ b2a307a: src/host/describe.rs 重写
// 裁剪：只留主机数据会说的几种（写不进数据库、保险库没收下、主机表单的错误）；
// 保险库失败不再按秘密种类分句，统一说「密码、口令或私钥」，不依赖 `SecretRef` 的变体。

//! 主机数据说的话，翻成用户的语言：主机核心不产出文案，只给类型。

use gpui_kit::SharedString;

use qishell_hosts::{Change, Failure, HostDraftError, OpenError, SecretError};

use crate::i18n::t;

/// 失败或错误给用户看的话。调用处 `use … as _`。
pub trait ErrorMessage {
    fn message(&self) -> SharedString;
}

impl ErrorMessage for Failure {
    fn message(&self) -> SharedString {
        match self {
            Failure::NotSaved { change, error } => not_saved(*change, error),
            Failure::Secret { writing, error, .. } => {
                let error = error.message();
                if *writing {
                    t!("host.store.secret.write", error = error)
                } else {
                    t!("host.store.secret.delete", error = error)
                }
            }
        }
    }
}

impl ErrorMessage for SecretError {
    fn message(&self) -> SharedString {
        match self {
            SecretError::Unavailable => t!("host.store.secret.unavailable"),
            SecretError::Locked => t!("host.store.secret.locked"),
            SecretError::NotSetUp => t!("host.store.secret.not_set_up"),
            SecretError::KeyLost => t!("host.store.secret.key_lost"),
            SecretError::Store(error) => error.clone().into(),
        }
    }
}

impl ErrorMessage for HostDraftError {
    fn message(&self) -> SharedString {
        match self {
            HostDraftError::Address => t!("host.draft.address"),
            HostDraftError::Port => t!("host.draft.port"),
            HostDraftError::NoJumpHosts => t!("host.draft.no_jump_hosts"),
            HostDraftError::DeletedJumpHost => t!("host.draft.deleted_jump_host"),
            HostDraftError::ProxyAddress => t!("host.draft.proxy_address"),
            HostDraftError::ProxyPort => t!("host.draft.proxy_port"),
        }
    }
}

impl ErrorMessage for OpenError {
    fn message(&self) -> SharedString {
        match self {
            OpenError::Newer { version, supported } => t!(
                "host.store.newer_database",
                version = version,
                supported = supported
            ),
            OpenError::Sqlite(error) => error.to_string().into(),
        }
    }
}

/// `change` 没写进数据库，数据库说的是 `error`。
fn not_saved(change: Change, error: &str) -> SharedString {
    match change {
        Change::NewHost => t!("host.store.not_saved.new_host", error = error),
        Change::SaveHost => t!("host.store.not_saved.save_host", error = error),
        Change::DeleteHost => t!("host.store.not_saved.delete_host", error = error),
        Change::DuplicateHost => t!("host.store.not_saved.duplicate_host", error = error),
        Change::HostOrder => t!("host.store.not_saved.host_order", error = error),
        Change::NewCredential => t!("host.store.not_saved.new_credential", error = error),
        Change::SaveCredential => t!("host.store.not_saved.save_credential", error = error),
        Change::DeleteCredential => t!("host.store.not_saved.delete_credential", error = error),
        Change::Recent => t!("host.store.not_saved.recent", error = error),
        Change::HostOs => t!("host.store.not_saved.host_os", error = error),
        Change::NewSnippet => t!("host.store.not_saved.new_snippet", error = error),
        Change::SaveSnippet => t!("host.store.not_saved.save_snippet", error = error),
        Change::DeleteSnippet => t!("host.store.not_saved.delete_snippet", error = error),
        Change::Batch => t!("host.store.not_saved.batch", error = error),
        Change::HostProbe => t!("host.store.not_saved.host_probe", error = error),
        Change::HostHostname => t!("host.store.not_saved.host_hostname", error = error),
    }
}
