//! 主机对话框的「认证」组。本工单（11）只有「密码」：用户名，加一个密码框（留空则每次连接时
//! 询问）。密码不进 `HostDraft`（它派生了 `Debug`），保存时由对话框单独写进保险库，按主机 id 存。
//!
//! 保险库（工单 13）：
//! - 保险库锁着、这台主机的密码又存过时，密码框换成「已保存，保险库锁定中 · 解锁后修改」和「解锁…」，
//!   解开后换回密码框、填上存的密码；
//! - 还没建保险库时，密码框下面提示「第一次保存秘密时会问你要不要设主密码，可以跳过」；
//! - 要写密码而保险库还收不下（没建、锁着、打不开）时，对话框先经 `vault::before_saving_secret` 弹框
//!   （[`AuthSection::writes_password`]）。
//!
//! 扩展点：工单 14（凭据）：`AuthChoice` 加「使用凭据」「无密码」，`fill` 按选择写 `auth` / `credential`。

use std::rc::Rc;

use gpui_kit::component::Icon;
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use zeroize::Zeroizing;

use qishell_hosts::{AuthKind, CredentialId, HostDraft, SecretRef};

use super::HostDialog;
use super::form::{FieldErrors, FieldNote, form_field, form_group, text_input};
use crate::app::{CatalogIcon, UnlockVault};
use crate::host::HostStore;
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::Tokens;
use crate::vault::{VaultStatus, VaultStore};

