//! 命令片段（工单 19）的 UI 测试：右侧栏的面板、搜索、单击输入或执行、悬停的 ⚡ / ▶、右键菜单、
//! 片段对话框，以及发不出去时（全屏程序、没有终端）面板说为什么。本地终端一律用假传输。
// 用例参考 since2006/shell-rs @ b2a307a: tests/ui/snippets.rs，按 MicaTerm 的元素 id 改写（没有分类）。

use std::time::Duration;

use gpui_kit::{ClipboardItem, Window, point};
use micaterm::app::{EditSnippet, ShowSnippets};
use micaterm::host::{SnippetDraft, SnippetId};
use micaterm::workspace::RightPanel;
use micaterm_terminal::TerminalLifecycle;

use crate::support::*;

/// 「查看磁盘」点击就执行；「Docker 容器」「重启 nginx」点击只输入。建的顺序和列表的顺序不一样。
fn hosts_with_snippets() -> HostData {
    let mut data = HostData::empty();
    data.insert_snippet_unnotified(SnippetDraft::new(
        "重启 nginx",
        "sudo systemctl restart nginx",
    ));
    data.insert_snippet_unnotified(SnippetDraft::new("查看磁盘", "df -h").with_run_on_click(true));
    data.insert_snippet_unnotified(SnippetDraft::new("Docker 容器", "docker ps"));
    data
}

const NGINX: u64 = 1;
const DISK: u64 = 2;
const DOCKER: u64 = 3;

/// Windows 上的 shell 不认 Ctrl-E Ctrl-U，片段接着已经打的字往后输入；别的系统先清掉那一行。
const CLEAR_LINE: &str = if cfg!(windows) { "" } else { "\x05\x15" };

/// 开一个本地终端标签（shell 是 pwsh），等假 shell 打出提示符，再展开命令片段面板。
fn open_snippets(
    cx: &mut TestAppContext,
    hosts: HostData,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeTerminalFactory>,
) {
    let factory = Arc::new(FakeTerminalFactory::default());
    let provider = Arc::new(
        FixedLocalTerminalTransportProvider::new(factory.clone())
            .with_shell_info(LocalShellInfo::new("pwsh", Some("PowerShell 7.5.2"))),
    );
    let mut services = services_with_terminals(provider);
    services.hosts = hosts;
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    dispatch(cx, handle, NewLocalTerminalTab);
    let ready = wait_until(cx, handle, |cx| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .card_screen_text(CardId(1), cx)
                .is_some_and(|text| text.contains("会议纪要"))
        }) && factory.last_size().is_some()
            // 终端要到 `Running` 才收输入，之前发的字丢掉；屏幕上有提示符还不算（见 programs.rs）。
            && cx.update(|cx| {
                workspace.read(cx).card(CardId(1), cx).is_some_and(|card| {
                    card.read(cx).terminal().read(cx).lifecycle(cx) == TerminalLifecycle::Running
                })
            })
    });
    assert!(ready, "假 shell 没有打出提示符");
    in_frame(cx, handle, |window, cx| {
        window.click("right-sidebar-snippets", cx)
    });
    (handle, workspace, factory)
}

/// 按真实时间等 `done` 成立，最多两秒：假 shell 跑在自己的线程上。
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

fn listed(window: &mut Window, snippet: u64) -> bool {
    window.try_find(("snippet", snippet)).is_some()
}

fn snippet_names(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .snippets_by_name()
            .iter()
            .map(|snippet| snippet.name.to_string())
            .collect()
    })
}

fn terminal_focused(window: &mut Window) -> Option<bool> {
    window.find(("terminal", 1usize)).focused()
}

#[gpui_kit::test]
fn the_line_left_of_the_panel_drags_its_width_and_the_width_survives_closing(
    cx: &mut TestAppContext,
) {
    let (handle, _, _) = open_snippets(cx, hosts_with_snippets());
    let width = |cx: &mut TestAppContext| {
        in_frame(cx, handle, |window, _| {
            window.find("snippets").bounds().size.width
        })
    };
    let drag_by = |cx: &mut TestAppContext, dx: f32| {
        in_frame(cx, handle, |window, cx| {
            // 从面板的左边线上按下（在柄的命中区里）：库按鼠标位置定内容区的右边。
            let handle = window.find("right-panel-handle").bounds();
            let from = point(window.find("snippets").bounds().left(), handle.center().y);
            window.drag(from, point(from.x + px(dx), from.y), cx);
        });
        cx.run_until_parked();
    };
    assert_eq!(width(cx), px(380.));

    // 往左拖 60：面板变宽 60，卡片让出来。
    drag_by(cx, -60.);
    assert_eq!(width(cx), px(440.));
    in_frame(cx, handle, |window, _| {
        let panel = window.find("snippets").bounds();
        assert!(window.find(("card", 1usize)).bounds().right() <= panel.left());
    });

    // 收起再展开：还是拖到的宽度。
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-s", cx));
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-s", cx));
    assert_eq!(width(cx), px(440.));

    // 往右拖过头：停在下限 200。
    drag_by(cx, 400.);
    assert_eq!(width(cx), px(200.));
}

