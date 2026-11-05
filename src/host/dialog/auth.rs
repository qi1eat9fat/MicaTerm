//! 主机对话框的「认证」组：分段按钮选「密码」「使用凭据」「无密码」（spec 第 4 节「认证」）。
//!
//! - 密码（工单 11）：用户名，加一个密码框（留空则每次连接时询问）。密码不进 `HostDraft`（它派生了
//!   `Debug`），保存时由对话框单独写进保险库，按主机 id 存；
//! - 使用凭据（工单 14）：下拉选一条凭据，用户名跟着凭据走；下拉最后一项「新建凭据…」在上面叠一层
//!   凭据对话框，保存后自动选上；没选凭据就保存时描红「请选一条凭据」；
//! - 无密码（工单 14）：用户名，连接时依次试 SSH Agent 和 `~/.ssh` 下的默认私钥。
//!
//! 保险库（工单 13）：
//! - 保险库锁着、这台主机的密码又存过时，密码框换成「已保存，保险库锁定中 · 解锁后修改」和「解锁…」，
//!   解开后换回密码框、填上存的密码；
//! - 还没建保险库时，密码框下面提示「第一次保存秘密时会问你要不要设主密码，可以跳过」；
//! - 要写密码而保险库还收不下（没建、锁着、打不开）时，对话框先经 `vault::before_saving_secret` 弹框
//!   （[`AuthSection::writes_password`]）。

use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use zeroize::Zeroizing;

use qishell_hosts::{AuthKind, Credential, CredentialId, HostDraft, SecretRef};

use super::HostDialog;
use crate::host::HostStore;
use crate::i18n::t;
use crate::settings::{OnChoose, segmented};
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, FieldNote, form_field, form_group, locked_secret, text_input};
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
/// 「使用凭据」的下拉框（当前选的凭据在无障碍名称里），点开是凭据列表。
pub const FIELD_CREDENTIAL: &str = "host-credential";
/// 下拉里的一条凭据，`(CREDENTIAL_OPTION, 凭据 id)`。
pub const CREDENTIAL_OPTION: &str = "host-credential-option";
/// 下拉最后一项「新建凭据…」。
pub const CREDENTIAL_NEW: &str = "host-credential-new";
/// 「用户名跟着凭据走：deploy」「依次尝试 SSH Agent 和 ~/.ssh 下的默认私钥」。
pub const AUTH_HINT: &str = "host-auth-hint";

/// 主机怎么登录。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AuthChoice {
    /// 用户名加密码，密码留空则每次连接时询问。
    #[default]
    Password,
    /// 一条保存的凭据，用户名跟着凭据走。
    Credential,
    /// 什么都不输：依次试 SSH Agent 和默认私钥。
    NoPassword,
}

impl AuthChoice {
    const ALL: [AuthChoice; 3] = [
        AuthChoice::Password,
        AuthChoice::Credential,
        AuthChoice::NoPassword,
    ];

    fn label(self) -> SharedString {
        match self {
            AuthChoice::Password => t!("host.dialog.auth_password"),
            AuthChoice::Credential => t!("host.dialog.auth_credential"),
            AuthChoice::NoPassword => t!("host.dialog.auth_no_password"),
        }
    }

    fn of(draft: &HostDraft) -> Self {
        match (draft.credential, draft.auth) {
            (Some(_), _) => AuthChoice::Credential,
            (None, AuthKind::NoPassword) => AuthChoice::NoPassword,
            (None, AuthKind::Password) => AuthChoice::Password,
        }
    }
}

pub(super) struct AuthSection {
    choice: AuthChoice,
    user: Entity<InputState>,
    password: Entity<InputState>,
    /// 「使用凭据」选的凭据。
    credential: Option<CredentialId>,
    /// 凭据下拉展开着。
    picking: bool,
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
            choice: AuthChoice::of(draft),
            user,
            password: password_field,
            credential: draft.credential,
            picking: false,
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

