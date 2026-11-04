//! 设置对话框（工单 07）的 UI 测试：外观与终端两类，改动立即生效并写进注入的设置存储。

use crate::support::*;

/// 按 Ctrl+,（macOS 上 ⌘,）打开设置对话框。
fn open_settings(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| {
        #[cfg(target_os = "macos")]
        window.press("cmd-,", cx);
        #[cfg(not(target_os = "macos"))]
        window.press("ctrl-,", cx);
    });
}

#[gpui_kit::test]
fn ctrl_comma_opens_the_settings_dialog_on_appearance(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    open_settings(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.find("settings").visible());
        let appearance = window.find(("settings-category", 0_usize));
        let terminal = window.find(("settings-category", 1_usize));
        assert_eq!(appearance.label(), Some("外观"));
        assert_eq!(terminal.label(), Some("终端"));
        assert_eq!(appearance.selected(), Some(true));
        assert_eq!(terminal.selected(), Some(false));
        // 外观在前：能看到应用外观，看不到终端字体。
        assert!(window.find(("appearance", 0_usize)).visible());
        assert!(window.try_find("terminal-font-family").is_none());
    });
}
