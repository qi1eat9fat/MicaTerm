//! SFTP 标签的入口和浏览（工单 20）：三个入口、已有就切换、本地卡片变灰、没有焦点卡片时按标签类型处理、
//! 不能分屏、两张文件卡片、路径栏、初始目录、状态栏、标签轨道、问答队列、解锁、关闭确认。
//! 连接和本地目录都是假的（`FakeSftpProvider`、`FakeLocalDirectory`），真协议的冒烟在 backend 的 `sftp_tests`。

use std::time::Duration;

use qishell::app::{
    CloseFocusedCard, CloseOtherTabs, ConnectHost, NextTab, OpenCardSftp, OpenHostSftp, OpenSftp,
    SftpAction, SftpTabCommand, ShowCredentials, ShowSnippets, ToggleHiddenFiles,
};
use qishell::backend::sftp::{FileStamp, RemotePath, SftpEvent, TextFile};
use qishell::backend::{
    AuthenticationRequest, ConnectionPrompt, ConnectionPromptKind, ProbedHost, RemoteEvent,
    RemoteTerminalId,
};
use qishell::editor::{EditorLocation, EditorOptions, EditorTab};
use qishell::sftp_tab::{CardSide, SftpTab, TransferJob};
use qishell::workspace::{FilesButton, RightPanel};
use qishell_hosts::SecretRef;
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

struct Fixture {
    handle: WindowHandle<Root>,
    workspace: Entity<Workspace>,
    remote: Arc<FakeRemoteTerminals>,
    sftp: Arc<FakeSftpProvider>,
    local: FakeLocalDirectory,
}

fn open_with(cx: &mut TestAppContext, sftp: FakeSftpProvider, base: WorkspaceServices) -> Fixture {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let sftp = Arc::new(sftp);
    let local = FakeLocalDirectory::default();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        sftp: sftp.clone(),
        local_files: Arc::new(local.clone()),
        ..base
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    Fixture {
        handle,
        workspace,
        remote,
        sftp,
        local,
    }
}

fn open(cx: &mut TestAppContext) -> Fixture {
    open_with(cx, FakeSftpProvider::default(), fake_services())
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

/// 在新标签里连接主机，等卡片连上。
fn connect(cx: &mut TestAppContext, fixture: &Fixture, host: u64, card: u64) {
    dispatch(cx, fixture.handle, ConnectHost(HostId(host)));
    let workspace = fixture.workspace.clone();
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            cx.update(|cx| {
                workspace
                    .read(cx)
                    .card(CardId(card), cx)
                    .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
            }) == Some(TerminalLifecycle::Running)
        }),
        "卡片没有连上"
    );
}

fn active_sftp(cx: &mut TestAppContext, fixture: &Fixture) -> Option<Entity<SftpTab>> {
    cx.update(|cx| fixture.workspace.read(cx).active_sftp_tab())
}

/// 这一侧现在的目录（路径栏的值：读完了才写）。
fn shown_path(cx: &mut TestAppContext, fixture: &Fixture, side: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(side)
            .and_then(|element| element.value().map(str::to_owned))
    })
}

/// 等 SFTP 标签两侧都读完目录。
fn wait_listed(cx: &mut TestAppContext, fixture: &Fixture, remote: &str, local: &str) {
    let (remote, local) = (remote.to_string(), local.to_string());
    let handle = fixture.handle;
    let done = wait_until(cx, handle, |cx| {
        shown_path(cx, fixture, "remote-path").as_deref() == Some(remote.as_str())
            && shown_path(cx, fixture, "local-path").as_deref() == Some(local.as_str())
    });
    assert!(
        done,
        "两侧没有读完：远程 {:?}，本地 {:?}",
        shown_path(cx, fixture, "remote-path"),
        shown_path(cx, fixture, "local-path")
    );
}

/// 连上主机 1，打开它的 SFTP 标签，等两侧读完。
fn open_tab(cx: &mut TestAppContext, fixture: &Fixture) {
    connect(cx, fixture, 1, 1);
    dispatch(cx, fixture.handle, OpenSftp);
    wait_listed(cx, fixture, "/home/tester", LOCAL_DOWNLOADS);
}

