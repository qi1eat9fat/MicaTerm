//! 设置标签（UI 重做工单 12）的 UI 测试：外观与终端两类，改动立即生效并写进注入的设置存储。

use micaterm::app::CloseActiveTab;
use micaterm::theme::INTERFACE_FONT_SIZE;

use crate::support::*;

/// 按 Ctrl+,（macOS 上 ⌘,）打开设置标签。
fn open_settings(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| {
        #[cfg(target_os = "macos")]
        window.press("cmd-,", cx);
        #[cfg(not(target_os = "macos"))]
        window.press("ctrl-,", cx);
    });
}

/// 设置是一个标签（26 号）：Ctrl+, 打开；再按一次切回同一个，不开第二个；关掉就没了。
#[gpui_kit::test]
fn ctrl_comma_opens_the_settings_tab_on_appearance(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_settings(cx, handle);
    let tabs = |cx: &mut TestAppContext| cx.update(|cx| workspace.read(cx).tab_ids().len());
    assert_eq!(tabs(cx), 1);
    assert!(cx.update(|cx| workspace.read(cx).settings_tab().is_some()));

    in_frame(cx, handle, |window, _| {
        assert!(window.find("settings").visible());
        // 左栏右边界与上方第一个标签（Settings）严格对齐。
        assert_eq!(
            window.find("settings-categories").bounds().right(),
            window.find(("tab", 1_usize)).bounds().right(),
        );
        assert_eq!(window.find("settings-title").label(), Some("Appearance"));
        assert!(window.find("settings-applies-at-once").visible());
        let appearance = window.find(("settings-category", 0_usize));
        let terminal = window.find(("settings-category", 1_usize));
        assert_eq!(appearance.label(), Some("Appearance"));
        assert_eq!(terminal.label(), Some("Terminal"));
        assert_eq!(appearance.selected(), Some(true));
        assert_eq!(terminal.selected(), Some(false));
        // 外观在前：能看到应用外观，看不到终端字体。
        assert!(window.find(("appearance", 0_usize)).visible());
        assert!(window.try_find("terminal-font-family").is_none());
        // 都是默认值：没有「恢复默认」。
        assert!(window.try_find("appearance-reset").is_none());
        assert!(window.try_find("mica-reset").is_none());
    });

    dispatch(cx, handle, NewLocalTerminalTab);
    assert_eq!(tabs(cx), 2);
    open_settings(cx, handle);
    assert_eq!(tabs(cx), 2);
    in_frame(cx, handle, |window, _| {
        assert!(window.find("settings").visible())
    });

    dispatch(cx, handle, CloseActiveTab);
    assert_eq!(tabs(cx), 1);
    assert!(cx.update(|cx| workspace.read(cx).settings_tab().is_none()));
}

/// 改过的设置在标题后面有「恢复默认」，点了改回默认、它也就没了（26 号 `.stg-row .rs`）。
#[gpui_kit::test]
fn a_changed_setting_can_be_reset_to_its_default(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_settings(cx, handle);

    in_frame(cx, handle, |window, cx| window.click("mica", cx));
    assert!(stored(cx, &workspace).interface.mica);
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("mica-reset").label(), Some("Restore Default"));
        // 别的没改过的还是没有。
        assert!(window.try_find("appearance-reset").is_none());
        window.click("mica-reset", cx);
    });
    assert!(!stored(cx, &workspace).interface.mica);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("mica-reset").is_none());
        assert_eq!(window.find("mica").checked(), Some(false));
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
            Some("System")
        );
        assert_eq!(window.find(("appearance", 0_usize)).selected(), Some(true));
        window.click(("appearance", 2_usize), cx);
    });
    assert_eq!(stored(cx, &workspace).appearance, Appearance::Dark);
    cx.update(|cx| {
        let theme = cx.theme();
        assert!(theme.is_dark());
        // 弹窗、菜单这些用原型 `.win.dark` 的颜色。
        assert_eq!(theme.theme_name().as_ref(), "MicaTerm Dark");
        assert_eq!(theme.primary, Tokens::dark().brand);
        // 组件主题按 8 位十六进制写颜色，浮层色是混出来的，留一档误差。
        let popup = Tokens::dark().popup.to_rgb();
        let theme_popup = theme.popover.to_rgb();
        let close = |a: f32, b: f32| (a - b).abs() <= 1. / 255.;
        assert!(
            close(popup.r, theme_popup.r)
                && close(popup.g, theme_popup.g)
                && close(popup.b, theme_popup.b),
            "{theme_popup:?} != {popup:?}"
        );
    });
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find(("appearance", 2_usize)).selected(), Some(true));
        assert_eq!(window.find(("appearance", 0_usize)).selected(), Some(false));
        window.click(("appearance", 1_usize), cx);
    });
    assert_eq!(stored(cx, &workspace).appearance, Appearance::Light);
    cx.update(|cx| {
        assert!(!cx.theme().is_dark());
        assert_eq!(cx.theme().theme_name().as_ref(), "MicaTerm Light");
        assert_eq!(cx.theme().primary, Tokens::light().brand);
    });
}

