//! 主机对话框的「认证」组。本工单（11）只有「密码」：用户名，加一个密码框（留空则每次连接时
//! 询问）。密码不进 `HostDraft`（它派生了 `Debug`），保存时由对话框单独写进保险库，按主机 id 存。
//!
//! 扩展点：
//! - 工单 13（保险库界面）：保险库锁着时密码框换成「已保存，保险库锁定中 · 解锁后修改」，
//!   第一次保存秘密时的提示，改 `render` 和 `load_saved_password`；
//! - 工单 14（凭据）：`AuthChoice` 加「使用凭据」「无密码」，`fill` 按选择写 `auth` / `credential`。

use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use zeroize::Zeroizing;

use qishell_hosts::{AuthKind, CredentialId, HostDraft, SecretRef};

use super::HostDialog;
use super::form::{FieldErrors, FieldNote, form_field, form_group, text_input};
use crate::host::HostStore;
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::Tokens;

pub const FIELD_USER: &str = "host-user";
pub const FIELD_PASSWORD: &str = "host-password";
/// 认证方式的分段按钮。
pub const AUTH_CHOICE: &str = "host-auth";

/// 主机怎么登录。工单 14 加「使用凭据」「无密码」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AuthChoice {
    /// 用户名加密码，密码留空则每次连接时询问。
    #[default]
    Password,
}

impl AuthChoice {
    const ALL: [AuthChoice; 1] = [AuthChoice::Password];

    fn label(self) -> SharedString {
        match self {
            AuthChoice::Password => t!("host.dialog.auth_password"),
        }
    }
}

pub(super) struct AuthSection {
    choice: AuthChoice,
    user: Entity<InputState>,
    password: Entity<InputState>,
    /// 编辑的主机原来用的凭据。这一组还没有「使用凭据」，保存时不动它，免得丢掉。
    credential: Option<CredentialId>,
    /// 保险库里原来存的密码：`None` 是还没读回来（这时空的密码框不表示「删掉」），
    /// `Some(None)` 是没存过。
    saved_password: Option<Option<Zeroizing<String>>>,
    _load: Option<Task<()>>,
}

impl AuthSection {
    /// `password` 是编辑的主机的密码条目，新建主机时是 `None`。
    pub(super) fn new(
        draft: &HostDraft,
        password: Option<SecretRef>,
        store: &Entity<HostStore>,
        window: &mut Window,
        cx: &mut Context<HostDialog>,
    ) -> Self {
        let user = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(qishell_hosts::DEFAULT_USER)
                .default_value(draft.user.clone())
        });
        let password_field = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t!("host.dialog.password_placeholder"))
        });
        let (saved_password, load) = match password {
            Some(secret) => (
                None,
                Some(load_saved_password(store, secret, window, cx)),
            ),
            None => (Some(None), None),
        };
        Self {
            choice: AuthChoice::Password,
            user,
            password: password_field,
            credential: draft.credential,
            saved_password,
            _load: load,
        }
    }

    pub(super) fn inputs(&self) -> Vec<(&'static str, Entity<InputState>)> {
        vec![
            (FIELD_USER, self.user.clone()),
            (FIELD_PASSWORD, self.password.clone()),
        ]
    }

    pub(super) fn fill(&self, draft: &mut HostDraft, _: &mut FieldErrors, cx: &App) {
        match self.choice {
            AuthChoice::Password => {
                draft.user = self.user.read(cx).value();
                draft.auth = AuthKind::Password;
                draft.credential = self.credential;
            }
        }
    }

    /// 保存时密码要怎么改：`None` 不动，`Some(None)` 删掉，`Some(Some(..))` 写进去。
    pub(super) fn password_change(&self, cx: &App) -> Option<Option<String>> {
        if self.choice != AuthChoice::Password {
            return None;
        }
        let typed = self.password.read(cx).value();
        match &self.saved_password {
            // 还没读回来：只有真输了东西才写。
            None => (!typed.is_empty()).then(|| Some(typed.to_string())),
            Some(saved) => {
                let saved = saved.as_ref().map(|saved| saved.as_str());
                match (saved, typed.is_empty()) {
                    (None, true) => None,
                    (Some(_), true) => Some(None),
                    (Some(saved), false) if saved == typed.as_ref() => None,
                    (_, false) => Some(Some(typed.to_string())),
                }
            }
        }
    }

    pub(super) fn render(&self, errors: &FieldErrors, tokens: &Tokens) -> AnyElement {
        let choice = self.choice;
        let rows = vec![
            form_field(
                None,
                segmented(
                    AUTH_CHOICE,
                    AuthChoice::ALL.iter().map(|choice| choice.label()).collect(),
                    AuthChoice::ALL
                        .iter()
                        .position(|candidate| *candidate == choice)
                        .unwrap_or(0),
                    Rc::new(|_, _, _| {}),
                    tokens,
                ),
                Vec::new(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.user")),
                text_input(
                    FIELD_USER,
                    t!("host.dialog.user"),
                    &self.user,
                    errors.get(FIELD_USER).is_some(),
                    tokens,
                ),
                FieldNote::error_of(FIELD_USER, errors).into_iter().collect(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.password")),
                text_input(
                    FIELD_PASSWORD,
                    t!("host.dialog.password"),
                    &self.password,
                    false,
                    tokens,
                ),
                Vec::new(),
                tokens,
            ),
        ];
        form_group(t!("host.dialog.auth"), rows, tokens)
    }

    fn set_saved_password(
        &mut self,
        saved: Option<Zeroizing<String>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 读回来之前用户已经输了，就不盖掉。
        if self.password.read(cx).value().is_empty()
            && let Some(saved) = &saved
        {
            let value = SharedString::from(saved.to_string());
            self.password
                .update(cx, |field, cx| field.set_value(value, window, cx));
        }
        self.saved_password = Some(saved);
    }
}

/// 在后台读编辑的主机原来存的密码，读回来填进密码框。读不出（锁着、坏了）当作没存。
fn load_saved_password(
    store: &Entity<HostStore>,
    secret: SecretRef,
    window: &mut Window,
    cx: &mut Context<HostDialog>,
) -> Task<()> {
    let read = store.update(cx, |store, cx| store.read_secret(secret, cx));
    cx.spawn_in(window, async move |this, cx| {
        let saved = read.await;
        this.update_in(cx, |dialog, window, cx| {
            dialog.auth.set_saved_password(saved, window, cx);
            cx.notify();
        })
        .ok();
    })
}
