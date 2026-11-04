//! 卡片分屏（工单 05）：分屏、关闭、放大、焦点移动、拖动和推动间隙、变淡，本地卡片在同一个目录分屏。
//! 本地终端一律用假传输，不起真的 shell。

use crate::support::*;

const CARD_1: (&str, usize) = ("card", 1usize);
const CARD_2: (&str, usize) = ("card", 2usize);
const CARD_3: (&str, usize) = ("card", 3usize);
const DIVIDER_1: (&str, usize) = ("divider", 1usize);

/// 用假本地终端打开窗口壳，再开一个终端标签。窗口是激活的：
/// 窗口失活时 GPUI 不发焦点事件，点进卡片也不会记成焦点卡片（真机上点窗口先激活它）。
fn open_tab(
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
    in_frame(cx, handle, |window, _| window.activate_window());
    dispatch(cx, handle, NewLocalTerminalTab);
    (handle, workspace, factory, provider)
}

/// 画几帧，让分隔线的 resizable 组件量好尺寸、按树摆好。
fn settle(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    for _ in 0..3 {
        in_frame(cx, handle, |_, _| {});
    }
}

fn focused_card(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<CardId> {
    cx.update(|cx| workspace.read(cx).focused_card(cx))
}

fn ratio(cx: &mut TestAppContext, workspace: &Entity<Workspace>, node: usize) -> f32 {
    cx.update(|cx| workspace.read(cx).divider_ratio(NodeId(node as u64)))
        .expect("the divider is in the tab")
}

fn assert_near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
}

#[gpui_kit::test]
fn ctrl_shift_d_splits_the_card_right(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-d", cx));
    settle(cx, handle);

    assert_eq!(factory.starts(), 2);
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        // 并排、一样高、一样宽，中间是 8 的间隙。
        assert_eq!(first.top(), second.top());
        assert_eq!(first.size.height, second.size.height);
        assert!((first.size.width - second.size.width).abs() <= px(1.));
        assert!((second.left() - first.right() - px(8.)).abs() <= px(1.));
        // 新卡片拿到焦点。
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
        assert!(window.find(DIVIDER_1).visible());
    });
    assert_eq!(focused_card(cx, &workspace), Some(CardId(2)));
    // 还在同一个标签里。
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids(), [TabId(1)]));
}

#[gpui_kit::test]
fn ctrl_shift_e_splits_the_card_down(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-e", cx));
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        assert_eq!(first.left(), second.left());
        assert_eq!(first.size.width, second.size.width);
        assert!((first.size.height - second.size.height).abs() <= px(1.));
        assert!((second.top() - first.bottom() - px(8.)).abs() <= px(1.));
    });
    assert_eq!(focused_card(cx, &workspace), Some(CardId(2)));
}

/// 头部的分屏按钮分的是按钮所在的那张卡片，哪怕它不是焦点卡片。
#[gpui_kit::test]
fn the_header_buttons_split_their_own_card(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    in_frame(cx, handle, |window, cx| {
        window.click(("card-split-right", 1usize), cx)
    });
    settle(cx, handle);

    // 焦点在 2 上，点 1 的「向下分屏」：分的是 1。
    in_frame(cx, handle, |window, cx| {
        window.hover(CARD_1, cx);
        window.click(("card-split-down", 1usize), cx);
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        let third = window.find(CARD_3).bounds();
        // 1 在左上，3 在它下面，2 占右边一整列。
        assert_eq!(first.left(), third.left());
        assert!(third.top() > first.bottom());
        assert!(second.left() > first.right());
        assert!((second.size.height - (third.bottom() - first.top())).abs() <= px(1.));
    });
    assert_eq!(focused_card(cx, &workspace), Some(CardId(3)));
}

/// Ctrl+Shift+W 关闭焦点卡片和它的 shell，旁边的卡片占满标签，焦点交还给它。
#[gpui_kit::test]
fn ctrl_shift_w_closes_the_focused_card_and_its_neighbour_takes_its_place(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());
    let whole = in_frame(cx, handle, |window, _| window.find(CARD_1).bounds());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-w", cx));
    settle(cx, handle);

    assert_eq!(factory.wait_for_shutdowns(1), 1);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_2).is_none());
        assert!(window.try_find(DIVIDER_1).is_none());
        assert_eq!(window.find(CARD_1).bounds(), whole);
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids(), [TabId(1)]));
}

