//! 设置主密码框和更换主密码框（原型「第一次保存秘密：设置主密码」；样子是共用的 `ui::modal` 弹窗）。
//!
//! - 第一次保存秘密（[`PasswordMode::FirstSave`]）：主密码、再输一次、「在本机记住」（默认勾选），
//!   底部左边「不设置」（秘密照样存，只靠本机钥匙串），右边「取消」「设置并保存」；
//! - 设置 › 安全「设置主密码…」（[`PasswordMode::Set`]）：没有「不设置」；
//! - 设置 › 安全「更换主密码…」（[`PasswordMode::Change`]）：当前主密码、新主密码、再输一次。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use micaterm_hosts::VaultStatus;

use super::reply::{Reply, close_and_reply, open_small_dialog};
use super::store::{VaultStore, describe, is_wrong_password};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, typed_secret};
use crate::ui::modal::{
    ModalButtonTone, ModalInputKind, OnClick, modal_button, modal_checkbox, modal_field,
    modal_note, modal_question, modal_warn, render_modal,
};

pub const FIELD_CURRENT: &str = "vault-current-password";
pub const FIELD_MASTER: &str = "vault-master-password";
pub const FIELD_AGAIN: &str = "vault-master-password-again";

/// 主密码框是为什么打开的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordMode {
    /// 第一次保存秘密时：可以「不设置」。
    FirstSave,
    /// 设置 › 安全里补设主密码。
    Set,
    /// 设置 › 安全里更换主密码。
    Change,
}

pub struct PasswordDialog {
    vault: Entity<VaultStore>,
    mode: PasswordMode,
    current: Entity<InputState>,
    master: Entity<InputState>,
    again: Entity<InputState>,
    /// 「在本机记住」，默认勾选。
    remember: bool,
    errors: FieldErrors,
    working: Option<Task<()>>,
    reply: Reply,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PasswordDialog {
    fn new(
        vault: Entity<VaultStore>,
        mode: PasswordMode,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field =
            |placeholder: Option<SharedString>, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    let input = InputState::new(window, cx).masked(true);
                    match placeholder {
                        Some(placeholder) => input.placeholder(placeholder),
                        None => input,
                    }
                })
            };
        let current = field(None, window, cx);
        let master = field(None, window, cx);
        let again = field(Some(t!("vault.setup.again_placeholder")), window, cx);
        let subscriptions = [
            (FIELD_CURRENT, &current),
            (FIELD_MASTER, &master),
            (FIELD_AGAIN, &again),
        ]
        .into_iter()
        .map(|(field, input)| {
            cx.subscribe_in(
                input,
                window,
                move |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if this.errors.clear(field) {
                            cx.notify();
                        }
                    }
                    InputEvent::PressEnter { .. } => this.confirm(window, cx),
                    _ => {}
                },
            )
        })
        .collect();
        Self {
            vault,
            mode,
            current,
            master,
            again,
            remember: true,
            errors: FieldErrors::default(),
            working: None,
            reply,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn first_field(&self) -> &Entity<InputState> {
        match self.mode {
            PasswordMode::Change => &self.current,
            _ => &self.master,
        }
    }

    /// 「设置并保存」「设置」「更换」。
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let mut errors = FieldErrors::default();
        let current = typed_secret(&self.current, cx);
        let master = typed_secret(&self.master, cx);
        let again = typed_secret(&self.again, cx);
        if self.mode == PasswordMode::Change && current.is_empty() {
            errors.set(FIELD_CURRENT, t!("vault.setup.empty"));
        }
        if master.is_empty() {
            errors.set(FIELD_MASTER, t!("vault.setup.empty"));
        } else if *master != *again {
            errors.set(FIELD_AGAIN, t!("vault.setup.mismatch"));
        }
        if !errors.is_empty() {
            self.errors = errors;
            cx.notify();
            return;
        }
        let remember = self.remember;
        let status = self.vault.read(cx).status();
        let task = self
            .vault
            .update(cx, |vault, cx| match (self.mode, status) {
                (PasswordMode::Change, _) => vault.change_master_password(current, master, cx),
                (_, Some(VaultStatus::Unprotected)) => {
                    vault.set_master_password(master, remember, cx)
                }
                _ => vault.set_up(Some(master), remember, cx),
            });
        self.finish(task, window, cx);
    }

    /// 「不设置」：秘密照样存进保险库，数据密钥只在本机钥匙串里。
    fn skip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let task = self
            .vault
            .update(cx, |vault, cx| vault.set_up(None, false, cx));
        self.finish(task, window, cx);
    }

    fn finish(
        &mut self,
        task: Task<anyhow::Result<()>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.working = None;
                match result {
                    Ok(()) => close_and_reply(&this.reply.clone(), true, window, cx),
                    Err(error) => {
                        let field = if is_wrong_password(&error) {
                            FIELD_CURRENT
                        } else {
                            FIELD_MASTER
                        };
                        let message = describe(&error);
                        this.errors.set(field, message);
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn render_field(
        &self,
        field: &'static str,
        label: SharedString,
        input: &Entity<InputState>,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> Div {
        modal_field(
            field,
            label,
            input,
            ModalInputKind::Secret,
            self.errors.get(field).cloned(),
            tokens,
            window,
            cx,
        )
    }

    fn render_footer(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let working = self.working.is_some();
        let (confirm, confirming) = match self.mode {
            PasswordMode::FirstSave => (t!("vault.setup.save"), t!("vault.setup.working")),
            PasswordMode::Set => (t!("vault.setup.confirm"), t!("vault.setup.working")),
            PasswordMode::Change => (t!("vault.change.confirm"), t!("vault.change.working")),
        };
        let reply = self.reply.clone();
        // 「不设置」在最左，「取消」「设置并保存」在右。
        div()
            .flex()
            .flex_1()
            .gap(px(6.))
            .when(self.mode == PasswordMode::FirstSave, |footer| {
                footer.child(modal_button(
                    "vault-password-skip",
                    t!("vault.setup.skip"),
                    None,
                    ModalButtonTone::Plain,
                    (!working).then(|| -> OnClick {
                        Box::new(cx.listener(|this, _, window, cx| this.skip(window, cx)))
                    }),
                    tokens,
                    window,
                    cx,
                ))
            })
            .child(div().flex_1())
            .child(modal_button(
                "vault-password-cancel",
                t!("vault.setup.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                (!working).then(|| -> OnClick {
                    Box::new(move |_, window, cx| close_and_reply(&reply, false, window, cx))
                }),
                tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "vault-password-confirm",
                if working { confirming } else { confirm },
                Some("Enter"),
                ModalButtonTone::Primary,
                (!working).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.confirm(window, cx)))
                }),
                tokens,
                window,
                cx,
            ))
    }
}