fn label(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> Option<String> {
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn visible(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> bool {
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
}

fn files_button(cx: &mut TestAppContext, fixture: &Fixture) -> FilesButton {
    cx.update(|cx| fixture.workspace.read(cx).files_button(cx))
}

fn tab_count(cx: &mut TestAppContext, fixture: &Fixture) -> usize {
    cx.update(|cx| fixture.workspace.read(cx).tab_ids().len())
}

fn sftp_action(cx: &mut TestAppContext, fixture: &Fixture, command: SftpTabCommand) {
    let tab = active_sftp(cx, fixture).expect("an SFTP tab in front");
    let id = cx.update(|cx| tab.read(cx).id());
    dispatch(cx, fixture.handle, SftpAction::new(id, command));
}

/// 直接开一个编辑器标签：这里测的是「文件」按钮和关闭，不是读文件（读文件见 `editor.rs`）。
fn open_editor(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    location: EditorLocation,
    sftp: Option<gpui_kit::WeakEntity<SftpTab>>,
) {
    let (workspace, local_files) = (fixture.workspace.clone(), fixture.local.clone());
    in_frame(cx, fixture.handle, move |window, cx| {
        let dispatch = cx.focus_handle();
        let file = TextFile::decode::<String>(Vec::new(), FileStamp::new(0, None)).unwrap();
        let editor = cx.new(|cx| {
            EditorTab::new(
                EditorOptions {
                    location,
                    place: "web-01".into(),
                    link: "link".into(),
                    sftp,
                    local_files: Arc::new(local_files),
                    file,
                    dispatch,
                },
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            workspace.open_editor_tab(editor, window, cx);
        });
    });
}

/// 本地编辑器标签。
fn open_local_editor(cx: &mut TestAppContext, fixture: &Fixture) {
    let location = EditorLocation::Local("notes.txt".into());
    open_editor(cx, fixture, location, None);
}

/// `sftp` 打开的远程编辑器标签。
fn open_remote_editor(cx: &mut TestAppContext, fixture: &Fixture, sftp: &Entity<SftpTab>) {
    let location = EditorLocation::Remote(RemotePath::new("/etc/hosts").unwrap());
    open_editor(cx, fixture, location, Some(sftp.downgrade()));
}

// ---------- 三个入口 ----------

#[gpui_kit::test]
fn the_files_icon_opens_the_sftp_tab_of_the_focused_remote_card(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    assert_eq!(files_button(cx, &fixture), FilesButton::Open);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("right-sidebar-files", cx)
    });

    let tab = active_sftp(cx, &fixture).expect("the SFTP tab is in front");
    assert_eq!(cx.update(|cx| tab.read(cx).host_id()), HostId(1));
    assert_eq!(tab_count(cx, &fixture), 2);
    assert_eq!(files_button(cx, &fixture), FilesButton::Current);
    // 自己一条连接，不走连接表。
    assert_eq!(fixture.sftp.connections(), 1);
    assert_eq!(fixture.remote.targets().len(), 1);
}

#[gpui_kit::test]
fn ctrl_shift_u_opens_the_same_tab_and_a_second_time_switches_to_it(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    in_frame(cx, fixture.handle, |window, cx| {
        window.press("ctrl-shift-u", cx)
    });
    assert!(active_sftp(cx, &fixture).is_some());

    // 回到那张卡片，再按一次：切过去，不新开。
    dispatch(cx, fixture.handle, NextTab);
    assert!(active_sftp(cx, &fixture).is_none());
    in_frame(cx, fixture.handle, |window, cx| {
        window.press("ctrl-shift-u", cx)
    });
    assert!(active_sftp(cx, &fixture).is_some());
    assert_eq!(tab_count(cx, &fixture), 2);
    assert_eq!(fixture.sftp.connections(), 1);
}

#[gpui_kit::test]
fn the_terminal_menu_opens_the_host_of_that_card_and_switches_when_open(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    connect(cx, &fixture, 2, 2);

    // 右键的是卡片 1，不是焦点卡片 2。
    dispatch(cx, fixture.handle, OpenCardSftp(CardId(1)));
    let tab = active_sftp(cx, &fixture).expect("the SFTP tab");
    assert_eq!(cx.update(|cx| tab.read(cx).host_id()), HostId(1));

    dispatch(cx, fixture.handle, NextTab);
    dispatch(cx, fixture.handle, OpenCardSftp(CardId(1)));
    assert_eq!(fixture.sftp.connections(), 1);
    assert_eq!(tab_count(cx, &fixture), 3);
}

#[gpui_kit::test]
fn the_host_menu_opens_the_right_clicked_host_from_anywhere(cx: &mut TestAppContext) {
    let fixture = open(cx);
    // 开始卡片上也行，没有焦点卡片也行。
    dispatch(cx, fixture.handle, OpenHostSftp(HostId(3)));
    let tab = active_sftp(cx, &fixture).expect("the SFTP tab");
    assert_eq!(cx.update(|cx| tab.read(cx).host_id()), HostId(3));
    // 不是焦点卡片的主机：从主目录开始。
    wait_listed(cx, &fixture, "/home/tester", LOCAL_DOWNLOADS);

    // 已有就切过去。
    connect(cx, &fixture, 1, 1);
    dispatch(cx, fixture.handle, OpenHostSftp(HostId(3)));
    assert!(active_sftp(cx, &fixture).is_some());
    assert_eq!(fixture.sftp.connections(), 1);
}

// ---------- 变灰 ----------

#[gpui_kit::test]
fn a_local_card_greys_the_files_icon_and_says_why(cx: &mut TestAppContext) {
    let fixture = open(cx);
    dispatch(cx, fixture.handle, NewLocalTerminalTab);

    assert_eq!(
        files_button(cx, &fixture),
        FilesButton::Unavailable("本地终端没有远程文件".into())
    );
    in_frame(cx, fixture.handle, |window, cx| {
        window.press("ctrl-shift-u", cx)
    });
    assert!(active_sftp(cx, &fixture).is_none());
    assert_eq!(tab_count(cx, &fixture), 1);
    assert_eq!(fixture.sftp.connections(), 0);
}

#[gpui_kit::test]
fn the_start_card_and_a_local_editor_have_no_remote_files(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let reason = FilesButton::Unavailable("当前标签没有远程文件".into());
    assert_eq!(files_button(cx, &fixture), reason);
    dispatch(cx, fixture.handle, OpenSftp);
    assert_eq!(tab_count(cx, &fixture), 0);

    open_local_editor(cx, &fixture);
    assert_eq!(files_button(cx, &fixture), reason);
    dispatch(cx, fixture.handle, OpenSftp);
    assert_eq!(tab_count(cx, &fixture), 1);
    assert_eq!(fixture.sftp.connections(), 0);
}

#[gpui_kit::test]
fn in_an_sftp_tab_the_files_icon_does_nothing(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    dispatch(cx, fixture.handle, OpenSftp);
    assert!(active_sftp(cx, &fixture).is_some());
    assert_eq!(tab_count(cx, &fixture), 2);
    assert_eq!(fixture.sftp.connections(), 1);
}

#[gpui_kit::test]
fn a_remote_editor_switches_to_its_sftp_tab(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    let sftp = active_sftp(cx, &fixture).unwrap();
    open_remote_editor(cx, &fixture, &sftp);
    assert!(active_sftp(cx, &fixture).is_none());
    assert_eq!(files_button(cx, &fixture), FilesButton::Open);

    dispatch(cx, fixture.handle, OpenSftp);

    assert_eq!(active_sftp(cx, &fixture).as_ref(), Some(&sftp));
    assert_eq!(fixture.sftp.connections(), 1);
}

// ---------- 不能分屏，Ctrl+Shift+W 关闭 ----------

#[gpui_kit::test]
fn an_sftp_tab_cannot_be_split_and_ctrl_shift_w_closes_it(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    let tab = cx.update(|cx| fixture.workspace.read(cx).active_tab().unwrap());

    dispatch(cx, fixture.handle, SplitRight);
    dispatch(cx, fixture.handle, SplitDown);
    dispatch(cx, fixture.handle, ToggleZoomCard);
    assert_eq!(tab_count(cx, &fixture), 2);
    cx.update(|cx| {
        let workspace = fixture.workspace.read(cx);
        assert!(workspace.tab_cards(tab).is_empty());
        assert_eq!(workspace.focused_card(cx), None);
    });

    in_frame(cx, fixture.handle, |window, cx| {
        window.press("ctrl-shift-w", cx)
    });
    assert_eq!(tab_count(cx, &fixture), 1);
    assert!(active_sftp(cx, &fixture).is_none());
    // 焦点回到左边的终端标签：卡片还在。
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).focused_card(cx)),
        Some(CardId(1))
    );
}

