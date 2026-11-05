// 参考 since2006/shell-rs @ b2a307a: src/workspace/credentials.rs 的 `on_delete_credential`、
// `describe_credential_delete` 重写：样式照 `host::dialog::delete`（原型 `.dlg.confirm`），并列出主机名。

//! 删除凭据的确认（原型「删除有主机在用的凭据」）：写明是哪些主机在用；删除后这些主机改成「密码」
//! （密码凭据）或「无密码」（密钥、Agent 凭据），用户名保留；保险库里的秘密一起删除。
//! 已经连着的卡片不受影响。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::host::{AuthKind, CredentialId, CredentialKind, HostStore};
use crate::i18n::t;
use crate::theme::Tokens;

/// 确认框的宽（原型 `.dlg.confirm`）。
const WIDTH: Pixels = px(420.);

pub struct DeleteCredentialDialog {
    store: Entity<HostStore>,
    credential: CredentialId,
    name: SharedString,
    kind: CredentialKind,
    user: SharedString,
    /// 在用它的主机的名字。
    users: Vec<SharedString>,
    focus_handle: FocusHandle,
}

impl DeleteCredentialDialog {
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let credential = self.credential;
        self.store
            .update(cx, |store, cx| store.remove_credential(credential, cx));
        window.close_dialog(cx);
    }

    /// 确认框里的话，一行一句（主机名各占一行）。
    fn lines(&self) -> Vec<SharedString> {
        let mut lines = vec![t!("credential.delete.question", name = self.name)];
        if !self.users.is_empty() {
            lines.push(t!("credential.delete.users", count = self.users.len()));
            lines.extend(self.users.iter().cloned());
            lines.push(match self.kind.without_credential() {
                AuthKind::Password => t!("credential.delete.then_password", user = self.user),
                AuthKind::NoPassword => {
                    t!("credential.delete.then_no_password", user = self.user)
                }
            });
        }
        match self.kind {
            CredentialKind::Password => lines.push(t!("credential.delete.secrets_password")),
            CredentialKind::Key => lines.push(t!("credential.delete.secrets_key")),
            CredentialKind::Agent => {}
        }
        lines
    }
}

impl Focusable for DeleteCredentialDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DeleteCredentialDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let lines = self.lines();
        let description: SharedString = lines
            .iter()
            .map(|line| line.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let users = self.users.len();
        div()
            .id("credential-delete")
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(46.))
                    .px(px(18.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("credential.delete.title")),
            )
            .child(
                div()
                    .id("credential-delete-text")
                    .test_support()
                    .aria_label(description)
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .children(lines.into_iter().enumerate().map(|(index, line)| {
                        // 第 2 行起是主机名，缩进、带圆点。
                        let host = users > 0 && (2..2 + users).contains(&index);
                        div().when(host, |row| row.pl(px(18.))).child(if host {
                            SharedString::from(format!("\u{2022} {line}"))
                        } else {
                            line
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(12.))
                    .pb(px(14.))
                    .pl(px(18.))
                    .pr(px(14.))
                    .child(div().flex_1())
                    .child(
                        Button::new("credential-delete-cancel")
                            .small()
                            .label(t!("credential.delete.cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("credential-delete-confirm")
                            .danger()
                            .small()
                            .label(t!("credential.delete.confirm"))
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
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
    let view = cx.new(|cx| DeleteCredentialDialog {
        store,
        credential,
        name,
        kind,
        user,
        users,
        focus_handle: cx.focus_handle(),
    });
    let focus = view.read(cx).focus_handle.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(WIDTH)
            .p_0()
            .rounded(px(14.))
            .close_button(false)
            .overlay_closable(false)
            .child(view.clone())
    });
    window.focus(&focus, cx);
}
