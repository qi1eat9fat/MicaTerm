//! 终端交互（工单 03）的 UI 测试：复制粘贴、全选、清屏、字号的快捷键，查找条，右键行为，
//! 选中即复制，Ctrl+单击打开链接，鼠标事件交给程序，OSC 52，kitty 键盘协议，通知和响铃，
//! 只按 Ctrl 的组合交给终端。本地终端一律用假传输，不起真的 shell。
// 用例参考 since2006/shell-rs @ b2a307a: tests/ui/terminal.rs，按 QiShell 的元素 id 改写。

use std::time::Duration;

use gpui_kit::{
    ClipboardItem, InputEvent as _, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point,
};
use qishell::settings::RightClick;
use qishell_terminal::TerminalLifecycle;
use qishell_terminal::actions::{CopyTerminal, SelectAllTerminal};

use crate::support::*;

const TERMINAL: (&str, usize) = ("terminal", 1usize);

/// 用假本地终端打开窗口壳，开一个终端标签，等假 shell 打出提示符并排好版。
fn open_running(
    cx: &mut TestAppContext,
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
    dispatch(cx, handle, NewLocalTerminalTab);
    let ready = wait_until(cx, handle, |cx| {
        cx.update(|cx| {
            running(&workspace, CardId(1), cx) && screen(&workspace, cx).contains("会议纪要")
        }) && factory.last_size().is_some()
    });
    assert!(ready, "假 shell 没有打出提示符");
    (handle, workspace, factory)
}

/// 假 shell 打出 `text`（以 `marker` 结尾）的本地终端，排好版。
fn open_printing(
    cx: &mut TestAppContext,
    text: &'static str,
    marker: &str,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::printing(text));
    let printed = wait_until(cx, handle, |cx| {
        cx.update(|cx| screen(&workspace, cx)).contains(marker)
    });
    assert!(printed, "{}", cx.update(|cx| screen(&workspace, cx)));
    (handle, workspace, factory)
}

fn running(workspace: &Entity<Workspace>, card: CardId, cx: &App) -> bool {
    workspace.read(cx).card(card, cx).is_some_and(|card| {
        card.read(cx).terminal().read(cx).lifecycle(cx) == TerminalLifecycle::Running
    })
}

/// 第一张卡片终端屏幕上的文字。
fn screen(workspace: &Entity<Workspace>, cx: &App) -> String {
    workspace
        .read(cx)
        .card_screen_text(CardId(1), cx)
        .unwrap_or_default()
}

/// 按真实时间等 `done` 成立，最多两秒：假 shell 跑在自己的线程上，测试时钟等不到它。
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

fn clipboard(cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn set_clipboard(cx: &mut TestAppContext, text: &str) {
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.into())));
}

/// 等假 shell 收到的正好是 `expected`，然后清掉记录。
fn take_written(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    factory: &FakeTerminalFactory,
    expected: &str,
) {
    let written = wait_until(cx, handle, |_| factory.written_text() == expected);
    assert!(written, "假 shell 收到的是 {:?}", factory.written_text());
    factory.clear_writes();
}

/// 给一会儿时间，确认假 shell 什么也没收到。
fn assert_nothing_written(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    factory: &FakeTerminalFactory,
) {
    let written = wait_until(cx, handle, |_| !factory.written_text().is_empty());
    assert!(!written, "假 shell 收到了 {:?}", factory.written_text());
}

fn update_settings(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    change: impl FnOnce(&mut qishell::settings::AppSettings),
) {
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        settings.update(cx, |store, cx| store.update(change, cx));
    });
    cx.run_until_parked();
}

/// 终端第 `column` 列、第 `row` 行格子的中心，按终端告诉假 shell 的格子尺寸算。
fn cell_center(
    window: &mut Window,
    factory: &FakeTerminalFactory,
    column: usize,
    row: usize,
) -> Point<Pixels> {
    let size = factory.last_size().expect("the terminal has been laid out");
    let bounds = window.find(TERMINAL).bounds();
    point(
        bounds.left() + px(size.cell_width() as f32 * (column as f32 + 0.5)),
        bounds.top() + px(size.cell_height() as f32 * (row as f32 + 0.5)),
    )
}

