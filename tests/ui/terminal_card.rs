//! 本地终端卡片（工单 02）：从开始卡片、「+」和 Ctrl+Shift+T 新开终端标签，卡片的头部、
//! 状态栏，关卡片关 shell。本地终端一律用假传输，不起真的 shell。

use crate::support::*;

const TAB: (&str, usize) = ("tab", 1usize);
const CARD: (&str, usize) = ("card", 1usize);

/// 用假本地终端打开窗口壳，返回假传输的工厂和 provider。
fn open_with_fake_terminals(
    cx: &mut TestAppContext,
    factory: FakeTerminalFactory,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
    Arc<FixedLocalTerminalTransportProvider>,
) {
    let factory = Arc::new(factory);
    let provider = fake_local_terminals_with(factory.clone());
    let (handle, workspace) = open_workspace_with(
        cx,
        settings_store(),
        services_with_terminals(provider.clone()),
    );
    (handle, workspace, factory, provider)
}

/// 新开的终端标签里是一张本地卡片：标签、卡片、卡片里的终端都在，开始卡片不见了。
fn assert_one_local_card(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("start-card").is_none());
        let tab = window.find(TAB);
        assert!(tab.visible());
        assert!(
            window
                .find("tab-rail")
                .bounds()
                .contains(&tab.bounds().center())
        );
        assert!(window.find(CARD).visible());
        assert!(window.find(("terminal", 1usize)).visible());
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
}

#[gpui_kit::test]
fn the_start_card_opens_a_local_terminal_tab(cx: &mut TestAppContext) {
    let (handle, workspace, factory, provider) =
        open_with_fake_terminals(cx, FakeTerminalFactory::default());

    in_frame(cx, handle, |window, cx| {
        window.click("start-new-local-terminal", cx)
    });

    assert_one_local_card(cx, handle);
    assert_eq!(factory.starts(), 1);
    // 设置里没写 shell：自动检测。
    assert_eq!(provider.shells(), [LocalShell::default()]);
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids(), [TabId(1)]));
}

#[gpui_kit::test]
fn ctrl_shift_t_opens_a_local_terminal_tab(cx: &mut TestAppContext) {
    let (handle, _, factory, _) = open_with_fake_terminals(cx, FakeTerminalFactory::default());

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-t", cx));

    assert_one_local_card(cx, handle);
    assert_eq!(factory.starts(), 1);
}

#[gpui_kit::test]
fn the_plus_beside_the_tab_rail_opens_a_local_terminal_tab(cx: &mut TestAppContext) {
    let (handle, _, factory, _) = open_with_fake_terminals(cx, FakeTerminalFactory::default());

    in_frame(cx, handle, |window, cx| window.click("new-tab", cx));

    assert_one_local_card(cx, handle);
    assert_eq!(factory.starts(), 1);
}

/// 每次都是新的标签和新的 shell，新的那个在前面。
#[gpui_kit::test]
fn each_new_tab_starts_its_own_shell(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) =
        open_with_fake_terminals(cx, FakeTerminalFactory::default());

    dispatch(cx, handle, NewLocalTerminalTab);
    // 焦点在终端里，快捷键照样能用。
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-t", cx));

    assert_eq!(factory.starts(), 2);
    in_frame(cx, handle, |window, _| {
        assert!(window.find(("tab", 1usize)).visible());
        assert!(window.find(("tab", 2usize)).visible());
        assert!(window.try_find(("card", 1usize)).is_none());
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.tab_ids(), [TabId(1), TabId(2)]);
        assert_eq!(workspace.active_tab(), Some(TabId(2)));
    });
}

/// 设置里写了 shell，新的本地终端就跑它。
#[gpui_kit::test]
fn the_shell_from_the_settings_is_started(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let mut settings = settings_store();
    settings
        .update_unnotified(|settings| settings.local_terminal.shell = Some("cmd.exe /k".into()));
    let (handle, _) = open_workspace_with(cx, settings, services_with_terminals(provider.clone()));

    dispatch(cx, handle, NewLocalTerminalTab);

    assert_eq!(provider.shells(), [LocalShell::new("cmd.exe /k")]);
}

