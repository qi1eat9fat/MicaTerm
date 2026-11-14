//! 用到保险库之前要弹哪个框（spec §4「保险库」）。
//!
//! - 写秘密之前（[`before_saving_secret`]）：还没建保险库就问要不要设主密码（可以「不设置」）；
//!   锁着就先解锁；打不开（另一台电脑上建的）就问要不要重置。用户取消就不写。
//! - 连接用秘密之前（[`before_using_secrets`]）：只有这次登录要用的秘密真在保险库里时才弹：
//!   锁着弹解锁小卡片（每次运行最多一次，取消过就不再自动弹）；打不开就问一次要不要重置。
//!   不管怎么答都接着连接，拿不到的秘密由连接现场询问。
//! - 远程卡片连接之前（[`unlock_before_connecting`]，工单 33）：弹的框一样，只是把「解开了还是取消了」
//!   告诉调用方：取消了卡片就不连，显示未连接。原地重连一张取消过的卡片用 [`asks_before_retrying`]，
//!   这次运行取消过也再弹。

use gpui_kit::*;

use micaterm_hosts::{SecretRef, VaultStatus};

use super::confirm::{Confirm, open_confirm_dialog};
use super::password::{PasswordMode, open_password_dialog};
use super::reply::Reply;
use super::store::VaultStore;
use super::unlock::open_unlock_dialog;
use crate::i18n::t;

/// 写秘密之前要不要先弹框（还没建保险库、锁着、打不开）。为 false 时直接写，不必经
/// [`before_saving_secret`]：调用方在自己的实体里时，同步回调会重入。
pub fn asks_before_saving(cx: &App) -> bool {
    VaultStore::global(cx).is_some_and(|vault| !vault.read(cx).writable())
}

/// 连接用 `secrets` 之前要不要先弹框：秘密真存在保险库里，保险库却锁着（这次运行没取消过）或者
/// 打不开（这次运行没问过）。为 false 时直接连，不必经 [`before_using_secrets`]。
pub fn asks_before_using(secrets: &[SecretRef], cx: &App) -> bool {
    let Some(vault) = VaultStore::global(cx) else {
        return false;
    };
    let vault = vault.read(cx);
    if !secrets.iter().any(|secret| vault.contains(secret)) {
        return false;
    }
    match vault.status() {
        Some(VaultStatus::Locked) => !vault.unlock_declined(),
        Some(VaultStatus::KeyLost) => !vault.key_lost_asked(),
        _ => false,
    }
}

/// 和 [`asks_before_using`] 一样，只是保险库锁着时不管这次运行取消过没有都算要弹：用户在一张
/// 因为没解锁而没连的卡片上重连，就是要再试一次（工单 33）。
pub fn asks_before_retrying(secrets: &[SecretRef], cx: &App) -> bool {
    let Some(vault) = VaultStore::global(cx) else {
        return false;
    };
    let store = vault.read(cx);
    let saved = secrets.iter().any(|secret| store.contains(secret));
    (saved && store.is_locked()) || asks_before_using(secrets, cx)
}

/// 要往保险库里写一条秘密：保险库收得下了再调 `then`，用户取消就不调。没有保险库
/// （秘密在别处）、没设主密码或已解锁时立刻调。
pub fn before_saving_secret(
    window: &mut Window,
    cx: &mut App,
    then: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let Some(vault) = VaultStore::global(cx) else {
        then(window, cx);
        return;
    };
    let status = vault.read(cx).status();
    match status {
        None | Some(VaultStatus::Unprotected | VaultStatus::Unlocked { .. }) => then(window, cx),
        Some(VaultStatus::Absent) => {
            let reply = Reply::new(move |done, window, cx| {
                if done {
                    then(window, cx);
                }
            });
            open_password_dialog(vault, PasswordMode::FirstSave, reply, window, cx);
        }
        Some(VaultStatus::Locked) => open_unlock_dialog(
            vault,
            t!("vault.unlock.for_saving"),
            move |unlocked, window, cx| {
                if unlocked {
                    then(window, cx);
                }
            },
            window,
            cx,
        ),
        Some(VaultStatus::KeyLost) => {
            // 重置以后保险库是「还没建」：接着问要不要设主密码。
            let reply = Reply::new(move |reset, window, cx| {
                if reset {
                    before_saving_secret(window, cx, then);
                }
            });
            open_confirm_dialog(vault, Confirm::KeyLostSaving, reply, window, cx);
        }
    }
}

/// 连接主机 `host` 要用 `secrets`：要解锁就先弹解锁小卡片，打不开就问一次要不要重置；
/// 弹完（不管怎么答）或者不用弹，都调 `then`。无密码、Agent 登录的 `secrets` 是空的，不弹。
pub fn before_using_secrets(
    host: &str,
    secrets: &[SecretRef],
    window: &mut Window,
    cx: &mut App,
    then: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    if !asks_before_using(secrets, cx) {
        then(window, cx);
        return;
    }
    unlock_before_connecting(host, secrets, window, cx, move |_, window, cx| {
        then(window, cx)
    });
}

/// 连接主机 `host` 要用 `secrets`（工单 33）：保险库锁着就弹解锁小卡片（这次运行取消过也弹），
/// 解开了 `answer(true)`，取消了 `answer(false)`（记下取消过，之后的新连接不再自动弹）；
/// 打不开就问一次要不要重置，不管怎么答都 `answer(true)`（拿不到的秘密由连接现场询问）。
/// 不用弹（[`asks_before_retrying`] 为 false）时立刻 `answer(true)`：调用方在自己的实体里时
/// 先用它判断，同步回调会重入。
pub fn unlock_before_connecting(
    host: &str,
    secrets: &[SecretRef],
    window: &mut Window,
    cx: &mut App,
    answer: impl FnOnce(bool, &mut Window, &mut App) + 'static,
) {
    let Some(vault) = VaultStore::global(cx) else {
        answer(true, window, cx);
        return;
    };
    if !asks_before_retrying(secrets, cx) {
        answer(true, window, cx);
        return;
    }
    let store = vault.read(cx);
    let saved = secrets.iter().find(|secret| store.contains(secret));
    match store.status() {
        Some(VaultStatus::Locked) => {
            let secret = saved.map(kind_of).unwrap_or_default();
            let reason = t!("vault.unlock.for_host", host = host, secret = secret);
            let declined = vault.clone();
            open_unlock_dialog(
                vault,
                reason,
                move |unlocked, window, cx| {
                    if !unlocked {
                        declined.update(cx, |vault, _| vault.decline_unlock());
                    }
                    answer(unlocked, window, cx);
                },
                window,
                cx,
            );
        }
        Some(VaultStatus::KeyLost) => {
            vault.update(cx, |vault, _| vault.ask_about_lost_key());
            let reply = Reply::new(move |_, window, cx| answer(true, window, cx));
            open_confirm_dialog(vault, Confirm::KeyLostConnecting, reply, window, cx);
        }
        _ => answer(true, window, cx),
    }
}

/// 解锁卡片上说要用的是哪种秘密。
fn kind_of(secret: &SecretRef) -> SharedString {
    match secret {
        SecretRef::Password { .. } => t!("vault.unlock.secret.password"),
        SecretRef::Proxy { .. } => t!("vault.unlock.secret.proxy"),
        SecretRef::Credential { .. }
        | SecretRef::PrivateKey { .. }
        | SecretRef::Passphrase { .. } => t!("vault.unlock.secret.credential"),
    }
}