// ---------- 两张文件卡片 ----------

#[gpui_kit::test]
fn the_two_cards_show_their_place_and_files(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    assert_eq!(
        label(cx, fixture.handle, "local-header").as_deref(),
        Some(format!("本机 {LOCAL_DOWNLOADS}").as_str())
    );
    assert_eq!(
        label(cx, fixture.handle, "remote-header").as_deref(),
        Some("web-01 /home/tester")
    );
    in_frame(cx, fixture.handle, |window, _| {
        assert!(
            window
                .within("remote-card")
                .try_find("file:文件 甲.txt")
                .is_some()
        );
        assert!(
            window
                .within("local-card")
                .try_find("file:文件 甲.txt")
                .is_some()
        );
        // 上级目录那一行和原型一样。
        assert!(window.within("remote-card").try_find("file:..").is_some());
    });
    // 本地 `list` 只碰假目录。
    assert_eq!(
        fixture.local.listed.lock().unwrap().first(),
        Some(&LOCAL_DOWNLOADS.into())
    );
}

// ---------- 路径栏 ----------

/// 焦点放进这一侧的文件列表。
fn focus_list(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) {
    sftp_action(
        cx,
        fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::from_remote(remote),
        },
    );
}

fn editing_path(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) -> bool {
    let tab = active_sftp(cx, fixture).unwrap();
    cx.update(|cx| {
        tab.read(cx)
            .card(CardSide::from_remote(remote))
            .read(cx)
            .is_editing_path()
    })
}

fn path_error(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) -> Option<String> {
    let tab = active_sftp(cx, fixture).unwrap();
    cx.update(|cx| {
        tab.read(cx)
            .card(CardSide::from_remote(remote))
            .read(cx)
            .path_edit_error()
            .map(|error| error.to_string())
    })
}

