//! 快速连接与最近连接（工单 18）的 UI 测试：「⌄」和 Ctrl+Shift+O 打开快速连接，最近的在前并标「最近」、
//! 其余按名称；输入就筛选、↑↓ 移动高亮、回车或点一行在新标签里连接、Esc 关闭；
//! 开始卡片的最近连接列表（双击、回车连接）和一台主机都没有时的样子。
//! 远程终端是假的（`FakeRemoteTerminals`），连接本身在 `remote_card` 和 backend 的测试里。

use std::time::Duration;

use qishell::app::QuickConnect;
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

/// 用 seed 的六台主机打开（最近连接：staging-api 4 在前，web-01 1 在后），窗口在前。
fn open(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>) {
    let (handle, workspace, _) = open_with_seed(cx);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace)
}

/// 快速连接开着时列出的主机，从上到下；关着时是 `None`。
fn shown(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<Vec<u64>> {
    cx.update(|cx| {
        workspace.read(cx).quick_connect().map(|list| {
            list.read(cx)
                .choice()
                .shown()
                .map(|quick| quick.host.id.0)
                .collect()
        })
    })
}

/// 快速连接里高亮的那台。
fn highlighted(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<u64> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .quick_connect()
            .and_then(|list| list.read(cx).choice().highlighted())
            .map(|id| id.0)
    })
}

fn row(id: u64) -> (&'static str, usize) {
    ("quick-connect-host", id as usize)
}

fn recent_row(id: u64) -> (&'static str, usize) {
    ("start-recent-host", id as usize)
}

/// 每个标签里卡片的名字。
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

/// 等卡片 `card` 的终端进入 `Running`：假 shell 跑在真线程上，按真实时间等，同时拨测试时钟。
fn wait_running(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        let running = cx.update(|cx| {
            workspace
                .read(cx)
                .card(CardId(card), cx)
                .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
                == Some(TerminalLifecycle::Running)
        });
        if running {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("卡片 {card} 没有连上");
}

fn recent(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<u64> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .recent_hosts()
            .map(|host| host.id.0)
            .collect()
    })
}

#[gpui_kit::test]
fn the_chevron_lists_recent_hosts_first_marked_recent_then_the_rest_by_name(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open(cx);
    assert_eq!(shown(cx, &workspace), None);

    in_frame(cx, handle, |window, cx| window.click("quick-connect", cx));

    // 最近：staging-api、web-01；其余按名称：db-01、dev-box、qa-runner、web-02。
    assert_eq!(shown(cx, &workspace), Some(vec![4, 1, 3, 6, 5, 2]));
    in_frame(cx, handle, |window, _| {
        assert!(window.find("quick-connect-popup").visible());
        assert_eq!(window.find(row(4)).label(), Some("staging-api"));
        assert_eq!(
            window.find(("quick-connect-recent", 4usize)).label(),
            Some("最近")
        );
        assert!(window.try_find(("quick-connect-recent", 1usize)).is_some());
        assert!(window.try_find(("quick-connect-recent", 3usize)).is_none());
        // 第一个高亮，搜索框拿着焦点，直接就能打字。
        assert_eq!(window.find(row(4)).selected(), Some(true));
        assert_eq!(window.find(row(1)).selected(), Some(false));
        assert_eq!(window.find("quick-connect-search").focused(), Some(true));
        // 弹层挂在「+ ⌄」下面。
        let anchor = window.find("new-tab-group").bounds();
        let popup = window.find("quick-connect-popup").bounds();
        assert!(popup.top() >= anchor.bottom(), "{popup:?} {anchor:?}");
    });

    // 再点一下「⌄」关上。
    in_frame(cx, handle, |window, cx| window.click("quick-connect", cx));
    assert_eq!(shown(cx, &workspace), None);
}

#[gpui_kit::test]
fn ctrl_shift_o_opens_and_escape_closes_giving_the_focus_back(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-o", cx));
    assert!(shown(cx, &workspace).is_some());
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert_eq!(shown(cx, &workspace), None);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("quick-connect-popup").is_none());
        assert_eq!(window.find("start-card").focused(), Some(true));
    });

    // Ctrl+Shift+O 再按一次也关上。
    dispatch(cx, handle, QuickConnect);
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-o", cx));
    assert_eq!(shown(cx, &workspace), None);
}

#[gpui_kit::test]
fn clicking_outside_closes_it(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);
    dispatch(cx, handle, QuickConnect);

    in_frame(cx, handle, |window, cx| window.click("start-card", cx));

    assert_eq!(shown(cx, &workspace), None);
}

#[gpui_kit::test]
fn typing_filters_like_the_host_sidebar_and_arrows_move_the_highlight(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);
    dispatch(cx, handle, QuickConnect);

    // 地址、名称、用户名都查；高亮第一个匹配。
    in_frame(cx, handle, |window, cx| window.input("10.0.1", cx));
    assert_eq!(shown(cx, &workspace), Some(vec![1, 2]));
    assert_eq!(highlighted(cx, &workspace), Some(1));

    in_frame(cx, handle, |window, cx| window.press("down", cx));
    assert_eq!(highlighted(cx, &workspace), Some(2));
    in_frame(cx, handle, |window, cx| window.press("down", cx));
    assert_eq!(highlighted(cx, &workspace), Some(2));
    in_frame(cx, handle, |window, cx| window.press("up", cx));
    assert_eq!(highlighted(cx, &workspace), Some(1));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(row(1)).selected(), Some(true));
        assert!(window.try_find(row(3)).is_none());
    });

    // 几个词都要匹配。
    in_frame(cx, handle, |window, cx| window.input(" 13", cx));
    assert_eq!(shown(cx, &workspace), Some(vec![2]));

    in_frame(cx, handle, |window, cx| window.input("x", cx));
    assert_eq!(shown(cx, &workspace), Some(vec![]));
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("quick-connect-empty").label(),
            Some("没有匹配「10.0.1 13x」的主机")
        );
    });
}

