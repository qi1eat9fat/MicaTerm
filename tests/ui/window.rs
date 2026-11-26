//! 窗口壳（工单 01）：标题栏、主机栏、右侧栏、新标签页。

use micaterm::app::ShowCredentials;

use crate::support::*;

#[gpui_kit::test]
fn startup_shows_the_new_tab_page(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        assert!(window.find("new-tab-page").visible());
        assert!(window.find("new-tab-local-terminal").visible());
        assert!(window.find("new-tab-new-host").visible());
        // 没有标签时新标签页持有焦点，快捷键才有焦点路径可走。
        assert_eq!(window.find("new-tab-page").focused(), Some(true));
        // 标签栏是空的。
        assert!(window.try_find(("tab", 1usize)).is_none());
    });
}

/// 新标签页只有两行：新建主机、本地终端，右边是快捷键（`Ctrl+Shift+N` 写法）；
/// 新建主机在上，行高 32、列宽 300；没有快速连接和最近连接。
#[gpui_kit::test]
fn the_new_tab_page_lists_new_host_and_local_terminal_with_their_shortcuts(
    cx: &mut TestAppContext,
) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        let host = window.find("new-tab-new-host");
        let terminal = window.find("new-tab-local-terminal");
        assert_eq!(host.label(), Some("New Host"));
        assert_eq!(terminal.label(), Some("Local Terminal"));
        assert!(host.bounds().bottom() <= terminal.bounds().top());
        assert_eq!(host.bounds().size.height, px(32.));
        assert_eq!(host.bounds().size.width, px(300.));
        let keys = |id: &'static str| window.find((id, 1usize)).label().map(str::to_owned);
        if cfg!(target_os = "macos") {
            assert!(keys("new-tab-new-host").is_some());
        } else {
            assert_eq!(keys("new-tab-new-host").as_deref(), Some("Ctrl+Shift+N"));
            assert_eq!(
                keys("new-tab-local-terminal").as_deref(),
                Some("Ctrl+Shift+T")
            );
        }
        assert!(window.try_find("quick-connect").is_none());
    });
}

#[gpui_kit::test]
fn the_title_bar_has_a_drag_area_and_the_window_buttons(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        let drag = window.find("title-drag");
        assert!(drag.visible());
        let buttons = ["window-min", "window-max", "window-close"].map(|id| window.find(id));
        assert_eq!(
            buttons.each_ref().map(|button| button.label()),
            [Some("Minimize"), Some("Maximize"), Some("Close")]
        );
        // 从左到右：拖动区、最小化、最大化、关闭，都在标题栏里。
        let title_bar = window.find("title-bar").bounds();
        let mut left = drag.bounds().right();
        for button in &buttons {
            assert!(button.visible());
            assert!(button.bounds().left() >= left);
            assert!(title_bar.contains(&button.bounds().center()));
            left = button.bounds().right();
        }
    });
}

#[gpui_kit::test]
fn hovering_icon_buttons_shows_the_action_or_the_unavailable_reason(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    for (id, expected) in [
        ("right-sidebar-snippets", "Snippets"),
        ("host-sidebar-new-host", "New Host"),
        ("right-sidebar-files", "The current tab has no remote files"),
    ] {
        in_frame(cx, handle, |window, cx| window.hover(id, cx));
        cx.executor().advance_clock(std::time::Duration::from_secs(1));
        in_frame(cx, handle, |window, _| {
            assert_eq!(window.find("icon-button-tooltip").label(), Some(expected), "{id}");
        });
    }
}

/// 面板入口在标签栏右侧，从左到右排成一排，在三色窗口按钮左边。
#[gpui_kit::test]
fn the_panel_tools_sit_in_the_title_bar_left_of_the_window_buttons(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        let close = window.find("window-close").bounds();
        assert_eq!(close.size.width, px(22.));
        let title_bar = window.find("title-bar").bounds();
        let mut left = window.find("title-drag").bounds().right();
        for id in [
            "right-sidebar-files",
            "right-sidebar-snippets",
            "right-sidebar-credentials",
            "right-sidebar-agent",
        ] {
            let icon = window.find(id);
            assert!(title_bar.contains(&icon.bounds().center()), "{id}");
            assert!(icon.bounds().left() >= left, "{id}");
            assert!(icon.bounds().right() <= close.left(), "{id}");
            left = icon.bounds().right();
        }
    });
}

