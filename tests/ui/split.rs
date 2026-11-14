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
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    settle(cx, handle);

    assert_eq!(factory.starts(), 2);
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        // 并排、一样高、一样宽，中间没有间隙（只有一条 1px 线，26 号）。
        assert_eq!(first.top(), second.top());
        assert_eq!(first.size.height, second.size.height);
        assert!((first.size.width - second.size.width).abs() <= px(1.));
        assert!((second.left() - first.right()).abs() <= px(1.));
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
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        assert_eq!(first.left(), second.left());
        assert_eq!(first.size.width, second.size.width);
        assert!((first.size.height - second.size.height).abs() <= px(1.));
        assert!((second.top() - first.bottom()).abs() <= px(1.));
    });
    assert_eq!(focused_card(cx, &workspace), Some(CardId(2)));
}

/// Ctrl+Shift+W 关闭焦点卡片和它的 shell，旁边的卡片占满标签，焦点交还给它。
#[gpui_kit::test]
fn ctrl_shift_w_closes_the_focused_card_and_its_neighbour_takes_its_place(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());
    let whole = in_frame(cx, handle, |window, _| window.find(CARD_1).bounds());
    split_again(cx, handle, SplitRight);
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

/// 终端右键菜单的「关闭卡片」关闭那张卡片（卡片没有头部）；关掉最后一张，标签也关了。
#[gpui_kit::test]
fn closing_every_card_closes_the_tab(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitDown);
    settle(cx, handle);

    // 不是焦点卡片也能关：菜单是在它的终端上开的。
    click_terminal_menu(cx, handle, 1, "关闭卡片");
    settle(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_1).is_none());
        assert!(window.find(CARD_2).visible());
    });
    dispatch(cx, handle, CloseFocusedCard);

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    in_frame(cx, handle, |window, _| {
        assert!(window.find("new-tab-page").visible());
    });
    cx.update(|cx| assert!(workspace.read(cx).tab_ids().is_empty()));
}

/// Ctrl+Shift+Enter 放大焦点卡片：只画它，占满标签；再按一次还原，分屏不变。
#[gpui_kit::test]
fn ctrl_shift_enter_zooms_the_focused_card_and_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    let whole = in_frame(cx, handle, |window, _| window.find(CARD_1).bounds());
    split_again(cx, handle, SplitRight);
    settle(cx, handle);
    let tiled = in_frame(cx, handle, |window, _| window.find(CARD_2).bounds());

    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-shift-enter", cx)
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_1).is_none());
        assert_eq!(window.find(CARD_2).bounds(), whole);
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });
    assert!(terminal_menu_labels(cx, handle, 2).contains(&"还原卡片".to_string()));
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
    });
    assert!(terminal_menu_labels(cx, handle, 2).contains(&"放大卡片".to_string()));
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
}

/// 终端右键菜单的「放大卡片」放大的是那张卡片，再点「还原卡片」还原；只有一张卡片时不放大。
#[gpui_kit::test]
fn the_terminal_menu_zooms_its_card_and_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, ToggleZoomCard);
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
    split_again(cx, handle, SplitRight);
    settle(cx, handle);

    // 菜单最后一组是放大、移到新标签、关闭。
    let labels = terminal_menu_labels(cx, handle, 1);
    assert_eq!(
        labels[labels.len() - 3..],
        ["放大卡片", "移到新标签", "关闭卡片"],
        "{labels:?}"
    );

    click_terminal_menu(cx, handle, 1, "放大卡片");
    settle(cx, handle);
    assert_eq!(
        cx.update(|cx| workspace.read(cx).zoomed_card()),
        Some(CardId(1))
    );
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_2).is_none())
    });

    click_terminal_menu(cx, handle, 1, "还原卡片");
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
    split_again(cx, handle, SplitRight);
    split_again(cx, handle, SplitDown);
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
    split_again(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx)
    });
    settle(cx, handle);

    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
}

