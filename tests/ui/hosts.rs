//! 主机栏（工单 11）的 UI 测试：平铺列表、徽章、悬停提示、选中、搜索、Ctrl+Shift+H、
//! 拖动排序、右键菜单、创建副本、复制地址、删除、写不进数据库时报告。

use std::time::Duration;

use gpui_kit::point;
use qishell::app::{CopyHostAddress, DeleteHost, DuplicateHost};

use crate::support::*;

/// 往主机栏的搜索框里输入 `text`（先点一下搜索框）。
fn search(cx: &mut TestAppContext, handle: WindowHandle<Root>, text: &str) {
    in_frame(cx, handle, |window, cx| window.click("host-search", cx));
    in_frame(cx, handle, |window, cx| window.input(text, cx));
}

fn highlighted(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<u64> {
    cx.update(|cx| workspace.read(cx).highlighted_host(cx).map(|id| id.0))
}

fn selected(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<u64> {
    cx.update(|cx| workspace.read(cx).selected_host(cx).map(|id| id.0))
}

#[gpui_kit::test]
fn the_host_sidebar_lists_every_saved_host_with_its_badge_and_login(cx: &mut TestAppContext) {
    let (handle, _, _) = open_with_seed(cx);

    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-count").label(), Some("6"));
        // 从上到下，按保存的顺序。
        let tops: Vec<_> = (1..=6)
            .map(|id| window.find(host_row(id)).bounds().top())
            .collect();
        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
        assert_eq!(window.find(host_row(1)).label(), Some("web-01"));
        // 端口是 22 时不写，别的端口写上。
        assert_eq!(
            window.find(("host-address", 1usize)).label(),
            Some("root@10.0.1.12")
        );
        assert_eq!(
            window.find(("host-address", 4usize)).label(),
            Some("deploy@10.0.9.20:2222")
        );
        // 没探测出系统：名称首字。
        assert_eq!(window.find(("host-badge", 1usize)).label(), Some("W"));
        assert_eq!(window.find(("host-badge", 3usize)).label(), Some("D"));
    });
}

#[gpui_kit::test]
fn a_detected_system_puts_its_logo_on_the_badge(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);

    edit_hosts(cx, &workspace, |store, cx| {
        store.set_host_os(HostId(1), Some(HostOs::Ubuntu), cx)
    });

    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("host-badge", 1usize)).label(), Some("Ubuntu"));
    });
}

#[gpui_kit::test]
fn without_hosts_the_sidebar_says_how_to_make_one(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-count").label(), Some("0"));
        assert_eq!(window.find("host-list-empty").label(), Some("还没有主机"));
    });
}

#[gpui_kit::test]
fn hovering_a_host_shows_where_it_logs_in_and_its_notes(cx: &mut TestAppContext) {
    let mut data = HostData::empty();
    data.insert_unnotified(
        HostDraft::new("web-01", "10.0.1.12", 22, "root", AuthKind::Password)
            .with_notes("改配置前先在群里说一声"),
    );
    let (handle, _, _) = open_with_hosts(cx, data);

    in_frame(cx, handle, |window, cx| window.hover(host_row(1), cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("host-tooltip").label(),
            Some("root@10.0.1.12:22\n改配置前先在群里说一声")
        );
    });
}

#[gpui_kit::test]
fn clicking_a_host_selects_it(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    assert_eq!(selected(cx, &workspace), None);

    in_frame(cx, handle, |window, cx| window.click(host_row(3), cx));

    assert_eq!(selected(cx, &workspace), Some(3));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(host_row(3)).selected(), Some(true));
        assert_eq!(window.find(host_row(1)).selected(), Some(false));
    });
}

#[gpui_kit::test]
fn searching_matches_every_word_against_name_address_and_user(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);

    search(cx, handle, "web");
    assert_eq!(listed(cx, &workspace), [1, 2]);

    // 空格隔开的几个词都要匹配：名称里有 web、地址里有 .13。
    search(cx, handle, " .13");
    assert_eq!(listed(cx, &workspace), [2]);

    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert_eq!(listed(cx, &workspace), [1, 2, 3, 4, 5, 6]);

    // 用户名。
    search(cx, handle, "postgres");
    assert_eq!(listed(cx, &workspace), [3]);
}

#[gpui_kit::test]
fn the_first_match_is_highlighted_and_arrows_move_it(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    assert_eq!(highlighted(cx, &workspace), None, "not searching");

    search(cx, handle, "10.0.9");
    assert_eq!(listed(cx, &workspace), [4, 5]);
    assert_eq!(highlighted(cx, &workspace), Some(4));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(host_row(4)).selected(), Some(true));
    });

    in_frame(cx, handle, |window, cx| window.press("down", cx));
    assert_eq!(highlighted(cx, &workspace), Some(5));
    // 到底了就停在最后一个。
    in_frame(cx, handle, |window, cx| window.press("down", cx));
    assert_eq!(highlighted(cx, &workspace), Some(5));
    in_frame(cx, handle, |window, cx| window.press("up", cx));
    assert_eq!(highlighted(cx, &workspace), Some(4));

    // Esc 清空搜索，焦点还在搜索框里。
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert_eq!(highlighted(cx, &workspace), None);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-search").focused(), Some(true));
    });
}