/// 头部高 33：程序图标、shell 名、目录；右边只有关闭按钮。
#[gpui_kit::test]
fn the_card_header_shows_the_shell_and_its_folder(cx: &mut TestAppContext) {
    let factory = Arc::new(FakeTerminalFactory::default());
    let provider = Arc::new(
        FixedLocalTerminalTransportProvider::new(factory)
            .with_shell_info(LocalShellInfo::new("pwsh", Some("PowerShell 7.5.2"))),
    );
    let (handle, _) = open_workspace_with(cx, settings_store(), services_with_terminals(provider));

    dispatch(cx, handle, NewLocalTerminalTab);

    in_frame(cx, handle, |window, _| {
        let card = window.find(CARD).bounds();
        let header = window.find(("card-header", 1usize));
        assert_eq!(header.bounds().size.height, px(33.));
        // 紧贴卡片顶边（中间只有 1px 描边）。
        assert!(header.bounds().top() - card.top() <= px(1.));
        // 没有报告目录时是主目录。
        assert_eq!(header.label(), Some("pwsh ~"));
        let tile = window.find(("card-tile", 1usize));
        assert!(header.bounds().contains(&tile.bounds().center()));
        let close = window.find(("card-close", 1usize));
        assert_eq!(close.label(), Some("关闭卡片"));
        assert!(header.bounds().contains(&close.bounds().center()));
        // 关闭在最右边，图标在最左边。
        assert!(tile.bounds().right() < close.bounds().left());
        for button in ["card-split-right", "card-split-down", "card-zoom"] {
            assert!(window.try_find((button, 1usize)).is_none(), "{button}");
        }
        // 标签也写 shell 名和目录。
        assert_eq!(window.find(TAB).label(), Some("pwsh ~"));
    });
}

/// 本机说 shell 换了目录，头部跟着变。
#[gpui_kit::test]
fn the_header_follows_the_folder_of_the_shell(cx: &mut TestAppContext) {
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
    let folder: &'static str = Box::leak(
        std::path::Path::new(&home)
            .join("vibe")
            .join("QiShell")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let (handle, _, _, _) = open_with_fake_terminals(cx, FakeTerminalFactory::in_folder(folder));

    dispatch(cx, handle, NewLocalTerminalTab);

    let expected = if cfg!(windows) {
        r"pwsh ~\vibe\QiShell"
    } else {
        "pwsh ~/vibe/QiShell"
    };
    let label = wait_for_label(cx, handle, ("card-header", 1usize), expected);
    assert_eq!(label.as_deref(), Some(expected));
}

/// 底部状态栏：「本地终端」、shell 版本；右边是编码、终端类型、尺寸。
#[gpui_kit::test]
fn the_status_bar_says_local_terminal_and_the_shell_version(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_with_fake_terminals(cx, FakeTerminalFactory::default());

    dispatch(cx, handle, NewLocalTerminalTab);

    in_frame(cx, handle, |window, _| {
        let card = window.find(CARD).bounds();
        let status = window.find(("card-status", 1usize));
        assert_eq!(status.bounds().size.height, px(24.));
        // 紧贴卡片底边（中间只有 1px 描边）。
        assert!(card.bottom() - status.bounds().bottom() <= px(1.));
        let label = status.label().expect("the status bar says what it shows");
        assert!(
            label.starts_with("本地终端 · PowerShell 7.5.2 · UTF-8 · xterm-256color · "),
            "{label}"
        );
        // 尺寸是终端现在的列数 × 行数。
        let size = label.rsplit(" · ").next().unwrap();
        let (columns, rows) = size.split_once('×').expect("columns × rows");
        assert!(columns.parse::<usize>().unwrap() > 20, "{label}");
        assert!(rows.parse::<usize>().unwrap() > 5, "{label}");
    });
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids().len(), 1));
}

