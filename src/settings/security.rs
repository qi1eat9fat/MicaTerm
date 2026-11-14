//! 设置 › 安全（26 号「设置 › 安全」）：保险库（设置或更换主密码、在本机记住、立即锁定），
//! 危险操作（重置保险库，要确认），页底写保险库文件和加密方式。没设主密码时只有「设置主密码…」和重置：
//! 没有锁定这回事。
//!
//! 按钮派发保险库的 action（由窗口壳交给 `crate::vault`）；「在本机记住」开关直接改保险库。

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use super::controls::{page_note, row, section};
use super::tab::SettingsTab;
use crate::app::{LockVault, ResetVault, SetMasterPassword, UnlockVault};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::modal::{ModalButtonTone, OnClick, modal_button};
use crate::vault::{VaultStatus, VaultStore};

/// 点了派发 `action` 的按钮（26 号 `.hbtn.line`；危险操作是 `.hbtn.danger`）；`enabled` 为假时淡掉。
#[allow(clippy::too_many_arguments)]
fn action_button(
    id: &'static str,
    label: SharedString,
    action: Box<dyn Action>,
    tone: ModalButtonTone,
    enabled: bool,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let on_click = enabled.then(|| {
        Box::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            window.dispatch_action(action.boxed_clone(), cx)
        }) as OnClick
    });
    modal_button(id, label, None, tone, on_click, tokens, window, cx).into_any_element()
}

pub(super) fn security(
    _: &SettingsTab,
    tokens: &Tokens,
    window: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let Some(vault) = VaultStore::global(cx) else {
        return vec![no_vault(tokens)];
    };
    let store = vault.read(cx);
    let Some(status) = store.status() else {
        return vec![no_vault(tokens)];
    };
    let busy = store.is_busy();

    let mut page = vec![section(t!("settings.security.vault"), None, true, tokens)];
    match status {
        VaultStatus::Absent | VaultStatus::Unprotected => page.push(row(
            t!("settings.security.master"),
            Some(t!("settings.security.master_unset")),
            None,
            action_button(
                "security-set-master",
                t!("settings.security.set_master"),
                Box::new(SetMasterPassword),
                ModalButtonTone::Line,
                !busy,
                tokens,
                window,
                cx,
            ),
            tokens,
        )),
        VaultStatus::KeyLost => page.push(row(
            t!("settings.security.master"),
            Some(t!("settings.security.master_lost")),
            None,
            div(),
            tokens,
        )),
        VaultStatus::Locked | VaultStatus::Unlocked { .. } => {
            let remembered = matches!(status, VaultStatus::Unlocked { remembered: true });
            let locked = status == VaultStatus::Locked;
            page.push(row(
                t!("settings.security.master"),
                Some(t!("settings.security.master_set")),
                None,
                action_button(
                    "security-change-master",
                    t!("settings.security.change_master"),
                    Box::new(SetMasterPassword),
                    ModalButtonTone::Line,
                    !busy,
                    tokens,
                    window,
                    cx,
                ),
                tokens,
            ));
            let remember = {
                let vault = vault.clone();
                Switch::new("security-remember")
                    .small()
                    .checked(remembered)
                    // 锁着时不知道数据密钥，存不进钥匙串：先解锁。
                    .disabled(busy || locked)
                    .accessibility_label(t!("settings.security.remember"))
                    .on_click(move |checked: &bool, _, cx| {
                        let checked = *checked;
                        vault.update(cx, |vault, cx| vault.remember_here(checked, cx));
                    })
            };
            page.push(row(
                t!("settings.security.remember"),
                Some(t!("settings.security.remember_note")),
                None,
                remember,
                tokens,
            ));
            let lock = if locked {
                action_button(
                    "security-unlock",
                    t!("settings.security.unlock"),
                    Box::new(UnlockVault),
                    ModalButtonTone::Line,
                    !busy,
                    tokens,
                    window,
                    cx,
                )
            } else {
                action_button(
                    "security-lock",
                    t!("settings.security.lock_now"),
                    Box::new(LockVault),
                    ModalButtonTone::Line,
                    !busy,
                    tokens,
                    window,
                    cx,
                )
            };
            page.push(row(
                t!("settings.security.lock"),
                Some(t!("settings.security.lock_note")),
                None,
                lock,
                tokens,
            ));
        }
    }

    let reset = action_button(
        "security-reset",
        t!("settings.security.reset_button"),
        Box::new(ResetVault),
        ModalButtonTone::Danger,
        !busy && status != VaultStatus::Absent,
        tokens,
        window,
        cx,
    );
    page.push(section(t!("settings.security.danger"), None, false, tokens));
    page.push(row(
        t!("settings.security.reset"),
        Some(t!("settings.security.reset_note")),
        None,
        reset,
        tokens,
    ));
    page.push(page_note(t!("settings.security.vault_file"), tokens));
    page
}

/// 这次运行没有保险库（打不开）：只说一句。
fn no_vault(tokens: &Tokens) -> AnyElement {
    div()
        .id("security-no-vault")
        .test_support()
        .aria_label(t!("settings.security.no_vault"))
        .text_color(tokens.st_dim)
        .child(t!("settings.security.no_vault"))
        .into_any_element()
}
