//! 编辑器标签（工单 23）：从 SFTP 标签打开文件、原地写回、被别人改过要问、重新加载、关闭确认、
//! 打不开的文件，和关窗口、退出时合成一个框的询问。连接和本地目录都是假的。
// 参考 since2006/shell-rs @ b2a307a: tests/ui/editor.rs

use std::time::Duration;

use qishell::app::{OpenHostSftp, Quit, SftpAction, SftpTabCommand};
use qishell::editor::EditorTab;
use qishell::sftp_tab::{SftpTab, TransferJob};

use crate::support::*;

const REMOTE_FILE: &str = "/home/tester/文件 甲.txt";
const OTHER_REMOTE_FILE: &str = "/home/tester/文件 乙.txt";
const LOCAL_FILE: &str = "/local/tester/Downloads/文件 甲.txt";

struct Fixture {
    handle: WindowHandle<Root>,
    workspace: Entity<Workspace>,
    sftp: Arc<FakeSftpProvider>,
    local: FakeLocalDirectory,
}

/// 工作线程在真线程上：按真实时间等，同时拨测试时钟（标签每 16ms 收一次事件）。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// 服务器上有这些文件，打开主机 1 的 SFTP 标签，等两侧读完。
fn open_server(cx: &mut TestAppContext, files: &[(&str, &[u8])]) -> Fixture {
    let sftp = Arc::new(FakeSftpProvider::with_files(files));
    let local = FakeLocalDirectory::default();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        sftp: sftp.clone(),
        local_files: Arc::new(local.clone()),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    dispatch(cx, handle, OpenHostSftp(HostId(1)));
    let shown = |cx: &mut TestAppContext, id: &'static str| {
        in_frame(cx, handle, move |window, _| {
            window
                .try_find(id)
                .and_then(|element| element.value().map(str::to_owned))
        })
    };
    assert!(
        wait_until(cx, handle, |cx| {
            shown(cx, "remote-path").as_deref() == Some("/home/tester")
                && shown(cx, "local-path").as_deref() == Some(LOCAL_DOWNLOADS)
        }),
        "两侧没有读完"
    );
    Fixture {
        handle,
        workspace,
        sftp,
        local,
    }
}

fn sftp_tab(cx: &mut TestAppContext, fixture: &Fixture) -> Entity<SftpTab> {
    cx.update(|cx| {
        let workspace = fixture.workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .find_map(|tab| workspace.sftp_tab(tab))
            .expect("an SFTP tab")
    })
}

fn sftp_command(cx: &mut TestAppContext, fixture: &Fixture, command: SftpTabCommand) {
    let tab = sftp_tab(cx, fixture);
    let id = cx.update(|cx| tab.read(cx).id());
    dispatch(cx, fixture.handle, SftpAction::new(id, command));
}

fn editor(cx: &mut TestAppContext, fixture: &Fixture) -> Option<Entity<EditorTab>> {
    cx.update(|cx| fixture.workspace.read(cx).active_editor_tab())
}

/// 打开文件，等编辑器标签到前面来。
fn open_file(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, path: &str) {
    sftp_command(
        cx,
        fixture,
        SftpTabCommand::Edit {
            remote,
            path: Some(path.to_string()),
        },
    );
    let expected = path.rsplit(['/', '\\']).next().unwrap().to_string();
    let fixture_workspace = fixture.workspace.clone();
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            cx.update(|cx| {
                fixture_workspace
                    .read(cx)
                    .active_editor_tab()
                    .is_some_and(|editor| editor.read(cx).file_name() == expected.as_str())
            })
        }),
        "编辑器标签没有打开"
    );
}

fn text_of(cx: &mut TestAppContext, fixture: &Fixture) -> String {
    let editor = editor(cx, fixture).expect("an editor in front");
    cx.update(|cx| editor.read(cx).text(cx))
}

fn unsaved(cx: &mut TestAppContext, fixture: &Fixture) -> bool {
    let editor = editor(cx, fixture).expect("an editor in front");
    cx.update(|cx| editor.read(cx).has_unsaved_changes())
}

/// 在光标处打字：编辑器打开时就有焦点。
fn type_text(cx: &mut TestAppContext, fixture: &Fixture, text: &str) {
    in_frame(cx, fixture.handle, |window, cx| window.input(text, cx));
}

fn press(cx: &mut TestAppContext, fixture: &Fixture, key: &str) {
    in_frame(cx, fixture.handle, |window, cx| window.press(key, cx));
}

fn click(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) {
    in_frame(cx, fixture.handle, |window, cx| window.click(id, cx));
}

fn visible(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> bool {
    in_frame(cx, fixture.handle, move |window, _| {
        window.try_find(id).is_some()
    })
}

fn label(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> String {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .find(id)
            .label()
            .map(str::to_owned)
            .unwrap_or_default()
    })
}

fn wait_visible(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) {
    assert!(
        wait_until(cx, fixture.handle, |cx| visible(cx, fixture, id)),
        "等不到 {id}"
    );
}