/// 卡片里是能用的终端：shell 打的字出现在屏幕上，键盘输入送到 shell。
#[gpui_kit::test]
fn the_card_runs_the_shell(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) =
        open_with_fake_terminals(cx, FakeTerminalFactory::printing("你好"));

    dispatch(cx, handle, NewLocalTerminalTab);
    let card = CardId(1);
    let screen = wait_for_screen(cx, &workspace, card, "你好");
    assert!(screen.contains("run:1$ alpha.txt 会议纪要.md"), "{screen}");

    in_frame(cx, handle, |window, cx| window.input("ls", cx));
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    assert!(wait_until(|| factory.written_text().contains("ls\r")));
}

/// 卡片上的 × 关闭这张卡片和它的 shell；关掉最后一张，回到开始卡片。
#[gpui_kit::test]
fn closing_the_card_closes_its_shell(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) =
        open_with_fake_terminals(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, NewLocalTerminalTab);
    assert_eq!(factory.shutdowns(), 0);

    in_frame(cx, handle, |window, cx| {
        window.click(("card-close", 1usize), cx)
    });

    assert_eq!(factory.wait_for_shutdowns(1), 1);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD).is_none());
        assert!(window.try_find(TAB).is_none());
        assert!(window.find("start-card").visible());
        assert_eq!(window.find("start-card").focused(), Some(true));
    });
    cx.update(|cx| assert!(workspace.read(cx).tab_ids().is_empty()));
}

/// 关掉一张卡片，另一个标签的 shell 不受影响。
#[gpui_kit::test]
fn closing_one_card_leaves_the_other_shells_running(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) =
        open_with_fake_terminals(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, NewLocalTerminalTab);
    dispatch(cx, handle, NewLocalTerminalTab);

    dispatch(cx, handle, CloseCard(CardId(2)));

    assert_eq!(factory.wait_for_shutdowns(1), 1);
    // 再等一会儿，确认只关了一个。
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(factory.shutdowns(), 1);
    in_frame(cx, handle, |window, _| {
        assert!(window.find(("card", 1usize)).visible());
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.tab_ids(), [TabId(1)]);
        assert_eq!(workspace.active_tab(), Some(TabId(1)));
    });
}

/// shell 自己退出了，卡片留着，终端里写明退出码。
#[gpui_kit::test]
fn a_shell_that_exits_leaves_its_card_and_says_why(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_with_fake_terminals(cx, FakeTerminalFactory::exit_first());

    dispatch(cx, handle, NewLocalTerminalTab);

    let screen = wait_for_screen(cx, &workspace, CardId(1), "退出码 9");
    assert!(screen.contains("[进程已退出，退出码 9]"), "{screen}");
    in_frame(cx, handle, |window, _| assert!(window.find(CARD).visible()));
}

/// shell 起不来，卡片留着，终端里写明为什么。
#[gpui_kit::test]
fn a_shell_that_cannot_start_leaves_its_card_and_says_why(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_with_fake_terminals(cx, FakeTerminalFactory::fail_first());

    dispatch(cx, handle, NewLocalTerminalTab);

    let screen = wait_for_screen(cx, &workspace, CardId(1), "测试启动失败");
    assert!(screen.contains("[终端错误：测试启动失败]"), "{screen}");
    in_frame(cx, handle, |window, _| assert!(window.find(CARD).visible()));
}

/// 等 `card` 的终端屏幕上出现 `text`，最多两秒（传输跑在自己的线程上）。
fn wait_for_screen(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: CardId,
    text: &str,
) -> String {
    let started = std::time::Instant::now();
    loop {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        let screen = cx.update(|cx| {
            workspace
                .read(cx)
                .card_screen_text(card, cx)
                .unwrap_or_default()
        });
        if screen.contains(text) || started.elapsed() > std::time::Duration::from_secs(2) {
            return screen;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// 等元素的无障碍名称变成 `expected`，最多两秒。
fn wait_for_label(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: (&'static str, usize),
    expected: &str,
) -> Option<String> {
    let started = std::time::Instant::now();
    loop {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        let label = in_frame(cx, handle, |window, _| {
            window.find(id).label().map(str::to_owned)
        });
        if label.as_deref() == Some(expected)
            || started.elapsed() > std::time::Duration::from_secs(2)
        {
            return label;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(2) {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}