/// 在一个点上按住修饰键单击左键：`TestWindowExt` 的点击不带修饰键。
fn click_at(window: &mut Window, position: Point<Pixels>, modifiers: Modifiers, cx: &mut App) {
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

// ---- 复制、粘贴、全选、清屏 ----

#[gpui_kit::test]
fn ctrl_shift_a_c_v_select_all_copy_and_paste(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_running(cx, FakeTerminalFactory::default());

    set_clipboard(cx, "原剪贴板");
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-a", cx));
    // 全选只是选中：选中即复制没开，剪贴板不变。
    assert_eq!(clipboard(cx).as_deref(), Some("原剪贴板"));
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-c", cx));
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("run:1$ alpha.txt 会议纪要.md")
    );

    set_clipboard(cx, "echo 粘贴");
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-v", cx));
    take_written(cx, handle, &factory, "echo 粘贴");
}

/// 本地清屏只清这一边的屏幕和回滚，留下提示符那一行，不往 shell 发任何东西。
#[gpui_kit::test]
fn ctrl_shift_k_clears_the_screen_keeping_the_prompt_line(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    in_frame(cx, handle, |window, cx| {
        window.input("root@localhost:~# ", cx)
    });
    take_written(cx, handle, &factory, "root@localhost:~# ");
    let echoed = wait_until(cx, handle, |cx| {
        cx.update(|cx| screen(&workspace, cx))
            .contains("root@localhost:~#")
    });
    assert!(echoed);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-k", cx));

    assert_eq!(
        cx.update(|cx| screen(&workspace, cx)).trim_end(),
        "root@localhost:~#"
    );
    assert_nothing_written(cx, handle, &factory);
}

// ---- 字号 ----

/// Ctrl+= / Ctrl+- 一次一像素，Ctrl+0 回到默认的 11.6；改的是设置里的终端字号。
#[gpui_kit::test]
fn ctrl_plus_minus_and_zero_size_the_terminal_text(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    let size = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .settings()
                .read(cx)
                .settings()
                .terminal_font
                .size
        })
    };

    in_frame(cx, handle, |window, cx| window.press("ctrl-=", cx));
    assert_eq!(size(cx), 12.6);
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl--", cx);
        window.press("ctrl--", cx);
    });
    assert_eq!(size(cx), 10.6);
    in_frame(cx, handle, |window, cx| window.press("ctrl-0", cx));
    assert_eq!(size(cx), 11.6);
    // 开着的终端跟着变。
    cx.update(|cx| assert_eq!(cx.global::<qishell_terminal::TerminalFont>().size, px(11.6)));
    // 这几个键是终端的功能，不会也发给 shell。
    assert_nothing_written(cx, handle, &factory);
}

// ---- 查找 ----

fn find_count(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    in_frame(cx, handle, |window, _| {
        window
            .try_find("terminal-find-count")
            .and_then(|count| count.label().map(str::to_owned))
    })
}

fn wait_for_find_count(cx: &mut TestAppContext, handle: WindowHandle<Root>, label: &str) {
    let shown = wait_until(cx, handle, |cx| {
        find_count(cx, handle).as_deref() == Some(label)
    });
    assert!(
        shown,
        "查找计数是 {:?}，不是 {label}",
        find_count(cx, handle)
    );
}