/// 拖动间隙调整比例，拖到头停在 0.08–0.92；双击间隙恢复成一半。
#[gpui_kit::test]
fn dragging_the_gap_resizes_the_cards_within_limits_and_double_clicking_it_resets(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitRight);
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
    split_again(cx, handle, SplitRight);
    split_again(cx, handle, SplitDown);
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
            .join("micaterm-split")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let (handle, workspace, _, provider) = open_tab(cx, FakeTerminalFactory::in_folder(folder));
    wait_until_folder(cx, &workspace, CardId(1), folder);

    split_again(cx, handle, SplitRight);

    let launches = provider.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].shell, launches[0].shell);
    assert_eq!(
        launches[1].directory.as_deref(),
        Some(std::path::Path::new(folder))
    );
}

/// 窄卡片的状态栏按优先级隐藏，不溢出：留下的每一段都在状态栏里，状态栏留在卡片里。
#[gpui_kit::test]
fn a_narrow_card_hides_parts_of_its_status_bar(cx: &mut TestAppContext) {
    let (handle, _, _, _) = open_tab(cx, FakeTerminalFactory::default());
    // 每次分的都是右半边的新卡片：第 5 张只有标签宽度的 1/16。
    for _ in 0..4 {
        split_again(cx, handle, SplitRight);
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
        for part in ["card-status-state", "card-status-terminal"] {
            let bounds = window.find((part, 5usize)).bounds();
            assert!(
                bounds.left() >= bar.left() && bounds.right() <= bar.right() + px(0.5),
                "{part} {bounds:?} 出了状态栏 {bar:?}"
            );
        }
        // 正中的路径放不下就整段让位；宽的卡片照常写在正中。
        assert!(window.try_find(("card-status-path", 5usize)).is_none());
        assert!(window.find(("card-status-path", 1usize)).visible());
    });
}

/// 卡片顶行右边：「向右分屏」「向下分屏」，标签里不止一张时还有「关闭」（只有一张时和标签的 × 重复，不放）。
#[gpui_kit::test]
fn the_title_row_closes_the_card_only_when_the_tab_has_others(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    in_frame(cx, handle, |window, _| {
        assert!(window.find(("card-split-right", 1usize)).visible());
        assert!(window.try_find(("card-close", 1usize)).is_none());
    });

    split_again(cx, handle, SplitRight);
    settle(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("card-close", 1usize)).label(),
            Some("关闭卡片")
        );
    });
    in_frame(cx, handle, |window, cx| {
        window.click(("card-close", 1usize), cx)
    });
    settle(cx, handle);

    assert_eq!(card_ids(cx, &workspace), [2]);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(("card-close", 2usize)).is_none());
    });
}

/// 标签里不止一张卡片：悬停标签，提示列出每张卡片，焦点卡片排第一。
#[gpui_kit::test]
fn hovering_a_tab_lists_its_cards(cx: &mut TestAppContext) {
    let (handle, _, _, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| window.hover(("tab", 1usize), cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    in_frame(cx, handle, |window, _| {
        let tip = window.find("icon-button-tooltip");
        assert_eq!(tip.label(), Some("pwsh · pwsh"));
    });
}

/// 新卡片顶行的 ×：和 Esc 一样撤销，焦点回到分屏的那张卡片。
#[gpui_kit::test]
fn the_new_cards_close_button_takes_it_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, SplitRight);
    settle(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click("card-picker-close", cx)
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("card-picker-search").is_none());
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
    assert_eq!(card_ids(cx, &workspace), [1]);
}

