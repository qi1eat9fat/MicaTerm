//! 程序图块与状态点（工单 06）的 UI 测试：卡片和标签上的程序图块跟着前台程序走，
//! 标签上叠放各卡片的图块；有输出时亮绿点，在看不见的地方结束或响铃时亮橙点。
//! 本地终端一律用假传输，测试替假 shell 报告前台程序、打出输出。

use std::time::Duration;

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
    // 卡片的 shell 名（pwsh）在后台查到。
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())));
    (handle, workspace, factory)
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

fn tab_tiles(cx: &mut TestAppContext, workspace: &Entity<Workspace>, tab: u64) -> Vec<String> {
    cx.update(|cx| workspace.read(cx).tab_tiles(TabId(tab), cx))
}

#[gpui_kit::test]
fn the_tile_of_a_card_follows_its_foreground_program(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    assert_eq!(tab_tiles(cx, &workspace, 1), ["pwsh"]);

    factory.foreground(1, Some("nvim"));
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("Neovim".into())));
    assert_eq!(tab_tiles(cx, &workspace, 1), ["Neovim"]);

    // 头部和标签上画出来的图块是同一个程序。
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("card-tile", 1usize)).label(), Some("Neovim"));
        assert_eq!(window.find(("tab-tiles", 1usize)).label(), Some("Neovim"));
    });

    // 回到提示符：又是 shell 的终端图块。
    factory.foreground(1, None);
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())));
}

/// 一个标签里四张卡片：标签上叠三块，焦点卡片的在最前面，其余按从左到右的顺序。
#[gpui_kit::test]
fn a_tab_stacks_the_tiles_of_its_cards_with_the_focused_one_in_front(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tab(cx);
    for _ in 0..3 {
        dispatch(cx, handle, SplitRight);
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
    assert_eq!(tab_tiles(cx, &workspace, 1), ["pwsh", "Vim", "Activity"]);
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("tab-tiles", 1usize)).label(),
            Some("pwsh · Vim · Activity")
        );
    });

    // 点第 1 张：它的图块到最前面，后面是其余的前两张。
    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx)
    });
    cx.update(|cx| assert_eq!(workspace.read(cx).focused_card(cx), Some(CardId(1))));
    assert_eq!(tab_tiles(cx, &workspace, 1), ["Vim", "Activity", "Git"]);
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

    factory.print(1, "Compiling qishell\r\n");
    assert!(wait_until(cx, handle, |cx| card_dot(cx, &workspace, 1)
        == Some(StatusDot::Busy)));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::Busy));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("card-dot", 1usize)).label(), Some("有输出"));
        assert_eq!(window.find(("tab-dot", 1usize)).label(), Some("有输出"));
    });

    cx.executor().advance_clock(Duration::from_millis(1600));
    cx.run_until_parked();
    assert_eq!(card_dot(cx, &workspace, 1), Some(StatusDot::None));
    assert_eq!(tab_dot(cx, &workspace, 1), Some(StatusDot::None));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(("card-dot", 1usize)).is_none());
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
        assert_eq!(window.find(("tab-dot", 1usize)).label(), Some("需要注意"));
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
    dispatch(cx, handle, SplitRight);
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

/// 悬停标签时，状态点让位给 ×。
#[gpui_kit::test]
fn hovering_a_tab_puts_its_close_button_over_the_dot(cx: &mut TestAppContext) {
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
        assert!(!window.find(("tab-dot", 1usize)).visible());
        assert!(window.find(("tab-close", 1usize)).visible());
    });
}