/// Ctrl+Shift+F 打开查找条；小写的查询不分大小写，带大写的才区分（智能大小写）；
/// F3 / Shift+F3 跳到下一项 / 上一项，到头了绕回来；Esc 关掉，键盘还给终端。
#[gpui_kit::test]
fn the_find_bar_finds_with_smart_case_and_steps_with_f3(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    // 提示符里有一个 alpha，假 shell 再回显 Alpha 和 alpha。
    in_frame(cx, handle, |window, cx| window.input("Alpha alpha", cx));
    take_written(cx, handle, &factory, "Alpha alpha");
    let echoed = wait_until(cx, handle, |cx| {
        cx.update(|cx| screen(&workspace, cx))
            .contains("Alpha alpha")
    });
    assert!(echoed);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-f", cx));
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("terminal-find").focused(), Some(true));
        // 还没有查询时不显示计数。
        assert!(window.try_find("terminal-find-count").is_none());
        window.input("alpha", cx);
    });
    wait_for_find_count(cx, handle, "1/3");
    // 查找条里打的字不会到 shell。
    assert_nothing_written(cx, handle, &factory);

    in_frame(cx, handle, |window, cx| window.press("f3", cx));
    wait_for_find_count(cx, handle, "2/3");
    in_frame(cx, handle, |window, cx| {
        window.press("shift-f3", cx);
        window.press("shift-f3", cx);
    });
    // 从第一项往上，绕到最后一项。
    wait_for_find_count(cx, handle, "3/3");

    // 再按一次 Ctrl+Shift+F 选中原来的查询，打字就换掉它；带大写就区分大小写。
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-f", cx));
    in_frame(cx, handle, |window, cx| window.input("Alpha", cx));
    wait_for_find_count(cx, handle, "1/1");

    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("terminal-find").is_none());
        assert_eq!(window.find(TERMINAL).focused(), Some(true));
        window.input("!", cx);
    });
    take_written(cx, handle, &factory, "!");
}

/// 焦点在终端里时 F3 也能跳：查找的键在终端和查找条里都生效。
#[gpui_kit::test]
fn f3_steps_from_the_terminal_too(cx: &mut TestAppContext) {
    let (handle, _, _) = open_printing(cx, "beta beta\r\n", "beta beta");
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-f", cx));
    in_frame(cx, handle, |window, cx| window.input("beta", cx));
    wait_for_find_count(cx, handle, "1/2");

    // 点回终端，查找条还开着。
    in_frame(cx, handle, |window, cx| window.click(TERMINAL, cx));
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find(TERMINAL).focused(), Some(true));
        window.press("f3", cx);
    });
    wait_for_find_count(cx, handle, "2/2");
}

// ---- 右键行为 ----

fn set_right_click(cx: &mut TestAppContext, workspace: &Entity<Workspace>, action: RightClick) {
    update_settings(cx, workspace, |settings| {
        settings.terminal_interaction.right_click = action
    });
}

/// 默认显示菜单，照 shell-rs：复制、粘贴、全选，分隔线，查找、清屏。
#[gpui_kit::test]
fn right_click_shows_the_terminal_menu_by_default(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_running(cx, FakeTerminalFactory::default());
    set_clipboard(cx, "菜单粘贴");

    in_frame(cx, handle, |window, cx| window.right_click(TERMINAL, cx));
    in_frame(cx, handle, |window, cx| {
        let mut menu = window.within("popup-menu");
        let labels: Vec<_> = [0usize, 1, 2, 4, 5]
            .into_iter()
            .map(|item| menu.find(item).label().map(str::to_owned))
            .collect();
        assert_eq!(
            labels,
            ["复制", "粘贴", "全选", "查找…", "清屏"].map(|label| Some(label.to_owned()))
        );
        menu.click(1usize, cx);
    });
    take_written(cx, handle, &factory, "菜单粘贴");
}

#[gpui_kit::test]
fn right_click_pastes_when_set_to_paste(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    set_right_click(cx, &workspace, RightClick::Paste);
    set_clipboard(cx, "右键粘贴");

    in_frame(cx, handle, |window, cx| window.right_click(TERMINAL, cx));

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    take_written(cx, handle, &factory, "右键粘贴");
}

/// 有选中文字就复制（并取消选中），否则粘贴。
#[gpui_kit::test]
fn right_click_copies_a_selection_or_else_pastes(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    set_right_click(cx, &workspace, RightClick::CopyOrPaste);
    set_clipboard(cx, "原剪贴板");

    dispatch(cx, handle, SelectAllTerminal);
    in_frame(cx, handle, |window, cx| window.right_click(TERMINAL, cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("run:1$ alpha.txt 会议纪要.md")
    );
    assert_nothing_written(cx, handle, &factory);

    // 选中已经取消：这次是粘贴。
    in_frame(cx, handle, |window, cx| window.right_click(TERMINAL, cx));
    take_written(cx, handle, &factory, "run:1$ alpha.txt 会议纪要.md");
}

#[gpui_kit::test]
fn right_click_does_nothing_when_set_to_nothing(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    set_right_click(cx, &workspace, RightClick::Nothing);
    set_clipboard(cx, "不该粘贴");
    dispatch(cx, handle, SelectAllTerminal);

    in_frame(cx, handle, |window, cx| window.right_click(TERMINAL, cx));

    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    // 选中还在，也没有复制。
    assert_eq!(clipboard(cx).as_deref(), Some("不该粘贴"));
    assert_nothing_written(cx, handle, &factory);
    dispatch(cx, handle, CopyTerminal);
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("run:1$ alpha.txt 会议纪要.md")
    );
}