fn tab_count(cx: &mut TestAppContext, fixture: &Fixture) -> usize {
    cx.update(|cx| fixture.workspace.read(cx).tab_ids().len())
}

/// 等服务器收到第 `count` 次写，标签也收到了回答。
fn wait_for_writes(cx: &mut TestAppContext, fixture: &Fixture, count: usize) {
    let writes = fixture.sftp.writes.clone();
    assert!(
        wait_until(cx, fixture.handle, |_| writes.lock().unwrap().len()
            >= count),
        "服务器没收到第 {count} 次写"
    );
    for _ in 0..5 {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, fixture.handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 在前的标签的状态点。
fn active_dot(cx: &mut TestAppContext, fixture: &Fixture) -> Option<StatusDot> {
    cx.update(|cx| {
        let workspace = fixture.workspace.read(cx);
        workspace
            .active_tab()
            .and_then(|tab| workspace.tab_dot(tab, cx))
    })
}

// ---------- 打开 ----------

#[gpui_kit::test]
fn opening_a_remote_file_shows_it_and_opening_it_again_switches_to_it(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"hello world\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);

    assert_eq!(text_of(cx, &fixture), "hello world\n");
    assert_eq!(tab_count(cx, &fixture), 2);
    assert_eq!(active_dot(cx, &fixture), Some(StatusDot::None));
    assert_eq!(
        label(cx, &fixture, "editor-header"),
        "文件 甲.txt web-01:/home/tester/"
    );
    assert_eq!(
        label(cx, &fixture, "editor-status"),
        "经 web-01 的 SFTP 连接 · root@10.0.1.12 · 第 1 行，第 1 列 · UTF-8 · LF · 纯文本"
    );

    // 再打开同一个：切过去，不再读一遍，不多开标签。
    let sftp = sftp_tab(cx, &fixture);
    let id = cx.update(|cx| sftp.read(cx).id());
    dispatch(
        cx,
        fixture.handle,
        qishell::app::NextTab, // 切回 SFTP 标签
    );
    assert!(editor(cx, &fixture).is_none());
    dispatch(
        cx,
        fixture.handle,
        SftpAction::new(
            id,
            SftpTabCommand::Edit {
                remote: true,
                path: Some(REMOTE_FILE.to_string()),
            },
        ),
    );
    assert!(editor(cx, &fixture).is_some());
    assert_eq!(tab_count(cx, &fixture), 2);
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), 1);
}

#[gpui_kit::test]
fn f4_enter_and_a_double_click_open_the_file_under_the_cursor(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    let name = gpui_kit::ElementId::Name("name:文件 甲.txt".into());
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").double_click(name.clone(), cx)
    });
    let workspace = fixture.workspace.clone();
    let opened =
        |cx: &mut TestAppContext| cx.update(|cx| workspace.read(cx).active_editor_tab().is_some());
    assert!(wait_until(cx, fixture.handle, opened), "双击没有打开");
    let tab = cx.update(|cx| fixture.workspace.read(cx).active_tab().unwrap());

    // F4、回车：光标在这个文件上，切回同一个标签。
    for key in ["f4", "enter"] {
        dispatch(cx, fixture.handle, qishell::app::NextTab);
        assert!(editor(cx, &fixture).is_none());
        in_frame(cx, fixture.handle, |window, cx| {
            window.within("remote-card").click(name.clone(), cx);
            window.press(key, cx);
        });
        assert!(wait_until(cx, fixture.handle, opened), "{key} 没有打开");
        assert_eq!(
            cx.update(|cx| fixture.workspace.read(cx).active_tab()),
            Some(tab)
        );
    }
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), 1);
}

/// 右键菜单的「编辑」和 F4 是同一个 action，菜单对着的是选中的文件：光标不在它上面也开它。
#[gpui_kit::test]
fn the_one_selected_file_opens_even_with_the_cursor_elsewhere(cx: &mut TestAppContext) {
    let fixture = open_server(
        cx,
        &[
            (REMOTE_FILE, b"selected\n"),
            (OTHER_REMOTE_FILE, b"cursor\n"),
        ],
    );
    let name = gpui_kit::ElementId::Name("name:文件 甲.txt".into());
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").click(name, cx)
    });
    // 甲、乙都选上，光标到乙，再取消乙：只选中甲，光标在乙。
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::MoveCursor {
            remote: true,
            motion: qishell::sftp_tab::CursorMotion::Up,
            extend: true,
        },
    );
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::ToggleSelection { remote: true },
    );

    dispatch(
        cx,
        fixture.handle,
        qishell::app::SftpShortcut(SftpTabCommand::Edit {
            remote: true,
            path: None,
        }),
    );
    let workspace = fixture.workspace.clone();
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| workspace.read(cx).active_editor_tab().is_some())
    }));
    assert_eq!(text_of(cx, &fixture), "selected\n");
}

