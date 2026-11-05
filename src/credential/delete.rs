//! 删除凭据的确认（下一步实现）。

use gpui_kit::*;

use crate::host::{CredentialId, HostStore};

/// 问一下再删除 `credential`。
pub fn open_delete_credential_dialog(
    _store: Entity<HostStore>,
    _credential: CredentialId,
    _window: &mut Window,
    _cx: &mut App,
) {
}