/// 界面字号不是设置（用户定）：外观页没有这一项，主题字号就是固定的那个。
#[gpui_kit::test]
fn the_interface_font_size_is_fixed(cx: &mut TestAppContext) {
    let (handle, _workspace) = open_workspace(cx);
    open_settings(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("interface-font-size").is_none());
    });
    cx.update(|cx| assert_eq!(cx.theme().font_size, px(INTERFACE_FONT_SIZE)));
}

/// 终端主题只列当前界面明暗那一色；浅色和深色各自记住自己选的那套。
#[gpui_kit::test]
fn each_appearance_lists_only_its_own_terminal_themes(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    let in_effect = |cx: &mut TestAppContext| cx.update(|cx| TerminalColors::current(cx).name());
    open_settings(cx, handle);
    assert_eq!(in_effect(cx), "Air Neutral Light");

    in_frame(cx, handle, |window, cx| {
        // 浅色界面只列浅色主题：深色那一色的卡片根本不出现。
        assert!(window.try_find("terminal-theme-air-dark").is_none());
        assert!(window.try_find("terminal-theme-tokyo-night").is_none());
        assert_eq!(
            window.find("terminal-theme-air-light").selected(),
            Some(true)
        );
        assert_eq!(
            window.find("terminal-theme-tokyo-day").selected(),
            Some(false)
        );
        // 四张浅色卡片分两列：左列 air-light / tokyo-day，右列 catppuccin-latte / rose-pine-dawn。
        let air = window.find("terminal-theme-air-light").bounds();
        let tokyo = window.find("terminal-theme-tokyo-day").bounds();
        let latte = window.find("terminal-theme-catppuccin-latte").bounds();
        assert_eq!(air.left(), tokyo.left());
        assert_eq!(air.top(), latte.top());
        assert!(air.right() <= latte.left());
        window.click("terminal-theme-rose-pine-dawn", cx);
    });
    // 浅色外观下立刻换上新选的浅色主题，背景继续跟随界面的 `Tokens.content`。
    assert_eq!(
        stored(cx, &workspace).terminal_theme.light,
        "rose-pine-dawn"
    );
    assert_eq!(in_effect(cx), "Rosé Pine Dawn");
    cx.update(|cx| {
        assert_eq!(TerminalColors::background(cx), Tokens::light().content);
    });

    // 换成深色外观：列表只列深色主题，浅色卡片不再出现；深色界面用它自己那套（默认 Air Neutral Dark）。
    in_frame(cx, handle, |window, cx| {
        window.click(("appearance", 2_usize), cx)
    });
    assert_eq!(in_effect(cx), "Air Neutral Dark");
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("terminal-theme-air-light").is_none());
        assert!(window.try_find("terminal-theme-rose-pine-dawn").is_none());
        assert_eq!(
            window.find("terminal-theme-air-dark").selected(),
            Some(true)
        );
        window.click("terminal-theme-tokyo-night", cx);
    });
    let settings = stored(cx, &workspace);
    assert_eq!(settings.terminal_theme.dark, "tokyo-night");
    assert_eq!(settings.terminal_theme.light, "rose-pine-dawn");
    assert_eq!(in_effect(cx), "Tokyo Night");

    // 换回浅色：还是之前选的 Rosé Pine Dawn，深色选的不影响它。
    in_frame(cx, handle, |window, cx| {
        window.click(("appearance", 1_usize), cx)
    });
    assert_eq!(in_effect(cx), "Rosé Pine Dawn");
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("terminal-theme-tokyo-night").is_none());
        assert_eq!(
            window.find("terminal-theme-rose-pine-dawn").selected(),
            Some(true)
        );
    });
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
            // 测试环境没装 Maple Mono，退回随包的 JetBrains Mono。
            Some("JetBrains Mono 13 px, line height 1.6")
        );
        // 默认：右键显示菜单，选中即复制关着。
        assert_eq!(window.find("right-click").label(), Some("Show Menu"));
        assert_eq!(window.find("copy-on-select").checked(), Some(false));
    });
}

