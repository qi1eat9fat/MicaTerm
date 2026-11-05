//! 保险库的处理器（工单 13）：锁定、解锁、设置或更换主密码、重置。界面在 `crate::vault`。

use gpui_kit::*;

use crate::app::{LockVault, ResetVault, SetMasterPassword, UnlockVault};
use crate::i18n::t;
use crate::vault::{open_master_password_dialog, open_reset_dialog, open_unlock_dialog};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_vault_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_lock_vault))
        .on_action(cx.listener(Workspace::on_unlock_vault))
        .on_action(cx.listener(Workspace::on_set_master_password))
        .on_action(cx.listener(Workspace::on_reset_vault))
}

impl Workspace {
    /// 立即锁定（主机栏底部按钮、设置 › 安全）。没设主密码、已经锁着时什么都不做。
    fn on_lock_vault(&mut self, _: &LockVault, _: &mut Window, cx: &mut Context<Self>) {
        self.vault.update(cx, |vault, cx| vault.lock(cx));
    }

    /// 弹解锁小卡片（「解锁…」）。没锁着时什么都不做。
    fn on_unlock_vault(&mut self, _: &UnlockVault, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vault.read(cx).is_locked() {
            return;
        }
        open_unlock_dialog(
            self.vault.clone(),
            t!("vault.unlock.reason"),
            |_, _, _| {},
            window,
            cx,
        );
    }

    /// 设置或更换主密码（设置 › 安全）。
    fn on_set_master_password(
        &mut self,
        _: &SetMasterPassword,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        open_master_password_dialog(self.vault.clone(), window, cx);
    }

    /// 重置保险库，先确认（设置 › 安全）。
    fn on_reset_vault(&mut self, _: &ResetVault, window: &mut Window, cx: &mut Context<Self>) {
        open_reset_dialog(self.vault.clone(), window, cx);
    }
}