// ---- 选中即复制、链接、鼠标 ----

/// 选中即复制默认关：拖选不动剪贴板。打开以后拖选就复制；单击什么也没选中，剪贴板不变。
#[gpui_kit::test]
fn copy_on_select_puts_what_the_mouse_selects_on_the_clipboard(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    // 从第一格的左边缘拖起：从格子中间拖起会漏掉第一个字。
    let drag_first_row = |window: &mut Window, cx: &mut App| {
        let from = cell_center(window, &factory, 0, 0) - point(px(3.), px(0.));
        let to = cell_center(window, &factory, 40, 0);
        window.drag(from, to, cx);
    };

    set_clipboard(cx, "原剪贴板");
    in_frame(cx, handle, drag_first_row);
    assert_eq!(clipboard(cx).as_deref(), Some("原剪贴板"));

    update_settings(cx, &workspace, |settings| {
        settings.terminal_interaction.copy_on_select = true
    });
    in_frame(cx, handle, |window, cx| {
        let from = cell_center(window, &factory, 0, 0);
        click_at(window, from, Modifiers::none(), cx);
    });
    assert_eq!(clipboard(cx).as_deref(), Some("原剪贴板"));
    in_frame(cx, handle, drag_first_row);
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("run:1$ alpha.txt 会议纪要.md")
    );
}

/// 单击链接只是选择；按住 Ctrl 单击才打开（不带句末的句号）。
#[gpui_kit::test]
fn ctrl_click_opens_a_link(cx: &mut TestAppContext) {
    // 第 0 行是提示符，链接在第 1 行。
    let (handle, _, factory) = open_printing(cx, "https://example.com/docs. 文档\r\n", "文档");

    in_frame(cx, handle, |window, cx| {
        let link = cell_center(window, &factory, 2, 1);
        click_at(window, link, Modifiers::none(), cx);
    });
    assert_eq!(cx.opened_url(), None);

    in_frame(cx, handle, |window, cx| {
        let link = cell_center(window, &factory, 2, 1);
        click_at(window, link, Modifiers::secondary_key(), cx);
    });
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs"));
}

/// 程序要了鼠标（1000 点击，1006 SGR 编码），单击和滚轮就交给它；按住 Shift 单击是选择，不交给它。
#[gpui_kit::test]
fn mouse_events_go_to_a_program_that_asks_and_shift_selects(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_printing(cx, "\x1b[?1000h\x1b[?1006hmouse on", "mouse on");

    in_frame(cx, handle, |window, cx| {
        let cell = cell_center(window, &factory, 2, 0);
        click_at(window, cell, Modifiers::none(), cx);
    });
    take_written(cx, handle, &factory, "\x1b[<0;3;1M\x1b[<0;3;1m");

    in_frame(cx, handle, |window, cx| {
        let from = cell_center(window, &factory, 0, 0);
        click_at(window, from, Modifiers::shift(), cx);
        let position = cell_center(window, &factory, 4, 1);
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0., 1.)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    });
    // Shift 单击没有交给程序，之后的滚轮交给了。
    take_written(cx, handle, &factory, "\x1b[<64;5;2M");
}

// ---- 程序发来的：OSC 52、kitty 键盘协议 ----

/// 程序用 OSC 52 把文字放到本机剪贴板（远程复制）。
#[gpui_kit::test]
fn a_program_copies_to_the_local_clipboard_with_osc_52(cx: &mut TestAppContext) {
    // 「你好」的 base64。
    let (handle, _, _) = open_printing(cx, "\x1b]52;c;5L2g5aW9\x07copied", "copied");
    let copied = wait_until(cx, handle, |cx| clipboard(cx).as_deref() == Some("你好"));
    assert!(copied, "剪贴板是 {:?}", clipboard(cx));
}