#[gpui_kit::test]
fn ctrl_o_turns_the_path_bar_into_an_input_and_enter_goes_there(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    focus_list(cx, &fixture, true);

    in_frame(cx, fixture.handle, |window, cx| window.press("ctrl-o", cx));
    assert!(editing_path(cx, &fixture, true));
    assert_eq!(
        label(cx, fixture.handle, "remote-path-hint").as_deref(),
        Some("回车切换 · Esc 取消")
    );

    // 打开时选中了当前路径：打字就替换。
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("/var/log", cx);
        window.press("enter", cx);
    });
    let done = wait_until(cx, fixture.handle, |cx| {
        shown_path(cx, &fixture, "remote-path").as_deref() == Some("/var/log")
    });
    assert!(done, "没有切过去");
    assert!(!editing_path(cx, &fixture, true));
    assert_eq!(
        label(cx, fixture.handle, "remote-header").as_deref(),
        Some("web-01 /var/log")
    );
}

#[gpui_kit::test]
fn clicking_the_blank_of_the_path_bar_edits_and_escape_cancels(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("remote-path", cx)
    });
    assert!(editing_path(cx, &fixture, true));

    in_frame(cx, fixture.handle, |window, cx| window.press("escape", cx));
    assert!(!editing_path(cx, &fixture, true));
    assert_eq!(
        shown_path(cx, &fixture, "remote-path").as_deref(),
        Some("/home/tester")
    );
}

#[gpui_kit::test]
fn a_directory_that_is_not_there_turns_the_path_bar_red_and_stays(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::EditPath {
            side: CardSide::Remote,
        },
    );

    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::SubmitPath {
            side: CardSide::Remote,
            text: "/missing".into(),
        },
    );
    let failed = wait_until(cx, fixture.handle, |cx| {
        path_error(cx, &fixture, true).is_some()
    });
    assert!(failed, "没有报错");

    // 写原因，留在输入框，不切换，也不算卡片的错误。
    assert_eq!(
        label(cx, fixture.handle, "remote-path-hint").as_deref(),
        Some("无法读取目录：没有这个目录")
    );
    assert!(editing_path(cx, &fixture, true));
    let tab = active_sftp(cx, &fixture).unwrap();
    cx.update(|cx| {
        let remote = tab.read(cx).remote().read(cx);
        assert_eq!(remote.path(), "/home/tester");
        assert_eq!(remote.problem(), None);
    });
    // 改一下文字，红字就没了。
    in_frame(cx, fixture.handle, |window, cx| window.input("x", cx));
    assert_eq!(path_error(cx, &fixture, true), None);
}

#[gpui_kit::test]
fn the_path_bar_works_for_the_local_side_too(cx: &mut TestAppContext) {
    let fixture = open(cx);
    fixture
        .local
        .unreadable
        .lock()
        .unwrap()
        .push("/nowhere".into());
    open_tab(cx, &fixture);
    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::EditPath {
            side: CardSide::Local,
        },
    );

    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::SubmitPath {
            side: CardSide::Local,
            text: "/nowhere".into(),
        },
    );
    let failed = wait_until(cx, fixture.handle, |cx| {
        path_error(cx, &fixture, false).is_some()
    });
    assert!(failed, "没有报错");
    assert_eq!(
        label(cx, fixture.handle, "local-path-hint").as_deref(),
        Some("无法读取目录：找不到这个目录")
    );
    let tab = active_sftp(cx, &fixture).unwrap();
    assert_eq!(
        cx.update(|cx| tab.read(cx).local().read(cx).path()),
        LOCAL_DOWNLOADS
    );

    // 去得了的目录：切过去，路径栏变回分段。
    in_frame(cx, fixture.handle, |window, cx| window.press("escape", cx));
    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::EditPath {
            side: CardSide::Local,
        },
    );
    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::SubmitPath {
            side: CardSide::Local,
            text: "~".into(),
        },
    );
    let moved = wait_until(cx, fixture.handle, |cx| !editing_path(cx, &fixture, false));
    assert!(moved);
    assert_eq!(
        cx.update(|cx| tab.read(cx).local().read(cx).path()),
        std::path::Path::new(LOCAL_HOME)
            .components()
            .collect::<std::path::PathBuf>()
            .to_string_lossy()
    );
}

#[gpui_kit::test]
fn backspace_goes_up_and_tab_switches_sides(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    focus_list(cx, &fixture, true);

    in_frame(cx, fixture.handle, |window, cx| {
        window.press("backspace", cx)
    });
    let up = wait_until(cx, fixture.handle, |cx| {
        shown_path(cx, &fixture, "remote-path").as_deref() == Some("/home")
    });
    assert!(up, "没有回到上级");

    let tab = active_sftp(cx, &fixture).unwrap();
    let current = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let tab = tab.read(cx);
            (
                tab.local().read(cx).is_current(),
                tab.remote().read(cx).is_current(),
            )
        })
    };
    assert_eq!(current(cx), (false, true));
    in_frame(cx, fixture.handle, |window, cx| window.press("tab", cx));
    assert_eq!(current(cx), (true, false));
    in_frame(cx, fixture.handle, |window, cx| window.press("tab", cx));
    assert_eq!(current(cx), (false, true));
}

