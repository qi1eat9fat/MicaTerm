//! 「等待解锁保险库」一屏（工单 33）的 UI 测试：连接要用保险库里的秘密而保险库锁着时，先建出远程卡片
//! （卡片里写「正在连接 …」、状态栏写「等待解锁保险库」），同时弹解锁框；解开后在同一张卡片里接着连，
//! 取消则卡片显示未连接、Enter 原地重连会再弹解锁框；复用已有连接时不弹。
//!
//! 保险库是真的（`support/vault.rs`），远程终端是假的。

use std::time::Duration;

use qishell::app::{ConnectHost, ReconnectCard};
use qishell::backend::RemoteTerminalId;
use qishell_hosts::HostId;
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

/// 主机栏的第一台主机 web-01（root@10.0.1.12:22）的密码条目。
fn web01_password() -> SecretRef {
    SecretRef::password(1)
}

/// web-01 存了密码、保险库锁着。
fn locked_vault() -> TestVault {
    TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked()
}

fn open_with_vault(
    cx: &mut TestAppContext,
    vault: &TestVault,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        ..vault.services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote)
}

fn label(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn visible(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> bool {
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
}

fn status(cx: &mut TestAppContext, handle: WindowHandle<Root>, card: u64) -> String {
    label(cx, handle, ("card-status", card as usize)).unwrap_or_default()
}

/// 每个标签里的卡片名。
fn card_names(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<Vec<String>> {
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .map(|tab| {
                workspace
                    .tab_cards(tab)
                    .iter()
                    .map(|card| card.read(cx).name().to_string())
                    .collect()
            })
            .collect()
    })
}

/// 每次真去连接（建传输）的终端和主机，按顺序。
fn connections(remote: &FakeRemoteTerminals) -> Vec<(u64, u64)> {
    remote
        .targets()
        .into_iter()
        .map(|(RemoteTerminalId(terminal), target)| (terminal, target.host))
        .collect()
}

/// 在解锁框的主密码框里输入主密码，回车。
fn unlock(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| {
        window.click("vault-unlock-password", cx)
    });
    in_frame(cx, handle, |window, cx| window.input(MASTER, cx));
    press(cx, handle, "enter");
}

fn press(cx: &mut TestAppContext, handle: WindowHandle<Root>, key: &'static str) {
    in_frame(cx, handle, move |window, cx| window.press(key, cx));
}

fn click(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) {
    in_frame(cx, handle, move |window, cx| window.click(id, cx));
}

/// 假 shell 跑在真线程上：按真实时间等，同时拨测试时钟（终端每 16ms 收一次事件）。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn lifecycle(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: u64,
) -> Option<TerminalLifecycle> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(card), cx)
            .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
    })
}

fn wait_running(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) {
    assert!(
        wait_until(cx, handle, |cx| lifecycle(cx, workspace, card)
            == Some(TerminalLifecycle::Running)),
        "卡片 {card} 没有连上"
    );
}

fn tile_off(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: u64) -> Option<bool> {
    cx.update(|cx| workspace.read(cx).card_tile_off(CardId(card), cx))
}

#[gpui_kit::test]
fn a_locked_vault_opens_the_card_waiting_and_unlocking_connects_it_in_place(
    cx: &mut TestAppContext,
) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    // 卡片先建出来：写着「正在连接 …」，状态栏「等待解锁保险库」，图块灰着；还没去连。
    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    assert_eq!(
        label(cx, handle, ("card-connecting", 1usize)).as_deref(),
        Some("正在连接 root@10.0.1.12:22…")
    );
    assert!(
        status(cx, handle, 1).starts_with("等待解锁保险库 · root@10.0.1.12:22 · "),
        "{}",
        status(cx, handle, 1)
    );
    assert_eq!(tile_off(cx, &workspace, 1), Some(true));
    assert!(visible(cx, handle, "vault-unlock"));
    assert!(connections(&remote).is_empty());
    // 解锁框等卡片放好才弹：焦点在主密码框，没被新卡片抢走。
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("vault-unlock-password").focused(), Some(true));
    });

    unlock(cx, handle);
    assert!(!visible(cx, handle, "vault-unlock"));

    // 同一张卡片接着连，不新建卡片。
    assert_eq!(connections(&remote), [(1, 1)]);
    wait_running(cx, handle, &workspace, 1);
    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    assert_eq!(label(cx, handle, ("card-connecting", 1usize)), None);
    assert!(status(cx, handle, 1).starts_with("已连接"));
    assert_eq!(tile_off(cx, &workspace, 1), Some(false));
}

