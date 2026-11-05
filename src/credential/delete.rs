// 参考 since2006/shell-rs @ b2a307a: src/workspace/credentials.rs 的 `on_delete_credential`、
// `describe_credential_delete` 重写：确认框用共用的 `ui::confirm`（原型 `.dlg.confirm`），并列出主机名。

//! 删除凭据的确认（原型「删除有主机在用的凭据」）：写明是哪些主机在用；删除后这些主机改成「密码」
//! （密码凭据）或「无密码」（密钥、Agent 凭据），用户名保留；保险库里的秘密一起删除。
//! 已经连着的卡片不受影响。

use gpui_kit::*;

use crate::host::{AuthKind, CredentialId, CredentialKind, HostStore};
use crate::i18n::t;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};

/// 确认框里的话，一行一句（主机名各占一行）。
fn lines(
    name: &SharedString,
    kind: CredentialKind,
    user: &SharedString,
    users: Vec<SharedString>,
) -> Vec<ConfirmLine> {
    let mut lines = vec![ConfirmLine::Text(t!(
        "credential.delete.question",
        name = name
    ))];
    if !users.is_empty() {
        lines.push(ConfirmLine::Text(t!(
            "credential.delete.users",
            count = users.len()
        )));
        lines.extend(users.into_iter().map(ConfirmLine::Item));
        lines.push(ConfirmLine::Text(match kind.without_credential() {
            AuthKind::Password => t!("credential.delete.then_password", user = user),
            AuthKind::NoPassword => t!("credential.delete.then_no_password", user = user),
        }));
    }
    match kind {
        CredentialKind::Password => {
            lines.push(ConfirmLine::Text(t!("credential.delete.secrets_password")))
        }
        CredentialKind::Key => lines.push(ConfirmLine::Text(t!("credential.delete.secrets_key"))),
        CredentialKind::Agent => {}
    }
    lines
}

/// 问一下再删除 `credential`。凭据已经不在了就什么都不做。
pub fn open_delete_credential_dialog(
    store: Entity<HostStore>,
    credential: CredentialId,
    window: &mut Window,
    cx: &mut App,
) {
    let (name, kind, user, users) = {
        let data = store.read(cx);
        let Some(found) = data.credential(credential) else {
            return;
        };
        let users = data
            .hosts_using(credential)
            .map(|host| host.name.clone())
            .collect();
        (found.name.clone(), found.kind, found.user.clone(), users)
    };
    open_confirm_dialog(
        Confirm {
            id: "credential-delete",
            title: t!("credential.delete.title"),
            lines: lines(&name, kind, &user, users),
            cancel: t!("credential.delete.cancel"),
            confirm: t!("credential.delete.confirm"),
        },
        DialogLayer::new(),
        Box::new(move |_, _, cx| {
            store.update(cx, |store, cx| store.remove_credential(credential, cx));
            None
        }),
        Box::new(|_, _| {}),
        window,
        cx,
    );
}
