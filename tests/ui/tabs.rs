//! 多标签（工单 04）的 UI 测试：新建、点标签和快捷键切换、×、右键菜单的三种关闭、
//! 关掉最后一个回到开始卡片、标签宽度固定和轨道横向滚动；拖动排序（工单 29）。本地终端一律用假传输。
//!
//! 每个标签里只有一张卡片（分屏是工单 05），所以第 n 个新开的标签里是第 n 张卡片。

use gpui_kit::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, ScrollDelta, Window, point,
};
use qishell::app::{
    ActivateTab, CloseActiveTab, CloseOtherTabs, CloseTab, CloseTabsToRight, NextTab, SwitchToTab,
};

use crate::support::*;

/// 打开窗口壳，新开 `count` 个终端标签；返回假传输的工厂，看关了几个 shell。
fn open_tabs(
    cx: &mut TestAppContext,
    count: usize,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
) {
    open_tabs_with(cx, count, FakeTerminalFactory::default())
}

fn open_tabs_with(
    cx: &mut TestAppContext,
    count: usize,
    factory: FakeTerminalFactory,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
) {
    let factory = Arc::new(factory);
    let provider = fake_local_terminals_with(factory.clone());
    let (handle, workspace) =
        open_workspace_with(cx, settings_store(), services_with_terminals(provider));
    for _ in 0..count {
        dispatch(cx, handle, NewLocalTerminalTab);
    }
    (handle, workspace, factory)
}

fn tab(id: u64) -> (&'static str, usize) {
    ("tab", id as usize)
}

fn tab_ids(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<TabId> {
    cx.update(|cx| workspace.read(cx).tab_ids())
}

fn active_tab(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<TabId> {
    cx.update(|cx| workspace.read(cx).active_tab())
}

/// 第 `id` 个标签在前：画的是它的卡片，焦点在它的终端里。
fn assert_in_front(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: u64) {
    in_frame(cx, handle, |window, _| {
        assert!(window.find(("card", id as usize)).visible());
        assert_eq!(
            window.find(("terminal", id as usize)).focused(),
            Some(true),
            "terminal {id}"
        );
    });
}

/// 点右键菜单的第 `item` 项（从 0 数起），先确认菜单只有这三项。
fn pick_from_menu(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: u64, item: usize) {
    in_frame(cx, handle, |window, cx| window.right_click(tab(id), cx));
    in_frame(cx, handle, |window, cx| {
        let mut popup = window.within("popup-menu");
        let labels: Vec<_> = (0..3_usize)
            .map(|item| popup.find(item).label().map(str::to_owned))
            .collect();
        assert_eq!(
            labels,
            ["关闭标签", "关闭其他标签", "关闭右侧的标签"].map(|label| Some(label.to_owned()))
        );
        assert!(popup.try_find(3_usize).is_none(), "only three items");
        popup.click(item, cx);
    });
}

#[gpui_kit::test]
fn a_new_tab_goes_last_and_comes_to_the_front(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2), TabId(3)]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(3)));
    assert_in_front(cx, handle, 3);
    in_frame(cx, handle, |window, _| {
        let (first, second, third) = (
            window.find(tab(1)).bounds(),
            window.find(tab(2)).bounds(),
            window.find(tab(3)).bounds(),
        );
        assert!(first.right() <= second.left() && second.right() <= third.left());
        // 别的标签的卡片不画。
        assert!(window.try_find(("card", 1usize)).is_none());
    });
}

#[gpui_kit::test]
fn clicking_a_tab_brings_it_to_the_front(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    in_frame(cx, handle, |window, cx| window.click(tab(1), cx));

    assert_eq!(active_tab(cx, &workspace), Some(TabId(1)));
    assert_in_front(cx, handle, 1);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(("card", 3usize)).is_none())
    });
    // 切走的标签里的 shell 还在。
    cx.update(|cx| assert!(workspace.read(cx).card(CardId(3), cx).is_some()));

    dispatch(cx, handle, ActivateTab(TabId(2)));
    assert_in_front(cx, handle, 2);
}