/// 工单 21 的「新建文件」：建好了直接在编辑器里打开。
#[gpui_kit::test]
fn a_new_file_opens_in_the_editor(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[]);
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::New {
            remote: true,
            kind: qishell::sftp_tab::NewEntryKind::File,
        },
    );
    type_text(cx, &fixture, "a.txt");
    press(cx, &fixture, "enter");
    let workspace = fixture.workspace.clone();
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            cx.update(|cx| {
                workspace
                    .read(cx)
                    .active_editor_tab()
                    .is_some_and(|editor| editor.read(cx).file_name() == "a.txt")
            })
        }),
        "新建的文件没有打开"
    );
    assert_eq!(text_of(cx, &fixture), "");
    assert!(!unsaved(cx, &fixture));
}

#[gpui_kit::test]
fn the_editor_has_find_and_replace_on_its_own_keys(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"alpha beta\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    // 查找面板有切换替换的按钮；替换的按钮只在替换时有。
    assert!(!visible(cx, &fixture, "replace-mode"));
    press(cx, &fixture, "ctrl-f");
    assert!(visible(cx, &fixture, "replace-mode"));
    assert!(!visible(cx, &fixture, "replace-all"));
    // Esc 收起，焦点回到文字；从文字里按 Ctrl+H 直接是替换。
    press(cx, &fixture, "escape");
    assert!(!visible(cx, &fixture, "replace-mode"));
    press(cx, &fixture, "ctrl-h");
    assert!(visible(cx, &fixture, "replace-all"));
    assert_eq!(text_of(cx, &fixture), "alpha beta\n");
}

/// 深浅色下编辑器都用原型的颜色和随包的等宽字体：行号栏不能是默认主题的底色，和卡片不搭。
#[gpui_kit::test]
fn the_editor_takes_its_colors_and_font_from_the_prototype_in_both_modes(cx: &mut TestAppContext) {
    for appearance in [Appearance::Dark, Appearance::Light] {
        let mut store = settings_store();
        store.update_unnotified(|settings| settings.appearance = appearance);
        let (handle, _) = open_workspace_with(cx, store, fake_services());
        in_frame(cx, handle, |_, cx| {
            let tokens = Tokens::current(cx);
            let theme = cx.theme();
            assert_eq!(theme.mono_font_family.as_ref(), "JetBrains Mono");
            let style = &theme.highlight_theme.style;
            assert_eq!(style.editor_background, Some(tokens.card_focused));
            assert_eq!(style.editor_gutter_background, Some(tokens.card_focused));
            assert_eq!(style.editor_foreground, Some(tokens.terminal));
            assert_eq!(style.editor_active_line, Some(tokens.hover));
            assert_eq!(style.editor_line_number, Some(tokens.faint));
            assert_eq!(style.editor_active_line_number, Some(tokens.text));
            // 语法的颜色还在：不是整个换掉。
            assert!(style.syntax.keyword.is_some() || style.syntax.string.is_some());
        });
    }
}

/// 标签轨道上写「文件名 主机名」。
fn rail_label(cx: &mut TestAppContext, fixture: &Fixture) -> String {
    let tab = cx.update(|cx| fixture.workspace.read(cx).active_tab().unwrap());
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .find(("tab", tab.0 as usize))
            .label()
            .map(str::to_owned)
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn the_rail_and_the_status_bar_name_the_file_and_its_connection(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[("/home/tester/compose.yml", b"a: 1\n")]);
    open_file(cx, &fixture, true, "/home/tester/compose.yml");
    assert_eq!(rail_label(cx, &fixture), "compose.yml web-01");
    assert_eq!(
        label(cx, &fixture, "editor-status"),
        "经 web-01 的 SFTP 连接 · root@10.0.1.12 · 第 1 行，第 1 列 · UTF-8 · LF · YAML"
    );
}

#[gpui_kit::test]
fn the_status_bar_follows_the_cursor(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"one\ntwo\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    press(cx, &fixture, "down");
    press(cx, &fixture, "right");
    assert!(
        label(cx, &fixture, "editor-status").contains("第 2 行，第 2 列"),
        "{}",
        label(cx, &fixture, "editor-status")
    );
}

// ---------- 保存 ----------

#[gpui_kit::test]
fn ctrl_s_writes_the_file_back_and_clears_the_dot(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    assert!(!unsaved(cx, &fixture));

    type_text(cx, &fixture, "new ");
    assert!(unsaved(cx, &fixture));
    assert_eq!(active_dot(cx, &fixture), Some(StatusDot::Attention));

    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 1);

    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"new text\n");
    assert!(!unsaved(cx, &fixture));
    assert_eq!(active_dot(cx, &fixture), Some(StatusDot::None));
    // 保存前核对打开时的大小和修改时间。
    assert_eq!(
        fixture.sftp.writes.lock().unwrap()[0].2,
        Some(fake_stamp(b"text\n", 100))
    );
}

#[gpui_kit::test]
fn the_save_button_in_the_header_saves_too(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    click(cx, &fixture, "editor-save");
    wait_for_writes(cx, &fixture, 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"1text\n");
}

