//! 两个确认框：重置保险库（设置 › 安全，原型「重置保险库确认」），和「这个保险库在另一台电脑上
//! 创建、没设主密码，这里打不开」（用户 2026-11-04 决定：确认后重置，不重置就现场询问密码）。
//! 确认框本身是共用的 `ui::confirm`；重置在后台跑，跑完只关自己那一层。

use gpui_kit::*;

use super::reply::Reply;
use super::store::{VaultStore, VaultStoreEvent, describe};
use crate::i18n::t;
use crate::ui::confirm::{self, ConfirmLine};

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
    fn texts(self) -> confirm::Confirm {
        let (id, title, text, cancel, confirm) = match self {
            Confirm::Reset => (
                "vault-reset",
                t!("vault.reset.title"),
                t!("vault.reset.text"),
                t!("vault.reset.cancel"),
                t!("vault.reset.confirm"),
            ),
            Confirm::KeyLostConnecting => (
                "vault-key-lost",
                t!("vault.key_lost.title"),
                t!("vault.key_lost.text"),
                t!("vault.key_lost.keep"),
                t!("vault.key_lost.reset"),
            ),
            Confirm::KeyLostSaving => (
                "vault-key-lost",
                t!("vault.key_lost.title"),
                t!("vault.key_lost.save_text"),
                t!("vault.key_lost.cancel"),
                t!("vault.key_lost.reset"),
            ),
        };
        confirm::Confirm {
            id,
            title,
            lines: vec![ConfirmLine::Text(text)],
            cancel,
            confirm,
        }
    }
}

/// 叠一个确认框；重置了回答 `true`，没重置回答 `false`。重置没成也关掉，并告诉用户为什么。
pub(super) fn open_confirm_dialog(
    vault: Entity<VaultStore>,
    kind: Confirm,
    reply: Reply,
    window: &mut Window,
    cx: &mut App,
) {
    let cancelled = reply.clone();
    confirm::open_confirm_dialog(
        kind.texts(),
        reply.layer().clone(),
        Box::new(move |layer, window, cx| {
            let task = vault.update(cx, |vault, cx| vault.reset(cx));
            let layer = layer.clone();
            Some(window.spawn(cx, async move |cx| {
                let result = task.await;
                let _ = cx.update(|window, cx| {
                    if let Err(error) = &result {
                        let message = describe(error);
                        vault.update(cx, |_, cx| cx.emit(VaultStoreEvent::Failed(message)));
                    }
                    // 钥匙串里的旧密钥删不掉时，保险库照样已经重置。
                    let reset = vault.read(cx).status() == Some(micaterm_hosts::VaultStatus::Absent);
                    layer.close(window, cx);
                    reply.send(reset, window, cx);
                });
            }))
        }),
        Box::new(move |window, cx| cancelled.send(false, window, cx)),
        window,
        cx,
    );
}

/// 设置 › 安全「重置保险库…」：先确认。还没建保险库时什么都不做。
pub fn open_reset_dialog(vault: Entity<VaultStore>, window: &mut Window, cx: &mut App) {
    match vault.read(cx).status() {
        None | Some(micaterm_hosts::VaultStatus::Absent) => {}
        Some(_) => open_confirm_dialog(vault, Confirm::Reset, Reply::ignored(), window, cx),
    }
}
