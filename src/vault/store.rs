//! [`VaultStore`]：保险库的 GPUI 外壳。窗口壳建一个、装成全局量（[`VaultStore::global`]），
//! 主机对话框、设置 › 安全、主机栏都读它、观察它。

use std::sync::Arc;

use anyhow::Result;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Task};
use zeroize::Zeroizing;

use qishell_hosts::secrets::SecretsLocked;
use qishell_hosts::{SecretRef, Vault, VaultError, VaultStatus};

use crate::i18n::t;

/// 保险库的状态和操作。没有保险库（`vault` 为 `None`）时：秘密只在别处（测试的内存存储，
/// 或生产上保险库打不开时的内存存储），没有主密码、锁定这些，用到秘密之前也什么都不弹。
pub struct VaultStore {
    vault: Option<Arc<Vault>>,
    /// 保险库现在的状态；每次操作跑完刷新。
    status: Option<VaultStatus>,
    /// 这次运行里用户取消过「用到秘密时」的解锁：之后用到秘密不再自动弹（每次运行最多一次），
    /// 手动「解锁…」照样能用。手动锁定后重新算。
    unlock_declined: bool,
    /// 这次运行已经问过「这个保险库在另一台电脑上创建」：不重置就不再问。
    key_lost_asked: bool,
    /// 有一个保险库操作在后台跑。
    busy: bool,
}

/// 除了普通的变更通知以外，保险库要告诉窗口壳的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultStoreEvent {
    /// 一次操作没成（锁定、重置、钥匙串……），已经写成给用户看的话。对话框里的错误（主密码不对）
    /// 写在对话框里，不在这里。
    Failed(SharedString),
}

impl EventEmitter<VaultStoreEvent> for VaultStore {}

struct GlobalVault(Entity<VaultStore>);

impl Global for GlobalVault {}

impl VaultStore {
    pub fn new(vault: Option<Arc<Vault>>) -> Self {
        let status = vault.as_ref().map(|vault| vault.status());
        Self {
            vault,
            status,
            unlock_declined: false,
            key_lost_asked: false,
            busy: false,
        }
    }

    /// 装成全局量：窗口壳建好后调一次。
    pub fn install(store: &Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalVault(store.clone()));
    }

    /// 窗口壳装好的那一个；还没装时是 `None`（单独测对话框时）。
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalVault>()
            .map(|global| global.0.clone())
    }

    /// 保险库的状态；没有保险库时是 `None`。
    pub fn status(&self) -> Option<VaultStatus> {
        self.status
    }

    pub fn has_master_password(&self) -> bool {
        self.status.is_some_and(VaultStatus::has_master_password)
    }

    pub fn is_locked(&self) -> bool {
        self.status == Some(VaultStatus::Locked)
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// 这条秘密在保险库里存过没有。条目名是明文，不用解锁；只碰内存，不读文件。
    pub fn contains(&self, secret: &SecretRef) -> bool {
        self.vault
            .as_ref()
            .is_some_and(|vault| vault.contains(secret))
    }

    /// 现在能不能直接写秘密：没有保险库（秘密在别处）、没设主密码（本机能打开）或已解锁。
    pub fn writable(&self) -> bool {
        match self.status {
            None => true,
            Some(status) => matches!(
                status,
                VaultStatus::Unprotected | VaultStatus::Unlocked { .. }
            ),
        }
    }

    pub(super) fn unlock_declined(&self) -> bool {
        self.unlock_declined
    }

    pub(super) fn decline_unlock(&mut self) {
        self.unlock_declined = true;
    }

    pub(super) fn key_lost_asked(&self) -> bool {
        self.key_lost_asked
    }

    /// 记下这次运行问过「在另一台电脑上创建」了。
    pub(super) fn ask_about_lost_key(&mut self) {
        self.key_lost_asked = true;
    }

    /// 操作跑完，重读状态（只碰内存）。
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = self.vault.as_ref().map(|vault| vault.status());
        cx.notify();
    }

    /// 在后台跑一次保险库操作（保险库的方法都会阻塞），跑完刷新状态。没有保险库时直接失败。
    fn run<R: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&Vault) -> Result<R> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Task<Result<R>> {
        let Some(vault) = self.vault.clone() else {
            return Task::ready(Err(anyhow::anyhow!("there is no vault")));
        };
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { operation(&vault) }).await;
            this.update(cx, |store, cx| {
                store.busy = false;
                store.refresh(cx);
            })
            .ok();
            result
        })
    }

    /// 第一次保存秘密前建保险库：`master` 为 `None` 是「不设置」。
    pub fn set_up(
        &mut self,
        master: Option<Zeroizing<String>>,
        remember: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.run(
            move |vault| vault.set_up(master.as_deref().map(String::as_str), remember),
            cx,
        )
    }

    /// 给没设主密码的保险库补设主密码（设置 › 安全）。
    pub fn set_master_password(
        &mut self,
        password: Zeroizing<String>,
        remember: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.run(
            move |vault| vault.set_master_password(&password, remember),
            cx,
        )
    }

    pub fn change_master_password(
        &mut self,
        current: Zeroizing<String>,
        new: Zeroizing<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.run(
            move |vault| vault.change_master_password(&current, &new),
            cx,
        )
    }

    /// 用主密码解锁。钥匙串改不了时保险库照样已经解锁，失败只报告「没记住」。
    pub fn unlock(
        &mut self,
        password: Zeroizing<String>,
        remember: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.run(move |vault| vault.unlock(&password, remember), cx)
    }

    /// 立即锁定。之后再用到秘密时重新问主密码。
    pub fn lock(&mut self, cx: &mut Context<Self>) {
        if !self.has_master_password() || self.is_locked() {
            return;
        }
        self.unlock_declined = false;
        let task = self.run(|vault| vault.lock(), cx);
        self.report(task, cx);
    }

    /// 设置 › 安全的「在本机记住」。
    pub fn remember_here(&mut self, remember: bool, cx: &mut Context<Self>) {
        let task = self.run(move |vault| vault.remember_here(remember), cx);
        self.report(task, cx);
    }

    /// 重置保险库：清空所有秘密，回到「还没建」。主机和凭据不在这里，不受影响。
    pub fn reset(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.unlock_declined = false;
        self.key_lost_asked = false;
        self.run(|vault| vault.reset(), cx)
    }

    /// 跑完了，没成就发 `Failed`。
    pub(super) fn report(&self, task: Task<Result<()>>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |_, cx| {
                    cx.emit(VaultStoreEvent::Failed(describe(&error)))
                })
                .ok();
            }
        })
        .detach();
    }
}

/// 保险库操作的失败说给用户听。
pub(super) fn describe(error: &anyhow::Error) -> SharedString {
    if let Some(error) = error.downcast_ref::<VaultError>() {
        return match error {
            VaultError::WrongPassword => t!("vault.error.wrong_password"),
            VaultError::KeyLost => t!("host.store.secret.key_lost"),
            VaultError::NotSetUp => t!("host.store.secret.not_set_up"),
            VaultError::Damaged(why) => t!("vault.error.damaged", why = why),
            VaultError::TooNew(format) => t!("vault.error.too_new", format = format),
            other => t!("vault.error.failed", error = other.to_string()),
        };
    }
    if error.downcast_ref::<SecretsLocked>().is_some() {
        return t!("host.store.secret.locked");
    }
    t!("vault.error.failed", error = format!("{error:#}"))
}

/// 主密码不对。
pub(super) fn is_wrong_password(error: &anyhow::Error) -> bool {
    error.downcast_ref::<VaultError>() == Some(&VaultError::WrongPassword)
}