/// Ctrl+Tab / Ctrl+PageDown 往右，Ctrl+Shift+Tab / Ctrl+PageUp 往左，过了两头绕回来。
/// 焦点在终端里，快捷键照样能用。
#[gpui_kit::test]
fn the_keyboard_steps_through_the_tabs_round_the_ends(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    let mut steps = vec![];
    for key in [
        "ctrl-tab",
        "ctrl-tab",
        "ctrl-shift-tab",
        "ctrl-pagedown",
        "ctrl-pagedown",
        "ctrl-pageup",
        "ctrl-pageup",
    ] {
        in_frame(cx, handle, |window, cx| window.press(key, cx));
        steps.push(active_tab(cx, &workspace).unwrap().0);
    }

    assert_eq!(steps, [1, 2, 1, 2, 3, 2, 1]);
    assert_in_front(cx, handle, 1);
}

/// Ctrl+Shift+1…8 切到第 n 个，Ctrl+Shift+9 是最后一个；没有那个位置时不动。
#[gpui_kit::test]
fn ctrl_shift_digit_switches_to_the_nth_tab(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 4);

    let mut steps = vec![];
    for key in [
        "ctrl-shift-1",
        "ctrl-shift-3",
        "ctrl-shift-9",
        "ctrl-shift-2",
        "ctrl-shift-5",
    ] {
        in_frame(cx, handle, |window, cx| window.press(key, cx));
        steps.push(active_tab(cx, &workspace).unwrap().0);
    }

    assert_eq!(steps, [1, 3, 4, 2, 2]);
    assert_in_front(cx, handle, 2);
    dispatch(cx, handle, SwitchToTab(9));
    assert_in_front(cx, handle, 4);
}

/// × 只在悬停时出现，点了关闭整个标签和它的 shell。
#[gpui_kit::test]
fn the_cross_shows_on_hover_and_closes_the_tab(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 3);

    in_frame(cx, handle, |window, cx| {
        // 鼠标在别处：哪个标签都没有 ×，在前的那个也没有。
        window.hover("title-drag", cx);
        assert!(!window.find(("tab-close", 1usize)).visible());
        assert!(!window.find(("tab-close", 3usize)).visible());
        window.hover(tab(2), cx);
        let close = window.find(("tab-close", 2usize));
        assert!(close.visible());
        assert_eq!(close.label(), Some("关闭标签"));
        assert!(
            window
                .find(tab(2))
                .bounds()
                .contains(&close.bounds().center())
        );
        assert!(!window.find(("tab-close", 1usize)).visible());
        window.click(("tab-close", 2usize), cx);
    });

    assert_eq!(factory.wait_for_shutdowns(1), 1);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(3)]);
    // 关的不是在前的那个：在前的不变。
    assert_eq!(active_tab(cx, &workspace), Some(TabId(3)));
    cx.update(|cx| assert!(workspace.read(cx).card(CardId(2), cx).is_none()));
    assert_in_front(cx, handle, 3);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find(tab(2)).is_none())
    });
}

#[gpui_kit::test]
fn the_menu_closes_the_tab(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 3);

    pick_from_menu(cx, handle, 3, 0);

    assert_eq!(factory.wait_for_shutdowns(1), 1);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2)]);
    // 关的是在前的那个：左边的到前面来。
    assert_eq!(active_tab(cx, &workspace), Some(TabId(2)));
    assert_in_front(cx, handle, 2);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
}

/// 关闭整个标签（macOS 的 ⇧⌘W）：在前的标签和它的全部卡片一起关，左边的到前面来。
#[gpui_kit::test]
fn closing_the_active_tab_closes_all_its_cards(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 2);
    dispatch(cx, handle, SplitRight);
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_cards(TabId(2)).len(), 2));

    dispatch(cx, handle, CloseActiveTab);

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1)]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(1)));
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        for card in [2, 3] {
            assert!(workspace.card(CardId(card), cx).is_none(), "card {card}");
        }
    });
}

/// 「关闭其他标签」：只留下点的那个，它到前面来。
#[gpui_kit::test]
fn the_menu_closes_the_other_tabs(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 4);

    pick_from_menu(cx, handle, 2, 1);

    assert_eq!(factory.wait_for_shutdowns(3), 3);
    assert_eq!(tab_ids(cx, &workspace), [TabId(2)]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(2)));
    assert_in_front(cx, handle, 2);
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        for card in [1, 3, 4] {
            assert!(workspace.card(CardId(card), cx).is_none(), "card {card}");
        }
    });
}

