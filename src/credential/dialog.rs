//! 凭据对话框（下一步实现）。

use gpui_kit::*;

use crate::host::{CredentialId, HostStore};

/// 凭据对话框的内容。
pub struct CredentialDialog;

/// 保存了一条凭据：叠在主机对话框上新建时，主机对话框据此自动选上它。
pub type CredentialSaved = Box<dyn FnOnce(CredentialId, &mut Window, &mut App)>;

/// 打开新建（`editing` 为 `None`）或编辑凭据的对话框。
pub fn open_credential_dialog(
    _store: Entity<HostStore>,
    _editing: Option<CredentialId>,
    _on_saved: Option<CredentialSaved>,
    _window: &mut Window,
    _cx: &mut App,
) {
}