#[gpui_kit::test]
fn collapsing_the_host_sidebar_puts_an_expand_button_at_the_left_of_the_tab_rail(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| {
        assert!(window.find("host-sidebar").visible());
        assert!(window.try_find("expand-host-sidebar").is_none());
        // 收起按钮在主机栏底部，主机列表下面；Ctrl+Shift+B 照旧。
        let collapse = window.find("collapse-host-sidebar");
        assert_eq!(collapse.label(), Some("Collapse Hosts Sidebar"));
        assert!(collapse.bounds().top() > window.find("host-sidebar-new-host").bounds().bottom());
        window.click("collapse-host-sidebar", cx);
    });
    in_frame(cx, handle, |window, cx| {
        if cfg!(target_os = "macos") {
            // macOS 收起主机栏时整条侧栏让给红绿灯，展开按钮在标签轨道左边。
            assert!(window.try_find("host-sidebar").is_none());
        } else {
            assert_eq!(window.find("host-sidebar").bounds().size.width, px(44.));
        }
        let expand = window.find("expand-host-sidebar");
        assert_eq!(expand.label(), Some("Expand Hosts Sidebar"));
        if cfg!(target_os = "macos") {
            assert!(expand.bounds().right() <= window.find("tab-rail").bounds().left());
            assert!(expand.bounds().right() <= window.find("new-tab").bounds().left());
        } else {
            assert!(
                window
                    .find("host-sidebar")
                    .bounds()
                    .contains(&expand.bounds().center())
            );
        }
        window.click("expand-host-sidebar", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-sidebar").bounds().size.width, px(240.));
        assert!(window.try_find("expand-host-sidebar").is_none());
    });
    cx.update(|cx| assert!(workspace.read(cx).is_host_sidebar_open()));
}

#[gpui_kit::test]
fn ctrl_shift_b_toggles_the_host_sidebar(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-b", cx));
    in_frame(cx, handle, |window, _| {
        if cfg!(target_os = "macos") {
            assert!(window.try_find("host-sidebar").is_none());
        } else {
            assert_eq!(window.find("host-sidebar").bounds().size.width, px(44.));
        }
        assert!(window.find("expand-host-sidebar").visible());
    });
    cx.update(|cx| assert!(!workspace.read(cx).is_host_sidebar_open()));

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-b", cx));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-sidebar").bounds().size.width, px(240.));
    });
}

/// 一个命令一个处理器：按钮、快捷键和直接派发的 action 走的是同一个处理器。
#[gpui_kit::test]
fn the_button_the_shortcut_and_the_action_reach_one_handler(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    let open = |cx: &mut TestAppContext| cx.update(|cx| workspace.read(cx).is_host_sidebar_open());

    dispatch(cx, handle, ToggleHostSidebar);
    assert!(!open(cx));
    in_frame(cx, handle, |window, cx| {
        window.click("expand-host-sidebar", cx)
    });
    assert!(open(cx));
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-b", cx));
    assert!(!open(cx));
}

#[gpui_kit::test]
fn the_title_bar_shows_files_snippets_credentials_and_agent(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| {
        let icons = [
            "right-sidebar-files",
            "right-sidebar-snippets",
            "right-sidebar-credentials",
            "right-sidebar-agent",
        ]
        .map(|id| window.find(id));
        assert_eq!(
            icons.each_ref().map(|icon| icon.label()),
            [Some("Files"), Some("Snippets"), Some("Credentials"), Some("Agent")]
        );
        // 一排，从左到右，在标签栏右侧。
        let title_bar = window.find("title-bar").bounds();
        let mut left = window.find("title-drag").bounds().right();
        for icon in &icons {
            assert!(icon.visible());
            assert!(title_bar.contains(&icon.bounds().center()));
            assert!(icon.bounds().left() >= left);
            left = icon.bounds().right();
        }
        // 此时点了还没有内容。
        window.click("right-sidebar-snippets", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert!(window.find("new-tab-page").visible());
    });
}

/// 设置写不进文件时要告诉用户，不能静默吞掉。
#[gpui_kit::test]
fn a_settings_change_that_cannot_be_saved_is_reported(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // 设置文件的位置上是一个目录，写不进去。
    let path = dir.path().join("settings.json");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "").unwrap();
    let (settings, _) = SettingsStore::load(path);
    let (handle, workspace) = open_workspace_with(cx, settings, fake_services());

    cx.update(|cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.settings().update(cx, |settings, cx| {
                settings.update(|settings| settings.appearance = Appearance::Dark, cx)
            })
        })
    });
    cx.run_until_parked();

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.notifications(cx).len(), 1);
    });
    // 改动留在内存里。
    cx.update(|cx| {
        assert_eq!(
            workspace.read(cx).settings().read(cx).settings().appearance,
            Appearance::Dark
        );
    });
}