/// 「关闭右侧的标签」：在前的那个被关了，点的那个到前面来。
#[gpui_kit::test]
fn the_menu_closes_the_tabs_to_the_right(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 4);

    pick_from_menu(cx, handle, 2, 2);

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2)]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(2)));
    assert_in_front(cx, handle, 2);
}

/// 在前的标签在左边、没被关时，它留在前面。
#[gpui_kit::test]
fn closing_to_the_right_keeps_the_front_tab_on_the_left(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 4);
    dispatch(cx, handle, ActivateTab(TabId(1)));

    dispatch(cx, handle, CloseTabsToRight(TabId(2)));

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2)]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(1)));
    assert_in_front(cx, handle, 1);
}

/// 关掉最后一个标签：回到开始卡片，程序不退出，开始卡片拿到焦点、还能新开标签。
#[gpui_kit::test]
fn closing_the_last_tab_shows_the_start_card(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 2);

    dispatch(cx, handle, CloseOtherTabs(TabId(1)));
    in_frame(cx, handle, |window, cx| {
        window.hover(tab(1), cx);
        window.click(("tab-close", 1usize), cx);
    });

    assert_eq!(factory.wait_for_shutdowns(2), 2);
    assert!(tab_ids(cx, &workspace).is_empty());
    in_frame(cx, handle, |window, _| {
        assert!(window.find("start-card").visible());
        assert_eq!(window.find("start-card").focused(), Some(true));
        assert!(window.try_find(tab(1)).is_none());
    });
    // 窗口还在。
    assert!(cx.update(|cx| cx.windows().len()) == 1);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-t", cx));
    assert_eq!(tab_ids(cx, &workspace), [TabId(3)]);
    assert_in_front(cx, handle, 3);
}

/// 标签不随内容伸缩，一律宽 180；放不下时轨道横向滚动，新开的标签露出来，滚轮能滚回去。
#[gpui_kit::test]
fn tabs_keep_their_width_and_the_track_scrolls(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 10);

    in_frame(cx, handle, |window, _| {
        let track = window.find("tab-track").bounds();
        assert!(track.size.width < px(180. * 10.), "{track:?}");
        assert!(track.right() <= window.find("new-tab").bounds().left());
        for id in 1..=10 {
            assert_eq!(
                window.find(tab(id)).bounds().size.width,
                px(180.),
                "tab {id}"
            );
        }
        // 最后一个（在前的）滚进来了，第一个滚出去了。
        let last = window.find(tab(10));
        assert!(last.visible());
        assert!(track.contains(&last.bounds().center()));
        assert!(!window.find(tab(1)).visible());
    });

    // 普通滚轮只有竖直增量，也能横着滚。
    in_frame(cx, handle, |window, cx| {
        window.scroll(
            "tab-track",
            ScrollDelta::Pixels(point(px(0.), px(5000.))),
            cx,
        )
    });
    in_frame(cx, handle, |window, _| {
        let track = window.find("tab-track").bounds();
        assert!(track.contains(&window.find(tab(1)).bounds().center()));
        assert!(!window.find(tab(10)).visible());
    });

    // 开了右键菜单又 Esc 关掉：滚动位置不动。
    in_frame(cx, handle, |window, cx| window.right_click(tab(1), cx));
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none());
        let track = window.find("tab-track").bounds();
        assert!(track.contains(&window.find(tab(1)).bounds().center()));
    });

    // 切到滚出去的标签，它又露出来。
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-9", cx));
    assert_eq!(active_tab(cx, &workspace), Some(TabId(10)));
    in_frame(cx, handle, |window, _| {
        let track = window.find("tab-track").bounds();
        assert!(track.contains(&window.find(tab(10)).bounds().center()));
    });
}