#[gpui_kit::test]
fn cancelling_the_unlock_leaves_the_card_disconnected_and_reconnecting_asks_again(
    cx: &mut TestAppContext,
) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    click(cx, handle, "vault-unlock-cancel");
    // 不连；卡片显示未连接，可以原地重连。
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(connections(&remote).is_empty());
    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    assert_eq!(
        label(cx, handle, ("card-disconnected", 1usize)).as_deref(),
        Some("连接已断开 · 保险库没有解锁 · Enter 重新连接 · Space 关闭")
    );
    assert!(
        status(cx, handle, 1).starts_with("已断开 · root@10.0.1.12:22 · 保险库没有解锁"),
        "{}",
        status(cx, handle, 1)
    );
    assert_eq!(label(cx, handle, ("card-connecting", 1usize)), None);
    assert_eq!(tile_off(cx, &workspace, 1), Some(true));
    assert_eq!(vault.status(), VaultStatus::Locked);

    // Enter 原地重连：再弹解锁框，卡片又在等。
    press(cx, handle, "enter");
    assert!(visible(cx, handle, "vault-unlock"));
    assert!(status(cx, handle, 1).starts_with("等待解锁保险库"));
    assert_eq!(label(cx, handle, ("card-disconnected", 1usize)), None);
    assert!(connections(&remote).is_empty());

    // 再取消一次也一样；右键菜单的「重新连接」也再问。
    press(cx, handle, "escape");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(status(cx, handle, 1).starts_with("已断开"));
    dispatch(cx, handle, ReconnectCard(CardId(1)));
    assert!(visible(cx, handle, "vault-unlock"));

    // 这次解开：还是这张卡片接着连。
    unlock(cx, handle);
    assert_eq!(connections(&remote), [(1, 1)]);
    wait_running(cx, handle, &workspace, 1);
    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    assert!(status(cx, handle, 1).starts_with("已连接"));
}

/// 解锁框开着时又要连一台锁着的主机（分屏）：共用这一个框，一起解开或一起取消。
#[gpui_kit::test]
fn cards_waiting_together_share_one_unlock(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    dispatch(cx, handle, SplitRight);
    assert_eq!(card_names(cx, &workspace), [["web-01", "web-01"]]);
    assert!(status(cx, handle, 2).starts_with("等待解锁保险库"));
    assert!(connections(&remote).is_empty());

    // 只有一个框：一次取消，两张都没连。
    click(cx, handle, "vault-unlock-cancel");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(status(cx, handle, 1).starts_with("已断开"));
    assert!(status(cx, handle, 2).starts_with("已断开"));

    // 都原地重连，再一起解开。
    dispatch(cx, handle, ReconnectCard(CardId(1)));
    dispatch(cx, handle, ReconnectCard(CardId(2)));
    unlock(cx, handle);
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connections(&remote), [(1, 1), (2, 1)]);
    wait_running(cx, handle, &workspace, 1);
    wait_running(cx, handle, &workspace, 2);
}

/// 等着的卡片关了：解开后不去连它。
#[gpui_kit::test]
fn a_waiting_card_closed_before_the_unlock_does_not_connect(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    dispatch(cx, handle, CloseCard(CardId(1)));
    assert!(card_names(cx, &workspace).is_empty());
    assert!(visible(cx, handle, "vault-unlock"));
    unlock(cx, handle);
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(connections(&remote).is_empty());
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
}

/// 探测弄断了连接、自动停用探测原地重连（工单 34）时保险库锁着：卡片照样先等解锁，不绕开。
#[gpui_kit::test]
fn reconnecting_after_a_failed_probe_waits_for_a_locked_vault(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().with_secret(&web01_password(), "hunter2");
    let (handle, workspace, remote) = open_with_vault(cx, &vault);
    dispatch(cx, handle, ConnectHost(HostId(1)));
    wait_running(cx, handle, &workspace, 1);
    click(cx, handle, "lock-vault");
    assert_eq!(vault.status(), VaultStatus::Locked);

    remote.emit(qishell::backend::RemoteEvent::ProbeFailed {
        host: 1,
        terminals: vec![RemoteTerminalId(1)],
        lost: true,
    });
    assert!(wait_until(cx, handle, |cx| status(cx, handle, 1)
        .starts_with("等待解锁保险库")));
    assert!(visible(cx, handle, "vault-unlock"));
    assert_eq!(connections(&remote), [(1, 1)]);

    unlock(cx, handle);
    // 同一张卡片接着连，这次不探测。
    assert_eq!(connections(&remote), [(1, 1), (1, 1)]);
    assert!(!remote.targets()[1].1.probe_os);
    wait_running(cx, handle, &workspace, 1);
}

/// 复用已经连着的连接用不到秘密：保险库锁了，分屏、再双击那台主机都不等、不弹（M2 review S2）。
#[gpui_kit::test]
fn reusing_a_live_connection_never_waits_for_the_vault(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().with_secret(&web01_password(), "hunter2");
    let (handle, workspace, remote) = open_with_vault(cx, &vault);
    dispatch(cx, handle, ConnectHost(HostId(1)));
    wait_running(cx, handle, &workspace, 1);
    click(cx, handle, "lock-vault");
    assert_eq!(vault.status(), VaultStatus::Locked);

    dispatch(cx, handle, SplitRight);
    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connections(&remote), [(1, 1), (2, 1), (3, 1)]);
    assert!(!status(cx, handle, 2).starts_with("等待解锁保险库"));
    assert!(!status(cx, handle, 3).starts_with("等待解锁保险库"));
}