#[gpui_kit::test]
fn enter_connects_the_highlighted_host_in_a_new_tab(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);
    dispatch(cx, handle, QuickConnect);

    in_frame(cx, handle, |window, cx| window.input("web", cx));
    in_frame(cx, handle, |window, cx| window.press("down", cx));
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    wait_running(cx, handle, &workspace, 1);

    assert_eq!(shown(cx, &workspace), None);
    assert_eq!(card_names(cx, &workspace), [["web-02"]]);
    // 连上了就记进最近连接，下次排在最前面。
    assert_eq!(recent(cx, &workspace), [2, 4, 1]);
    dispatch(cx, handle, QuickConnect);
    assert_eq!(shown(cx, &workspace), Some(vec![2, 4, 1, 3, 6, 5]));
}

#[gpui_kit::test]
fn clicking_a_host_connects_it_too(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);
    dispatch(cx, handle, QuickConnect);

    in_frame(cx, handle, |window, cx| window.click(row(3), cx));
    wait_running(cx, handle, &workspace, 1);

    assert_eq!(shown(cx, &workspace), None);
    assert_eq!(card_names(cx, &workspace), [["db-01"]]);
}

#[gpui_kit::test]
fn without_hosts_quick_connect_says_there_are_none(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    dispatch(cx, handle, QuickConnect);

    assert_eq!(shown(cx, &workspace), Some(vec![]));
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("quick-connect-empty").label(),
            Some("还没有主机")
        );
    });
}

/// 最近连接每行右边写什么时候连的（原型「10 分钟前」，M2 code review S5）。
#[gpui_kit::test]
fn the_start_card_says_when_each_recent_host_connected(cx: &mut TestAppContext) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut hosts = HostData::seed();
    hosts.record_connection_at_unnotified(qishell_hosts::HostId(1), now - 10 * 60);
    hosts.record_connection_at_unnotified(qishell_hosts::HostId(4), now);
    let (handle, _, _) = open_with_hosts(cx, hosts);

    let mut time = |id: u64| {
        in_frame(cx, handle, move |window, _| {
            window
                .find(("start-recent-time", id as usize))
                .label()
                .map(str::to_owned)
        })
    };
    assert_eq!(time(4).as_deref(), Some("刚刚"));
    assert_eq!(time(1).as_deref(), Some("10 分钟前"));
}

#[gpui_kit::test]
fn the_start_card_lists_recent_connections_above_the_two_buttons(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);

    cx.update(|cx| {
        assert_eq!(
            workspace.read(cx).start_card_recent(cx),
            [HostId(4), HostId(1)]
        );
    });
    in_frame(cx, handle, |window, _| {
        let list = window.find("start-recent").bounds();
        let button = window.find("start-new-host").bounds();
        let keys = window.find("start-quick-connect-keys").bounds();
        assert!(list.bottom() <= button.top());
        assert!(button.bottom() <= keys.top());
        assert_eq!(window.find(recent_row(4)).label(), Some("staging-api"));
        // 第一项默认高亮；有主机时不显示欢迎语。
        assert_eq!(window.find(recent_row(4)).selected(), Some(true));
        assert_eq!(window.find(recent_row(1)).selected(), Some(false));
        assert!(window.try_find("start-hello").is_none());
        assert!(window.find("start-new-local-terminal").visible());
    });
}

#[gpui_kit::test]
fn enter_on_the_start_card_connects_the_highlighted_recent_host(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);

    in_frame(cx, handle, |window, cx| window.press("down", cx));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(recent_row(1)).selected(), Some(true));
    });
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    wait_running(cx, handle, &workspace, 1);

    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
}

#[gpui_kit::test]
fn double_clicking_a_recent_host_connects_it(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx);

    in_frame(cx, handle, |window, cx| {
        window.double_click(recent_row(1), cx)
    });
    wait_running(cx, handle, &workspace, 1);

    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    // 关掉这个标签回到开始卡片：刚连的排在最前面，又从第一行高亮。
    dispatch(cx, handle, CloseCard(CardId(1)));
    in_frame(cx, handle, |window, _| {
        assert!(window.find("start-card").visible());
        assert_eq!(window.find(recent_row(1)).selected(), Some(true));
    });
    cx.update(|cx| {
        assert_eq!(
            workspace.read(cx).start_card_recent(cx),
            [HostId(1), HostId(4)]
        );
    });
}

#[gpui_kit::test]
fn hosts_never_connected_to_leave_the_recent_list_saying_so(cx: &mut TestAppContext) {
    let mut data = HostData::empty();
    data.insert_unnotified(HostDraft::new(
        "web-01",
        "10.0.1.12",
        22,
        "root",
        AuthKind::Password,
    ));
    let (handle, _, _) = open_with_hosts(cx, data);

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("start-recent-empty").label(),
            Some("连过的主机会列在这里，双击或回车再连")
        );
        assert!(window.try_find("start-hello").is_none());
    });
}

#[gpui_kit::test]
fn without_hosts_the_start_card_shows_only_the_two_buttons_and_a_hint(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("start-hello").label(),
            Some("先新建一台主机，或者直接开一个本地终端。")
        );
        assert!(window.try_find("start-recent").is_none());
        assert!(window.find("start-new-local-terminal").visible());
        assert!(window.find("start-new-host").visible());
    });
}