#[gpui_kit::test]
fn a_second_save_checks_against_the_first(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 1);
    type_text(cx, &fixture, "2");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 2);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"12text\n");
    assert!(!visible(cx, &fixture, "editor-changed"));
}

#[gpui_kit::test]
fn crlf_and_a_byte_order_mark_are_written_back_as_they_were(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"\xEF\xBB\xBFa\r\nb\r\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    assert_eq!(text_of(cx, &fixture), "a\nb\n");
    assert!(label(cx, &fixture, "editor-status").contains("UTF-8 BOM · CRLF"));

    type_text(cx, &fixture, "x");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"\xEF\xBB\xBFxa\r\nb\r\n");
}

#[gpui_kit::test]
fn mixed_line_ends_are_left_as_they_are(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"a\r\nb\nc")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "x");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"xa\r\nb\nc");
}

// ---------- 被别人改过 ----------

#[gpui_kit::test]
fn a_file_changed_on_the_server_asks_and_shows_both_stamps(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"mine\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    fixture.sftp.change_file(REMOTE_FILE, b"someone else's\n");

    type_text(cx, &fixture, "1");
    press(cx, &fixture, "ctrl-s");
    wait_visible(cx, &fixture, "editor-changed");

    // 什么都没写。打开时和现在都写出来。
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"someone else's\n");
    let text = label(cx, &fixture, "editor-changed-text");
    assert!(
        text.contains("打开之后，远程的这个文件被改过"),
        "{text}"
    );
    assert!(text.contains("打开时 5 B"), "{text}");
    assert!(text.contains("现在 15 B"), "{text}");
    assert!(text.contains("覆盖会丢掉别人的修改"), "{text}");

    // 取消：修改还在，还是未保存。
    click(cx, &fixture, "editor-changed-cancel");
    assert!(!visible(cx, &fixture, "editor-changed"));
    assert!(unsaved(cx, &fixture));
    assert_eq!(text_of(cx, &fixture), "1mine\n");

    // 覆盖：不再核对。
    press(cx, &fixture, "ctrl-s");
    wait_visible(cx, &fixture, "editor-changed");
    click(cx, &fixture, "editor-changed-overwrite");
    wait_for_writes(cx, &fixture, 3);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"1mine\n");
    assert_eq!(fixture.sftp.writes.lock().unwrap()[2].2, None);
    assert!(!unsaved(cx, &fixture));
}

#[gpui_kit::test]
fn the_conflict_can_reload_what_is_on_the_server(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"mine\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    fixture.sftp.change_file(REMOTE_FILE, b"theirs\n");
    type_text(cx, &fixture, "1");
    press(cx, &fixture, "ctrl-s");
    wait_visible(cx, &fixture, "editor-changed");

    click(cx, &fixture, "editor-changed-reload");
    let workspace = fixture.workspace.clone();
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .active_editor_tab()
                .is_some_and(|editor| editor.read(cx).text(cx) == "theirs\n")
        })
    }));
    assert!(!unsaved(cx, &fixture));
    // 之后按新读到的核对，不再问。
    type_text(cx, &fixture, "2");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 2);
    assert!(!visible(cx, &fixture, "editor-changed"));
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"2theirs\n");
}

// ---------- 重新加载 ----------

#[gpui_kit::test]
fn reloading_reads_the_file_again_and_asks_first_when_there_are_changes(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"one\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);

    // 没有修改：直接读。
    fixture.sftp.change_file(REMOTE_FILE, b"two\n");
    click(cx, &fixture, "editor-reload");
    let workspace = fixture.workspace.clone();
    let shows = |cx: &mut TestAppContext, text: &'static str| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .active_editor_tab()
                .is_some_and(|editor| editor.read(cx).text(cx) == text)
        })
    };
    assert!(wait_until(cx, fixture.handle, |cx| shows(cx, "two\n")));
    assert!(!visible(cx, &fixture, "editor-reload-text"));

    // 有修改：先问，取消就什么都不变。
    type_text(cx, &fixture, "x");
    click(cx, &fixture, "editor-reload");
    assert!(visible(cx, &fixture, "editor-reload-text"));
    click(cx, &fixture, "editor-reload-cancel");
    assert_eq!(text_of(cx, &fixture), "xtwo\n");
    assert!(unsaved(cx, &fixture));

    // 确认：丢掉修改。
    fixture.sftp.change_file(REMOTE_FILE, b"three\n");
    click(cx, &fixture, "editor-reload");
    click(cx, &fixture, "editor-reload-confirm");
    assert!(wait_until(cx, fixture.handle, |cx| shows(cx, "three\n")));
    assert!(!unsaved(cx, &fixture));
}

// ---------- 关闭 ----------

#[gpui_kit::test]
fn closing_an_unchanged_editor_does_not_ask(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    dispatch(cx, fixture.handle, CloseFocusedCard);
    assert!(!visible(cx, &fixture, "editor-close"));
    assert_eq!(tab_count(cx, &fixture), 1);
}