#[gpui_kit::test]
fn the_keyboard_moves_the_cursor_and_enter_opens_a_folder(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    focus_list(cx, &fixture, true);

    // `..`、目录、链接目录、文件甲、文件乙：第二次按下是「目录」。
    in_frame(cx, fixture.handle, |window, cx| {
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    });
    let done = wait_until(cx, fixture.handle, |cx| {
        shown_path(cx, &fixture, "remote-path").as_deref() == Some("/home/tester/目录")
    });
    assert!(done, "现在在 {:?}", shown_path(cx, &fixture, "remote-path"));
}

// ---------- 初始目录 ----------

/// 让卡片 1 报告它在 `directory`（OSC 7），系统也探测出来了。
fn report_directory(cx: &mut TestAppContext, fixture: &Fixture, directory: &str) {
    fixture
        .remote
        .factory
        .print(1, &format!("\x1b]7;file://web-01{directory}\x07$ "));
    fixture.remote.emit(RemoteEvent::Probed {
        terminal: RemoteTerminalId(1),
        host: ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        },
    });
    let workspace = fixture.workspace.clone();
    let directory = directory.to_string();
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .card(CardId(1), cx)
                .and_then(|card| card.read(cx).remote_start_directory().map(str::to_owned))
        })
        .as_deref()
            == Some(directory.as_str())
    }));
}

#[gpui_kit::test]
fn the_remote_side_starts_at_the_directory_of_the_focused_card(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    report_directory(cx, &fixture, "/srv/app");

    dispatch(cx, fixture.handle, OpenSftp);

    wait_listed(cx, &fixture, "/srv/app", LOCAL_DOWNLOADS);
}

#[gpui_kit::test]
fn a_start_directory_that_cannot_be_read_falls_back_to_home_quietly(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    report_directory(cx, &fixture, "/denied");

    dispatch(cx, fixture.handle, OpenSftp);

    wait_listed(cx, &fixture, "/home/tester", LOCAL_DOWNLOADS);
    let tab = active_sftp(cx, &fixture).unwrap();
    assert_eq!(
        cx.update(|cx| tab.read(cx).remote().read(cx).problem().map(str::to_owned)),
        None
    );
    assert_eq!(
        *fixture.sftp.listed.lock().unwrap(),
        ["/denied", "/home/tester"]
    );
}

#[gpui_kit::test]
fn the_host_menu_uses_the_card_directory_only_for_the_same_host(cx: &mut TestAppContext) {
    let fixture = open(cx);
    connect(cx, &fixture, 1, 1);
    report_directory(cx, &fixture, "/srv/app");

    // 焦点卡片是主机 1：右键别的主机从主目录开始。
    dispatch(cx, fixture.handle, OpenHostSftp(HostId(2)));
    wait_listed(cx, &fixture, "/home/tester", LOCAL_DOWNLOADS);

    // 回到卡片，右键它自己的主机：从它的目录开始。
    dispatch(cx, fixture.handle, NextTab);
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).focused_card(cx)),
        Some(CardId(1))
    );
    dispatch(cx, fixture.handle, OpenHostSftp(HostId(1)));
    wait_listed(cx, &fixture, "/srv/app", LOCAL_DOWNLOADS);
}

#[gpui_kit::test]
fn the_local_side_falls_back_to_home_when_downloads_cannot_be_read(cx: &mut TestAppContext) {
    let fixture = open(cx);
    fixture
        .local
        .unreadable
        .lock()
        .unwrap()
        .push(LOCAL_DOWNLOADS.into());
    connect(cx, &fixture, 1, 1);
    dispatch(cx, fixture.handle, OpenSftp);
    wait_listed(cx, &fixture, "/home/tester", LOCAL_HOME);
}

// ---------- 状态栏、标签轨道 ----------

