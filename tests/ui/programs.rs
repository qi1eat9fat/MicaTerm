//! 程序图块与状态点（工单 06）的 UI 测试：卡片和标签上的程序图块跟着前台程序走，
//! 标签上叠放各卡片的图块；有输出时亮绿点，在看不见的地方结束或响铃时亮橙点。
//! 本地终端一律用假传输，测试替假 shell 报告前台程序、打出输出。

use std::time::Duration;

use micaterm_terminal::TerminalLifecycle;

use crate::support::*;

/// 用假本地终端打开窗口壳（窗口激活着），开一个终端标签，等假 shell 跑起来。
fn open_tab(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
) {
    let factory = Arc::new(FakeTerminalFactory::default());
    let provider = fake_local_terminals_with(factory.clone());
    let (handle, workspace) =
        open_workspace_with(cx, settings_store(), services_with_terminals(provider));
    in_frame(cx, handle, |window, _| window.activate_window());
    dispatch(cx, handle, NewLocalTerminalTab);
    // 卡片的 shell 名（pwsh）在后台查到。终端要到 `Running` 才收键盘输入，之前打的字丢掉；
    // 屏幕上出现提示符还不算：解析线程把输出直接画进终端，而 `Started` 要等 UI 线程下一次
    // （每 16ms）收事件才把终端改成 `Running`，测试并行跑、机器忙时两者之间会隔一轮。
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())
        && cx.update(|cx| running(&workspace, cx)
            && workspace
                .read(cx)
                .card_screen_text(CardId(1), cx)
                .is_some_and(|text| text.contains("会议纪要")))));
    (handle, workspace, factory)
}

/// 第一张卡片的终端收不收键盘输入了。
fn running(workspace: &Entity<Workspace>, cx: &App) -> bool {
    workspace.read(cx).card(CardId(1), cx).is_some_and(|card| {
        card.read(cx).terminal().read(cx).lifecycle(cx) == TerminalLifecycle::Running
    })
}

/// 按真实时间等 `done` 成立，最多两秒：假 shell 跑在自己的线程上。
/// 每一轮把测试时钟拨过一帧（终端每 16ms 收一次事件）、画一帧、跑完引发的事情。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn card_tile(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: u64) -> Option<String> {
    cx.update(|cx| workspace.read(cx).card_tile(CardId(card), cx))
}

fn tab_tile(cx: &mut TestAppContext, workspace: &Entity<Workspace>, tab: u64) -> Option<String> {
    cx.update(|cx| workspace.read(cx).tab_tile(TabId(tab), cx))
}

#[gpui_kit::test]
fn the_tile_of_a_card_follows_its_foreground_program(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    assert_eq!(tab_tile(cx, &workspace, 1).as_deref(), Some("pwsh"));

    factory.foreground(1, Some("nvim"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Neovim".into())));
    assert_eq!(tab_tile(cx, &workspace, 1).as_deref(), Some("Neovim"));

    // 标签上画出来的图块就是它（卡片没有头部，图块只在标签上）。
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(("card-tile", 1usize)).is_none());
        assert_eq!(window.find(("tab-tiles", 1usize)).label(), Some("Neovim"));
    });

    // 回到提示符：又是 shell 的终端图块。
    factory.foreground(1, None);
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())));
}