#[gpui_kit::test]
fn the_panel_opens_beside_the_focused_card_and_says_where_snippets_go(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_snippets(cx, hosts_with_snippets());
    assert_eq!(
        cx.update(|cx| workspace.read(cx).right_panel()),
        Some(RightPanel::Snippets)
    );
    in_frame(cx, handle, |window, _| {
        let panel = window.find("snippets").bounds();
        // 右侧面板宽 380，在卡片右边。
        assert_eq!(panel.size.width, px(380.));
        assert!(window.find(("card", 1usize)).bounds().right() <= panel.left());
        assert_eq!(window.find("snippets-target").label(), Some("→ pwsh"));
    });

    // Ctrl+Shift+S 收起，再按一次展开。
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-s", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("snippets").is_none());
    });
    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-s", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.find("snippets").visible());
    });
}

#[gpui_kit::test]
fn the_headers_collapse_button_closes_the_panel(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_snippets(cx, hosts_with_snippets());
    in_frame(cx, handle, |window, cx| window.click("snippets-close", cx));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("snippets").is_none());
    });
    assert_eq!(cx.update(|cx| workspace.read(cx).right_panel()), None);
}

#[gpui_kit::test]
fn snippets_are_listed_by_name(cx: &mut TestAppContext) {
    let (handle, _, _) = open_snippets(cx, hosts_with_snippets());
    in_frame(cx, handle, |window, _| {
        // 按名称排，不分大小写：Docker 容器、查看磁盘、重启 nginx；不设搜索框，圆角行间距 2px。
        assert!(window.try_find("snippets-search").is_none());
        let docker = window.find(("snippet", DOCKER)).bounds();
        let disk = window.find(("snippet", DISK)).bounds();
        let nginx = window.find(("snippet", NGINX)).bounds();
        assert_eq!(disk.top() - docker.bottom(), px(2.));
        assert_eq!(nginx.top() - disk.bottom(), px(2.));
        assert_eq!(
            window.find(("snippet", DOCKER)).label(),
            Some("Docker 容器 · docker ps")
        );
    });
}

#[gpui_kit::test]
fn a_click_types_the_snippet_without_running(cx: &mut TestAppContext) {
    let (handle, _, factory) = open_snippets(cx, hosts_with_snippets());
    factory.clear_writes();

    // 没有尾部执行按钮；所有片段（包括旧库里曾带 run_on_click 的记录）单击一律只上屏、不带回车执行。
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find(("snippet-run", DOCKER)).is_none());
        assert!(window.try_find(("snippet-run", DISK)).is_none());
        window.click(("snippet", DOCKER), cx);
    });
    take_written(cx, handle, &factory, &format!("{CLEAR_LINE}docker ps"));
    in_frame(cx, handle, |window, _| {
        assert_eq!(terminal_focused(window), Some(true));
    });

    in_frame(cx, handle, |window, cx| window.click(("snippet", DISK), cx));
    take_written(cx, handle, &factory, &format!("{CLEAR_LINE}df -h"));
    assert_nothing_written(cx, handle, &factory);
}

#[gpui_kit::test]
fn a_new_snippet_is_made_in_the_dialog(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_snippets(cx, HostData::empty());
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("snippets-empty").label(),
            Some("No snippets yet")
        );
        window.click("snippets-new", cx);
    });

    // 名称马上能打字，没有「自动执行」勾选框；没写命令就不让存，说清楚缺什么。
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("snippet-run-on-click").is_none());
        assert_eq!(window.find("snippet-name").focused(), Some(true));
        window.input("查看端口", cx);
        window.click("snippet-save", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("snippet-form-error").label(),
            Some("Enter a command")
        );
    });
    assert!(snippet_names(&workspace, cx).is_empty());

    in_frame(cx, handle, |window, cx| {
        window.click("snippet-command", cx);
        window.input("ss -tlnp", cx);
        window.click("snippet-save", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("snippet-name").is_none(), "对话框关了");
        assert_eq!(
            window.find(("snippet", 1u64)).label(),
            Some("查看端口 · ss -tlnp")
        );
    });
    let snippet = cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .snippet(SnippetId(1))
            .cloned()
            .unwrap()
    });
    assert_eq!(snippet.command, "ss -tlnp");
    assert!(!snippet.run_on_click);
    // 存的时候不往终端里发东西；单击只上屏不执行。
    factory.clear_writes();
    in_frame(cx, handle, |window, cx| window.click(("snippet", 1u64), cx));
    take_written(cx, handle, &factory, &format!("{CLEAR_LINE}ss -tlnp"));
}

