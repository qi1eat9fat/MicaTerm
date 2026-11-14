//! 窗口壳（工单 01）：标题栏、主机栏、右侧栏、新标签页。

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
/// 新建主机在上，行高 32、列宽 320；没有快速连接和最近连接。
#[gpui_kit::test]
fn the_new_tab_page_lists_new_host_and_local_terminal_with_their_shortcuts(
    cx: &mut TestAppContext,
) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        let host = window.find("new-tab-new-host");
        let terminal = window.find("new-tab-local-terminal");
        assert_eq!(host.label(), Some("新建主机"));
        assert_eq!(terminal.label(), Some("本地终端"));
        assert!(host.bounds().bottom() <= terminal.bounds().top());
        assert_eq!(host.bounds().size.height, px(32.));
        assert_eq!(host.bounds().size.width, px(320.));
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
            [Some("最小化"), Some("最大化"), Some("关闭")]
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

/// 关闭按钮的 × 和右侧图标栏那列图标在一条竖线上（用户定：窗口按钮和图标栏内宽一样，都是 32）。
#[gpui_kit::test]
fn the_close_button_lines_up_with_the_right_rail_icons(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        let close = window.find("window-close").bounds();
        assert_eq!(close.size.width, px(32.));
        for id in [
            "right-sidebar-files",
            "right-sidebar-snippets",
            "right-sidebar-credentials",
        ] {
            assert_eq!(
                window.find(id).bounds().center().x,
                close.center().x,
                "{id}"
            );
        }
    });
}

#[gpui_kit::test]
fn collapsing_the_host_sidebar_puts_an_expand_button_at_the_left_of_the_tab_rail(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        assert!(window.find("host-sidebar").visible());
        assert!(window.try_find("expand-host-sidebar").is_none());
    });
    // 主机栏里没有收起按钮（26 号），收起用 Ctrl+Shift+B。
    dispatch(cx, handle, ToggleHostSidebar);
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("host-sidebar").is_none());
        let expand = window.find("expand-host-sidebar");
        assert_eq!(expand.label(), Some("展开主机栏"));
        // 标签轨道最左边：在轨道和「+」前面。
        assert!(expand.bounds().right() <= window.find("tab-rail").bounds().left());
        assert!(expand.bounds().right() <= window.find("new-tab").bounds().left());
        window.click("expand-host-sidebar", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert!(window.find("host-sidebar").visible());
        assert!(window.try_find("expand-host-sidebar").is_none());
    });
    cx.update(|cx| assert!(workspace.read(cx).is_host_sidebar_open()));
}

#[gpui_kit::test]
fn ctrl_shift_b_toggles_the_host_sidebar(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-b", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("host-sidebar").is_none());
        assert!(window.find("expand-host-sidebar").visible());
    });
    cx.update(|cx| assert!(!workspace.read(cx).is_host_sidebar_open()));

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-b", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.find("host-sidebar").visible());
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
fn the_right_sidebar_shows_files_snippets_and_credentials(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| {
        let icons = [
            "right-sidebar-files",
            "right-sidebar-snippets",
            "right-sidebar-credentials",
        ]
        .map(|id| window.find(id));
        assert_eq!(
            icons.each_ref().map(|icon| icon.label()),
            [Some("文件"), Some("片段"), Some("凭据")]
        );
        // 一列，从上到下，在新标签页右边。
        let new_tab_page = window.find("new-tab-page").bounds();
        let mut top = new_tab_page.top();
        for icon in &icons {
            assert!(icon.visible());
            assert!(icon.bounds().top() >= top);
            assert!(icon.bounds().left() >= new_tab_page.right());
            top = icon.bounds().bottom();
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
        assert_eq!(window.find("toggle-appearance").label(), Some("切换到深色"));
        window.click("toggle-appearance", cx);
    });
    assert_eq!(appearance(cx), Appearance::Dark);
    cx.update(|cx| assert!(cx.theme().is_dark()));

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("toggle-appearance").label(), Some("切换到浅色"));
        window.click("toggle-appearance", cx);
    });
    assert_eq!(appearance(cx), Appearance::Light);
    cx.update(|cx| assert!(!cx.theme().is_dark()));
}

/// 设置按钮在标题栏右侧，和深浅切换一排；分屏按钮不在这里，在每张卡片的状态栏上（2026-11-14）。
#[gpui_kit::test]
fn the_title_bar_holds_the_appearance_and_settings_buttons(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("split-right").is_none());
        assert!(window.try_find("split-down").is_none());
        let title_bar = window.find("title-bar").bounds();
        let ids = ["toggle-appearance", "open-settings"];
        let mut left = window.find("title-drag").bounds().right();
        for id in ids {
            let button = window.find(id);
            assert!(title_bar.contains(&button.bounds().center()), "{id}");
            assert!(button.bounds().left() >= left, "{id}");
            left = button.bounds().right();
        }
        assert!(left <= window.find("window-min").bounds().left());
    });
}
