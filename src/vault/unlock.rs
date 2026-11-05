//! 解锁小卡片（原型「用到秘密时：解锁保险库」）：锁图标、「解锁保险库」、谁要用哪条秘密、
//! 主密码框、「在本机记住」（默认勾选）、「取消」「解锁」。主密码不对时框描红，下面写原因。
//! Argon2 在后台跑，跑的时候按钮写「解锁中…」，不卡界面。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::*;
use zeroize::Zeroizing;

use super::reply::{Reply, close_and_reply, open_small_dialog};
use super::store::{VaultStore, describe};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, text_input};

pub const FIELD_UNLOCK: &str = "vault-unlock-password";

const WIDTH: Pixels = px(340.);

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
        let password = Zeroizing::new(self.password.read(cx).value().to_string());
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let working = self.working.is_some();
        let reply = self.reply.clone();
        let invalid = self.errors.get(FIELD_UNLOCK).is_some();
        div()
            .id("vault-unlock")
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .items_center()
            .px(px(22.))
            .pt(px(22.))
            .pb(px(18.))
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(44.))
                    .mb(px(10.))
                    .rounded(px(13.))
                    .bg(tokens.attention.opacity(0.18))
                    .text_color(tokens.attention)
                    .child(Icon::new(CatalogIcon::Lock).size(px(22.))),
            )
            .child(
                div()
                    .mb(px(4.))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("vault.unlock.title")),
            )
            .child(
                div()
                    .id("vault-unlock-reason")
                    .test_support()
                    .aria_label(self.reason.clone())
                    .mb(px(14.))
                    .text_size(px(12.))
                    .text_color(tokens.muted)
                    .child(self.reason.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .gap(px(3.))
                    .child(text_input(
                        FIELD_UNLOCK,
                        t!("vault.unlock.placeholder"),
                        &self.password,
                        invalid,
                        &tokens,
                    ))
                    .children(self.errors.get(FIELD_UNLOCK).map(|message| {
                        div()
                            .id("vault-unlock-password-error")
                            .test_support()
                            .aria_label(message.clone())
                            .text_size(px(11.5))
                            .text_color(tokens.bad)
                            .child(message.clone())
                    })),
            )
            .child(
                div().w_full().mt(px(10.)).mb(px(14.)).child(
                    Checkbox::new("vault-unlock-remember")
                        .checked(self.remember)
                        .label(t!("vault.setup.remember"))
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.remember = *checked;
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .flex()
                    .w_full()
                    .gap(px(8.))
                    .child(
                        Button::new("vault-unlock-cancel")
                            .small()
                            .flex_1()
                            .label(t!("vault.unlock.cancel"))
                            .disabled(working)
                            .on_click(move |_, window, cx| {
                                close_and_reply(&reply, false, window, cx)
                            }),
                    )
                    .child(
                        Button::new("vault-unlock-confirm")
                            .primary()
                            .small()
                            .flex_1()
                            .label(if working {
                                t!("vault.unlock.working")
                            } else {
                                t!("vault.unlock.confirm")
                            })
                            .disabled(working)
                            .on_click(cx.listener(|this, _, window, cx| this.unlock(window, cx))),
                    ),
            )
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
    open_small_dialog(view, WIDTH, reply, focus, window, cx);
    password.update(cx, |field, cx| field.focus(window, cx));
}
