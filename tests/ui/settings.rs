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

/// 注入的设置存储里现在的设置。
fn stored(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> AppSettings {
    cx.update(|cx| workspace.read(cx).settings().read(cx).settings())
}

#[gpui_kit::test]
fn the_app_appearance_changes_at_once_and_is_stored(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_settings(cx, handle);

    in_frame(cx, handle, |window, cx| {
        // 默认跟随系统。
        assert_eq!(
            window.find(("appearance", 0_usize)).label(),
            Some("跟随系统")
        );
        assert_eq!(window.find(("appearance", 0_usize)).selected(), Some(true));
        window.click(("appearance", 2_usize), cx);
    });
    assert_eq!(stored(cx, &workspace).appearance, Appearance::Dark);
    cx.update(|cx| {
        let theme = cx.theme();
        assert!(theme.is_dark());
        // 对话框、菜单这些用原型 `.win.dark` 的颜色。
        assert_eq!(theme.theme_name().as_ref(), "QiShell Dark");
        assert_eq!(theme.primary, Tokens::dark().accent);
        assert_eq!(theme.popover, Tokens::dark().panel);
    });
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find(("appearance", 2_usize)).selected(), Some(true));
        assert_eq!(window.find(("appearance", 0_usize)).selected(), Some(false));
        window.click(("appearance", 1_usize), cx);
    });
    assert_eq!(stored(cx, &workspace).appearance, Appearance::Light);
    cx.update(|cx| {
        assert!(!cx.theme().is_dark());
        assert_eq!(cx.theme().theme_name().as_ref(), "QiShell Light");
        assert_eq!(cx.theme().primary, Tokens::light().accent);
    });
}

#[gpui_kit::test]
fn the_interface_font_size_changes_at_once_and_is_stored(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_settings(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click("interface-font-size", cx)
    });
    in_frame(cx, handle, |window, cx| {
        // 12 到 20，第 5 项是 16。
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(4_usize).label(), Some("16"));
        menu.click(4_usize, cx);
    });
    assert_eq!(stored(cx, &workspace).interface.font_size, 16.);
    cx.update(|cx| assert_eq!(cx.theme().font_size, px(16.)));
}

#[gpui_kit::test]
fn each_appearance_has_its_own_terminal_theme(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    let in_effect = |cx: &mut TestAppContext| cx.update(|cx| TerminalColors::current(cx).name());
    open_settings(cx, handle);
    assert_eq!(in_effect(cx), "QiShell Light");

    in_frame(cx, handle, |window, cx| {
        let qishell = window.find("terminal-theme-qishell-light");
        assert_eq!(qishell.selected(), Some(true));
        assert_eq!(
            window.find("terminal-theme-qishell-dark").selected(),
            Some(true)
        );
        assert_eq!(
            window.find("terminal-theme-tokyo-day").selected(),
            Some(false)
        );
        // 四对：浅色栏在左、深色栏在右，同一行是一对。
        for (light, dark) in [
            ("qishell-light", "qishell-dark"),
            ("tokyo-day", "tokyo-night"),
            ("catppuccin-latte", "catppuccin-mocha"),
            ("rose-pine-dawn", "rose-pine"),
        ] {
            let id = |key: &str| ElementId::Name(format!("terminal-theme-{key}").into());
            let light = window.find(id(light)).bounds();
            let dark = window.find(id(dark)).bounds();
            assert!(light.right() <= dark.left());
            assert_eq!(light.top(), dark.top());
        }
        window.click("terminal-theme-rose-pine-dawn", cx);
    });
    // 浅色外观下立刻换上新选的浅色主题。
    assert_eq!(
        stored(cx, &workspace).terminal_theme.light,
        "rose-pine-dawn"
    );
    assert_eq!(in_effect(cx), "Rosé Pine Dawn");

    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("terminal-theme-rose-pine-dawn").selected(),
            Some(true)
        );
        assert_eq!(
            window.find("terminal-theme-qishell-light").selected(),
            Some(false)
        );
        // 选深色栏的不影响浅色外观下的终端。
        window.click("terminal-theme-tokyo-night", cx);
    });
    let settings = stored(cx, &workspace);
    assert_eq!(settings.terminal_theme.dark, "tokyo-night");
    assert_eq!(settings.terminal_theme.light, "rose-pine-dawn");
    assert_eq!(in_effect(cx), "Rosé Pine Dawn");

    in_frame(cx, handle, |window, cx| {
        window.click(("appearance", 2_usize), cx)
    });
    assert_eq!(in_effect(cx), "Tokyo Night");
}