/// 一个标签里四张卡片：标签上只画焦点卡片的那一块（用户定：不叠放），焦点换了它跟着换。
/// 标签前面叠放每张卡片的图块（2026-11-14 用户定）：焦点卡片在最前，其余按分屏里的顺序，最多 3 块。
/// 卡片顶行正中写它在跑的程序。
#[gpui_kit::test]
fn a_tab_stacks_the_tiles_of_its_cards_with_the_focused_one_first(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    for _ in 0..3 {
        split_again(cx, handle, SplitRight);
    }
    factory.foreground(1, Some("vim"));
    factory.foreground(2, Some("htop"));
    factory.foreground(3, Some("git"));
    // 每张卡片的终端各自收事件：都到了再看。
    assert!(wait_until(cx, handle, |cx| {
        [(1, "Vim"), (2, "Activity"), (3, "Git"), (4, "pwsh")]
            .into_iter()
            .all(|(card, tile)| card_tile(cx, &workspace, card) == Some(tile.into()))
    }));

    // 焦点在新开的第 4 张（shell 在提示符下）。
    assert_eq!(tab_tile(cx, &workspace, 1).as_deref(), Some("pwsh"));
    let stacked = |window: &mut gpui_kit::Window| -> Vec<String> {
        let tiles = window.within(("tab-tiles", 1usize));
        (0..4_usize)
            .filter_map(|index| tiles.try_find(("tab-tile", index)))
            .filter_map(|tile| tile.label().map(str::to_owned))
            .collect()
    };
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("tab-tiles", 1usize)).label(), Some("pwsh"));
        assert_eq!(stacked(window), ["pwsh", "Vim", "Activity"]);
        // 顶行：在跑的程序写在正中，回到提示符的卡片没有。
        assert_eq!(
            window.find(("card-title", 1usize)).label(),
            Some("pwsh · Vim")
        );
        assert!(window.find(("card-title-program", 1usize)).visible());
        assert!(window.try_find(("card-title-program", 4usize)).is_none());
    });

    // 点第 1 张：标签前换成它的图块。
    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx)
    });
    cx.update(|cx| assert_eq!(workspace.read(cx).focused_card(cx), Some(CardId(1))));
    assert_eq!(tab_tile(cx, &workspace, 1).as_deref(), Some("Vim"));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("tab-tiles", 1usize)).label(), Some("Vim"));
        assert_eq!(stacked(window), ["Vim", "Activity", "Git"]);
    });
}

fn card_dot(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: u64,
) -> Option<StatusDot> {
    cx.update(|cx| workspace.read(cx).card_dot(CardId(card), cx))
}

fn tab_dot(cx: &mut TestAppContext, workspace: &Entity<Workspace>, tab: u64) -> Option<StatusDot> {
    cx.update(|cx| workspace.read(cx).tab_dot(TabId(tab), cx))
}

/// 前台是 shell 以外的程序、在输出：卡片和标签上亮绿点；1.5 秒没输出就熄灭。
#[gpui_kit::test]
fn a_program_printing_shows_the_busy_dot_until_it_goes_quiet(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    factory.foreground(1, Some("cargo"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Cargo".into())));

    factory.print(1, "Compiling micaterm\r\n");
    assert!(wait_until(cx, handle, |cx| card_dot(cx, &workspace, 1)
        == Some(StatusDot::Busy)));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::Busy));
    // 点只画在标签上（卡片没有头部；状态栏写前台程序名）。
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("tab-dot", 1usize)).label(), Some("Output"));
    });

    cx.executor().advance_clock(Duration::from_millis(1600));
    cx.run_until_parked();
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::None));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(("tab-dot", 1usize)).is_none());
    });
}

/// 在提示符下打字：shell 回显出来的字不算输出，不亮绿点。
#[gpui_kit::test]
fn typing_at_the_prompt_shows_no_dot(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tab(cx);
    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx);
        window.input("ls", cx);
    });
    assert!(wait_until(cx, handle, |cx| cx.update(|cx| workspace
        .read(cx)
        .card_screen_text(CardId(1), cx)
        .unwrap_or_default()
        .contains("ls"))));
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::None));
}

/// 程序在后面的标签里结束：那张卡片和它的标签亮橙点；切过去看到了就熄。
#[gpui_kit::test]
fn a_program_that_ends_out_of_sight_asks_for_attention(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    factory.foreground(1, Some("cargo"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Cargo".into())));
    dispatch(cx, handle, NewLocalTerminalTab);
    cx.update(|cx| assert_eq!(workspace.read(cx).active_tab(), Some(TabId(2))));

    factory.print(1, "Finished\r\n");
    factory.foreground(1, None);
    assert!(wait_until(cx, handle, |cx| tab_dot(cx, &workspace, 1)
        == Some(StatusDot::Attention)));
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::Attention));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("tab-dot", 1usize)).label(), Some("Needs Attention"));
    });
    // 别的标签不受影响。
    assert_eq!(tab_dot(cx, &workspace, 2), Some(StatusDot::None));

    in_frame(cx, handle, |window, cx| window.click(("tab", 1usize), cx));
    cx.update(|cx| assert_eq!(workspace.read(cx).active_tab(), Some(TabId(1))));
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::None));
}

