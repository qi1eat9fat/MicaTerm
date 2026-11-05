//! 设置 › 安全（工单 13，原型「设置 › 安全」）：保险库一组（设置或更换主密码、在本机记住、立即锁定），
//! 危险操作一组（重置保险库，要确认）。没设主密码时只有「设置主密码…」和重置：没有锁定这回事。
//!
//! 按钮派发保险库的 action（由窗口壳交给 `crate::vault`）；「在本机记住」开关直接改保险库。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use super::controls::{group, row, section_title};
use super::dialog::SettingsDialog;
use crate::app::{LockVault, ResetVault, SetMasterPassword, UnlockVault};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::vault::{VaultStatus, VaultStore};

/// 点了派发 `action` 的按钮。
fn action_button(id: &'static str, label: SharedString, action: impl Action + Clone) -> Button {
    Button::new(id)
        .label(label)
        .outline()
        .small()
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}

pub(super) fn security(
    _: &SettingsDialog,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let Some(vault) = VaultStore::global(cx) else {
        return vec![no_vault(tokens)];
    };
    let store = vault.read(cx);
    let Some(status) = store.status() else {
        return vec![no_vault(tokens)];
    };
    let busy = store.is_busy();

    let mut rows = Vec::new();
    match status {
        VaultStatus::Absent | VaultStatus::Unprotected => rows.push(row(
            t!("settings.security.master"),
            Some(t!("settings.security.master_unset")),
            action_button(
                "security-set-master",
                t!("settings.security.set_master"),
                SetMasterPassword,
            )
            .disabled(busy),
            tokens,
        )),
        VaultStatus::KeyLost => rows.push(row(
            t!("settings.security.master"),
            Some(t!("settings.security.master_lost")),
            div(),
            tokens,
        )),
        VaultStatus::Locked | VaultStatus::Unlocked { .. } => {
            let remembered = matches!(status, VaultStatus::Unlocked { remembered: true });
            let locked = status == VaultStatus::Locked;
            rows.push(row(
                t!("settings.security.master"),
                Some(t!("settings.security.master_set")),
                action_button(
                    "security-change-master",
                    t!("settings.security.change_master"),
                    SetMasterPassword,
                )
                .disabled(busy),
                tokens,
            ));
            let remember = {
                let vault = vault.clone();
                Switch::new("security-remember")
                    .checked(remembered)
                    // 锁着时不知道数据密钥，存不进钥匙串：先解锁。
                    .disabled(busy || locked)
                    .accessibility_label(t!("settings.security.remember"))
                    .on_click(move |checked: &bool, _, cx| {
                        let checked = *checked;
                        vault.update(cx, |vault, cx| vault.remember_here(checked, cx));
                    })
            };
            rows.push(row(
                t!("settings.security.remember"),
                Some(t!("settings.security.remember_note")),
                remember,
                tokens,
            ));
            let lock = if locked {
                action_button(
                    "security-unlock",
                    t!("settings.security.unlock"),
                    UnlockVault,
                )
            } else {
                action_button("security-lock", t!("settings.security.lock_now"), LockVault)
            };
            rows.push(row(
                t!("settings.security.lock"),
                Some(t!("settings.security.lock_note")),
                lock.disabled(busy),
                tokens,
            ));
        }
    }

    let reset = Button::new("security-reset")
        .label(t!("settings.security.reset_button"))
        .outline()
        .danger()
        .small()
        .disabled(busy || status == VaultStatus::Absent)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ResetVault), cx));

    vec![
        section_title(t!("settings.security.vault"), None, tokens).into_any_element(),
        group(rows, tokens).into_any_element(),
        section_title(t!("settings.security.danger"), None, tokens).into_any_element(),
        group(
            [row(
                t!("settings.security.reset"),
                Some(t!("settings.security.reset_note")),
                reset,
                tokens,
            )],
            tokens,
        )
        .into_any_element(),
    ]
}

/// 这次运行没有保险库（打不开）：只说一句。
fn no_vault(tokens: &Tokens) -> AnyElement {
    div()
        .id("security-no-vault")
        .test_support()
        .aria_label(t!("settings.security.no_vault"))
        .text_size(px(12.5))
        .text_color(tokens.muted)
        .child(t!("settings.security.no_vault"))
        .into_any_element()
}