/// 字号、行高是下拉框（26 号 `.stg-dd`）：字号 10–28 每个整数，行高 1.0–2.0 每档 0.1。
#[gpui_kit::test]
fn the_terminal_font_size_and_line_height_change_at_once_and_are_stored(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_terminal_settings(cx, handle);

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("terminal-font-size").label(), Some("13"));
        assert_eq!(window.find("terminal-line-height").label(), Some("1.6"));
        window.click("terminal-font-size", cx)
    });
    in_frame(cx, handle, |window, cx| {
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(("menu-option", 0_usize)).label(), Some("10"));
        assert_eq!(menu.find(("menu-option", 18_usize)).label(), Some("28"));
        menu.click(4_usize, cx);
    });
    assert_eq!(stored(cx, &workspace).terminal_font.size, 14.);
    cx.update(|cx| assert_eq!(cx.global::<TerminalFont>().size, px(14.)));

    in_frame(cx, handle, |window, cx| {
        window.click("terminal-line-height", cx)
    });
    in_frame(cx, handle, |window, cx| {
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(("menu-option", 0_usize)).label(), Some("1.0"));
        assert_eq!(menu.find(("menu-option", 10_usize)).label(), Some("2.0"));
        menu.click(2_usize, cx);
    });
    assert_eq!(stored(cx, &workspace).terminal_font.line_height, 1.2);
    cx.update(|cx| assert_eq!(cx.global::<TerminalFont>().line_height, 1.2));
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("terminal-font-preview").label(),
            Some("JetBrains Mono 14 px, line height 1.2")
        );
        assert_eq!(window.find("terminal-font-size").label(), Some("14"));
        assert!(window.find("terminal-font-size-reset").visible());
    });

    // 在终端里按 Ctrl+= 调出来的数不在菜单里，框上照样显示。
    cx.update(|cx| {
        workspace
            .read(cx)
            .settings()
            .clone()
            .update(cx, |store, cx| {
                store.update(|settings| settings.terminal_font.size = 14.5, cx)
            })
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("terminal-font-size").label(), Some("14.5"));
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
        assert_eq!(
            menu.find(("menu-option", 0_usize)).label(),
            Some("Show Menu")
        );
        assert_eq!(menu.find(("menu-option", 1_usize)).label(), Some("Paste"));
        assert_eq!(
            menu.find(("menu-option", 2_usize)).label(),
            Some("Copy if text is selected, otherwise paste")
        );
        assert_eq!(menu.find(("menu-option", 3_usize)).label(), Some("Do Nothing"));
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
            Some("Copy if text is selected, otherwise paste")
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

    // 关掉设置标签，新开一个本地终端：用的是刚填的 shell。
    dispatch(cx, handle, CloseActiveTab);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("settings").is_none())
    });
    dispatch(cx, handle, NewLocalTerminalTab);
    assert_eq!(provider.shells(), [LocalShell::new("cmd.exe /k")]);
}
