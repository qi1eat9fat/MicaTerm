//! 凭据的处理器（工单 14）：展开、收起凭据面板，开凭据对话框、删除前确认、复制公钥。
//! 界面在 `crate::credential`。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/credentials.rs

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use crate::app::{
    CopyCredentialPublicKey, DeleteCredential, EditCredential, NewCredential, ShowCredentials,
};
use crate::credential::{open_credential_dialog, open_delete_credential_dialog};
use crate::i18n::t;
use crate::workspace::Workspace;
use crate::workspace::right_sidebar::RightPanel;

pub(in crate::workspace) fn bind_credentials_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_show_credentials))
        .on_action(cx.listener(Workspace::on_new_credential))
        .on_action(cx.listener(Workspace::on_edit_credential))
        .on_action(cx.listener(Workspace::on_copy_credential_public_key))
        .on_action(cx.listener(Workspace::on_delete_credential))
}

impl Workspace {
    /// 右侧栏「凭据」：展开面板（焦点给它，回车、↑↓ 才有去处），展开着就收起。
    fn on_show_credentials(
        &mut self,
        _: &ShowCredentials,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.right_panel = match self.right_panel {
            Some(RightPanel::Credentials) => None,
            _ => {
                let focus = self.credential_panel.read(cx).panel_focus();
                window.focus(&focus, cx);
                Some(RightPanel::Credentials)
            }
        };
        cx.notify();
    }

    fn on_new_credential(
        &mut self,
        _: &NewCredential,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        open_credential_dialog(self.hosts.clone(), None, None, window, cx);
    }

    fn on_edit_credential(
        &mut self,
        &EditCredential(id): &EditCredential,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hosts.read(cx).credential(id).is_some() {
            open_credential_dialog(self.hosts.clone(), Some(id), None, window, cx);
        }
    }

    /// 复制密钥凭据的公钥：公钥存在数据库里，保险库锁着也能复制。
    fn on_copy_credential_public_key(
        &mut self,
        &CopyCredentialPublicKey(id): &CopyCredentialPublicKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(credential) = self.hosts.read(cx).credential(id) else {
            return;
        };
        match credential.public_key.clone() {
            Some(line) => {
                cx.write_to_clipboard(ClipboardItem::new_string(line.to_string()));
                window.push_notification(
                    Notification::success(t!("credential.public_key_copied")),
                    cx,
                );
            }
            None => window.push_notification(
                Notification::warning(t!("credential.public_key_unknown")),
                cx,
            ),
        }
    }

    fn on_delete_credential(
        &mut self,
        &DeleteCredential(id): &DeleteCredential,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        open_delete_credential_dialog(self.hosts.clone(), id, window, cx);
    }
}