/// 标签写焦点卡片的名称和目录，目录跟着 shell 变。
#[gpui_kit::test]
fn the_tab_follows_the_folder_of_its_card(cx: &mut TestAppContext) {
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
    let folder: &'static str = Box::leak(
        std::path::Path::new(&home)
            .join("vibe")
            .display()
            .to_string()
            .into_boxed_str(),
    );
    let (handle, _, _) = open_tabs_with(cx, 1, FakeTerminalFactory::in_folder(folder));

    let expected = if cfg!(windows) {
        r"pwsh ~\vibe"
    } else {
        "pwsh ~/vibe"
    };
    let started = std::time::Instant::now();
    let label = loop {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        let label = in_frame(cx, handle, |window, _| {
            window.find(tab(1)).label().map(str::to_owned)
        });
        if label.as_deref() == Some(expected)
            || started.elapsed() > std::time::Duration::from_secs(2)
        {
            break label;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(label.as_deref(), Some(expected));
}

/// 没有这个标签的 action 什么都不做。
#[gpui_kit::test]
fn actions_for_a_tab_that_is_gone_do_nothing(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 2);

    dispatch(cx, handle, CloseTab(TabId(9)));
    dispatch(cx, handle, CloseOtherTabs(TabId(9)));
    dispatch(cx, handle, CloseTabsToRight(TabId(9)));
    dispatch(cx, handle, ActivateTab(TabId(9)));
    dispatch(cx, handle, NextTab);

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(factory.shutdowns(), 0);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2)]);
    // 只有 Ctrl+Tab 动了：从 2 绕回 1。
    assert_eq!(active_tab(cx, &workspace), Some(TabId(1)));
}

/// 把第 `source` 个标签拖到第 `target` 个标签的左半边（`before`）或右半边，松开。
fn drag_tab(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    source: u64,
    target: u64,
    before: bool,
) {
    in_frame(cx, handle, |window, cx| {
        let from = window.find(tab(source)).bounds().center();
        let bounds = window.find(tab(target)).bounds();
        let x = if before {
            bounds.left() + bounds.size.width * 0.25
        } else {
            bounds.left() + bounds.size.width * 0.75
        };
        window.drag(from, point(x, bounds.center().y), cx);
    });
}

/// 标签在轨道里从左到右的顺序（按画出来的位置），和窗口壳里的顺序一致。
fn assert_order(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    order: &[u64],
) {
    let ids: Vec<TabId> = order.iter().map(|id| TabId(*id)).collect();
    assert_eq!(tab_ids(cx, workspace), ids);
    in_frame(cx, handle, |window, _| {
        let lefts: Vec<_> = order
            .iter()
            .map(|id| window.find(tab(*id)).bounds().left())
            .collect();
        assert!(lefts.windows(2).all(|pair| pair[0] < pair[1]), "{lefts:?}");
    });
}

/// 拖动排序（工单 29）：拖到别的标签的右半边放在它后面，左半边放在它前面；
/// 拖动不切换在前的标签，松开后在前的还是原来那个。
#[gpui_kit::test]
fn dragging_a_tab_moves_it_and_keeps_the_front_one(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 4);

    drag_tab(cx, handle, 1, 3, false);
    assert_order(cx, handle, &workspace, &[2, 3, 1, 4]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(4)));

    drag_tab(cx, handle, 4, 2, true);
    assert_order(cx, handle, &workspace, &[4, 2, 3, 1]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(4)));
    assert_in_front(cx, handle, 4);
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("tab-drop-line").is_none())
    });
}

/// 拖回自己身上、或自己两边的缝：顺序不变，也不切到前面（拖动不是点击）。
#[gpui_kit::test]
fn dragging_a_tab_onto_itself_changes_nothing(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    drag_tab(cx, handle, 2, 2, false);
    drag_tab(cx, handle, 2, 3, true);
    drag_tab(cx, handle, 2, 1, false);

    assert_order(cx, handle, &workspace, &[1, 2, 3]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(3)));
    assert_in_front(cx, handle, 3);
}

/// 不经 `window.drag`，一步一步发鼠标事件：拖到半路看插入线。
fn pointer(window: &mut Window, input: PlatformInput, cx: &mut App) {
    window.dispatch_event(input, cx);
    window.render_frame(cx);
}

