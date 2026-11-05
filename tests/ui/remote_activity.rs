//! 远程卡片被动解析（工单 17）的 UI 测试：远程 shell 自己打出的序列和标题变成卡片的程序图块、
//! 目录和悬停提示；拿不到程序名时是系统徽章。OSC 解析本身的各种情况在终端 crate 的
//! `activity` 单元测试里。

use std::time::Duration;

use qishell::app::ConnectHost;
use qishell::backend::{ProbedHost, RemoteEvent, RemoteTerminalId};
use qishell::programs::program_of;
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

fn open_remote(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>, Arc<FakeRemoteTerminals>) {
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

/// 连接第一台主机，等卡片 1 连上（终端进了 `Running` 才往里打字）。
fn connect(cx: &mut TestAppContext, handle: WindowHandle<Root>, workspace: &Entity<Workspace>) {
    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert!(
        wait_until(cx, handle, |cx| {
            cx.update(|cx| {
                workspace
                    .read(cx)
                    .card(CardId(1), cx)
                    .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
            }) == Some(TerminalLifecycle::Running)
        }),
        "卡片没有连上"
    );
}

fn tile(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<String> {
    cx.update(|cx| workspace.read(cx).card_tile(CardId(1), cx))
}

fn directory(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> String {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(1), cx)
            .map(|card| card.read(cx).directory_label())
            .unwrap_or_default()
    })
}

fn probe(remote: &FakeRemoteTerminals, cx: &mut TestAppContext) {
    remote.emit(RemoteEvent::Probed {
        terminal: RemoteTerminalId(1),
        host: ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        },
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_command_the_shell_integration_reports_becomes_the_cards_tile(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace);
    // 拿不到程序名：系统还没探测出，是名称的首字。
    assert_eq!(tile(cx, &workspace), Some("W".into()));

    remote
        .factory
        .print(1, "\x1b]133;C;cmdline_url=sudo%20htop%20-d%2010\x07");
    let htop = program_of("htop").name;
    assert!(wait_until(cx, handle, |cx| tile(cx, &workspace) == Some(htop.clone())));
    assert_eq!(
        cx.update(|cx| workspace.read(cx).tab_tiles(TabId(1), cx)),
        [htop.clone()]
    );

    // 回到提示符：换回系统徽章。
    remote.factory.print(1, "\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    assert!(wait_until(cx, handle, |cx| tile(cx, &workspace) == Some("W".into())));
    probe(&remote, cx);
    assert_eq!(tile(cx, &workspace), Some("Ubuntu".into()));
}

#[gpui_kit::test]
fn a_full_screen_program_with_no_name_keeps_the_badge_and_lights_the_dot(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace);
    remote.factory.print(1, "\x1b[?1049hloading…");
    assert!(wait_until(cx, handle, |cx| {
        cx.update(|cx| workspace.read(cx).card_dot(CardId(1), cx)) == Some(StatusDot::Busy)
    }));
    assert_eq!(tile(cx, &workspace), Some("W".into()));
}

#[gpui_kit::test]
fn a_reported_folder_naming_the_host_shows_once_its_name_is_known(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace);
    remote.factory.print(1, "\x1b]7;file://web-01/srv/app\x07$ ");
    // 还没探测出 `uname -n`：带主机名的报告先不信。
    wait_until(cx, handle, |cx| !directory(cx, &workspace).is_empty());
    assert_eq!(directory(cx, &workspace), "");

    probe(&remote, cx);
    assert_eq!(directory(cx, &workspace), "/srv/app");
    assert_eq!(
        cx.update(|cx| workspace
            .read(cx)
            .card(CardId(1), cx)
            .and_then(|card| card.read(cx).remote_start_directory().map(str::to_string))),
        Some("/srv/app".into())
    );

    // 在卡片里又 ssh 到了别的主机：它报的目录不动卡片。
    remote.factory.print(1, "\x1b]7;file://db-01/var/lib\x07$ ");
    wait_until(cx, handle, |cx| directory(cx, &workspace) != "/srv/app");
    assert_eq!(directory(cx, &workspace), "/srv/app");
}

#[gpui_kit::test]
fn without_reports_the_folder_comes_from_the_title_which_the_header_tip_shows_in_full(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace);
    probe(&remote, cx);
    remote.factory.print(1, "\x1b]0;root@web-01: ~\x07$ ");
    assert!(wait_until(cx, handle, |cx| directory(cx, &workspace) == "~"));
    let card = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let card = workspace.read(cx).card(CardId(1), cx).unwrap().read(cx);
            (
                card.remote_start_directory().map(str::to_string),
                card.remote_activity().and_then(|activity| activity.title.clone()),
            )
        })
    };
    // 单独一个 `~` 只用来显示，不能用作 SFTP 的初始目录。
    assert_eq!(card(cx), (None, Some("root@web-01: ~".into())));

    remote.factory.print(1, "\x1b]0;root@web-01: ~/src/app\x07$ ");
    assert!(wait_until(cx, handle, |cx| directory(cx, &workspace) == "~/src/app"));
    assert_eq!(
        card(cx),
        (Some("~/src/app".into()), Some("root@web-01: ~/src/app".into()))
    );
    // 头部固定是「主机名 + 目录」。
    let header = in_frame(cx, handle, |window, _| {
        window
            .try_find(("card-header", 1usize))
            .and_then(|element| element.label().map(str::to_owned))
    });
    assert_eq!(header.as_deref(), Some("web-01 ~/src/app"));
}