/// 没有 kitty 键盘协议时 Shift+Enter 就是 Enter。
#[gpui_kit::test]
fn shift_enter_is_enter_without_the_kitty_keyboard_protocol(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_running(cx, FakeTerminalFactory::default());
    in_frame(cx, handle, |window, cx| window.press("shift-enter", cx));
    take_written(cx, handle, &factory, "\r");
}

/// 程序打开 kitty 键盘协议（CSI > 1 u）以后，Shift+Enter 能和 Enter 分开。
#[gpui_kit::test]
fn a_program_that_asks_for_the_kitty_keyboard_protocol_tells_keys_apart(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_printing(cx, "\x1b[>1ukitty on", "kitty on");
    in_frame(cx, handle, |window, cx| window.press("shift-enter", cx));
    take_written(cx, handle, &factory, "\x1b[13;2u");
}

// ---- 只按 Ctrl 的组合 ----

/// 只按 Ctrl 加字母全部交给终端里的 shell（Ctrl+C 中断、tmux 的 Ctrl+B、readline 的 Ctrl+W……），
/// 应用和 gpui-kit 都不拦截。
#[gpui_kit::test]
fn ctrl_with_a_letter_goes_to_the_shell(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_running(cx, FakeTerminalFactory::default());
    // 屏幕上有选中的文字时 Ctrl+C 照样是中断，不是复制。
    dispatch(cx, handle, SelectAllTerminal);
    set_clipboard(cx, "原剪贴板");

    for letter in 'a'..='z' {
        in_frame(cx, handle, |window, cx| {
            window.press(&format!("ctrl-{letter}"), cx)
        });
        let control = char::from(letter as u8 - b'a' + 1).to_string();
        take_written(cx, handle, &factory, &control);
    }

    assert_eq!(clipboard(cx).as_deref(), Some("原剪贴板"));
    // 没有因为 Ctrl+T、Ctrl+W 开关标签。
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids(), [TabId(1)]));
}

/// 按键先过绑定、再到终端：在终端里，Ctrl 加字母不能被任何绑定拿走。gpui-kit 的根节点
/// 把 Ctrl+C 绑成了复制（界面里有选中文字时就会吞掉中断），要在终端的上下文里让回去。
#[gpui_kit::test]
fn no_binding_takes_ctrl_with_a_letter_inside_a_terminal(cx: &mut TestAppContext) {
    init_app(cx);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        // 终端在窗口里的键上下文路径。
        let path = ["Root", "Workspace", qishell_terminal::TERMINAL_KEY_CONTEXT]
            .map(|context| gpui_kit::KeyContext::parse(context).unwrap());
        for letter in 'a'..='z' {
            let keystroke = gpui_kit::Keystroke::parse(&format!("ctrl-{letter}")).unwrap();
            let (bindings, _) = keymap.bindings_for_input(&[keystroke], &path);
            let taken: Vec<_> = bindings
                .iter()
                .map(|binding| binding.action().name())
                .collect();
            assert!(taken.is_empty(), "ctrl-{letter} 被 {taken:?} 拿走了");
        }
    });
}

// ---- 通知和响铃（OSC 9 / 777、BEL） ----

/// 让卡片 `card` 的终端打出 `text`：假 shell 回显写进来的东西，括号粘贴没开时粘贴原样发过去。
fn print_into(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: CardId, text: &str) {
    set_clipboard(cx, text);
    cx.update(|cx| {
        let terminal = workspace
            .read(cx)
            .card(card, cx)
            .expect("the card is open")
            .read(cx)
            .terminal()
            .clone();
        terminal.update(cx, |terminal, cx| terminal.paste_clipboard(cx));
    });
}

fn in_window_notifications(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> usize {
    in_frame(cx, handle, |window, cx| window.notifications(cx).len())
}

fn activate_window(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, _| window.activate_window());
}

