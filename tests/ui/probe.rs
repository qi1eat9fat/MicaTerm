//! 系统探测可关、失败后自动停用（工单 34）的 UI 测试：主机对话框的「登录后探测系统」开关，
//! 连接表说探测失败后主机记成不再探测、提示一句、断了的卡片原地重连。远程终端是假的
//! （`FakeRemoteTerminals`），真的堡垒机（只给一个通道、再开通道就断开）在 backend 的 `probe_tests`。

use std::time::Duration;

use gpui_kit::{ScrollDelta, point};

use qishell::app::{ConnectHost, EditHost};
use qishell::backend::{ProbeFailure, RemoteEvent, RemoteTerminalId};
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

fn open_remote(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote)
}

/// 假 shell 跑在真线程上：按真实时间等，同时拨测试时钟。
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

fn running(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: u64) -> bool {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(card), cx)
            .is_some_and(|card| {
                card.read(cx).terminal().read(cx).lifecycle(cx) == TerminalLifecycle::Running
            })
    })
}

/// 连接主机 `host`，等卡片 `card` 连上。
fn connect(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    host: u64,
    card: u64,
) {
    dispatch(cx, handle, ConnectHost(HostId(host)));
    assert!(
        wait_until(cx, handle, |cx| running(cx, workspace, card)),
        "卡片 {card} 没有连上"
    );
}

/// 每次建传输（打开、重连）时登录后探不探测系统，按顺序。
fn probes(remote: &FakeRemoteTerminals) -> Vec<bool> {
    remote
        .targets()
        .iter()
        .map(|(_, target)| target.probe_os)
        .collect()
}

fn notifications(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> usize {
    in_frame(cx, handle, |window, cx| window.notifications(cx).len())
}

fn probe_switch(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<bool> {
    in_frame(cx, handle, |window, _| {
        window.find("host-probe-os").checked()
    })
}

/// 拨「登录后探测系统」：它在对话框最下面的「高级」组里，先滚到底。
fn toggle_the_probe(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| {
        window.scroll(
            "host-dialog",
            ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            cx,
        )
    });
    in_frame(cx, handle, |window, cx| window.click("host-probe-os", cx));
}

fn edit_and_toggle_the_probe(cx: &mut TestAppContext, handle: WindowHandle<Root>, host: u64) {
    dispatch(cx, handle, EditHost(HostId(host)));
    toggle_the_probe(cx, handle);
    in_frame(cx, handle, |window, cx| {
        window.click("host-dialog-save", cx)
    });
}

#[gpui_kit::test]
fn the_host_dialog_turns_the_probe_off_and_back_on(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);

    // 默认开着。
    dispatch(cx, handle, EditHost(HostId(1)));
    assert_eq!(probe_switch(cx, handle), Some(true));
    in_frame(cx, handle, |window, cx| {
        window.click("host-dialog-cancel", cx)
    });

    edit_and_toggle_the_probe(cx, handle, 1);
    assert!(!saved_host(cx, &workspace, 1).unwrap().probe_os);
    connect(cx, handle, &workspace, 1, 1);
    assert_eq!(probes(&remote), [false], "关掉后登录不开 exec 通道探测");

    // 再打开对话框是关着的；打开它，下次连接又探测。
    dispatch(cx, handle, EditHost(HostId(1)));
    assert_eq!(probe_switch(cx, handle), Some(false));
    toggle_the_probe(cx, handle);
    in_frame(cx, handle, |window, cx| {
        window.click("host-dialog-save", cx)
    });
    assert!(saved_host(cx, &workspace, 1).unwrap().probe_os);
    connect(cx, handle, &workspace, 1, 2);
    assert_eq!(probes(&remote), [false, true]);
}

#[gpui_kit::test]
fn a_probe_that_drops_the_connection_turns_the_probe_off_and_reconnects_in_place(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace, 1, 1);
    assert_eq!(probes(&remote), [true]);
    assert_eq!(notifications(cx, handle), 0);

    remote.emit(RemoteEvent::ProbeFailed {
        host: HostId(1),
        terminals: vec![RemoteTerminalId(1)],
        failure: ProbeFailure::ConnectionLost,
    });

    assert!(wait_until(cx, handle, |_| probes(&remote).len() == 2));
    // 同一张卡片原地重连，这次不探测。
    assert_eq!(remote.targets()[1].0, RemoteTerminalId(1));
    assert_eq!(probes(&remote), [true, false]);
    assert!(wait_until(cx, handle, |cx| running(cx, &workspace, 1)));
    assert!(
        !saved_host(cx, &workspace, 1).unwrap().probe_os,
        "主机记成不再探测"
    );
    assert_eq!(notifications(cx, handle), 1, "提示已自动关闭探测");

    // 之后打开的卡片也不探测；对话框里看得到它关着。
    connect(cx, handle, &workspace, 1, 2);
    assert_eq!(probes(&remote), [true, false, false]);
    dispatch(cx, handle, EditHost(HostId(1)));
    assert_eq!(probe_switch(cx, handle), Some(false));
}

#[gpui_kit::test]
fn a_refused_probe_channel_turns_the_probe_off_without_reconnecting(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace, 1, 1);

    remote.emit(RemoteEvent::ProbeFailed {
        host: HostId(1),
        terminals: vec![RemoteTerminalId(1)],
        failure: ProbeFailure::Refused,
    });

    assert!(wait_until(cx, handle, |cx| !saved_host(cx, &workspace, 1)
        .unwrap()
        .probe_os));
    assert_eq!(notifications(cx, handle), 1);
    // 连接还在：卡片不重连。
    assert_eq!(probes(&remote), [true]);
    assert!(running(cx, &workspace, 1));

    // 同一台主机的另一条连接也失败了：已经关了，不再提示。
    remote.emit(RemoteEvent::ProbeFailed {
        host: HostId(1),
        terminals: vec![RemoteTerminalId(1)],
        failure: ProbeFailure::Refused,
    });
    wait_until(cx, handle, |_| false);
    assert_eq!(notifications(cx, handle), 1);
}

#[gpui_kit::test]
fn a_deleted_host_reconnects_from_its_snapshot_without_probing(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace, 1, 1);
    edit_hosts(cx, &workspace, |store, cx| {
        store.remove(HostId(1), cx);
    });

    remote.emit(RemoteEvent::ProbeFailed {
        host: HostId(1),
        terminals: vec![RemoteTerminalId(1)],
        failure: ProbeFailure::ConnectionLost,
    });

    assert!(wait_until(cx, handle, |_| probes(&remote).len() == 2));
    assert_eq!(
        probes(&remote),
        [true, false],
        "按卡片记着的快照重连，也不探测"
    );
    assert_eq!(notifications(cx, handle), 1);
}