/// 工单 31：shell 用 OSC 9;9 报告的目录比本机读到的（PEB）优先，哪怕本机后来又读到了别的：
/// Windows PowerShell 5.1 `cd` 以后进程的当前目录不变，只有报告是对的。路径带空格和中文。
#[gpui_kit::test]
fn splitting_uses_the_folder_the_shell_reports_before_the_one_read_from_it(
    cx: &mut TestAppContext,
) {
    let read: &'static str = Box::leak(
        std::env::temp_dir()
            .join("micaterm-split-read")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let reported: &'static str = Box::leak(
        std::env::temp_dir()
            .join("micaterm split 中文")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let (handle, workspace, factory, provider) = open_tab(cx, FakeTerminalFactory::in_folder(read));
    wait_until_folder(cx, &workspace, CardId(1), read);

    factory.print(1, &format!("\x1b]9;9;\"{reported}\"\x1b\\"));
    wait_until_folder(cx, &workspace, CardId(1), reported);
    // 本机读到的还是启动时的目录（PowerShell 5.1 就是这样），报告的照样算数。
    factory.send(
        1,
        micaterm_terminal::TerminalTransportEvent::WorkingDirectory(read.into()),
    );
    cx.run_until_parked();

    split_again(cx, handle, SplitRight);

    let launches = provider.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(
        launches[1].directory.as_deref(),
        Some(std::path::Path::new(reported))
    );
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

/// 在前那个标签里卡片的 id，从左到右、从上到下。
fn card_ids(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<u64> {
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        let tab = workspace.active_tab().expect("a tab is open");
        workspace
            .tab_cards(tab)
            .iter()
            .map(|card| card.read(cx).id().0)
            .collect()
    })
}

/// 新卡片里列出来的每一项的无障碍名称。
fn picker_items(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Vec<String> {
    in_frame(cx, handle, |window, _| {
        (0..16_usize)
            .filter_map(|item| window.try_find(("card-picker-item", item)))
            .filter_map(|item| item.label().map(str::to_owned))
            .collect()
    })
}

/// 分屏重做（2026-11-14）：Ctrl+Shift+D 先在右边放一张新卡片，焦点在它的搜索框里，
/// 第一项「再开一个」、第二项本地终端；Esc 撤销，什么都没开，焦点回到原来的卡片。
#[gpui_kit::test]
fn splitting_shows_a_new_card_to_choose_from_and_escape_takes_it_back(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());
    let whole = in_frame(cx, handle, |window, _| window.find(CARD_1).bounds());

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-d", cx));
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("card-picker-search").focused(), Some(true));
        let card = window.find(CARD_1).bounds();
        let search = window.find("card-picker-search").bounds();
        assert!(search.left() >= card.right(), "{card:?} {search:?}");
    });
    let items = picker_items(cx, handle);
    assert_eq!(items[1], "本地终端", "{items:?}");
    assert!(items[0].starts_with("再开一个 "), "{items:?}");
    assert_eq!(factory.starts(), 1);
    // 新卡片不是卡片：标签里还是一张，焦点卡片还是它。
    assert_eq!(card_ids(cx, &workspace), [1]);
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));

    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    settle(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("card-picker-search").is_none());
        assert_eq!(window.find(CARD_1).bounds(), whole);
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
    assert_eq!(factory.starts(), 1);
}

/// 在新卡片里选「本地终端」：开在那一格（下面），焦点给它。
#[gpui_kit::test]
fn choosing_the_local_terminal_opens_it_in_the_new_card(cx: &mut TestAppContext) {
    let (handle, workspace, factory, _) = open_tab(cx, FakeTerminalFactory::default());

    dispatch(cx, handle, SplitDown);
    in_frame(cx, handle, |window, cx| {
        window.click(("card-picker-item", 1usize), cx)
    });
    settle(cx, handle);

    assert_eq!(factory.starts(), 2);
    assert_eq!(card_ids(cx, &workspace), [1, 2]);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("card-picker-search").is_none());
        let first = window.find(CARD_1).bounds();
        let second = window.find(CARD_2).bounds();
        assert_eq!(first.left(), second.left());
        assert!(second.top() >= first.bottom());
        assert!(window.find(DIVIDER_1).visible());
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });
}