#[gpui_kit::test]
fn closing_an_edited_file_asks_whether_to_save_it(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");

    // 取消：标签和修改都在。
    dispatch(cx, fixture.handle, CloseFocusedCard);
    assert!(visible(cx, &fixture, "editor-close"));
    click(cx, &fixture, "editor-close-cancel");
    assert_eq!(tab_count(cx, &fixture), 2);
    assert!(unsaved(cx, &fixture));

    // 保存：存好了才关。
    dispatch(cx, fixture.handle, CloseFocusedCard);
    click(cx, &fixture, "editor-close-save");
    wait_for_writes(cx, &fixture, 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"1text\n");
    assert_eq!(tab_count(cx, &fixture), 1);
}

#[gpui_kit::test]
fn closing_without_saving_throws_the_changes_away(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    dispatch(cx, fixture.handle, CloseFocusedCard);
    click(cx, &fixture, "editor-close-discard");
    assert_eq!(tab_count(cx, &fixture), 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"text\n");
    assert!(fixture.sftp.writes.lock().unwrap().is_empty());
}

#[gpui_kit::test]
fn a_save_that_is_refused_keeps_the_tab_open_when_closing(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"mine\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    fixture.sftp.change_file(REMOTE_FILE, b"theirs\n");
    type_text(cx, &fixture, "1");

    dispatch(cx, fixture.handle, CloseFocusedCard);
    click(cx, &fixture, "editor-close-save");
    wait_visible(cx, &fixture, "editor-changed");
    assert_eq!(tab_count(cx, &fixture), 2);

    // 覆盖以后才关。
    click(cx, &fixture, "editor-changed-overwrite");
    wait_for_writes(cx, &fixture, 2);
    assert_eq!(tab_count(cx, &fixture), 1);
    assert_eq!(fixture.sftp.file(REMOTE_FILE), b"1mine\n");
}

#[gpui_kit::test]
fn closing_the_sftp_tab_asks_once_and_takes_its_files_along(cx: &mut TestAppContext) {
    let fixture = open_server(
        cx,
        &[(REMOTE_FILE, b"one\n"), (OTHER_REMOTE_FILE, b"two\n")],
    );
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    open_file(cx, &fixture, true, OTHER_REMOTE_FILE);
    type_text(cx, &fixture, "2");
    assert_eq!(tab_count(cx, &fixture), 3);

    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    let text = label(cx, &fixture, "close-tabs-text");
    assert!(text.contains("文件 甲.txt"), "{text}");
    assert!(text.contains("文件 乙.txt"), "{text}");

    click(cx, &fixture, "close-tabs-cancel");
    assert_eq!(tab_count(cx, &fixture), 3);

    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    click(cx, &fixture, "close-tabs-confirm");
    assert_eq!(tab_count(cx, &fixture), 0);
}

#[gpui_kit::test]
fn closing_all_other_tabs_asks_once_for_every_unsaved_file(cx: &mut TestAppContext) {
    let fixture = open_server(
        cx,
        &[(REMOTE_FILE, b"one\n"), (OTHER_REMOTE_FILE, b"two\n")],
    );
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    open_file(cx, &fixture, true, OTHER_REMOTE_FILE);
    type_text(cx, &fixture, "2");
    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    // 关掉两个有修改的编辑器：合成一个框，不是一个一个问。
    dispatch(cx, fixture.handle, qishell::app::CloseOtherTabs(sftp));
    assert!(!visible(cx, &fixture, "editor-close"));
    let text = label(cx, &fixture, "close-tabs-text");
    assert!(text.contains("文件 甲.txt"), "{text}");
    assert!(text.contains("文件 乙.txt"), "{text}");
    click(cx, &fixture, "close-tabs-confirm");
    assert_eq!(tab_count(cx, &fixture), 1);
    assert!(fixture.sftp.writes.lock().unwrap().is_empty());
}

// ---------- 打不开的文件 ----------

#[gpui_kit::test]
fn a_binary_file_is_explained_not_opened_and_offers_the_download(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"PK\x03\x04\x00\x00")]);
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::Edit {
            remote: true,
            path: Some(REMOTE_FILE.to_string()),
        },
    );
    wait_visible(cx, &fixture, "editor-refused");
    assert_eq!(tab_count(cx, &fixture), 1);
    let text = label(cx, &fixture, "editor-refused-text");
    assert!(text.contains("不是 UTF-8 文本文件"), "{text}");
    assert!(text.contains("可以先下载到本机"), "{text}");
    assert!(visible(cx, &fixture, "editor-refused-download"));
    click(cx, &fixture, "editor-refused-close");
    assert!(!visible(cx, &fixture, "editor-refused"));
    assert_eq!(tab_count(cx, &fixture), 1);
}

#[gpui_kit::test]
fn a_file_over_the_limit_says_how_large_it_is(cx: &mut TestAppContext) {
    let big = vec![b'a'; qishell::backend::sftp::EDIT_LIMIT as usize + 1];
    let fixture = open_server(cx, &[(REMOTE_FILE, big.as_slice())]);
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::Edit {
            remote: true,
            path: Some(REMOTE_FILE.to_string()),
        },
    );
    wait_visible(cx, &fixture, "editor-refused");
    let text = label(cx, &fixture, "editor-refused-text");
    assert!(
        text.contains("编辑器只打开 5.0 MB 以内的 UTF-8 文本文件"),
        "{text}"
    );
    assert_eq!(tab_count(cx, &fixture), 1);
}