fn press_at(window: &mut Window, position: Point<Pixels>, cx: &mut App) {
    pointer(
        window,
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    pointer(
        window,
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn move_to(window: &mut Window, position: Point<Pixels>, cx: &mut App) {
    pointer(
        window,
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn release_at(window: &mut Window, position: Point<Pixels>, cx: &mut App) {
    pointer(
        window,
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// 拖动中有插入线：落在要放下的缝上；拖回自己身上没有；拖出标题栏没有，
/// 在那里松开就取消，什么都不变。
#[gpui_kit::test]
fn dragging_shows_where_the_tab_will_go(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    in_frame(cx, handle, |window, cx| {
        let first = window.find(tab(1)).bounds();
        let third = window.find(tab(3)).bounds();
        press_at(window, first.center(), cx);
        move_to(window, point(third.right() - px(20.), third.center().y), cx);
        move_to(window, point(third.right() - px(10.), third.center().y), cx);
        // 第三个的右半边：线在它右边。
        let line = window.find("tab-drop-line").bounds();
        assert!(
            (line.center().x - third.right()).abs() < px(2.),
            "{line:?} {third:?}"
        );

        let second = window.find(tab(2)).bounds();
        move_to(
            window,
            point(second.left() + px(10.), second.center().y),
            cx,
        );
        // 第二个的左半边，就是第一个自己右边的缝：不挪，没有线。
        assert!(window.try_find("tab-drop-line").is_none());

        move_to(
            window,
            point(second.right() - px(10.), second.center().y),
            cx,
        );
        let line = window.find("tab-drop-line").bounds();
        assert!(
            (line.center().x - second.right()).abs() < px(2.),
            "{line:?}"
        );

        // 拖到终端里：不在标题栏，没有线；在那里松开就取消。
        let card = window.find(("card", 3usize)).bounds().center();
        move_to(window, card, cx);
        assert!(window.try_find("tab-drop-line").is_none());
        release_at(window, card, cx);
    });

    assert_order(cx, handle, &workspace, &[1, 2, 3]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(3)));
    assert_in_front(cx, handle, 3);
}

/// 拖过轨道右端，放在「+」或拖动区上：放到最后。
#[gpui_kit::test]
fn dropping_past_the_track_puts_the_tab_last(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 3);

    in_frame(cx, handle, |window, cx| {
        let from = window.find(tab(1)).bounds().center();
        let to = window.find("new-tab").bounds().center();
        window.drag(from, to, cx);
    });
    assert_order(cx, handle, &workspace, &[2, 3, 1]);

    in_frame(cx, handle, |window, cx| {
        let from = window.find(tab(2)).bounds().center();
        let to = window.find("title-drag").bounds().center();
        window.drag(from, to, cx);
    });
    assert_order(cx, handle, &workspace, &[3, 1, 2]);
    assert_eq!(active_tab(cx, &workspace), Some(TabId(3)));
}

/// 轨道放不下时，拖到轨道左端停住：轨道自己往回滚，滚到头放下就是第一个。
#[gpui_kit::test]
fn dragging_to_the_end_of_the_track_scrolls_it(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_tabs(cx, 10);

    in_frame(cx, handle, |window, cx| {
        assert!(!window.find(tab(1)).visible());
        let track = window.find("tab-track").bounds();
        let from = window.find(tab(10)).bounds().center();
        let edge = point(track.left() + px(6.), from.y);
        press_at(window, from, cx);
        move_to(window, point(from.x - px(40.), from.y), cx);
        move_to(window, edge, cx);
        // 鼠标停着不动，一帧帧滚回最左边。
        for _ in 0..400 {
            window.render_frame(cx);
        }
        let track = window.find("tab-track").bounds();
        assert!(track.contains(&window.find(tab(1)).bounds().center()));
        let line = window.find("tab-drop-line").bounds();
        assert!(
            (line.center().x - window.find(tab(1)).bounds().left()).abs() < px(2.),
            "{line:?}"
        );
        release_at(window, edge, cx);
    });

    let mut order: Vec<u64> = vec![10];
    order.extend(1..=9);
    assert_eq!(
        tab_ids(cx, &workspace),
        order.into_iter().map(TabId).collect::<Vec<_>>()
    );
    assert_eq!(active_tab(cx, &workspace), Some(TabId(10)));
}

/// Esc 关掉右键菜单：什么都不关，焦点回到在前标签的终端。
#[gpui_kit::test]
fn escape_dismisses_the_menu_and_gives_the_focus_back(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_tabs(cx, 2);

    in_frame(cx, handle, |window, cx| window.right_click(tab(1), cx));
    in_frame(cx, handle, |window, cx| {
        assert!(window.find("popup-menu").visible());
        window.press("escape", cx);
    });

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    assert_in_front(cx, handle, 2);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(factory.shutdowns(), 0);
    assert_eq!(tab_ids(cx, &workspace), [TabId(1), TabId(2)]);
}