/// 点了别的卡片：新卡片撤销，焦点留在点的那张。
#[gpui_kit::test]
fn clicking_another_card_takes_the_new_card_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitRight);
    settle(cx, handle);

    dispatch(cx, handle, SplitDown);
    settle(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert!(window.find("card-picker-search").visible())
    });
    in_frame(cx, handle, |window, cx| {
        window.click(("terminal", 1usize), cx)
    });
    settle(cx, handle);

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("card-picker-search").is_none());
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });
    assert_eq!(card_ids(cx, &workspace), [1, 2]);
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
}

/// 状态栏最右边的「向右分屏」分的是这张卡片（不一定是焦点卡片）。
#[gpui_kit::test]
fn the_status_bar_buttons_split_their_own_card(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitDown);
    settle(cx, handle);
    assert_eq!(focused_card(cx, &workspace), Some(CardId(2)));

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("card-split-right", 1usize)).label(),
            Some("向右分屏")
        );
        assert_eq!(
            window.find(("card-split-down", 1usize)).label(),
            Some("向下分屏")
        );
        // 不放大时没有「还原」。
        assert!(window.try_find(("card-restore", 1usize)).is_none());
    });
    in_frame(cx, handle, |window, cx| {
        window.click(("card-split-right", 1usize), cx)
    });
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    settle(cx, handle);

    assert_eq!(card_ids(cx, &workspace), [1, 3, 2]);
    in_frame(cx, handle, |window, _| {
        let first = window.find(CARD_1).bounds();
        let third = window.find(CARD_3).bounds();
        assert_eq!(first.top(), third.top());
        assert!(third.left() >= first.right());
    });
}

/// 双击状态栏放大或还原这张卡片；放大时状态栏上多一个「还原」。只有一张卡片时双击没反应。
#[gpui_kit::test]
fn double_clicking_the_status_bar_zooms_the_card_and_back(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    in_frame(cx, handle, |window, cx| {
        window.double_click(("card-status", 1usize), cx)
    });
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);

    split_again(cx, handle, SplitRight);
    settle(cx, handle);
    in_frame(cx, handle, |window, cx| {
        window.double_click(("card-status", 1usize), cx)
    });
    settle(cx, handle);
    assert_eq!(
        cx.update(|cx| workspace.read(cx).zoomed_card()),
        Some(CardId(1))
    );
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(CARD_2).is_none());
        assert_eq!(
            window.find(("card-restore", 1usize)).label(),
            Some("还原卡片")
        );
    });

    in_frame(cx, handle, |window, cx| {
        window.click(("card-restore", 1usize), cx)
    });
    settle(cx, handle);
    assert_eq!(cx.update(|cx| workspace.read(cx).zoomed_card()), None);
    in_frame(cx, handle, |window, _| {
        assert!(window.find(CARD_2).visible());
        assert!(window.try_find(("card-restore", 1usize)).is_none());
    });
}

/// 右键菜单「移到新标签」：卡片从分屏里拆出去，单独成一个在前的标签，shell 照常跑；
/// 只剩它一张时再点什么都不做。
#[gpui_kit::test]
fn the_terminal_menu_moves_a_card_to_a_new_tab(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_tab(cx, FakeTerminalFactory::default());
    split_again(cx, handle, SplitRight);
    settle(cx, handle);

    click_terminal_menu(cx, handle, 1, "移到新标签");
    settle(cx, handle);

    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.tab_ids(), [TabId(1), TabId(2)]);
        assert_eq!(workspace.active_tab(), Some(TabId(2)));
    });
    assert_eq!(card_ids(cx, &workspace), [1]);
    assert_eq!(focused_card(cx, &workspace), Some(CardId(1)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("terminal", 1usize)).focused(), Some(true));
    });

    click_terminal_menu(cx, handle, 1, "移到新标签");
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids().len(), 2));
}