/// 标题栏右侧的深浅切换：点一下把「应用外观」固定成相反的那种，图标跟着换（浅色显示月亮，深色显示太阳）。
#[gpui_kit::test]
fn the_title_bar_toggle_switches_between_light_and_dark(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    let appearance = |cx: &mut TestAppContext| {
        cx.update(|cx| workspace.read(cx).settings().read(cx).settings().appearance)
    };
    assert_eq!(appearance(cx), Appearance::System);
    cx.update(|cx| assert!(!cx.theme().is_dark()));

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("toggle-appearance").label(), Some("Switch to Dark"));
        window.click("toggle-appearance", cx);
    });
    assert_eq!(appearance(cx), Appearance::Dark);
    cx.update(|cx| assert!(cx.theme().is_dark()));

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("toggle-appearance").label(), Some("Switch to Light"));
        window.click("toggle-appearance", cx);
    });
    assert_eq!(appearance(cx), Appearance::Light);
    cx.update(|cx| assert!(!cx.theme().is_dark()));
}

/// 深浅切换和设置按钮在主机栏底部，和收起按钮一排；分屏按钮不在这里，在每张卡片的顶行。
#[gpui_kit::test]
fn the_sidebar_footer_holds_the_appearance_and_settings_buttons(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    dispatch(cx, handle, NewLocalTerminalTab);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("split-right").is_none());
        assert!(window.try_find("split-down").is_none());
        let sidebar = window.find("host-sidebar").bounds();
        let title_bar = window.find("title-bar").bounds();
        let status_center_y = window.find(("card-status-path-group", 1usize)).bounds().center().y;
        let ids = ["toggle-appearance", "open-settings"];
        let mut left = sidebar.left();
        for id in ids {
            let button = window.find(id);
            assert!(sidebar.contains(&button.bounds().center()), "{id}");
            assert!(!title_bar.contains(&button.bounds().center()), "{id}");
            assert!(button.bounds().left() >= left, "{id}");
            assert!(
                (button.bounds().center().y - status_center_y).abs() < px(0.01),
                "{id} center_y {:?} != status {:?}",
                button.bounds().center().y,
                status_center_y
            );
            left = button.bounds().right();
        }
        let collapse = window.find("collapse-host-sidebar");
        assert!(sidebar.contains(&collapse.bounds().center()));
        assert!(collapse.bounds().left() >= left);
        assert!(
            (collapse.bounds().center().y - status_center_y).abs() < px(0.01),
            "collapse center_y {:?} != status {:?}",
            collapse.bounds().center().y,
            status_center_y
        );
    });
}

/// 顶栏下面那一排（卡片顶行、右侧面板标题行）最右的几个按钮，中心要和窗口的最小化、
/// 最大化、关闭圆点对齐。
#[gpui_kit::test]
fn the_buttons_below_the_title_bar_line_up_with_the_window_buttons(cx: &mut TestAppContext) {
    if cfg!(target_os = "macos") {
        return;
    }
    let (handle, _) = open_workspace(cx);
    dispatch(cx, handle, NewLocalTerminalTab);

    // 卡片顶行：关闭对关闭圆点，向下分屏对最大化圆点，向右分屏对最小化圆点。
    in_frame(cx, handle, |window, _| {
        let window_x = |id: &'static str| window.find(id).bounds().center().x;
        let close = window_x("window-close");
        let maximize = window_x("window-max");
        let minimize = window_x("window-min");
        let card = |name: &'static str| window.find((name, 1usize)).bounds().center().x;
        assert!((card("card-close") - close).abs() < px(0.01));
        assert!((card("card-split-down") - maximize).abs() < px(0.01));
        assert!((card("card-split-right") - minimize).abs() < px(0.01));
        // 本地卡片不能交给 Agent，没有那个按钮。
        assert!(window.try_find(("card-agent", 1usize)).is_none());
    });

    // 刚分出来的新卡片（选择器）顶行：关闭按钮也对齐窗口关闭圆点。
    dispatch(cx, handle, SplitRight);
    for _ in 0..3 {
        in_frame(cx, handle, |_, _| {});
    }
    in_frame(cx, handle, |window, _| {
        let close = window.find("window-close").bounds().center().x;
        let picker = window.find("card-picker-close").bounds().center().x;
        assert!(
            (picker - close).abs() < px(0.01),
            "picker {picker:?} != close {close:?}"
        );
    });

    // 右侧面板标题行：关闭对关闭圆点，加号对最大化圆点。
    dispatch(cx, handle, ShowCredentials);
    in_frame(cx, handle, |window, _| {
        let window_x = |id: &'static str| window.find(id).bounds().center().x;
        assert!(
            (window.find("credentials-close").bounds().center().x - window_x("window-close")).abs()
                < px(0.01)
        );
        assert!(
            (window.find("credentials-new").bounds().center().x - window_x("window-max")).abs()
                < px(0.01)
        );
    });
}