#[gpui_kit::test]
fn the_status_bars_count_items_and_say_how_the_connection_is(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    assert_eq!(
        label(cx, fixture.handle, "local-status").as_deref(),
        Some("4 项 · 已选 0 项")
    );
    let endpoint = cx.update(|cx| {
        let tab = fixture.workspace.read(cx).active_sftp_tab().unwrap();
        let tab = tab.read(cx);
        let login = tab.login();
        format!("{}@{}:{}", login.user, login.host, login.port)
    });
    assert_eq!(
        label(cx, fixture.handle, "remote-status"),
        Some(format!("已连接 · {endpoint} · 4 项 · 已选 0 项"))
    );

    fixture
        .sftp
        .emit(SftpEvent::Latency(qishell::backend::Latency::Measured(
            Duration::from_millis(38),
        )));
    let measured = wait_until(cx, fixture.handle, |cx| {
        label(cx, fixture.handle, "remote-status").as_deref()
            == Some(format!("已连接 · {endpoint} · 38 ms · 4 项 · 已选 0 项").as_str())
    });
    assert!(measured, "{:?}", label(cx, fixture.handle, "remote-status"));

    // 选中几项。
    focus_list(cx, &fixture, false);
    in_frame(cx, fixture.handle, |window, cx| window.press("ctrl-a", cx));
    assert_eq!(
        label(cx, fixture.handle, "local-status").as_deref(),
        Some("4 项 · 已选 4 项")
    );

    // 断开：变红（`problem`）写原因。
    fixture.sftp.emit(SftpEvent::Disconnected(
        "连接超时（已自动重连 3 次）".into(),
    ));
    let lost = wait_until(cx, fixture.handle, |cx| {
        label(cx, fixture.handle, "remote-status").as_deref()
            == Some("已断开 · 连接超时（已自动重连 3 次） · 4 项")
    });
    assert!(lost, "{:?}", label(cx, fixture.handle, "remote-status"));
}

#[gpui_kit::test]
fn the_local_status_bar_counts_hidden_files(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            side: CardSide::Remote,
            path: "/dotfiles".into(),
        },
    );
    let hidden = wait_until(cx, fixture.handle, |cx| {
        label(cx, fixture.handle, "remote-status")
            .is_some_and(|status| status.ends_with("1 项 · 已选 0 项 · 隐藏 2 项"))
    });
    assert!(hidden, "{:?}", label(cx, fixture.handle, "remote-status"));

    // 开关写进设置，这一侧马上显示。
    dispatch(cx, fixture.handle, ToggleHiddenFiles(CardSide::Remote));
    let settings = cx.update(|cx| fixture.workspace.read(cx).settings().clone());
    assert!(cx.update(|cx| settings.read(cx).settings().sftp.show_hidden.remote));
    let shown = wait_until(cx, fixture.handle, |cx| {
        label(cx, fixture.handle, "remote-status")
            .is_some_and(|status| status.ends_with("3 项 · 已选 0 项"))
    });
    assert!(shown, "{:?}", label(cx, fixture.handle, "remote-status"));
}

#[gpui_kit::test]
fn the_tab_rail_shows_a_folder_tile_the_host_and_the_remote_directory(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    let tab = cx.update(|cx| fixture.workspace.read(cx).active_tab().unwrap());

    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_tiles(tab, cx)),
        ["SFTP"]
    );
    let rail = move |cx: &mut TestAppContext, fixture: &Fixture| {
        in_frame(cx, fixture.handle, move |window, _| {
            window
                .find(("tab", tab.0 as usize))
                .label()
                .map(str::to_owned)
        })
    };
    assert_eq!(rail(cx, &fixture).as_deref(), Some("web-01 /home/tester"));

    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            side: CardSide::Remote,
            path: "/var/log".into(),
        },
    );
    let moved = wait_until(cx, fixture.handle, |cx| {
        rail(cx, &fixture).as_deref() == Some("web-01 /var/log")
    });
    assert!(moved, "{:?}", rail(cx, &fixture));
}

// ---------- 右侧栏 ----------

#[gpui_kit::test]
fn in_an_sftp_tab_snippets_say_there_is_no_terminal_and_credentials_work(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);

    dispatch(cx, fixture.handle, ShowSnippets);
    assert!(visible(cx, fixture.handle, "snippets-no-terminal"));

    dispatch(cx, fixture.handle, ShowCredentials);
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).right_panel()),
        Some(RightPanel::Credentials)
    );
}

// ---------- 连接的问题、解锁 ----------

#[gpui_kit::test]
fn the_connection_of_the_tab_asks_through_the_same_queue(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);

    fixture.sftp.emit(SftpEvent::Prompt(ConnectionPrompt::new(
        41,
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        }),
    )));
    let workspace = fixture.workspace.clone();
    let shown = wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| workspace.read(cx).shown_sftp_prompt()) == Some(41)
    });
    assert!(shown, "问答框没有开");
    assert!(
        label(cx, fixture.handle, "connection-prompt").is_some(),
        "问答框要写是问谁"
    );

    in_frame(cx, fixture.handle, |window, cx| window.press("escape", cx));

    let replied = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.prompt_replies.lock().unwrap().contains(&41)
    });
    assert!(replied, "回答没有交回工作线程");
    assert_eq!(cx.update(|cx| workspace.read(cx).shown_sftp_prompt()), None);
}

#[gpui_kit::test]
fn closing_the_tab_withdraws_its_question(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    fixture.sftp.emit(SftpEvent::Prompt(ConnectionPrompt::new(
        42,
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        }),
    )));
    let workspace = fixture.workspace.clone();
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| workspace.read(cx).shown_sftp_prompt()) == Some(42)
    }));

    dispatch(cx, fixture.handle, CloseFocusedCard);

    assert_eq!(tab_count(cx, &fixture), 1);
    assert_eq!(cx.update(|cx| workspace.read(cx).shown_sftp_prompt()), None);
    assert!(!visible(cx, fixture.handle, "connection-prompt"));
}