/// 打开设置，切到「终端」。
fn open_terminal_settings(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    open_settings(cx, handle);
    in_frame(cx, handle, |window, cx| {
        window.click(("settings-category", 1_usize), cx)
    });
}

#[gpui_kit::test]
fn the_terminal_category_shows_the_font_interaction_and_local_shell(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    open_terminal_settings(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("settings-category", 1_usize)).selected(),
            Some(true)
        );
        assert_eq!(
            window.find(("settings-category", 0_usize)).selected(),
            Some(false)
        );
        assert!(window.try_find(("appearance", 0_usize)).is_none());
        for id in [
            "terminal-font-family",
            "terminal-font-size",
            "terminal-line-height",
            "terminal-font-preview",
            "copy-on-select",
            "right-click",
            "local-shell",
        ] {
            assert!(window.find(id).visible(), "{id}");
        }
        assert_eq!(
            window.find("terminal-font-preview").label(),
            Some("JetBrains Mono 11.6 px，行高 1.45")
        );
        // 默认：右键显示菜单，选中即复制关着。
        assert_eq!(window.find("right-click").label(), Some("显示菜单"));
        assert_eq!(window.find("copy-on-select").checked(), Some(false));
    });
}

#[gpui_kit::test]
fn the_terminal_font_size_and_line_height_change_at_once_and_are_stored(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_terminal_settings(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click("terminal-font-size", cx)
    });
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.input("14", cx);
    });
    assert_eq!(stored(cx, &workspace).terminal_font.size, 14.);
    cx.update(|cx| assert_eq!(cx.global::<TerminalFont>().size, px(14.)));

    // 行高点「+」加一档（0.05）。
    in_frame(cx, handle, |window, cx| {
        window.within("terminal-line-height").click("increment", cx)
    });
    assert_eq!(stored(cx, &workspace).terminal_font.line_height, 1.5);
    cx.update(|cx| assert_eq!(cx.global::<TerminalFont>().line_height, 1.5));
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("terminal-font-preview").label(),
            Some("JetBrains Mono 14 px，行高 1.5")
        );
    });
}

#[gpui_kit::test]
fn copy_on_select_and_the_right_click_action_change_at_once_and_are_stored(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open_workspace(cx);
    open_terminal_settings(cx, handle);

    in_frame(cx, handle, |window, cx| window.click("copy-on-select", cx));
    assert!(stored(cx, &workspace).terminal_interaction.copy_on_select);
    cx.update(|cx| assert!(TerminalInteraction::current(cx).copy_on_select));

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("copy-on-select").checked(), Some(true));
        window.click("right-click", cx);
    });
    in_frame(cx, handle, |window, cx| {
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(0_usize).label(), Some("显示菜单"));
        assert_eq!(menu.find(1_usize).label(), Some("粘贴"));
        assert_eq!(
            menu.find(2_usize).label(),
            Some("有选中文字就复制，否则粘贴")
        );
        assert_eq!(menu.find(3_usize).label(), Some("无动作"));
        menu.click(2_usize, cx);
    });
    assert_eq!(
        stored(cx, &workspace).terminal_interaction.right_click,
        RightClick::CopyOrPaste
    );
    cx.update(|cx| {
        assert_eq!(
            TerminalInteraction::current(cx).right_click,
            RightClick::CopyOrPaste
        )
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("right-click").label(),
            Some("有选中文字就复制，否则粘贴")
        );
    });
}

#[gpui_kit::test]
fn the_local_shell_typed_in_is_stored_and_new_local_terminals_run_it(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open_workspace_with(
        cx,
        settings_store(),
        services_with_terminals(provider.clone()),
    );
    open_terminal_settings(cx, handle);

    in_frame(cx, handle, |window, cx| window.click("local-shell", cx));
    in_frame(cx, handle, |window, cx| window.input("cmd.exe /k", cx));
    assert_eq!(
        stored(cx, &workspace).local_terminal.shell.as_deref(),
        Some("cmd.exe /k")
    );

    // 关掉对话框，新开一个本地终端：用的是刚填的 shell。
    in_frame(cx, handle, |window, cx| window.click("settings-close", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("settings").is_none())
    });
    dispatch(cx, handle, NewLocalTerminalTab);
    assert_eq!(provider.shells(), [LocalShell::new("cmd.exe /k")]);
}
