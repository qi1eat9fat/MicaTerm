//! 两个确认框：重置保险库（设置 › 安全，原型「重置保险库确认」），和「这个保险库在另一台电脑上
//! 创建、没设主密码，这里打不开」（用户 2026-11-04 决定：确认后重置，不重置就现场询问密码）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use super::reply::{Reply, close_and_reply, open_small_dialog};
use super::store::{VaultStore, VaultStoreEvent, describe};
use crate::i18n::t;
use crate::theme::Tokens;

const WIDTH: Pixels = px(420.);

/// 确认什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    /// 设置 › 安全「重置保险库…」。
    Reset,
    /// 连接要用秘密，保险库却打不开：重置，或这次现场输入密码。
    KeyLostConnecting,
    /// 要保存秘密，保险库却打不开：重置才能保存，或者取消。
    KeyLostSaving,
}

impl Confirm {
    fn id(self) -> &'static str {
        match self {
            Confirm::Reset => "vault-reset",
            Confirm::KeyLostConnecting | Confirm::KeyLostSaving => "vault-key-lost",
        }
    }
}

pub struct ConfirmDialog {
    vault: Entity<VaultStore>,
    kind: Confirm,
    working: Option<Task<()>>,
    reply: Reply,
    focus_handle: FocusHandle,
}

impl ConfirmDialog {
    /// 重置，跑完关掉确认框、回答 `true`；没成也关掉，并告诉用户为什么。
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let task = self.vault.update(cx, |vault, cx| vault.reset(cx));
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.working = None;
                if let Err(error) = &result {
                    let message = describe(error);
                    this.vault
                        .update(cx, |_, cx| cx.emit(VaultStoreEvent::Failed(message)));
                }
                // 钥匙串里的旧密钥删不掉时，保险库照样已经重置。
                let reset =
                    this.vault.read(cx).status() == Some(qishell_hosts::VaultStatus::Absent);
                close_and_reply(&this.reply.clone(), reset, window, cx);
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Focusable for ConfirmDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfirmDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let id = self.kind.id();
        let (title, text, cancel) = match self.kind {
            Confirm::Reset => (
                t!("vault.reset.title"),
                t!("vault.reset.text"),
                t!("vault.reset.cancel"),
            ),
            Confirm::KeyLostConnecting => (
                t!("vault.key_lost.title"),
                t!("vault.key_lost.text"),
                t!("vault.key_lost.keep"),
            ),
            Confirm::KeyLostSaving => (
                t!("vault.key_lost.title"),
                t!("vault.key_lost.save_text"),
                t!("vault.key_lost.cancel"),
            ),
        };
        let confirm = match self.kind {
            Confirm::Reset => t!("vault.reset.confirm"),
            _ => t!("vault.key_lost.reset"),
        };
        let working = self.working.is_some();
        let reply = self.reply.clone();
        div()
            .id(id)
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
                    .child(title),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-text")))
                    .test_support()
                    .aria_label(text.clone())
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .child(text),
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
                        Button::new(SharedString::from(format!("{id}-cancel")))
                            .small()
                            .label(cancel)
                            .disabled(working)
                            .on_click(move |_, window, cx| {
                                close_and_reply(&reply, false, window, cx)
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{id}-confirm")))
                            .danger()
                            .small()
                            .label(confirm)
                            .disabled(working)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    ),
            )
    }
}

/// 叠一个确认框；重置了回答 `true`，没重置回答 `false`。
pub(super) fn open_confirm_dialog(
    vault: Entity<VaultStore>,
    kind: Confirm,
    reply: Reply,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| ConfirmDialog {
        vault,
        kind,
        working: None,
        reply: reply.clone(),
        focus_handle: cx.focus_handle(),
    });
    let focus = view.read(cx).focus_handle.clone();
    open_small_dialog(view, WIDTH, reply, focus, window, cx);
}

/// 设置 › 安全「重置保险库…」：先确认。还没建保险库时什么都不做。
pub fn open_reset_dialog(vault: Entity<VaultStore>, window: &mut Window, cx: &mut App) {
    match vault.read(cx).status() {
        None | Some(qishell_hosts::VaultStatus::Absent) => {}
        Some(_) => open_confirm_dialog(vault, Confirm::Reset, Reply::ignored(), window, cx),
    }
}
