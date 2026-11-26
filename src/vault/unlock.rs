//! 解锁保险库（26 号「解锁保险库」，共用的 `ui::modal` 弹窗）：锁 +「解锁保险库」、谁要用哪条秘密、
//! 主密码框、「在本机记住」（默认勾选）、「取消 Esc」「解锁 Enter」。主密码不对时框描红，下面写原因。
//! Argon2 在后台跑，跑的时候按钮写「解锁中…」，不卡界面。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::reply::{Reply, close_and_reply, open_small_dialog};
use super::store::{VaultStore, describe};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::theme::space;
use crate::ui::form::{FieldErrors, FieldNote, typed_secret};
use crate::ui::modal::{
    ModalButtonTone, ModalInputKind, OnClick, modal_button, modal_checkbox, modal_input,
    modal_question, modal_warn, render_modal,
};

pub const FIELD_UNLOCK: &str = "vault-unlock-password";

pub struct UnlockDialog {
    vault: Entity<VaultStore>,
    reason: SharedString,
    password: Entity<InputState>,
    remember: bool,
    errors: FieldErrors,
    working: Option<Task<()>>,
    reply: Reply,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl UnlockDialog {
    fn new(
        vault: Entity<VaultStore>,
        reason: SharedString,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t!("vault.unlock.placeholder"))
        });
        let subscription = cx.subscribe_in(
            &password,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if this.errors.clear(FIELD_UNLOCK) {
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => this.unlock(window, cx),
                _ => {}
            },
        );
        Self {
            vault,
            reason,
            password,
            remember: true,
            errors: FieldErrors::default(),
            working: None,
            reply,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    fn unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let password = typed_secret(&self.password, cx);
        if password.is_empty() {
            self.errors.set(FIELD_UNLOCK, t!("vault.setup.empty"));
            cx.notify();
            return;
        }
        let remember = self.remember;
        let task = self
            .vault
            .update(cx, |vault, cx| vault.unlock(password, remember, cx));
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.working = None;
                let unlocked = !this.vault.read(cx).is_locked();
                match result {
                    // 钥匙串没记住也算解开了：保险库已经解锁，只是下次还要输。
                    Err(error) if !unlocked => {
                        this.errors.set(FIELD_UNLOCK, describe(&error));
                        cx.notify();
                    }
                    _ => close_and_reply(&this.reply.clone(), true, window, cx),
                }
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Focusable for UnlockDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UnlockDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let working = self.working.is_some();
        let reply = self.reply.clone();
        let invalid = self.errors.get(FIELD_UNLOCK).is_some();
        // 26 号「解锁保险库」：锁 + 「解锁保险库」、谁要用秘密、主密码框、在本机记住。
        let body = div()
            .flex()
            .flex_col()
            .gap(space::SPACE_8)
            .child(modal_warn(
                CatalogIcon::Lock,
                t!("vault.unlock.title"),
                false,
                &tokens,
                cx,
            ))
            .child(
                modal_question(self.reason.clone(), &tokens)
                    .id("vault-unlock-reason")
                    .test_support()
                    .aria_label(self.reason.clone()),
            )
            .child(modal_input(
                FIELD_UNLOCK,
                t!("vault.unlock.placeholder"),
                &self.password,
                ModalInputKind::Secret,
                invalid,
                &tokens,
                window,
                cx,
            ))
            .children(self.errors.get(FIELD_UNLOCK).map(|message| {
                // 元素 id 由公共件按字段名生成：`vault-unlock-password-error`。
                FieldNote::Error {
                    field: FIELD_UNLOCK,
                    message: message.clone(),
                }
                .render(&tokens)
            }))
            .child(modal_checkbox(
                "vault-unlock-remember",
                self.remember,
                t!("vault.setup.remember"),
                Some(t!("vault.unlock.remember_note")),
                Box::new(cx.listener(|this, _, _, cx| {
                    this.remember = !this.remember;
                    cx.notify();
                })),
                &tokens,
            ));
        let footer = div()
            .flex()
            .gap(space::SPACE_6)
            .child(modal_button(
                "vault-unlock-cancel",
                t!("vault.unlock.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                (!working).then(|| -> OnClick {
                    Box::new(move |_, window, cx| close_and_reply(&reply, false, window, cx))
                }),
                &tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "vault-unlock-confirm",
                if working {
                    t!("vault.unlock.working")
                } else {
                    t!("vault.unlock.confirm")
                },
                Some("Enter"),
                ModalButtonTone::Primary,
                (!working).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.unlock(window, cx)))
                }),
                &tokens,
                window,
                cx,
            ));
        render_modal(None, body, footer, &tokens, window, cx)
            .id("vault-unlock")
            .test_support()
            .track_focus(&self.focus_handle)
    }
}

/// 叠一张解锁小卡片。`reason` 说谁要用秘密。解开了回答 `true`，取消（或 Esc）回答 `false`。
/// 保险库没锁着时直接回答 `true`。
pub fn open_unlock_dialog(
    vault: Entity<VaultStore>,
    reason: SharedString,
    answer: impl FnOnce(bool, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    if !vault.read(cx).is_locked() {
        answer(true, window, cx);
        return;
    }
    let reply = Reply::new(answer);
    let view = cx.new(|cx| UnlockDialog::new(vault, reason, reply.clone(), window, cx));
    let password = view.read(cx).password.clone();
    let focus = view.read(cx).focus_handle.clone();
    open_small_dialog(view, reply, focus, window, cx);
    password.update(cx, |field, cx| field.focus(window, cx));
}