#[gpui_kit::test]
fn a_snippet_is_edited_in_the_same_dialog(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_snippets(cx, hosts_with_snippets());
    dispatch(cx, handle, EditSnippet(SnippetId(DISK)));
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("snippet-run-on-click").is_none());
        assert_eq!(window.find("snippet-name").focused(), Some(true));
        window.press("ctrl-a", cx);
        window.input("磁盘", cx);
        window.click("snippet-save", cx);
    });
    let snippet = cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .snippet(SnippetId(DISK))
            .cloned()
            .unwrap()
    });
    assert_eq!(snippet.name, "磁盘");
    assert_eq!(snippet.command, "df -h");
    assert!(!snippet.run_on_click);
}

/// 右键菜单：输入到终端 / 复制命令 / 编辑… / 删除。
#[gpui_kit::test]
fn the_menu_types_copies_edits_and_deletes(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_snippets(cx, hosts_with_snippets());
    factory.clear_writes();
    let pick = |cx: &mut TestAppContext, item: usize| {
        in_frame(cx, handle, |window, cx| {
            window.right_click(("snippet", DISK), cx)
        });
        in_frame(cx, handle, |window, cx| {
            let mut popup = window.within("popup-menu");
            // 第 2 项是分隔线。
            let labels: Vec<_> = (0..5_usize)
                .map(|item| popup.find(item).label().map(str::to_owned))
                .collect();
            assert_eq!(
                labels,
                [
                    Some("Type into Terminal"),
                    Some("Copy Command"),
                    None,
                    Some("Edit…"),
                    Some("Delete")
                ]
                .map(|label| label.map(str::to_owned))
            );
            assert!(popup.try_find(5_usize).is_none(), "only four items");
            popup.click(item, cx);
        });
    };

    // 「输入到终端」只输入，即使片段是点击就执行的。
    pick(cx, 0);
    take_written(cx, handle, &factory, &format!("{CLEAR_LINE}df -h"));
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("popup-menu").is_none());
        assert_eq!(terminal_focused(window), Some(true));
    });

    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("旧的".into())));
    pick(cx, 1);
    assert_eq!(
        cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("df -h".to_owned())
    );

    pick(cx, 3);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("snippet-name").focused(), Some(true));
    });
    in_frame(cx, handle, |window, cx| window.press("escape", cx));

    // 删除要确认。
    pick(cx, 4);
    assert_eq!(snippet_names(&workspace, cx).len(), 3);
    in_frame(cx, handle, |window, cx| {
        window.click("snippet-delete-confirm", cx)
    });
    assert_eq!(snippet_names(&workspace, cx), ["Docker 容器", "重启 nginx"]);
    in_frame(cx, handle, |window, _| {
        assert!(!listed(window, DISK));
    });
}

#[gpui_kit::test]
fn a_full_screen_program_keeps_the_snippet_out_and_the_panel_says_why(cx: &mut TestAppContext) {
    let (handle, workspace, factory) = open_snippets(cx, hosts_with_snippets());
    // 假 shell 进了备用屏幕，像 vim、htop 那样。
    factory.print(1, "\x1b[?1049h");
    let full_screen = wait_until(cx, handle, |cx| {
        cx.update(|cx| {
            let card = workspace.read(cx).card(CardId(1), cx).unwrap();
            card.read(cx).terminal().read(cx).is_full_screen(cx)
        })
    });
    assert!(full_screen);
    factory.clear_writes();

    in_frame(cx, handle, |window, cx| window.click(("snippet", DISK), cx));
    assert_nothing_written(cx, handle, &factory);
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("snippets-notice").label(),
            Some("pwsh is running a full-screen program and did not receive \"查看磁盘\"")
        );
        // 点一下收起。
        window.click("snippets-notice", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("snippets-notice").is_none());
    });

    // 退出全屏程序以后照常发（仅上屏不执行）。
    factory.print(1, "\x1b[?1049l");
    let back = wait_until(cx, handle, |cx| {
        cx.update(|cx| {
            let card = workspace.read(cx).card(CardId(1), cx).unwrap();
            !card.read(cx).terminal().read(cx).is_full_screen(cx)
        })
    });
    assert!(back);
    factory.clear_writes();
    in_frame(cx, handle, |window, cx| window.click(("snippet", DISK), cx));
    take_written(cx, handle, &factory, &format!("{CLEAR_LINE}df -h"));
}