#[gpui_kit::test]
fn a_local_file_that_cannot_open_has_no_download(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[]);
    fixture
        .local
        .files
        .lock()
        .unwrap()
        .insert(LOCAL_FILE.into(), vec![0xD6, 0xD0, 0xCE, 0xC4]);
    sftp_command(
        cx,
        &fixture,
        SftpTabCommand::Edit {
            remote: false,
            path: Some(LOCAL_FILE.to_string()),
        },
    );
    wait_visible(cx, &fixture, "editor-refused");
    assert!(!visible(cx, &fixture, "editor-refused-download"));
    click(cx, &fixture, "editor-refused-close");
}

// ---------- 本地文件 ----------

#[gpui_kit::test]
fn a_local_file_is_edited_on_this_computer(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[]);
    fixture
        .local
        .files
        .lock()
        .unwrap()
        .insert(LOCAL_FILE.into(), b"local\n".to_vec());
    open_file(cx, &fixture, false, LOCAL_FILE);
    assert_eq!(text_of(cx, &fixture), "local\n");
    assert_eq!(
        label(cx, &fixture, "editor-status"),
        "本机文件 · 第 1 行，第 1 列 · UTF-8 · LF · 纯文本"
    );
    // 标签上灰字写「本机」，「文件」是灰的。
    assert_eq!(rail_label(cx, &fixture), "文件 甲.txt 本机");
    assert!(matches!(
        cx.update(|cx| fixture.workspace.read(cx).files_button(cx)),
        qishell::workspace::FilesButton::Unavailable(_)
    ));

    type_text(cx, &fixture, "x");
    press(cx, &fixture, "ctrl-s");
    let files = fixture.local.files.clone();
    let path = std::path::PathBuf::from(LOCAL_FILE);
    assert!(wait_until(cx, fixture.handle, |_| {
        files.lock().unwrap().get(&path).map(Vec::as_slice) == Some(b"xlocal\n".as_slice())
    }));
    assert!(wait_until(cx, fixture.handle, |cx| !unsaved(cx, &fixture)));
    // 没经过远程连接。
    assert!(fixture.sftp.writes.lock().unwrap().is_empty());
}

#[gpui_kit::test]
fn closing_the_sftp_tab_leaves_local_files_open(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[]);
    fixture
        .local
        .files
        .lock()
        .unwrap()
        .insert(LOCAL_FILE.into(), b"local\n".to_vec());
    open_file(cx, &fixture, false, LOCAL_FILE);
    type_text(cx, &fixture, "x");
    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    assert!(
        !visible(cx, &fixture, "close-tabs"),
        "本地文件不跟着关，不用问"
    );
    assert_eq!(tab_count(cx, &fixture), 1);
    assert!(unsaved(cx, &fixture));
}

// ---------- 保存后的文件卡片 ----------

#[gpui_kit::test]
fn saving_rereads_the_folder_the_remote_card_is_showing(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    let before = fixture.sftp.listed.lock().unwrap().len();
    type_text(cx, &fixture, "1");
    press(cx, &fixture, "ctrl-s");
    wait_for_writes(cx, &fixture, 1);
    let listed = fixture.sftp.listed.clone();
    assert!(
        wait_until(cx, fixture.handle, |_| listed.lock().unwrap().len()
            > before),
        "文件卡片没有重读目录"
    );
    assert_eq!(
        fixture.sftp.listed.lock().unwrap().last().unwrap(),
        "/home/tester"
    );
}

// ---------- 断开 ----------

/// 读到一半连接断了：等着的读有了回答（失败），同一个文件之后还能再打开。
#[gpui_kit::test]
fn a_read_cut_off_by_a_lost_connection_can_be_tried_again(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[("/slow/a.txt", b"x\n")]);
    let edit = SftpTabCommand::Edit {
        remote: true,
        path: Some("/slow/a.txt".to_string()),
    };
    sftp_command(cx, &fixture, edit.clone());
    let reads = fixture.sftp.reads.clone();
    assert!(wait_until(cx, fixture.handle, |_| {
        reads.lock().unwrap().len() == 1
    }));

    fixture
        .sftp
        .emit(qishell::backend::sftp::SftpEvent::Disconnected(
            "连接超时".into(),
        ));
    let sftp = sftp_tab(cx, &fixture);
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| sftp.read(cx).remote().read(cx).problem().is_some())
    }));
    assert_eq!(tab_count(cx, &fixture), 1);

    sftp_command(cx, &fixture, SftpTabCommand::Reconnect);
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| sftp.read(cx).connection_state())
            == qishell::sftp_tab::ConnectionState::Connected
    }));
    sftp_command(cx, &fixture, edit);
    assert!(wait_until(cx, fixture.handle, |_| {
        reads.lock().unwrap().len() == 2
    }));
}