#[gpui_kit::test]
fn a_search_without_matches_says_so(cx: &mut TestAppContext) {
    let (handle, _, _) = open_with_seed(cx);

    search(cx, handle, "zzz");

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("host-list-empty").label(),
            Some("没有匹配「zzz」的主机")
        );
        assert!(window.try_find(host_row(1)).is_none());
    });
}

#[gpui_kit::test]
fn ctrl_shift_h_opens_the_host_sidebar_and_focuses_the_search(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    dispatch(cx, handle, ToggleHostSidebar);
    cx.update(|cx| assert!(!workspace.read(cx).is_host_sidebar_open()));

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-h", cx));

    cx.update(|cx| assert!(workspace.read(cx).is_host_sidebar_open()));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-search").focused(), Some(true));
    });
}

/// 拖到 `target` 那一行的上半部（`before`）或下半部。
fn drag_host(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    source: u64,
    target: u64,
    before: bool,
) {
    in_frame(cx, handle, |window, cx| {
        let from = window.find(host_row(source)).bounds().center();
        let bounds = window.find(host_row(target)).bounds();
        let y = if before {
            bounds.top() + bounds.size.height * 0.25
        } else {
            bounds.top() + bounds.size.height * 0.75
        };
        window.drag(from, point(bounds.center().x, y), cx);
    });
}

#[gpui_kit::test]
fn dragging_a_host_puts_it_before_or_after_another(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);

    drag_host(cx, handle, 5, 2, true);
    assert_eq!(
        host_names(cx, &workspace),
        [
            "web-01",
            "qa-runner",
            "web-02",
            "db-01",
            "staging-api",
            "dev-box"
        ]
    );

    drag_host(cx, handle, 1, 4, false);
    assert_eq!(
        host_names(cx, &workspace),
        [
            "qa-runner",
            "web-02",
            "db-01",
            "staging-api",
            "web-01",
            "dev-box"
        ]
    );
    // 列表跟着变。
    in_frame(cx, handle, |window, _| {
        assert!(window.find(host_row(5)).bounds().top() < window.find(host_row(2)).bounds().top());
        assert!(window.try_find(("host-drop-line", 2usize)).is_none());
    });
}

#[gpui_kit::test]
fn dragging_is_paused_while_searching(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    search(cx, handle, "web");

    drag_host(cx, handle, 2, 1, true);

    assert_eq!(
        host_names(cx, &workspace),
        [
            "web-01",
            "web-02",
            "db-01",
            "staging-api",
            "qa-runner",
            "dev-box"
        ]
    );
}

/// 右键菜单里各项的文字，从上到下（分隔线不算）。
fn menu_labels(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: u64) -> Vec<String> {
    in_frame(cx, handle, |window, cx| {
        window.right_click(host_row(id), cx)
    });
    let labels = in_frame(cx, handle, |window, _| {
        let popup = window.within("popup-menu");
        (0..12_usize)
            .filter_map(|item| popup.try_find(item))
            .filter_map(|item| item.label().map(str::to_owned))
            .collect::<Vec<_>>()
    });
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    labels
}

#[gpui_kit::test]
fn the_host_menu_edits_copies_and_deletes(cx: &mut TestAppContext) {
    let mut data = HostData::seed();
    data.insert_unnotified(HostDraft::new(
        "官网",
        "www.example.com",
        22,
        "root",
        AuthKind::Password,
    ));
    let (handle, workspace, _) = open_with_hosts(cx, data);

    assert_eq!(
        menu_labels(cx, handle, 1),
        [
            "连接",
            "在右侧分屏打开",
            "在下方分屏打开",
            "打开 SFTP",
            "编辑…",
            "创建副本",
            "复制 IP",
            "删除"
        ]
    );
    // 右键也选中那一台。
    assert_eq!(selected(cx, &workspace), Some(1));
    // 地址是域名时叫「复制主机名」。
    assert_eq!(
        menu_labels(cx, handle, 7),
        [
            "连接",
            "在右侧分屏打开",
            "在下方分屏打开",
            "打开 SFTP",
            "编辑…",
            "创建副本",
            "复制主机名",
            "删除"
        ]
    );
}