    pub(super) fn fill(&self, draft: &mut HostDraft, errors: &mut FieldErrors, cx: &App) {
        match self.choice {
            AuthChoice::Password => {
                draft.user = self.user.read(cx).value();
                draft.auth = AuthKind::Password;
                draft.credential = None;
            }
            AuthChoice::Credential => {
                // 用户名由存储从凭据取（`HostData` 保存时照凭据改）。
                draft.user = self.user.read(cx).value();
                draft.auth = AuthKind::Password;
                match self.chosen(cx) {
                    Some(credential) => draft.credential = Some(credential.id),
                    None => errors.set(FIELD_CREDENTIAL, t!("host.dialog.credential_required")),
                }
            }
            AuthChoice::NoPassword => {
                draft.user = self.user.read(cx).value();
                draft.auth = AuthKind::NoPassword;
                draft.credential = None;
            }
        }
    }

    /// 选的凭据，还在的话。
    fn chosen<'a>(&self, cx: &'a App) -> Option<&'a Credential> {
        self.credential
            .and_then(|id| self.store.read(cx).credential(id))
    }

    /// 改选认证方式：收起凭据下拉和它的错误。
    fn choose(&mut self, choice: AuthChoice, errors: &mut FieldErrors) {
        self.choice = choice;
        self.picking = false;
        errors.clear(FIELD_CREDENTIAL);
    }

    /// 选上凭据 `id`（下拉里点的，或刚在叠起来的凭据对话框里新建的）。
    pub(super) fn choose_credential(&mut self, id: CredentialId, errors: &mut FieldErrors) {
        self.choice = AuthChoice::Credential;
        self.credential = Some(id);
        self.picking = false;
        errors.clear(FIELD_CREDENTIAL);
    }

    /// 密码框里现在的字（「测试连接」用表单里的密码登录，存没存过都一样，工单 12）。
    pub(super) fn typed_password(&self, cx: &App) -> SharedString {
        if self.locked || self.choice != AuthChoice::Password {
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

    pub(super) fn render(
        &self,
        errors: &FieldErrors,
        tokens: &Tokens,
        cx: &mut Context<HostDialog>,
    ) -> AnyElement {
        let choice = self.choice;
        let dialog = cx.entity().downgrade();
        let on_choose: OnChoose = Rc::new(move |index, _, cx| {
            let Some(choice) = AuthChoice::ALL.get(index).copied() else {
                return;
            };
            dialog
                .update(cx, |dialog, cx| {
                    dialog.auth.choose(choice, &mut dialog.errors);
                    cx.notify();
                })
                .ok();
        });
        let mut rows = vec![form_field(
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
                on_choose,
                tokens,
            ),
            Vec::new(),
            tokens,
        )];
        match choice {
            AuthChoice::Password => {
                rows.push(self.render_user(errors, None, tokens));
                rows.push(self.render_password(tokens));
            }
            AuthChoice::Credential => rows.push(self.render_credential(errors, tokens, cx)),
            AuthChoice::NoPassword => rows.push(self.render_user(
                errors,
                Some(t!("host.dialog.no_password_hint")),
                tokens,
            )),
        }
        form_group(t!("host.dialog.auth"), rows, tokens)
    }

    fn render_user(
        &self,
        errors: &FieldErrors,
        hint: Option<SharedString>,
        tokens: &Tokens,
    ) -> AnyElement {
        let notes: Vec<FieldNote> = FieldNote::error_of(FIELD_USER, errors)
            .into_iter()
            .collect();
        let control = div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(text_input(
                FIELD_USER,
                t!("host.dialog.user"),
                &self.user,
                errors.get(FIELD_USER).is_some(),
                tokens,
            ))
            .children(hint.map(|hint| hint_line(hint, tokens)));
        form_field(Some(t!("host.dialog.user")), control, notes, tokens)
    }

    /// 「凭据」一行：下拉框（原型 `.in.sel`），下面是「用户名跟着凭据走：deploy」或出错的原因；
    /// 点开是凭据列表，最后一项「新建凭据…」。
    fn render_credential(
        &self,
        errors: &FieldErrors,
        tokens: &Tokens,
        cx: &mut Context<HostDialog>,
    ) -> AnyElement {
        let chosen = self.chosen(cx).cloned();
        let invalid = errors.get(FIELD_CREDENTIAL).is_some();
        let (text, placeholder) = match &chosen {
            Some(credential) => (credential_label(credential), false),
            None => (t!("host.dialog.choose_credential"), true),
        };
        let select = div()
            .id(FIELD_CREDENTIAL)
            .test_support()
            .aria_label(if placeholder {
                SharedString::default()
            } else {
                text.clone()
            })
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .px(px(10.))
            .rounded(px(8.))
            .bg(tokens.field)
            .border_1()
            .border_color(if invalid {
                tokens.bad
            } else if self.picking {
                tokens.field_focused
            } else {
                tokens.field_border
            })
            .text_size(px(12.5))
            .cursor_pointer()
            .on_click(cx.listener(|dialog, _, _, cx| {
                dialog.auth.picking = !dialog.auth.picking;
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(placeholder, |text| text.text_color(tokens.faint))
                    .child(text),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(14.))
                    .text_color(tokens.icon),
            );
        let picker = self.picking.then(|| self.render_picker(tokens, cx));
        let hint = match (&chosen, invalid) {
            (Some(credential), false) => Some(hint_line(
                t!("host.dialog.credential_user", user = credential.user),
                tokens,
            )),
            _ => None,
        };
        form_field(
            Some(t!("host.dialog.credential")),
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(select)
                .children(picker)
                .children(hint),
            FieldNote::error_of(FIELD_CREDENTIAL, errors)
                .into_iter()
                .collect(),
            tokens,
        )
    }

    /// 展开的凭据列表：每条一行，最后是「新建凭据…」。
    fn render_picker(&self, tokens: &Tokens, cx: &mut Context<HostDialog>) -> AnyElement {
        let hover = tokens.hover;
        let credentials: Vec<Credential> = self.store.read(cx).credentials().to_vec();
        let option = |id: ElementId, label: SharedString| {
            div()
                .id(id)
                .test_support()
                .aria_label(label.clone())
                .px(px(8.))
                .py(px(4.))
                .rounded(px(6.))
                .text_size(px(12.5))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(div().truncate().child(label))
        };
        div()
            .id("host-credential-picker")
            .test_support()
            .flex()
            .flex_col()
            .mt(px(4.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens.line)
            .children(credentials.into_iter().map(|credential| {
                let id = credential.id;
                option(
                    (CREDENTIAL_OPTION, id.0 as usize).into(),
                    credential_label(&credential),
                )
                .when(self.credential == Some(id), |row| row.bg(tokens.selection))
                .on_click(cx.listener(move |dialog, _, _, cx| {
                    dialog.auth.choose_credential(id, &mut dialog.errors);
                    cx.notify();
                }))
            }))
            .child(
                option(CREDENTIAL_NEW.into(), t!("host.dialog.new_credential"))
                    .text_color(tokens.accent)
                    .on_click(cx.listener(|dialog, _, window, cx| {
                        dialog.auth.picking = false;
                        cx.notify();
                        dialog.new_credential(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_password(&self, tokens: &Tokens) -> AnyElement {
        if self.locked {
            return form_field(
                Some(t!("host.dialog.password")),
                locked_secret(PASSWORD_LOCKED, PASSWORD_UNLOCK, tokens),
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

/// 下拉里、下拉框上凭据的叫法：「部署密钥（密钥 · deploy）」。
fn credential_label(credential: &Credential) -> SharedString {
    t!(
        "host.dialog.credential_option",
        name = credential.name,
        kind = crate::credential::kind_label(credential.kind),
        user = credential.user
    )
}

/// 字段下面的一句灰字（id 是 [`AUTH_HINT`]，文字在无障碍名称里）。
fn hint_line(hint: SharedString, tokens: &Tokens) -> impl IntoElement + use<> {
    div()
        .id(AUTH_HINT)
        .test_support()
        .aria_label(hint.clone())
        .text_size(px(11.5))
        .line_height(relative(1.45))
        .text_color(tokens.faint)
        .child(hint)
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