impl Focusable for PasswordDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PasswordDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let changing = self.mode == PasswordMode::Change;
        let (title, intro) = match self.mode {
            PasswordMode::FirstSave => (t!("vault.setup.title"), t!("vault.setup.first_intro")),
            PasswordMode::Set => (t!("vault.setup.title"), t!("vault.setup.intro")),
            PasswordMode::Change => (t!("vault.change.title"), t!("vault.change.intro")),
        };
        let master_label = if changing {
            t!("vault.change.new")
        } else {
            t!("vault.setup.master")
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(modal_warn(
                CatalogIcon::Shield,
                title.clone(),
                false,
                &tokens,
            ))
            .child(modal_question(intro, &tokens))
            .when(changing, |body| {
                body.child(self.render_field(
                    FIELD_CURRENT,
                    t!("vault.change.current"),
                    &self.current,
                    &tokens,
                    window,
                    cx,
                ))
            })
            .child(self.render_field(
                FIELD_MASTER,
                master_label,
                &self.master,
                &tokens,
                window,
                cx,
            ))
            .child(self.render_field(
                FIELD_AGAIN,
                t!("vault.setup.again"),
                &self.again,
                &tokens,
                window,
                cx,
            ))
            .when(!changing, |body| {
                body.child(modal_checkbox(
                    "vault-remember",
                    self.remember,
                    t!("vault.setup.remember"),
                    None,
                    Box::new(cx.listener(|this, _, _, cx| {
                        this.remember = !this.remember;
                        cx.notify();
                    })),
                    &tokens,
                ))
                .child(modal_note(t!("vault.setup.remember_note"), &tokens))
                .child(modal_note(t!("vault.setup.forget_note"), &tokens))
            });
        let footer = self.render_footer(&tokens, window, cx);
        render_modal(None, body, footer, &tokens, window, cx)
            .id("vault-password-dialog")
            .test_support()
            .aria_label(title)
            .track_focus(&self.focus_handle)
    }
}

/// 叠一个主密码框。做成了回答 `true`（第一次保存时，「不设置」也算做成了），取消回答 `false`。
pub(super) fn open_password_dialog(
    vault: Entity<VaultStore>,
    mode: PasswordMode,
    reply: Reply,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| PasswordDialog::new(vault, mode, reply.clone(), window, cx));
    let first = view.read(cx).first_field().clone();
    let focus = view.read(cx).focus_handle.clone();
    open_small_dialog(view, reply, focus, window, cx);
    first.update(cx, |field, cx| field.focus(window, cx));
}

/// 设置 › 安全「设置主密码…」「更换主密码…」：没设主密码时设，设了就换。打不开的保险库
/// （另一台电脑上建的）要先重置，这里什么都不做。
pub fn open_master_password_dialog(vault: Entity<VaultStore>, window: &mut Window, cx: &mut App) {
    let mode = match vault.read(cx).status() {
        Some(VaultStatus::Absent | VaultStatus::Unprotected) => PasswordMode::Set,
        Some(VaultStatus::Locked | VaultStatus::Unlocked { .. }) => PasswordMode::Change,
        Some(VaultStatus::KeyLost) | None => return,
    };
    open_password_dialog(vault, mode, Reply::ignored(), window, cx);
}