#[gpui_kit::test]
fn a_copy_goes_right_after_the_original_without_its_password(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    let original = saved_host(cx, &workspace, 1).unwrap();
    secrets.set(&original.password_secret(), "hunter2").unwrap();

    dispatch(cx, handle, DuplicateHost(HostId(1)));

    assert_eq!(
        host_names(cx, &workspace)[..3],
        ["web-01", "web-01 副本", "web-02"]
    );
    let copy = saved_host(cx, &workspace, 7).unwrap();
    assert_eq!(copy.address.as_ref(), "10.0.1.12");
    assert_eq!(copy.user.as_ref(), "root");
    assert!(secrets.get(&copy.password_secret()).unwrap().is_none());
    // 副本自动选上。
    assert_eq!(selected(cx, &workspace), Some(7));
    in_frame(cx, handle, |window, _| {
        assert!(window.find(host_row(7)).bounds().top() < window.find(host_row(2)).bounds().top());
    });
}

#[gpui_kit::test]
fn copy_ip_puts_the_address_on_the_clipboard(cx: &mut TestAppContext) {
    let (handle, _, _) = open_with_seed(cx);

    dispatch(cx, handle, CopyHostAddress(HostId(4)));

    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("10.0.9.20"));
}

#[gpui_kit::test]
fn deleting_a_host_asks_first(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    let web01 = saved_host(cx, &workspace, 1).unwrap();
    secrets.set(&web01.password_secret(), "hunter2").unwrap();

    dispatch(cx, handle, DeleteHost(HostId(1)));
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("host-delete-text").label(),
            Some("确定删除「web-01」吗？\n已打开的卡片不受影响。")
        );
        window.click("host-delete-cancel", cx);
    });
    assert_eq!(host_names(cx, &workspace).len(), 6);

    dispatch(cx, handle, DeleteHost(HostId(1)));
    in_frame(cx, handle, |window, cx| {
        window.click("host-delete-confirm", cx)
    });

    assert_eq!(
        host_names(cx, &workspace),
        ["web-02", "db-01", "staging-api", "qa-runner", "dev-box"]
    );
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(host_row(1)).is_none());
        assert_eq!(window.find("host-count").label(), Some("5"));
    });
    // 主机自己的密码跟着主机走。
    assert!(secrets.get(&web01.password_secret()).unwrap().is_none());
}

#[gpui_kit::test]
fn deleting_a_jump_host_names_the_hosts_that_go_through_it(cx: &mut TestAppContext) {
    let mut data = HostData::seed();
    data.insert_unnotified(
        HostDraft::new("db-master", "10.0.2.8", 22, "root", AuthKind::Password)
            .with_route(qishell_hosts::Route::Jump(vec![Some(HostId(1))])),
    );
    let (handle, _, _) = open_with_hosts(cx, data);

    dispatch(cx, handle, DeleteHost(HostId(1)));

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("host-delete-text").label(),
            Some(
                "确定删除「web-01」吗？\n有 1 台主机把它当作跳板：\ndb-master\n\
                 删除后，这些主机的跳板列表里会留一项「已删除的主机」，连接时直接报错。\n\
                 已打开的卡片不受影响。"
            )
        );
    });
}

#[gpui_kit::test]
fn a_change_the_database_refuses_still_shows_and_is_reported(cx: &mut TestAppContext) {
    // 一台用着数据库不认识的凭据的主机：复制它写不进数据库。
    let mut data = HostData::load(HostDatabase::in_memory().unwrap()).unwrap();
    let credential = data.insert_credential_unnotified(qishell_hosts::CredentialDraft::new(
        "部署",
        qishell_hosts::CredentialKind::Password,
        "deploy",
    ));
    data.insert_unnotified(
        HostDraft::new("web-01", "10.0.1.12", 22, "root", AuthKind::Password)
            .with_credential(credential),
    );
    let (handle, workspace, _) = open_with_hosts(cx, data);
    let notifications =
        |cx: &mut TestAppContext| in_frame(cx, handle, |window, cx| window.notifications(cx).len());
    assert_eq!(notifications(cx), 0);

    dispatch(cx, handle, DuplicateHost(HostId(1)));

    // 内存为准：副本照样在列表里，同时告诉用户没存进去。
    assert_eq!(host_names(cx, &workspace), ["web-01", "web-01 副本"]);
    assert!(notifications(cx) >= 1);
}

#[gpui_kit::test]
fn a_database_that_cannot_be_opened_is_reported_once_the_window_is_up(cx: &mut TestAppContext) {
    let services = WorkspaceServices {
        hosts_problem: Some("主机数据库打不开，这次的改动不会保存：disk I/O error".into()),
        ..fake_services()
    };
    let (handle, _) = open_workspace_with(cx, settings_store(), services);

    let shown = in_frame(cx, handle, |window, cx| window.notifications(cx).len());
    assert_eq!(shown, 1);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-count").label(), Some("0"));
    });
}
