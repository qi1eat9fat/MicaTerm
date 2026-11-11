//! 连接前取秘密的钩子：这次登录要用的秘密拿得到了，再连接。
//!
//! 登录要用的秘密（密码、私钥和口令、跳板和代理的密码，见 `vault::login_secrets`）在保险库里存着
//! 而保险库锁着时：
//!
//! - **远程卡片**（工单 33，新标签、分屏、原地重连都一样）：
//!   1. 卡片照常建出来、放进标签、拿到焦点，但先不连：终端是占位的，正文写「正在连接 …」，状态栏写
//!      「等待解锁保险库」（`TerminalCard::remote_waiting_for_vault`，原地重连是
//!      `TerminalCard::wait_for_vault`）；
//!   2. 卡片排进 `Workspace::vault_waiters`（[`Workspace::wait_for_vault`]）。第一张排进来时，等这次
//!      更新结束（卡片已经放好、拿到焦点）再弹解锁框，焦点落在主密码框；框开着时再排进来的卡片
//!      共用这一个框；
//!   3. 用户答了（[`Workspace::vault_answered`]）：解开了，等着的卡片各自在原地接着连
//!      （`TerminalCard::reconnect`，连接表的问答照常弹）；取消了，卡片显示未连接，原因是
//!      「保险库没有解锁」（`TerminalCard::vault_declined`），Enter 原地重连会再弹解锁框；
//!   4. 保险库是另一台电脑上建的、这里打不开时，问一次要不要重置，不管怎么答都接着连（现场询问密码）。
//! - **SFTP 标签**（工单 20，[`Workspace::with_new_login_secrets`]）：先弹解锁框，解开了或者取消了
//!   都接着开（取消了就现场询问密码，这次运行不再自动弹）。
//!
//! 无密码和 Agent 登录不弹；复用已经连着（或正在连）的连接时不登录，也不弹（M2 review S2）。
//! 连接经 `SecretStore`（`WorkspaceServices.secrets`，就是保险库）读秘密，读不出时连接现场询问。

use gpui_kit::*;

use crate::backend::RemoteTarget;
use crate::card::{CardId, VaultHold};
use crate::host::HostId;
use crate::vault::{
    asks_before_retrying, asks_before_using, before_using_secrets, login_secrets,
    unlock_before_connecting,
};
use crate::workspace::Workspace;

impl Workspace {
    /// 远程卡片连 `target` 之前要不要先等保险库：复用已有的连接时不登录、用不到秘密，不等。
    /// `retry` 是在一张取消过解锁的卡片上原地重连：这次运行取消过也再问。
    pub(in crate::workspace) fn waits_for_vault(
        &self,
        target: &RemoteTarget,
        retry: bool,
        cx: &App,
    ) -> bool {
        if self.services.remote_terminals.reuses_connection(target) {
            return false;
        }
        let secrets = login_secrets(&target.login);
        if retry {
            asks_before_retrying(&secrets, cx)
        } else {
            asks_before_using(&secrets, cx)
        }
    }

    /// 卡片 `card` 在等保险库（卡片已经是「等待解锁保险库」）：排进等待表。第一张排进来时，等这次
    /// 更新结束（卡片放好、拿到焦点以后）再弹解锁框；框已经开着就共用它。
    pub(in crate::workspace) fn wait_for_vault(
        &mut self,
        card: CardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vault_waiters.contains(&card) {
            return;
        }
        self.vault_waiters.push(card);
        if self.vault_waiters.len() == 1 {
            cx.defer_in(window, |this, window, cx| this.ask_to_unlock(window, cx));
        }
    }

    /// 弹解锁框：说的是第一张还在等的卡片的主机和秘密。
    fn ask_to_unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let waiting = self.vault_waiters.iter().find_map(|card| {
            let card = self.card(*card, cx)?;
            let card = card.read(cx);
            (card.vault_hold() == Some(VaultHold::Waiting))
                .then(|| {
                    Some((
                        card.remote_host_snapshot()?.name.clone(),
                        card.remote_target()?.clone(),
                    ))
                })
                .flatten()
        });
        let Some((name, target)) = waiting else {
            // 等着的卡片都关了（或者断开了）：没什么要问的。
            self.vault_waiters.clear();
            return;
        };
        let secrets = login_secrets(&target.login);
        // 等的这会儿保险库已经解开了（比如从凭据面板手动解锁）：直接连。同步回调会重入正在更新的窗口壳。
        if !asks_before_retrying(&secrets, cx) {
            self.vault_answered(true, window, cx);
            return;
        }
        let this = cx.entity().downgrade();
        unlock_before_connecting(&name, &secrets, window, cx, move |unlocked, window, cx| {
            this.update(cx, |this, cx| this.vault_answered(unlocked, window, cx))
                .ok();
        });
    }

    /// 用户答了解锁框（工单 33）：解开了（`unlocked`），等着的卡片各自在原地接着连；取消了，
    /// 它们显示未连接、可以原地重连。关掉了、断开了的卡片不管。焦点不在这里动：解锁框开着时新开的
    /// 卡片等框关了才拿焦点，连接接着弹的问答框也不被抢（工单 32，`ui::focus_after_dialogs`）。
    pub(in crate::workspace) fn vault_answered(
        &mut self,
        unlocked: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for card in std::mem::take(&mut self.vault_waiters) {
            let Some(entity) = self.card(card, cx) else {
                continue;
            };
            let (host, target) = {
                let card = entity.read(cx);
                if card.vault_hold() != Some(VaultHold::Waiting) {
                    continue;
                }
                let (Some(host), Some(target)) =
                    (card.remote_host_snapshot(), card.remote_target())
                else {
                    continue;
                };
                (host.clone(), target.clone())
            };
            if unlocked {
                let factory = self
                    .services
                    .remote_terminals
                    .factory_for(super::terminal_of(card), &target);
                entity.update(cx, |card, cx| card.reconnect(host, target, factory, cx));
            } else {
                entity.update(cx, |card, cx| card.vault_declined(cx));
            }
        }
    }

    /// 给一定会新登录一次的连接用（SFTP 标签有自己的连接，工单 20）：不看连接表里有没有能复用的，
    /// 用不到的秘密不问，用得到又锁着就先弹解锁框。
    pub(in crate::workspace) fn with_new_login_secrets(
        &mut self,
        target: &RemoteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let secrets = login_secrets(&target.login);
        // 不用弹框就直接连：同步回调会重入正在更新的窗口壳。
        if !asks_before_using(&secrets, cx) {
            then(self, window, cx);
            return;
        }
        let name = self
            .hosts
            .read(cx)
            .host(HostId(target.host))
            .map(|host| host.name.to_string())
            .unwrap_or_else(|| target.login.host.clone());
        let this = cx.entity().downgrade();
        before_using_secrets(&name, &secrets, window, cx, move |window, cx| {
            this.update(cx, |this, cx| then(this, window, cx)).ok();
        });
    }
}