#[gpui_kit::test]
fn a_locked_vault_is_unlocked_first_even_though_the_card_reuses_its_connection(
    cx: &mut TestAppContext,
) {
    let vault = TestVault::unlocked().with_secret(&SecretRef::Password { host: 1 }, "hunter2");
    let fixture = open_with(cx, FakeSftpProvider::default(), vault.services());
    connect(cx, &fixture, 1, 1);
    // 卡片连着（连接表能复用），这时保险库锁上了。
    let store = cx.update(|cx| fixture.workspace.read(cx).vault().clone());
    store.update(cx, |store, cx| store.lock(cx));
    cx.run_until_parked();

    dispatch(cx, fixture.handle, OpenSftp);

    // SFTP 标签要新登录一次：先解锁。
    assert!(visible(cx, fixture.handle, "vault-unlock"));
    assert_eq!(fixture.sftp.connections(), 0);
    assert!(active_sftp(cx, &fixture).is_none());

    // 取消了也接着开，现场问密码（卡片不一样：取消了就不连，见工单 33）。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("vault-unlock-cancel", cx)
    });
    assert!(active_sftp(cx, &fixture).is_some());
    assert_eq!(fixture.sftp.connections(), 1);
}

// ---------- 关闭确认 ----------

fn enqueue_upload(cx: &mut TestAppContext, fixture: &Fixture) {
    sftp_action(
        cx,
        fixture,
        SftpTabCommand::Enqueue(TransferJob::Upload {
            paths: vec!["/local/a".into()],
            target: "/srv".into(),
        }),
    );
    let tab = active_sftp(cx, fixture).unwrap();
    assert_eq!(cx.update(|cx| tab.read(cx).unfinished_transfers()), 1);
}

#[gpui_kit::test]
fn closing_a_tab_with_unfinished_transfers_asks_first(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    enqueue_upload(cx, &fixture);

    dispatch(cx, fixture.handle, CloseFocusedCard);
    assert!(visible(cx, fixture.handle, "close-tabs"));
    assert_eq!(tab_count(cx, &fixture), 2);
    assert!(
        label(cx, fixture.handle, "close-tabs-text")
            .unwrap()
            .contains("web-01 还有 1 批传输没完成")
    );

    // 取消：什么都不变。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("close-tabs-cancel", cx)
    });
    assert_eq!(tab_count(cx, &fixture), 2);
    assert!(active_sftp(cx, &fixture).is_some());

    // 确认：关掉。
    dispatch(cx, fixture.handle, CloseFocusedCard);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("close-tabs-confirm", cx)
    });
    assert_eq!(tab_count(cx, &fixture), 1);
    assert!(active_sftp(cx, &fixture).is_none());
}

#[gpui_kit::test]
fn closing_other_tabs_asks_once_and_only_when_it_matters(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    enqueue_upload(cx, &fixture);
    let tabs = cx.update(|cx| fixture.workspace.read(cx).tab_ids());
    let terminal = tabs[0];
    dispatch(cx, fixture.handle, NewLocalTerminalTab);

    dispatch(cx, fixture.handle, CloseOtherTabs(terminal));
    assert!(visible(cx, fixture.handle, "close-tabs"));
    // 问的时候什么都没动。
    assert_eq!(tab_count(cx, &fixture), 3);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("close-tabs-confirm", cx)
    });
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_ids()),
        [terminal]
    );
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).active_tab()),
        Some(terminal)
    );

    // 只有终端标签：不问。
    dispatch(cx, fixture.handle, NewLocalTerminalTab);
    dispatch(cx, fixture.handle, CloseOtherTabs(terminal));
    assert!(!visible(cx, fixture.handle, "close-tabs"));
    assert_eq!(tab_count(cx, &fixture), 1);
}

#[gpui_kit::test]
fn closing_an_sftp_tab_closes_its_remote_editors_only(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    let sftp = active_sftp(cx, &fixture).unwrap();
    open_remote_editor(cx, &fixture, &sftp);
    open_local_editor(cx, &fixture);
    assert_eq!(tab_count(cx, &fixture), 4);

    let tab = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[1]);
    dispatch(cx, fixture.handle, qishell::app::CloseTab(tab));

    // 卡片那个终端标签和本地文件的编辑器留着。
    assert_eq!(tab_count(cx, &fixture), 2);
}