#[gpui_kit::test]
fn saving_while_disconnected_offers_to_reconnect(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    fixture
        .sftp
        .emit(qishell::backend::sftp::SftpEvent::Disconnected(
            "连接超时".into(),
        ));
    let sftp = sftp_tab(cx, &fixture);
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| sftp.read(cx).remote().read(cx).problem().is_some())
    }));

    press(cx, &fixture, "ctrl-s");
    assert!(visible(cx, &fixture, "sftp-reconnect"));
    assert!(fixture.sftp.writes.lock().unwrap().is_empty());
    assert!(unsaved(cx, &fixture));
}

// ---------- 关窗口、退出 ----------

fn enqueue_upload(cx: &mut TestAppContext, fixture: &Fixture) {
    sftp_command(
        cx,
        fixture,
        SftpTabCommand::Enqueue(TransferJob::Upload {
            paths: vec!["/local/a".into()],
            target: "/srv".into(),
        }),
    );
}

#[gpui_kit::test]
fn quitting_with_nothing_to_lose_does_not_ask(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    assert!(!in_frame(cx, fixture.handle, qishell::app::quit_held_back));
    assert!(!visible(cx, &fixture, "quit-confirm"));
}

#[gpui_kit::test]
fn quitting_with_unsaved_changes_and_unfinished_transfers_asks_once(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[(REMOTE_FILE, b"text\n")]);
    open_file(cx, &fixture, true, REMOTE_FILE);
    type_text(cx, &fixture, "1");
    enqueue_upload(cx, &fixture);

    // 从应用级的 Quit 来，焦点在不在窗口壳都一样。
    cx.update(|cx| cx.dispatch_action(&Quit));
    cx.run_until_parked();
    assert!(visible(cx, &fixture, "quit-confirm"));
    let text = label(cx, &fixture, "quit-confirm-text");
    assert!(text.contains("web-01 还有 1 批传输没完成"), "{text}");
    assert!(text.contains("「文件 甲.txt」有未保存的修改"), "{text}");

    // 再来一次：还是拦着，也不多叠一个框。
    assert!(in_frame(cx, fixture.handle, qishell::app::quit_held_back));
    click(cx, &fixture, "quit-confirm-cancel");
    assert!(!visible(cx, &fixture, "quit-confirm"));
    assert_eq!(tab_count(cx, &fixture), 2);
    assert!(unsaved(cx, &fixture));
}

#[gpui_kit::test]
fn closing_the_window_with_only_a_transfer_left_asks_too(cx: &mut TestAppContext) {
    let fixture = open_server(cx, &[]);
    enqueue_upload(cx, &fixture);
    assert!(in_frame(cx, fixture.handle, qishell::app::quit_held_back));
    assert!(visible(cx, &fixture, "quit-confirm"));
    click(cx, &fixture, "quit-confirm-confirm");
    assert!(!visible(cx, &fixture, "quit-confirm"));
}

// ---------- 写回进行中 ----------

const HELD_FILE: &str = "/hold-write/文件 甲.txt";

/// 打开 `HELD_FILE`，改一个字，保存：写回卡在半路。
fn start_held_save(cx: &mut TestAppContext) -> Fixture {
    let fixture = open_server(cx, &[(HELD_FILE, b"text\n")]);
    open_file(cx, &fixture, true, HELD_FILE);
    type_text(cx, &fixture, "1");
    press(cx, &fixture, "ctrl-s");
    let held = fixture.sftp.held_writes.clone();
    assert!(
        wait_until(cx, fixture.handle, |_| !held.lock().unwrap().is_empty()),
        "写回没有开始"
    );
    fixture
}