pub const FIELD_USER: &str = "host-user";
pub const FIELD_PASSWORD: &str = "host-password";
/// 认证方式的分段按钮。
pub const AUTH_CHOICE: &str = "host-auth";
/// 保险库锁着时代替密码框的那一行，和它的「解锁…」。
pub const PASSWORD_LOCKED: &str = "host-password-locked";
pub const PASSWORD_UNLOCK: &str = "host-password-unlock";
/// 还没建保险库时密码框下面的提示。
pub const PASSWORD_HINT: &str = "host-password-hint";

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
    store: Entity<HostStore>,
    /// 编辑的主机的密码条目，新建主机时是 `None`。
    secret: Option<SecretRef>,
    /// 保险库里原来存的密码：`None` 是还没读回来（这时空的密码框不表示「删掉」），
    /// `Some(None)` 是没存过。
    saved_password: Option<Option<Zeroizing<String>>>,
    /// 保险库的状态（没有保险库时是 `None`），跟着保险库变。
    vault_status: Option<VaultStatus>,
    /// 密码存过而保险库锁着：显示「已保存，保险库锁定中」，不读也不改密码。
    locked: bool,
    _load: Option<Task<()>>,
    _vault: Option<Subscription>,
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
        let vault = VaultStore::global(cx);
        let vault_status = vault.as_ref().and_then(|vault| vault.read(cx).status());
        let locked = is_locked_away(vault.as_ref(), password.as_ref(), cx);
        let watch = vault.map(|vault| {
            cx.observe_in(&vault, window, |dialog, vault, window, cx| {
                dialog.auth.vault_changed(&vault, window, cx);
                cx.notify();
            })
        });
        let mut section = Self {
            choice: AuthChoice::Password,
            user,
            password: password_field,
            credential: draft.credential,
            store: store.clone(),
            secret: password,
            saved_password: Some(None),
            vault_status,
            locked,
            _load: None,
            _vault: watch,
        };
        if !locked {
            section.load_saved(window, cx);
        }
        section
    }

    /// 在后台读回原来存的密码，读回来填进密码框。
    fn load_saved(&mut self, window: &mut Window, cx: &mut Context<HostDialog>) {
        if let Some(secret) = self.secret.clone() {
            self.saved_password = None;
            self._load = Some(load_saved_password(&self.store, secret, window, cx));
        }
    }

    /// 保险库变了：锁着的密码解开了就换回密码框、读回存的密码；又锁上了就换成锁着的那一行。
    fn vault_changed(
        &mut self,
        vault: &Entity<VaultStore>,
        window: &mut Window,
        cx: &mut Context<HostDialog>,
    ) {
        self.vault_status = vault.read(cx).status();
        let locked = is_locked_away(Some(vault), self.secret.as_ref(), cx);
        if self.locked && !locked {
            self.locked = false;
            self.load_saved(window, cx);
        } else if locked && !self.locked && self.password.read(cx).value().is_empty() {
            self.locked = true;
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

    /// 密码框里现在的字（「测试连接」用表单里的密码登录，存没存过都一样，工单 12）。
    pub(super) fn typed_password(&self, cx: &App) -> SharedString {
        if self.locked {
            return SharedString::default();
        }
        self.password.read(cx).value()
    }

    /// 保存时密码要怎么改：`None` 不动，`Some(None)` 删掉，`Some(Some(..))` 写进去。
    pub(super) fn password_change(&self, cx: &App) -> Option<Option<String>> {
        if self.choice != AuthChoice::Password || self.locked {
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

    /// 保存时要往保险库里写密码：保险库收不下时对话框先弹框（设主密码、解锁、重置）。
    pub(super) fn writes_password(&self, cx: &App) -> bool {
        matches!(self.password_change(cx), Some(Some(_)))
    }

    pub(super) fn render(&self, errors: &FieldErrors, tokens: &Tokens) -> AnyElement {
        let choice = self.choice;
        let rows = vec![
            form_field(
                None,
                segmented(
                    AUTH_CHOICE,
                    AuthChoice::ALL
                        .iter()
                        .map(|choice| choice.label())
                        .collect(),
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
                FieldNote::error_of(FIELD_USER, errors)
                    .into_iter()
                    .collect(),
                tokens,
            ),
            self.render_password(tokens),
        ];
        form_group(t!("host.dialog.auth"), rows, tokens)
    }

    fn render_password(&self, tokens: &Tokens) -> AnyElement {
        if self.locked {
            return form_field(
                Some(t!("host.dialog.password")),
                locked_password(tokens),
                Vec::new(),
                tokens,
            );
        }
        let first_time = self.vault_status == Some(VaultStatus::Absent);
        form_field(
            Some(t!("host.dialog.password")),
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(text_input(
                    FIELD_PASSWORD,
                    t!("host.dialog.password"),
                    &self.password,
                    false,
                    tokens,
                ))
                .children(first_time.then(|| {
                    let hint = t!("vault.field.first_hint");
                    div()
                        .id(PASSWORD_HINT)
                        .test_support()
                        .aria_label(hint.clone())
                        .text_size(px(11.5))
                        .line_height(relative(1.45))
                        .text_color(tokens.faint)
                        .child(hint)
                })),
            Vec::new(),
            tokens,
        )
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

/// 这条密码存过，保险库却锁着：读不出来，也不能改。
fn is_locked_away(
    vault: Option<&Entity<VaultStore>>,
    secret: Option<&SecretRef>,
    cx: &App,
) -> bool {
    match (vault, secret) {
        (Some(vault), Some(secret)) => {
            let vault = vault.read(cx);
            vault.is_locked() && vault.contains(secret)
        }
        _ => false,
    }
}

/// 「已保存，保险库锁定中 · 解锁后修改」，右边「解锁…」（原型 `.lockedf`）。
fn locked_password(tokens: &Tokens) -> impl IntoElement + use<> {
    let text = t!("vault.field.locked");
    let accent = tokens.accent;
    div()
        .id(PASSWORD_LOCKED)
        .test_support()
        .aria_label(text.clone())
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .px(px(10.))
        .rounded(px(8.))
        .bg(tokens.field)
        .text_color(tokens.faint)
        .text_size(px(12.))
        .child(Icon::new(CatalogIcon::Lock).size(px(13.)))
        .child(div().flex_1().min_w_0().truncate().child(text))
        .child(
            div()
                .id(PASSWORD_UNLOCK)
                .test_support()
                .aria_label(t!("vault.field.unlock"))
                .flex_none()
                .text_color(accent)
                .cursor_pointer()
                .on_click(|_, window, cx| window.dispatch_action(Box::new(UnlockVault), cx))
                .child(t!("vault.field.unlock")),
        )
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