/// 卡片上的 × 关闭那张卡片；关掉最后一张，标签也关了。
#[gpui_kit::test]
fn closing_every_card_closes_the_tab(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitDown);
    settle(cx, handle);

    // 不是焦点卡片：按钮只在悬停时出现。
    in_frame(cx, handle, |window, _| {
        assert!(!window.find(("card-close", 1usize)).visible());
        assert!(window.find(("card-close", 2usize)).visible());
    });
    in_frame(cx, handle, |window, cx| {
        window.hover(CARD_1, cx);
        window.click(("card-close", 1usize), cx);
    });
    settle(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_1).is_none());
        assert!(window.find(CARD_2).visible());
    });
    dispatch(cx, handle, CloseFocusedCard);

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    in_frame(cx, handle, |window, _| {
        assert!(window.find("start-card").visible());
    });
    cx.update(|cx| assert!(workspace.read(cx).tab_ids().is_empty()));
}

/// Ctrl+Shift+Enter 放大焦点卡片：只画它，占满标签；再按一次还原，分屏不变。
#[gpui_kit::test]
fn ctrl_shift_enter_zooms_the_focused_card_and_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    let whole = in_frame(cx, handle, |window, _| window.find(CARD_1).bounds());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);
    let tiled = in_frame(cx, handle, |window, _| window.find(CARD_2).bounds());

    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-shift-enter", cx)
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_1).is_none());
        assert_eq!(window.find(CARD_2).bounds(), whole);
        assert_eq!(window.find(("card-zoom", 2usize)).label(), Some("还原卡片"));
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });
    assert_eq!(
        cx.update(|cx| workspace.read(cx).zoomed_card()),
        Some(CardId(2))
    );

    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-shift-enter", cx)
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.find(CARD_1).visible());
        assert_eq!(window.find(CARD_2).bounds(), tiled);
        assert_eq!(window.find(("card-zoom", 2usize)).label(), Some("放大卡片"));
    });
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
}

/// 双击卡片头部、点放大按钮，放大的是那张卡片；只有一张卡片时不放大。
#[gpui_kit::test]
fn double_clicking_the_header_or_the_zoom_button_zooms_that_card(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, ToggleZoomCard);
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.double_click(("card-header", 1usize), cx)
    });
    settle(cx, handle);
    assert_eq!(
        cx.update(|cx| workspace.read(cx).zoomed_card()),
        Some(CardId(1))
    );
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_2).is_none())
    });

    in_frame(cx, handle, |window, cx| {
        window.click(("card-zoom", 1usize), cx)
    });
    settle(cx, handle);
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
    in_frame(cx, handle, |window, _| {
        assert!(window.find(CARD_2).visible())
    });
}

/// Alt+方向键在卡片之间移动焦点，标签跟着焦点卡片。
#[gpui_kit::test]
fn alt_arrows_move_the_focus_between_cards(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    dispatch(cx, handle, SplitDown);
    settle(cx, handle);
    assert_eq!(focused_card(cx, &workspace), Some(CardId(3)));

    in_frame(cx, handle, |window, cx| window.press("alt-up", cx));
    assert_eq!(focused_card(cx, &workspace), Some(CardId(2)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });

    in_frame(cx, handle, |window, cx| window.press("alt-left", cx));
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });

    // 左边没有卡片了：焦点不动。
    in_frame(cx, handle, |window, cx| window.press("alt-left", cx));
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
}

/// 点进另一张卡片，它就成了焦点卡片：描边、按钮、不变淡都跟着它。
#[gpui_kit::test]
fn clicking_a_card_makes_it_the_focused_card(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx)
    });
    settle(cx, handle);

    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
    in_frame(cx, handle, |window, cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.card_opacity(CardId(1), window, cx), Some(1.));
        assert_eq!(workspace.card_opacity(CardId(2), window, cx), Some(0.85));
    });
}

/// 非焦点卡片的内容 0.85，窗口失活时 0.7；焦点卡片不变淡。
#[gpui_kit::test]
fn cards_other_than_the_focused_one_are_dimmed_more_when_the_window_is_inactive(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);
    let opacities = |cx: &mut TestAppContext| {
        in_frame(cx, handle, |window, cx| {
            let workspace = workspace.read(cx);
            (
                workspace.card_opacity(CardId(1), window, cx),
                workspace.card_opacity(CardId(2), window, cx),
            )
        })
    };
    assert_eq!(opacities(cx), (Some(0.85), Some(1.)));

    VisualTestContext::from_window(handle.into(), cx).deactivate_window();

    assert_eq!(opacities(cx), (Some(0.7), Some(1.)));
}

