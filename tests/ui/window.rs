//! 窗口壳（工单 01）：标题栏、主机栏、右侧栏、开始卡片。

use crate::support::*;

#[gpui_kit::test]
fn startup_shows_the_start_card(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, _| {
        assert!(window.find("start-card").visible());
        assert!(window.find("start-new-local-terminal").visible());
        assert!(window.find("start-new-host").visible());
        // 没有标签时开始卡片持有焦点，快捷键才有焦点路径可走。
        assert_eq!(window.find("start-card").focused(), Some(true));
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

#[gpui_kit::test]
fn collapsing_the_host_sidebar_puts_an_expand_button_at_the_left_of_the_tab_rail(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| {
        assert!(window.find("host-sidebar").visible());
        assert!(window.try_find("expand-host-sidebar").is_none());
        window.click("collapse-host-sidebar", cx);
    });
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
            [Some("文件"), Some("命令片段"), Some("凭据")]
        );
        // 一列，从上到下，在开始卡片右边。
        let start_card = window.find("start-card").bounds();
        let mut top = start_card.top();
        for icon in &icons {
            assert!(icon.visible());
            assert!(icon.bounds().top() >= top);
            assert!(icon.bounds().left() >= start_card.right());
            top = icon.bounds().bottom();
        }
        // 此时点了还没有内容。
        window.click("right-sidebar-snippets", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert!(window.find("start-card").visible());
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