/// 写回有了结果。
fn finish_held_save(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    result: Result<(), qishell::backend::sftp::SaveFailure>,
) {
    let request_id = fixture.sftp.held_writes.lock().unwrap().remove(0);
    let bytes = b"1text\n";
    fixture
        .sftp
        .emit(qishell::backend::sftp::SftpEvent::FileWritten {
            request_id,
            result: result.map(|()| fake_stamp(bytes, 101)),
        });
    for _ in 0..5 {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, fixture.handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 写回是截断重写：写到一半关 SFTP 标签，标签要等它写完才关，不能断开连接。
#[gpui_kit::test]
fn closing_the_sftp_tab_during_a_save_waits_for_it(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);

    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    assert!(visible(cx, &fixture, "save-wait"));
    let text = label(cx, &fixture, "save-wait-text");
    assert!(text.contains("文件 甲.txt"), "{text}");
    assert_eq!(tab_count(cx, &fixture), 2, "写完之前标签都在");

    finish_held_save(cx, &fixture, Ok(()));
    assert!(!visible(cx, &fixture, "save-wait"));
    assert_eq!(tab_count(cx, &fixture), 0, "写完了就关");
}

/// 编辑器自己也一样：不能「不保存」扔下写了一半的文件。
#[gpui_kit::test]
fn closing_an_editor_during_its_save_waits_instead_of_asking(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    dispatch(cx, fixture.handle, CloseFocusedCard);
    assert!(visible(cx, &fixture, "save-wait"));
    assert!(!visible(cx, &fixture, "editor-close"));
    finish_held_save(cx, &fixture, Ok(()));
    assert_eq!(tab_count(cx, &fixture), 1);
}

/// 写回失败了：不走，错误显示着，文件可能不完整，用户得看到。
#[gpui_kit::test]
fn a_save_that_fails_while_waiting_keeps_everything_open_and_says_so(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    finish_held_save(
        cx,
        &fixture,
        Err(qishell::backend::sftp::SaveFailure::Interrupted(
            "连接断开".into(),
        )),
    );
    assert!(!visible(cx, &fixture, "save-wait"));
    assert_eq!(tab_count(cx, &fixture), 2);
    let notes = in_frame(cx, fixture.handle, |window, cx| {
        window.notifications(cx).len()
    });
    assert_eq!(notes, 1);
}

/// 用户可以不等了：标签都留着，写完也不关。
#[gpui_kit::test]
fn the_wait_can_be_called_off(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    let sftp = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    dispatch(cx, fixture.handle, qishell::app::CloseTab(sftp));
    click(cx, &fixture, "save-wait-cancel");
    assert!(!visible(cx, &fixture, "save-wait"));
    assert_eq!(tab_count(cx, &fixture), 2);
    finish_held_save(cx, &fixture, Ok(()));
    assert_eq!(tab_count(cx, &fixture), 2);
}

/// 退出也等：写完才接着走。退出还有别的要问（这里是一批传输），写完再问。
#[gpui_kit::test]
fn quitting_during_a_save_waits_and_then_asks_about_the_rest(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    enqueue_upload(cx, &fixture);
    assert!(in_frame(cx, fixture.handle, qishell::app::quit_held_back));
    assert!(visible(cx, &fixture, "save-wait"));
    assert!(!visible(cx, &fixture, "quit-confirm"));
    // 再要求一次：还是拦着，不多叠一个框。
    assert!(in_frame(cx, fixture.handle, qishell::app::quit_held_back));

    finish_held_save(cx, &fixture, Ok(()));
    assert!(!visible(cx, &fixture, "save-wait"));
    assert!(visible(cx, &fixture, "quit-confirm"));
    let text = label(cx, &fixture, "quit-confirm-text");
    assert!(text.contains("还有 1 批传输没完成"), "{text}");
    assert!(!text.contains("未保存"), "{text}");
}

/// 退出时写回失败：不退出。
#[gpui_kit::test]
fn a_save_that_fails_while_quitting_stops_the_quit(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    assert!(in_frame(cx, fixture.handle, qishell::app::quit_held_back));
    finish_held_save(
        cx,
        &fixture,
        Err(qishell::backend::sftp::SaveFailure::Failed(
            "权限不足".into(),
        )),
    );
    assert!(!visible(cx, &fixture, "save-wait"));
    assert!(!visible(cx, &fixture, "quit-confirm"));
    assert_eq!(tab_count(cx, &fixture), 2);
}

/// 写回时不重新连接：工作线程还在写，编辑器不能以为写断了。
#[gpui_kit::test]
fn reconnecting_waits_for_the_save(cx: &mut TestAppContext) {
    let fixture = start_held_save(cx);
    sftp_command(cx, &fixture, SftpTabCommand::Reconnect);
    assert_eq!(*fixture.sftp.reconnects.lock().unwrap(), 0);
    let tab = sftp_tab(cx, &fixture);
    assert!(!cx.update(|cx| tab.read(cx).can_reconnect()));
    finish_held_save(cx, &fixture, Ok(()));
    assert!(cx.update(|cx| tab.read(cx).can_reconnect()));
}

// ---------- 关闭其他 ----------

/// 「关闭其他」对着远程编辑器：它自己永远留下，所以它所属的 SFTP 标签也留着（关了会把它连带关掉）。
#[gpui_kit::test]
fn closing_the_others_of_a_remote_editor_keeps_it_and_its_sftp_tab(cx: &mut TestAppContext) {
    let fixture = open_server(
        cx,
        &[(REMOTE_FILE, b"one\n"), (OTHER_REMOTE_FILE, b"two\n")],
    );
    open_file(cx, &fixture, true, REMOTE_FILE);
    open_file(cx, &fixture, true, OTHER_REMOTE_FILE);
    let (sftp, kept) = cx.update(|cx| {
        let workspace = fixture.workspace.read(cx);
        (workspace.tab_ids()[0], workspace.active_tab().unwrap())
    });

    dispatch(cx, fixture.handle, qishell::app::CloseOtherTabs(kept));
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_ids()),
        [sftp, kept]
    );
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).active_tab()),
        Some(kept)
    );

    // 只剩它和它的 SFTP 标签：没什么可关的。
    dispatch(cx, fixture.handle, qishell::app::CloseOtherTabs(kept));
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_ids()),
        [sftp, kept]
    );
}