/// 看得见的卡片里程序结束、响铃：用户就看着它，不亮橙点。
#[gpui_kit::test]
fn a_card_in_sight_needs_no_attention(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    factory.foreground(1, Some("vim"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Vim".into())));
    factory.print(1, "\x07");
    factory.foreground(1, None);
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())));
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
}

/// 窗口不在前时响铃：亮橙点，而且盖过绿点；窗口回到前面就熄。
#[gpui_kit::test]
fn a_bell_with_the_window_away_asks_for_attention_over_the_busy_dot(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    factory.foreground(1, Some("python"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Python".into())));
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();

    factory.print(1, "继续吗 y/N?\x07");
    assert!(wait_until(cx, handle, |cx| card_dot(cx, &workspace, 1)
        == Some(StatusDot::Attention)));
    // 程序刚有输出，橙点也优先。
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::Attention));

    in_frame(cx, handle, |window, _| window.activate_window());
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::Busy));
}

/// 放大一张卡片时，被藏起来的卡片算看不见：它响铃会亮橙点，还原后就熄（修正 `card_in_sight`）。
#[gpui_kit::test]
fn a_card_hidden_by_zoom_is_out_of_sight(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    split_again(cx, handle, SplitRight);
    dispatch(cx, handle, ToggleZoomCard);
    cx.update(|cx| assert_eq!(workspace.read(cx).zoomed_card(), Some(CardId(2))));

    factory.print(1, "y/N?\x07");
    assert!(wait_until(cx, handle, |cx| card_dot(cx, &workspace, 1)
        == Some(StatusDot::Attention)));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::Attention));
    // 响铃的提示也送到了窗口里（看得见的卡片响铃不提示）。
    assert_eq!(
        in_frame(cx, handle, |window, cx| window.notifications(cx).len()),
        1
    );

    dispatch(cx, handle, ToggleZoomCard);
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
}

/// 点通知回到一张被放大藏起来的卡片：先还原放大，焦点给它，橙点也熄了——
/// 焦点卡片、画出来的卡片和橙点说的是同一件事。
#[gpui_kit::test]
fn a_notice_of_a_card_hidden_by_zoom_brings_it_back_into_sight(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    split_again(cx, handle, SplitRight);
    dispatch(cx, handle, ToggleZoomCard);
    cx.update(|cx| assert_eq!(workspace.read(cx).zoomed_card(), Some(CardId(2))));
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();

    factory.print(1, "y/N?");
    assert!(wait_until(cx, handle, |cx| !cx
        .shown_system_notifications()
        .is_empty()));
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::Attention));

    let tag = cx.shown_system_notifications()[0].tag.clone();
    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag,
        action_id: None,
    });
    cx.run_until_parked();
    in_frame(cx, handle, |_, _| {});

    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.zoomed_card(), None);
        assert_eq!(workspace.focused_card(cx), Some(CardId(1)));
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
        assert!(window.find(("card", 1usize)).visible());
    });
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
}

/// 状态点在名称后面一直亮着，× 在最右，悬停时才出现（spec 第 3 节「标签外观」）。
#[gpui_kit::test]
fn the_tab_dot_stays_beside_the_name_and_the_cross_shows_on_hover(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    dispatch(cx, handle, NewLocalTerminalTab);
    factory.print(1, "\x07");
    assert!(wait_until(cx, handle, |cx| tab_dot(cx, &workspace, 1)
        == Some(StatusDot::Attention)));
    in_frame(cx, handle, |window, _| {
        assert!(window.find(("tab-dot", 1usize)).visible());
        assert!(!window.find(("tab-close", 1usize)).visible());
    });
    in_frame(cx, handle, |window, cx| window.hover(("tab", 1usize), cx));
    in_frame(cx, handle, |window, _| {
        let dot = window.find(("tab-dot", 1usize));
        let close = window.find(("tab-close", 1usize));
        assert!(dot.visible());
        assert!(close.visible());
        assert!(dot.bounds().right() <= close.bounds().left());
    });
}