/// 拖动间隙调整比例，拖到头停在 0.08–0.92；双击间隙恢复成一半。
#[gpui_kit::test]
fn dragging_the_gap_resizes_the_cards_within_limits_and_double_clicking_it_resets(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);
    let (gap, whole) = in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        (
            window.find(DIVIDER_1).bounds().center(),
            second.right() - first.left(),
        )
    });

    // 往右拖 100。
    in_frame(cx, handle, |window, cx| {
        window.drag(gap, gpui_kit::point(gap.x + px(100.), gap.y), cx)
    });
    settle(cx, handle);
    let dragged = ratio(cx, &workspace, 1);
    assert!(dragged > 0.55 && dragged < 0.65, "{dragged}");
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds().size.width;
        let second = window.find(CARD_2).bounds().size.width;
        assert!(first - second > px(150.), "{first:?} {second:?}");
    });

    // 一直拖到最右边：停在 0.92。
    let gap = in_frame(cx, handle, |window, _| {
        window.find(DIVIDER_1).bounds().center()
    });
    in_frame(cx, handle, |window, cx| {
        window.drag(gap, gpui_kit::point(gap.x + whole, gap.y), cx)
    });
    settle(cx, handle);
    assert_near(ratio(cx, &workspace, 1), 0.92);
    in_frame(cx, handle, |window, _| {
        assert!(window.find(CARD_2).bounds().size.width > whole * 0.05);
    });

    // 双击间隙：两边各一半。
    in_frame(cx, handle, |window, cx| window.double_click(DIVIDER_1, cx));
    settle(cx, handle);
    assert_eq!(ratio(cx, &workspace, 1), 0.5);
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds().size.width;
        let second = window.find(CARD_2).bounds().size.width;
        assert!((first - second).abs() <= px(1.), "{first:?} {second:?}");
    });
}

/// Alt+Shift+方向键推焦点卡片最近的那条分隔线，卡片跟着变大变小。
#[gpui_kit::test]
fn alt_shift_arrows_move_the_nearest_divider(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    dispatch(cx, handle, SplitDown);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| window.press("alt-shift-left", cx));
    settle(cx, handle);
    in_frame(cx, handle, |window, cx| window.press("alt-shift-down", cx));
    settle(cx, handle);

    assert_near(ratio(cx, &workspace, 1), 0.45);
    assert_near(ratio(cx, &workspace, 2), 0.55);
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds().size.width;
        let second = window.find(CARD_2).bounds().size.width;
        assert!(second > first, "{first:?} {second:?}");
        let upper = window.find(CARD_2).bounds().size.height;
        let lower = window.find(CARD_3).bounds().size.height;
        assert!(upper > lower, "{upper:?} {lower:?}");
    });
}

/// 本地卡片分屏：新卡片用同一个 shell，在 shell 现在所在的目录里开。
#[gpui_kit::test]
fn splitting_a_local_card_opens_the_new_one_in_the_same_folder(cx: &mut TestAppContext) {
    let folder: &'static str = Box::leak(
        std::env::temp_dir()
            .join("qishell-split")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let (handle, workspace, _, provider) = open_tab(cx, FakeTerminalFactory::in_folder(folder));
    wait_until_folder(cx, &workspace, CardId(1), folder);

    dispatch(cx, handle, SplitRight);

    let launches = provider.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].shell, launches[0].shell);
    assert_eq!(
        launches[1].directory.as_deref(),
        Some(std::path::Path::new(folder))
    );
}

/// 窄卡片的状态栏放不下时截断，不溢出：每一段都留在状态栏里，状态栏留在卡片里。
#[gpui_kit::test]
fn a_narrow_card_truncates_its_status_bar(cx: &mut TestAppContext) {
    let (handle, _, _, _) = open_tab(cx, FakeTerminalFactory::default());
    // 每次分的都是右半边的新卡片：第 5 张只有标签宽度的 1/16。
    for _ in 0..4 {
        dispatch(cx, handle, SplitRight);
        settle(cx, handle);
    }

    in_frame(cx, handle, |window, _| {
        let card = window.find(("card", 5usize)).bounds();
        let bar = window.find(("card-status", 5usize)).bounds();
        assert!(card.size.width < px(120.), "{card:?}");
        assert!(
            bar.left() >= card.left() && bar.right() <= card.right(),
            "{bar:?} {card:?}"
        );
        for part in [
            "card-status-state",
            "card-status-detail",
            "card-status-terminal",
        ] {
            let bounds = window.find((part, 5usize)).bounds();
            assert!(
                bounds.left() >= bar.left() && bounds.right() <= bar.right() + px(0.5),
                "{part} {bounds:?} 出了状态栏 {bar:?}"
            );
        }
    });
}

/// 等卡片知道 shell 在 `folder` 里，最多两秒（传输跑在自己的线程上）。
fn wait_until_folder(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: CardId,
    folder: &str,
) {
    let started = std::time::Instant::now();
    loop {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        let directory = cx.update(|cx| {
            workspace
                .read(cx)
                .card(card, cx)
                .and_then(|card| card.read(cx).directory())
        });
        if directory.as_deref() == Some(std::path::Path::new(folder)) {
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{directory:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