/// 窗口在前、卡片看得见：程序要的通知在窗口里提示；响铃不提示（用户就看着它）。
#[gpui_kit::test]
fn a_program_notifies_in_the_window_and_the_bell_in_sight_says_nothing(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_running(cx, FakeTerminalFactory::default());
    activate_window(cx, handle);

    print_into(
        cx,
        &workspace,
        CardId(1),
        "\x1b]777;notify;构建;完成\x07\x07",
    );

    let shown = wait_until(cx, handle, |cx| in_window_notifications(cx, handle) == 1);
    assert!(
        shown,
        "窗口里有 {} 条提示",
        in_window_notifications(cx, handle)
    );
    // 响铃没有再加一条。
    assert!(!wait_until(cx, handle, |cx| in_window_notifications(
        cx, handle
    ) > 1));
    assert!(cx.shown_system_notifications().is_empty());
}

/// 卡片在后面的标签里（看不见）：响铃在窗口里提示。
#[gpui_kit::test]
fn the_bell_of_a_card_out_of_sight_notifies_in_the_window(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_running(cx, FakeTerminalFactory::default());
    activate_window(cx, handle);
    dispatch(cx, handle, NewLocalTerminalTab);
    cx.update(|cx| assert_eq!(workspace.read(cx).active_tab(), Some(TabId(2))));

    print_into(cx, &workspace, CardId(1), "y/N?\x07");

    let shown = wait_until(cx, handle, |cx| in_window_notifications(cx, handle) == 1);
    assert!(
        shown,
        "窗口里有 {} 条提示",
        in_window_notifications(cx, handle)
    );
    assert!(cx.shown_system_notifications().is_empty());
}

/// 窗口不在前：通知走系统，标题是卡片的名称；点它就把窗口和那张卡片带到前面。
#[gpui_kit::test]
fn with_the_window_away_a_notice_goes_to_the_system(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_running(cx, FakeTerminalFactory::default());
    dispatch(cx, handle, NewLocalTerminalTab);
    gpui_kit::VisualTestContext::from_window(handle.into(), cx).deactivate_window();

    print_into(cx, &workspace, CardId(1), "\x1b]9;部署完成\x07");

    let shown = wait_until(cx, handle, |cx| !cx.shown_system_notifications().is_empty());
    assert!(shown, "没有系统通知");
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title.as_ref(), "pwsh");
    assert_eq!(shown[0].body.as_ref(), "部署完成");
    assert_eq!(in_window_notifications(cx, handle), 0);

    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: shown[0].tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(workspace.read(cx).active_tab(), Some(TabId(1))));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(TERMINAL).focused(), Some(true))
    });
}

/// 响铃的系统通知标题写明是响铃，正文是光标所在的那一行。
#[gpui_kit::test]
fn a_bell_notice_is_titled_as_a_bell(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_running(cx, FakeTerminalFactory::default());
    gpui_kit::VisualTestContext::from_window(handle.into(), cx).deactivate_window();

    print_into(cx, &workspace, CardId(1), "继续吗 y/N?\x07");

    let shown = wait_until(cx, handle, |cx| !cx.shown_system_notifications().is_empty());
    assert!(shown, "没有系统通知");
    let shown = cx.shown_system_notifications();
    assert_eq!(shown[0].title.as_ref(), "pwsh：响铃");
    assert_eq!(shown[0].body.as_ref(), "继续吗 y/N?");
}

/// 卡片关了，它的通知限频记录跟着丢掉：关卡片和关整个标签都一样。
#[gpui_kit::test]
fn closing_a_card_forgets_when_it_last_notified(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_running(cx, FakeTerminalFactory::default());
    activate_window(cx, handle);
    dispatch(cx, handle, qishell::app::SplitRight);
    assert!(wait_until(cx, handle, |cx| cx.update(|cx| running(
        &workspace,
        CardId(2),
        cx
    ))));
    dispatch(cx, handle, NewLocalTerminalTab);
    for card in [1, 2] {
        print_into(cx, &workspace, CardId(card), "\x1b]9;完成\x07");
    }
    assert!(wait_until(cx, handle, |cx| cx.update(|cx| {
        workspace.read(cx).notified_cards() == [CardId(1), CardId(2)]
    })));

    dispatch(cx, handle, CloseCard(CardId(2)));
    cx.update(|cx| assert_eq!(workspace.read(cx).notified_cards(), [CardId(1)]));
    dispatch(cx, handle, qishell::app::CloseTab(TabId(1)));
    cx.update(|cx| assert!(workspace.read(cx).notified_cards().is_empty()));
}