#[gpui_kit::test]
fn disconnected_a_remote_command_offers_to_reconnect(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    fixture
        .sftp
        .emit(SftpEvent::Disconnected("连接超时".into()));
    let tab = active_sftp(cx, &fixture).unwrap();
    assert!(wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| tab.read(cx).remote().read(cx).problem().map(str::to_owned))
            .is_some()
    }));

    sftp_action(
        cx,
        &fixture,
        SftpTabCommand::Refresh {
            side: CardSide::Remote,
        },
    );
    assert!(visible(cx, fixture.handle, "sftp-reconnect"));
    // 本地一侧不受影响。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-reconnect-reconnect", cx)
    });
    let back = wait_until(cx, fixture.handle, |_| {
        *fixture.sftp.reconnects.lock().unwrap() == 1
    });
    assert!(back, "没有重新连接");
    let connected = wait_until(cx, fixture.handle, |cx| {
        label(cx, fixture.handle, "remote-status")
            .is_some_and(|status| status.starts_with("已连接"))
    });
    assert!(
        connected,
        "{:?}",
        label(cx, fixture.handle, "remote-status")
    );
}

// ---------- 拖动排序（工单 29）：终端、SFTP、编辑器标签混排 ----------

fn tab_element(id: TabId) -> (&'static str, usize) {
    ("tab", id.0 as usize)
}

fn tab_ids(cx: &mut TestAppContext, fixture: &Fixture) -> Vec<TabId> {
    cx.update(|cx| fixture.workspace.read(cx).tab_ids())
}

/// 把标签 `source` 拖到标签 `target` 的左四分之一（`before`）或右四分之一处松开。
fn drag_tab(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    source: TabId,
    target: TabId,
    before: bool,
) {
    in_frame(cx, fixture.handle, |window, cx| {
        let from = window.find(tab_element(source)).bounds().center();
        let bounds = window.find(tab_element(target)).bounds();
        let x = if before {
            bounds.left() + bounds.size.width * 0.25
        } else {
            bounds.left() + bounds.size.width * 0.75
        };
        window.drag(from, gpui_kit::point(x, bounds.center().y), cx);
    });
}

/// 三种标签混排时照样能拖动排序，拖动不切换在前的标签；拖完悬停 ×、右键菜单照常，
/// 点标签切过去，焦点落到它的内容：终端、文件列表、编辑器的文字。
#[gpui_kit::test]
fn tabs_of_every_kind_reorder_by_dragging_and_keep_working(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture);
    open_local_editor(cx, &fixture);
    let [terminal, sftp, editor] = tab_ids(cx, &fixture)[..] else {
        panic!("应有终端、SFTP、编辑器三个标签");
    };

    drag_tab(cx, &fixture, editor, terminal, true);
    assert_eq!(tab_ids(cx, &fixture), [editor, terminal, sftp]);
    drag_tab(cx, &fixture, terminal, sftp, false);
    assert_eq!(tab_ids(cx, &fixture), [editor, sftp, terminal]);
    let front = cx.update(|cx| fixture.workspace.read(cx).active_tab());
    assert_eq!(front, Some(editor), "拖动不切换在前的标签");

    // 悬停 ×：只出现在悬停的那个标签上。
    in_frame(cx, fixture.handle, |window, cx| {
        window.hover(tab_element(sftp), cx);
        assert!(window.find(("tab-close", sftp.0 as usize)).visible());
        assert!(!window.find(("tab-close", terminal.0 as usize)).visible());
    });

    // 右键菜单：还是那三项，Esc 关掉什么都不关。
    in_frame(cx, fixture.handle, |window, cx| {
        window.right_click(tab_element(terminal), cx)
    });
    in_frame(cx, fixture.handle, |window, cx| {
        {
            let popup = window.within("popup-menu");
            assert_eq!(popup.find(0_usize).label(), Some("关闭标签"));
            assert_eq!(popup.find(2_usize).label(), Some("关闭右侧的标签"));
        }
        window.press("escape", cx);
    });
    assert!(!visible(cx, fixture.handle, "popup-menu"));
    assert_eq!(tab_ids(cx, &fixture), [editor, sftp, terminal]);

    // 点 SFTP 标签：焦点在它的文件列表里。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click(tab_element(sftp), cx)
    });
    let tab = active_sftp(cx, &fixture).expect("SFTP 标签在前");
    let lists = cx.update(|cx| {
        let tab = tab.read(cx);
        [tab.local(), tab.remote()].map(|card| card.read(cx).focus_handle(cx))
    });
    in_frame(cx, fixture.handle, move |window, _| {
        assert!(
            lists.iter().any(|list| list.is_focused(window)),
            "焦点不在文件列表里"
        );
    });

    // 点终端标签：焦点在终端里。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click(tab_element(terminal), cx)
    });
    in_frame(cx, fixture.handle, |window, _| {
        assert_eq!(window.find(("terminal", 1_usize)).focused(), Some(true));
    });

    // 点编辑器标签：焦点在文字里，打的字进得去。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click(tab_element(editor), cx)
    });
    in_frame(cx, fixture.handle, |window, cx| window.input("x", cx));
    let text = cx.update(|cx| {
        let editor = fixture.workspace.read(cx).active_editor_tab();
        editor.map(|editor| editor.read(cx).text(cx))
    });
    assert_eq!(text.as_deref(), Some("x"));
}
