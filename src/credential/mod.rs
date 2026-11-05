//! 凭据（工单 14）：多台主机共用的登录材料（密码、密钥、SSH Agent），带用户名。
//!
//! - `panel`：右侧栏的凭据面板（顶部保险库状态条、列表、双击或回车编辑、右键菜单）；
//! - `dialog`：凭据对话框（名称、类型、用户名；密钥的私钥来源：本机文件只读一次、粘贴、生成
//!   Ed25519 或 RSA 4096；最后是口令）。私钥、口令、密码都加密存进保险库；
//! - `delete`：删除凭据的确认（写明是哪些主机在用，删除后它们改成「密码」或「无密码」）。
//!
//! 功能模块不依赖窗口壳：面板的按钮、菜单派发 `crate::app` 的凭据 action，由窗口壳交给这里。

mod delete;
mod dialog;
mod panel;

pub use delete::open_delete_credential_dialog;
pub use dialog::{CredentialDialog, CredentialSaved, open_credential_dialog};
pub use panel::CredentialPanel;

use gpui_kit::SharedString;
use gpui_kit::assets::IconName;

use crate::app::CatalogIcon;
use crate::host::{Credential, CredentialKind};
use crate::i18n::t;

/// 凭据类型的叫法：密码 / 密钥 / SSH Agent。
pub fn kind_label(kind: CredentialKind) -> SharedString {
    match kind {
        CredentialKind::Password => t!("credential.kind.password"),
        CredentialKind::Key => t!("credential.kind.key"),
        CredentialKind::Agent => t!("credential.kind.agent"),
    }
}

/// 凭据类型的图标（原型：密钥是钥匙，密码是锁，Agent 是人）。
pub fn kind_icon(kind: CredentialKind) -> IconName {
    match kind {
        CredentialKind::Password => CatalogIcon::Lock,
        CredentialKind::Key => CatalogIcon::KeyRound,
        CredentialKind::Agent => CatalogIcon::User,
    }
}

/// 凭据的类型和用户名，一行：「密钥 · ED25519 · deploy」「密码 · root」「SSH Agent · ops」。
/// 读不出公钥的密钥不写算法。
pub fn summary(credential: &Credential) -> SharedString {
    let mut parts = vec![kind_label(credential.kind).to_string()];
    if credential.kind == CredentialKind::Key
        && let Some(algorithm) = credential
            .public_key
            .as_deref()
            .and_then(crate::host::key_algorithm)
    {
        parts.push(algorithm);
    }
    parts.push(credential.user.to_string());
    parts.join(" · ").into()
}
