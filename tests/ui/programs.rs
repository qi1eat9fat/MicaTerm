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

    // 回到提示符：又是 shell 的终端图块。
    factory.foreground(1, None);
    assert!(wait_until(cx, handle, |cx| card_tile(cx, &workspace, 1)
        == Some("pwsh".into())));
}
