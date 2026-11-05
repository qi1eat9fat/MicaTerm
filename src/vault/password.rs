//! 设置主密码框和更换主密码框（原型「第一次保存秘密：设置主密码」）。
//!
//! - 第一次保存秘密（[`PasswordMode::FirstSave`]）：主密码、再输一次、「在本机记住」（默认勾选），
//!   底部左边「不设置」（秘密照样存，只靠本机钥匙串），右边「取消」「设置并保存」；
//! - 设置 › 安全「设置主密码…」（[`PasswordMode::Set`]）：没有「不设置」；
//! - 设置 › 安全「更换主密码…」（[`PasswordMode::Change`]）：当前主密码、新主密码、再输一次。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::VaultStatus;

use super::reply::{Reply, close_and_reply, open_small_dialog};
use super::store::{VaultStore, describe, is_wrong_password};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, FieldNote, form_field, text_input, typed_secret};

pub const FIELD_CURRENT: &str = "vault-current-password";
pub const FIELD_MASTER: &str = "vault-master-password";
pub const FIELD_AGAIN: &str = "vault-master-password-again";

const WIDTH: Pixels = px(440.);

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
    ) -> AnyElement {
        form_field(
            Some(label.clone()),
            text_input(
                field,
                label,
                input,
                self.errors.get(field).is_some(),
                tokens,
            ),
            FieldNote::error_of(field, &self.errors)
                .into_iter()
                .collect(),
            tokens,
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let working = self.working.is_some();
        let (confirm, confirming) = match self.mode {
            PasswordMode::FirstSave => (t!("vault.setup.save"), t!("vault.setup.working")),
            PasswordMode::Set => (t!("vault.setup.confirm"), t!("vault.setup.working")),
            PasswordMode::Change => (t!("vault.change.confirm"), t!("vault.change.working")),
        };
        let reply = self.reply.clone();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(12.))
            .pb(px(14.))
            .pl(px(18.))
            .pr(px(14.))
            .when(self.mode == PasswordMode::FirstSave, |footer| {
                footer.child(
                    Button::new("vault-password-skip")
                        .small()
                        .label(t!("vault.setup.skip"))
                        .disabled(working)
                        .on_click(cx.listener(|this, _, window, cx| this.skip(window, cx))),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("vault-password-cancel")
                    .small()
                    .label(t!("vault.setup.cancel"))
                    .disabled(working)
                    .on_click(move |_, window, cx| close_and_reply(&reply, false, window, cx)),
            )
            .child(
                Button::new("vault-password-confirm")
                    .primary()
                    .small()
                    .label(if working { confirming } else { confirm })
                    .disabled(working)
                    .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
            )
    }
}

impl Focusable for PasswordDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PasswordDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        div()
            .id("vault-password-dialog")
            .test_support()
            .aria_label(title.clone())
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(46.))
                    .px(px(18.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.))
                            .rounded(px(7.))
                            .bg(tokens.selection)
                            .text_color(tokens.accent)
                            .child(Icon::new(CatalogIcon::Shield).size(px(14.))),
                    )
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pt(px(4.))
                    .child(
                        div()
                            .mb(px(12.))
                            .text_size(px(12.5))
                            .line_height(relative(1.6))
                            .text_color(tokens.muted)
                            .child(intro),
                    )
                    .when(changing, |body| {
                        body.child(self.render_field(
                            FIELD_CURRENT,
                            t!("vault.change.current"),
                            &self.current,
                            &tokens,
                        ))
                    })
                    .child(self.render_field(FIELD_MASTER, master_label, &self.master, &tokens))
                    .child(self.render_field(
                        FIELD_AGAIN,
                        t!("vault.setup.again"),
                        &self.again,
                        &tokens,
                    ))
                    .when(!changing, |body| {
                        body.child(form_field(
                            None,
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    Checkbox::new("vault-remember")
                                        .checked(self.remember)
                                        .label(t!("vault.setup.remember"))
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            this.remember = *checked;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .line_height(relative(1.5))
                                        .text_color(tokens.faint)
                                        .child(t!("vault.setup.remember_note")),
                                ),
                            Vec::new(),
                            &tokens,
                        ))
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(8.))
                                .bg(tokens.field)
                                .text_size(px(11.5))
                                .line_height(relative(1.5))
                                .text_color(tokens.faint)
                                .child(t!("vault.setup.forget_note")),
                        )
                    }),
            )
            .child(self.render_footer(cx))
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
    open_small_dialog(view, WIDTH, reply, focus, window, cx);
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