/// 多行命令在不支持括号粘贴的 shell 里只能直接执行；只输入时不发，面板说为什么。
#[gpui_kit::test]
fn a_command_of_several_lines_is_only_typed_where_the_shell_can_take_it(cx: &mut TestAppContext) {
    let mut hosts = HostData::empty();
    hosts.insert_snippet_unnotified(SnippetDraft::new("更新", "cd /srv\ngit pull"));
    let (handle, _, factory) = open_snippets(cx, hosts);
    factory.clear_writes();

    in_frame(cx, handle, |window, cx| window.click(("snippet", 1u64), cx));
    assert_nothing_written(cx, handle, &factory);
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("snippets-notice").label(),
            Some("pwsh did not receive \"更新\": This is a multi-line command and can only be run directly in this terminal")
        );
    });

    // shell 打开括号粘贴以后，整段原样放到提示符后面，等用户回车。
    factory.print(1, "\x1b[?2004h");
    let started = std::time::Instant::now();
    wait_until(cx, handle, |_| {
        started.elapsed() > Duration::from_millis(150)
    });
    factory.clear_writes();
    in_frame(cx, handle, |window, cx| window.click(("snippet", 1u64), cx));
    take_written(
        cx,
        handle,
        &factory,
        &format!("{CLEAR_LINE}\x1b[200~cd /srv\ngit pull\x1b[201~"),
    );
    in_frame(cx, handle, |window, _| {
        assert!(window.try_find("snippets-notice").is_none());
    });
}

/// 悬停看一整条长命令时，提示条限宽换行，不会撑成横贯窗口的一条；命令仍是完整的。
#[gpui_kit::test]
fn a_long_command_gets_a_wrapped_bounded_tooltip(cx: &mut TestAppContext) {
    // 像重装脚本那样：两行命令，第二行带着一整段没有空格的 ssh 公钥。
    let command = format!(
        "curl -O https://raw.githubusercontent.com/bin456789/reinstall/main/reinstall.sh || wget -O reinstall.sh $_\nbash reinstall.sh debian 13 --ssh-key \"ssh-rsa {}\"",
        "A".repeat(600)
    );
    let mut hosts = HostData::empty();
    hosts.insert_snippet_unnotified(SnippetDraft::new("重装系统", command.clone()));
    let (handle, _, _) = open_snippets(cx, hosts);

    in_frame(cx, handle, |window, cx| window.hover(("snippet", 1u64), cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    in_frame(cx, handle, |window, _| {
        let tip = window.find("icon-button-tooltip");
        // 悬停的仍是完整命令（只是显示上限宽四行）。
        assert_eq!(tip.label(), Some(command.as_str()));
        // 320 的内容宽 + 左右各 8 的内边距。
        let bounds = tip.bounds();
        let (width, height) = (bounds.size.width, bounds.size.height);
        assert!(width <= px(336.), "长命令把提示条撑到了 {width:?}");
        assert!(width >= px(200.), "提示条缩成了一列竖排字：{width:?} × {height:?}");
        assert!(height <= px(120.), "提示条没有限行：{width:?} × {height:?}");
    });
}

/// 没有终端标签时面板照常列出和新建片段；点片段发送时在底部提示「当前标签没有终端」。
#[gpui_kit::test]
fn without_a_terminal_tab_the_panel_says_there_is_no_terminal(cx: &mut TestAppContext) {
    let mut services = fake_services();
    services.hosts = hosts_with_snippets();
    let (handle, _) = open_workspace_with(cx, settings_store(), services);
    dispatch(cx, handle, ShowSnippets);
    in_frame(cx, handle, |window, cx| {
        assert!(window.try_find("snippets-target").is_none());
        assert!(listed(window, DOCKER));
        assert!(listed(window, DISK));
        assert!(listed(window, NGINX));
        window.click(("snippet", DISK), cx);
    });
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("snippets-notice").label(),
            Some("The current tab has no terminal")
        );
        // 没有终端标签时新建保存，也立刻显示在列表里。
        window.click("snippets-new", cx);
    });
    in_frame(cx, handle, |window, cx| {
        window.input("查看内存", cx);
        window.click("snippet-command", cx);
        window.input("free -h", cx);
        window.click("snippet-save", cx);
    });
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("snippet", 4u64)).label(),
            Some("查看内存 · free -h")
        );
    });
}
