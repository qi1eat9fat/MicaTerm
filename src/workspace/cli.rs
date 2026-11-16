//! 窗口壳和外部 CLI（Agent CLI 工单 01、02、04、06）：把「启用外部 Agent」的开关、主机和凭据的快照
//! 交给在听的服务；服务说有请求因为保险库锁着没做时，提醒用户解锁；服务排着的主机、凭据改动取来
//! 做掉（`cli_changes.rs`）；交给 Agent 的卡片定时列过去，排着的 `exec --card` 交给卡片
//! （`cli_cards.rs`）。
//! 服务的请求线程碰不到实体，所以主机或保险库一变，就在 UI 线程上重算一份快照交过去
//! （保险库变了，「存没存密码」「锁没锁」跟着变）；反方向由这里定时去问。

use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use super::Workspace;
use crate::app::UnlockVault;
use crate::cli::{ChangeReply, CliError, Reply};
use crate::host::HostStore;
use crate::i18n::t;
use crate::vault::VaultStore;

/// 多久问一次服务有没有要做的事。
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// 「Agent 要用保险库」的提醒：同一时间只留一条。
struct VaultWantedNotice;

impl Workspace {
    /// 开始跟着设置、主机和保险库更新外部 CLI。没在听（另一个 MicaTerm 在听、测试）时什么也不做。
    pub(super) fn watch_cli(
        &self,
        hosts: &Entity<HostStore>,
        vault: &Entity<VaultStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        if self.services.cli.server().is_none() {
            return Vec::new();
        }
        self.sync_cli_enabled(cx);
        self.sync_cli_hosts(cx);
        // 改动一次一个：这一个回完话才取下一个，服务的快照那时已经是改完的样子。
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let taken = this.update_in(cx, |this, window, cx| this.poll_cli(window, cx));
                match taken {
                    Ok(Some((outcome, reply))) => reply.send(outcome.await),
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .detach();
        vec![
            cx.observe(&self.settings, |this, _, cx| this.sync_cli_enabled(cx)),
            cx.observe(hosts, |this, _, cx| this.sync_cli_hosts(cx)),
            cx.observe(vault, |this, _, cx| this.sync_cli_hosts(cx)),
        ]
    }

    fn sync_cli_enabled(&self, cx: &App) {
        if let Some(server) = self.services.cli.server() {
            server.set_enabled(self.settings.read(cx).settings().cli.enabled);
        }
    }

    fn sync_cli_hosts(&self, cx: &App) {
        let Some(server) = self.services.cli.server() else {
            return;
        };
        let vault = self.vault.read(cx);
        let data = self.hosts.read(cx);
        let saved = |secret: &micaterm_hosts::SecretRef| vault.contains(secret);
        server.set_targets(crate::cli::targets(data, saved, vault.is_locked()));
        server.set_credentials(crate::cli::credentials(data, saved));
    }

    /// 看看服务那边有什么：被保险库挡回去的请求，和排着的改动（取一个）。
    fn poll_cli(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Task<Result<Reply, CliError>>, ChangeReply)> {
        let server = self.services.cli.server()?.clone();
        if server.take_vault_wanted() && self.vault.read(cx).is_locked() {
            self.remind_to_unlock(window, cx);
        }
        self.take_cli_runs(cx);
        self.sync_cli_cards(cx);
        // Agent 的状态 hook（工单 09）。
        while let Some((card, event, message)) = server.take_hook() {
            self.apply_hook(&card, event, message, window, cx);
        }
        let (change, reply) = server.take_change()?;
        Some((self.apply_cli_change(change, window, cx), reply))
    }

    /// 有 Agent 的命令因为保险库锁着没做：弹一条不自动消失的提醒，点它解锁。不弹解锁框：
    /// 后台的 Agent 不该抢焦点（规格「保险库」一节）。
    pub(super) fn remind_to_unlock(&self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.focus_handle.clone();
        window.push_notification(
            Notification::warning(t!("workspace.cli.vault_wanted"))
                .id::<VaultWantedNotice>()
                .autohide(false)
                .on_click(move |_, window, cx| target.dispatch_action(&UnlockVault, window, cx)),
            cx,
        );
    }
}
