//! SFTP 文件操作（工单 21）：新建、重命名、删除、属性、隐藏文件、排序。
//! 连接和本地目录都是假的（`FakeSftpProvider`、`FakeLocalDirectory`），真协议在 backend 的测试里。

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::AsKeystroke as _;
use micaterm::app::{OpenHostSftp, SftpAction, SftpShortcut, SftpTabCommand};
use micaterm::backend::sftp::{PermissionEdit, RemoteOperation, RemotePath, SftpEvent};
use micaterm::sftp_tab::{CardSide, NewEntryKind, SftpTab};

use crate::support::*;

struct Fixture {
    handle: WindowHandle<Root>,
    workspace: Entity<Workspace>,
    sftp: Arc<FakeSftpProvider>,
    local: FakeLocalDirectory,
}

/// 打开主机 1 的 SFTP 标签，等两侧读完。
fn open(cx: &mut TestAppContext) -> Fixture {
    let sftp = Arc::new(FakeSftpProvider::default());
    let local = FakeLocalDirectory::default();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        sftp: sftp.clone(),
        local_files: Arc::new(local.clone()),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    let fixture = Fixture {
        handle,
        workspace,
        sftp,
        local,
    };
    dispatch(cx, handle, OpenHostSftp(HostId(1)));
    wait_for_path(cx, &fixture, true, "/home/tester");
    wait_for_path(cx, &fixture, false, LOCAL_DOWNLOADS);
    fixture
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

/// SFTP 标签，不一定在前：新建的文件会在编辑器标签里打开（工单 23）。
fn tab(cx: &mut TestAppContext, fixture: &Fixture) -> Entity<SftpTab> {
    cx.update(|cx| {
        let workspace = fixture.workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .find_map(|tab| workspace.sftp_tab(tab))
    })
    .expect("an SFTP tab")
}

fn command(cx: &mut TestAppContext, fixture: &Fixture, command: SftpTabCommand) {
    let id = {
        let tab = tab(cx, fixture);
        cx.update(|cx| tab.read(cx).id())
    };
    dispatch(cx, fixture.handle, SftpAction::new(id, command));
}

fn wait_for_path(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, path: &str) {
    let done = wait_until(cx, fixture.handle, |cx| {
        let tab = tab(cx, fixture);
        cx.update(|cx| {
            let card = tab.read(cx).card(CardSide::from_remote(remote)).read(cx);
            card.path() == path && !card.is_loading()
        })
    });
    assert!(done, "没有读完 {path}");
}

/// 这一侧显示的名字，按显示的顺序。
fn names(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) -> Vec<String> {
    let tab = tab(cx, fixture);
    cx.update(|cx| {
        tab.read(cx)
            .card(CardSide::from_remote(remote))
            .read(cx)
            .entries(cx)
            .iter()
            .map(|entry| entry.name.to_string())
            .collect()
    })
}

fn selected(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) -> Vec<String> {
    let tab = tab(cx, fixture);
    cx.update(|cx| {
        tab.read(cx)
            .card(CardSide::from_remote(remote))
            .read(cx)
            .selected_names(cx)
    })
}

fn card_id(remote: bool) -> &'static str {
    if remote { "remote-card" } else { "local-card" }
}

/// 焦点放进这一侧的列表，只选中 `name`。
fn select(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, name: &str) {
    command(
        cx,
        fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::from_remote(remote),
        },
    );
    let cell = format!("name-cell:{name}");
    in_frame(cx, fixture.handle, move |window, cx| {
        window.within(card_id(remote)).click(cell, cx)
    });
    assert_eq!(selected(cx, fixture, remote), [name]);
}

fn visible(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> bool {
    in_frame(cx, fixture.handle, move |window, _| {
        window.try_find(id).is_some()
    })
}

fn press(cx: &mut TestAppContext, fixture: &Fixture, key: &'static str) {
    in_frame(cx, fixture.handle, move |window, cx| window.press(key, cx));
}

fn click(cx: &mut TestAppContext, fixture: &Fixture, id: impl Into<ElementId>) {
    let id = id.into();
    in_frame(cx, fixture.handle, move |window, cx| window.click(id, cx));
}

fn value_of(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window.find(id).value().map(str::to_owned)
    })
}

fn label_of(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn type_text(cx: &mut TestAppContext, fixture: &Fixture, text: &'static str) {
    in_frame(cx, fixture.handle, move |window, cx| window.input(text, cx));
}

fn remote_path(path: &str) -> RemotePath {
    RemotePath::new(path).unwrap()
}

/// 等工作线程收到 `count` 个操作，返回它们。
fn wait_for_operations(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    count: usize,
) -> Vec<RemoteOperation> {
    let done = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.operations.lock().unwrap().len() >= count
    });
    assert!(done, "{:?}", fixture.sftp.operations.lock().unwrap());
    fixture.sftp.operations.lock().unwrap().clone()
}

/// 操作答完，这一侧不再忙。
fn wait_not_busy(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) {
    let tab = tab(cx, fixture);
    let done = wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| {
            let card = tab.read(cx).card(CardSide::from_remote(remote)).read(cx);
            !card.is_busy() && !card.is_loading()
        })
    });
    assert!(done, "还在忙");
}

fn local_call(cx: &mut TestAppContext, fixture: &Fixture, call: String) {
    let done = wait_until(cx, fixture.handle, |_| {
        fixture.local.calls.lock().unwrap().contains(&call)
    });
    assert!(
        done,
        "{call} 不在 {:?}",
        fixture.local.calls.lock().unwrap()
    );
}

fn in_downloads(name: &str) -> PathBuf {
    Path::new(LOCAL_DOWNLOADS).join(name)
}

// ---------- 新建 ----------

#[gpui_kit::test]
fn a_new_folder_asks_for_a_name_and_is_created_in_this_directory(cx: &mut TestAppContext) {
    let fixture = open(cx);

    command(
        cx,
        &fixture,
        SftpTabCommand::New {
            side: CardSide::Remote,
            kind: NewEntryKind::Folder,
        },
    );
    // 名字已经填好、选中：打字就替换。
    assert_eq!(
        value_of(cx, &fixture, "entry-name").as_deref(),
        Some("New Folder")
    );
    let listed_before = fixture.sftp.listed.lock().unwrap().len();
    type_text(cx, &fixture, "logs");
    click(cx, &fixture, "entry-name-commit");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::CreateDirectory {
            path: remote_path("/home/tester/logs")
        }]
    );
    assert!(!visible(cx, &fixture, "entry-name"));
    // 建好以后重读这个目录。
    let reread = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.listed.lock().unwrap().len() > listed_before
    });
    assert!(reread);
}

#[gpui_kit::test]
fn f7_also_opens_it_and_the_local_side_creates_on_its_own_disk(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::Local,
        },
    );

    press(cx, &fixture, "f7");
    type_text(cx, &fixture, "本地目录");
    click(cx, &fixture, "entry-name-commit");

    let path = in_downloads("本地目录");
    local_call(cx, &fixture, format!("mkdir {}", path.display()));
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
    wait_not_busy(cx, &fixture, false);
}

#[gpui_kit::test]
fn a_new_file_is_selected_once_it_shows_and_enter_commits(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            side: CardSide::Remote,
            path: "/pictures".into(),
        },
    );
    wait_for_path(cx, &fixture, true, "/pictures");

    command(
        cx,
        &fixture,
        SftpTabCommand::New {
            side: CardSide::Remote,
            kind: NewEntryKind::File,
        },
    );
    assert_eq!(
        value_of(cx, &fixture, "entry-name").as_deref(),
        Some("New File.txt")
    );
    type_text(cx, &fixture, "a.txt");
    press(cx, &fixture, "enter");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::CreateFile {
            path: remote_path("/pictures/a.txt")
        }]
    );
    let shown = wait_until(cx, fixture.handle, |cx| {
        selected(cx, &fixture, true) == ["a.txt"]
    });
    assert!(shown, "{:?}", names(cx, &fixture, true));
}

#[gpui_kit::test]
fn a_bad_name_is_said_under_the_field_and_the_dialog_stays(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::New {
            side: CardSide::Remote,
            kind: NewEntryKind::Folder,
        },
    );

    type_text(cx, &fixture, "目录");
    click(cx, &fixture, "entry-name-commit");
    assert_eq!(
        label_of(cx, &fixture, "entry-name-error").as_deref(),
        Some("An item named \"目录\" already exists")
    );
    assert!(visible(cx, &fixture, "entry-name"));
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());

    // 改了字，错误就收起来。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("entry-name", cx)
    });
    type_text(cx, &fixture, "2");
    assert!(!visible(cx, &fixture, "entry-name-error"));

    // Esc：什么都不做。
    press(cx, &fixture, "escape");
    assert!(!visible(cx, &fixture, "entry-name"));
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
}

// ---------- 重命名 ----------

#[gpui_kit::test]
fn f2_renames_the_selected_file_and_the_list_keeps_the_keyboard(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");

    press(cx, &fixture, "f2");
    assert_eq!(
        value_of(cx, &fixture, "entry-name").as_deref(),
        Some("文件 甲.txt")
    );
    type_text(cx, &fixture, "丙.txt");
    click(cx, &fixture, "entry-name-commit");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::Rename {
            from: remote_path("/home/tester/文件 甲.txt"),
            to: remote_path("/home/tester/丙.txt"),
        }]
    );
    wait_not_busy(cx, &fixture, true);
    // 焦点回到列表：快捷键接着能用。
    press(cx, &fixture, "ctrl-a");
    assert_eq!(selected(cx, &fixture, true).len(), 4);
}

#[gpui_kit::test]
fn f2_with_several_selected_does_nothing_and_the_same_name_is_refused(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::SelectAll {
            side: CardSide::Remote,
        },
    );
    command(
        cx,
        &fixture,
        SftpTabCommand::Rename {
            side: CardSide::Remote,
        },
    );
    assert!(!visible(cx, &fixture, "entry-name"));

    select(cx, &fixture, true, "文件 甲.txt");
    press(cx, &fixture, "f2");
    click(cx, &fixture, "entry-name-commit");
    assert_eq!(
        label_of(cx, &fixture, "entry-name-error").as_deref(),
        Some("The name did not change")
    );
}

#[gpui_kit::test]
fn a_rename_on_the_local_side_goes_to_the_local_files(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, false, "文件 乙.txt");

    press(cx, &fixture, "f2");
    type_text(cx, &fixture, "丁.txt");
    press(cx, &fixture, "enter");

    let (from, to) = (in_downloads("文件 乙.txt"), in_downloads("丁.txt"));
    local_call(
        cx,
        &fixture,
        format!("rename {} -> {}", from.display(), to.display()),
    );
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
}

// ---------- 删除 ----------

#[gpui_kit::test]
fn delete_asks_first_and_cancel_keeps_the_file(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");

    press(cx, &fixture, "delete");
    assert_eq!(
        label_of(cx, &fixture, "file-delete-text").as_deref(),
        Some("Delete \"文件 甲.txt\"?\nRemote files cannot be recovered; a folder is deleted with all of its contents.")
    );
    click(cx, &fixture, "file-delete-cancel");
    assert!(!visible(cx, &fixture, "file-delete"));
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());

    press(cx, &fixture, "delete");
    click(cx, &fixture, "file-delete-confirm");
    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::Delete {
            paths: vec![remote_path("/home/tester/文件 甲.txt")]
        }]
    );
    wait_not_busy(cx, &fixture, true);
    // 焦点回到列表。
    press(cx, &fixture, "ctrl-a");
    assert_eq!(selected(cx, &fixture, true).len(), 4);
}

#[gpui_kit::test]
fn delete_takes_every_selected_item_but_never_the_parent_row(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::Remote,
        },
    );
    // 只点了 `..`：没有选中项，不问。
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").click("name-cell:..", cx)
    });
    press(cx, &fixture, "delete");
    assert!(!visible(cx, &fixture, "file-delete"));

    press(cx, &fixture, "ctrl-a");
    press(cx, &fixture, "delete");
    assert!(
        label_of(cx, &fixture, "file-delete-text")
            .unwrap()
            .starts_with("Delete the 4 selected items?")
    );
    click(cx, &fixture, "file-delete-confirm");
    let [RemoteOperation::Delete { paths }] = &wait_for_operations(cx, &fixture, 1)[..] else {
        panic!("不是一次删除");
    };
    assert_eq!(paths.len(), 4);
}

#[gpui_kit::test]
fn a_local_delete_goes_to_the_trash_and_says_so(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, false, "文件 甲.txt");

    press(cx, &fixture, "delete");
    let text = label_of(cx, &fixture, "file-delete-text").unwrap();
    assert!(text.ends_with(if cfg!(windows) {
        "The items will be moved to the Recycle Bin."
    } else {
        "The items will be moved to the Trash."
    }));
    click(cx, &fixture, "file-delete-confirm");

    let paths = vec![in_downloads("文件 甲.txt")];
    local_call(cx, &fixture, format!("trash {paths:?}"));
    wait_not_busy(cx, &fixture, false);
}

// ---------- 刷新、失败、断开 ----------

#[gpui_kit::test]
fn ctrl_r_reads_the_directory_of_this_side_again(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let (remote_before, local_before) = (
        fixture.sftp.listed.lock().unwrap().len(),
        fixture.local.listed.lock().unwrap().len(),
    );

    command(
        cx,
        &fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::Remote,
        },
    );
    press(cx, &fixture, "ctrl-r");
    let reread = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.listed.lock().unwrap().len() == remote_before + 1
    });
    assert!(reread);
    assert_eq!(fixture.local.listed.lock().unwrap().len(), local_before);

    command(
        cx,
        &fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::Local,
        },
    );
    press(cx, &fixture, "ctrl-r");
    let reread = wait_until(cx, fixture.handle, |_| {
        fixture.local.listed.lock().unwrap().len() == local_before + 1
    });
    assert!(reread);
}

#[gpui_kit::test]
fn a_refused_operation_is_a_notification_and_the_directory_is_read_again(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");
    let listed_before = fixture.sftp.listed.lock().unwrap().len();

    press(cx, &fixture, "f2");
    type_text(cx, &fixture, "denied.txt");
    click(cx, &fixture, "entry-name-commit");

    let told = wait_until(cx, fixture.handle, |cx| {
        in_frame(cx, fixture.handle, |window, cx| {
            window.notifications(cx).len() == 1
        })
    });
    assert!(told, "没有通知");
    let reread = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.listed.lock().unwrap().len() > listed_before
    });
    assert!(reread);
    wait_not_busy(cx, &fixture, true);
}

#[gpui_kit::test]
fn while_an_operation_runs_the_side_takes_no_other_and_a_lost_connection_ends_the_wait(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");
    press(cx, &fixture, "f2");
    // 假工作线程不回答名字里有这个的操作。
    type_text(cx, &fixture, "hold-answer");
    click(cx, &fixture, "entry-name-commit");
    wait_for_operations(cx, &fixture, 1);
    let tab = tab(cx, &fixture);
    assert!(cx.update(|cx| tab.read(cx).remote().read(cx).is_busy()));

    press(cx, &fixture, "f2");
    assert!(!visible(cx, &fixture, "entry-name"));
    press(cx, &fixture, "delete");
    assert!(!visible(cx, &fixture, "file-delete"));

    fixture
        .sftp
        .emit(SftpEvent::Disconnected("连接超时".into()));
    let free = wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| !tab.read(cx).remote().read(cx).is_busy())
    });
    assert!(free, "断开以后还在等");

    // 断开时要连接的命令先问要不要重新连接，本地一侧不受影响。
    command(
        cx,
        &fixture,
        SftpTabCommand::Delete {
            side: CardSide::Remote,
        },
    );
    assert!(visible(cx, &fixture, "sftp-reconnect"));
    click(cx, &fixture, "sftp-reconnect-cancel");
    select(cx, &fixture, false, "文件 甲.txt");
    press(cx, &fixture, "delete");
    assert!(visible(cx, &fixture, "file-delete"));
}

#[gpui_kit::test]
fn a_dialog_older_than_the_connection_sends_nothing(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");
    press(cx, &fixture, "delete");
    let tab = tab(cx, &fixture);
    let generation = cx.update(|cx| tab.read(cx).generation());

    fixture.sftp.emit(SftpEvent::Connecting);
    fixture.sftp.emit(SftpEvent::Connected {
        home: remote_path("/home/tester"),
    });
    let moved = wait_until(cx, fixture.handle, |cx| {
        cx.update(|cx| tab.read(cx).generation()) != generation
    });
    assert!(moved);
    wait_not_busy(cx, &fixture, true);

    click(cx, &fixture, "file-delete-confirm");
    in_frame(cx, fixture.handle, |_, _| {});
    wait_not_busy(cx, &fixture, true);
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
}

// ---------- 属性 ----------

fn perm(index: usize) -> ElementId {
    ("perm", index).into()
}

#[gpui_kit::test]
fn f9_shows_the_rights_and_a_checkbox_changes_only_that_bit(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");

    press(cx, &fixture, "f9");
    assert_eq!(
        label_of(cx, &fixture, "file-properties-location").as_deref(),
        Some("/home/tester")
    );
    assert_eq!(
        label_of(cx, &fixture, "file-properties-owner").as_deref(),
        Some("root : wheel")
    );
    assert_eq!(value_of(cx, &fixture, "perm-octal").as_deref(), Some("644"));
    // 文件没有递归。
    assert!(!visible(cx, &fixture, "perm-recursive"));

    // 所有者的执行。
    click(cx, &fixture, perm(2));
    assert_eq!(value_of(cx, &fixture, "perm-octal").as_deref(), Some("744"));
    click(cx, &fixture, "perm-commit");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::SetPermissions {
            paths: vec![remote_path("/home/tester/文件 甲.txt")],
            edit: PermissionEdit::new(0o100, 0),
            recursive: false,
            add_x_to_dirs: false,
        }]
    );
    assert!(!visible(cx, &fixture, "file-properties"));
}

#[gpui_kit::test]
fn typing_the_octal_replaces_all_nine_bits(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");

    press(cx, &fixture, "f9");
    // 打开时八进制框有焦点、全选。
    type_text(cx, &fixture, "600");
    press(cx, &fixture, "enter");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::SetPermissions {
            paths: vec![remote_path("/home/tester/文件 甲.txt")],
            edit: PermissionEdit::exact(0o600),
            recursive: false,
            add_x_to_dirs: false,
        }]
    );
}

#[gpui_kit::test]
fn nothing_changed_sends_nothing(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");
    press(cx, &fixture, "f9");
    click(cx, &fixture, "perm-commit");
    assert!(!visible(cx, &fixture, "file-properties"));
    let tab = tab(cx, &fixture);
    assert!(!cx.update(|cx| tab.read(cx).remote().read(cx).is_busy()));
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
}

#[gpui_kit::test]
fn a_folder_can_be_applied_recursively_and_gains_x_by_default(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "目录");
    press(cx, &fixture, "f9");
    assert_eq!(value_of(cx, &fixture, "perm-octal").as_deref(), Some("755"));

    click(cx, &fixture, "perm-recursive");
    // 其他的读：去掉。
    click(cx, &fixture, perm(6));
    click(cx, &fixture, "perm-commit");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::SetPermissions {
            paths: vec![remote_path("/home/tester/目录")],
            edit: PermissionEdit::new(0, 0o004),
            recursive: true,
            add_x_to_dirs: true,
        }]
    );
}

#[gpui_kit::test]
fn x_for_folders_can_be_turned_off(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "目录");
    press(cx, &fixture, "f9");
    click(cx, &fixture, "perm-recursive");
    click(cx, &fixture, "perm-dir-x");
    click(cx, &fixture, perm(6));
    click(cx, &fixture, "perm-commit");
    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::SetPermissions {
            paths: vec![remote_path("/home/tester/目录")],
            edit: PermissionEdit::new(0, 0o004),
            recursive: true,
            add_x_to_dirs: false,
        }]
    );
}

#[gpui_kit::test]
fn with_several_items_differing_bits_stay_and_links_take_no_part(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(
        cx,
        &fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::Remote,
        },
    );
    press(cx, &fixture, "ctrl-a");
    press(cx, &fixture, "f9");

    // 目录 755，两个文件 644：三位都有不一致的位（链接不算）。
    assert_eq!(value_of(cx, &fixture, "perm-octal").as_deref(), Some("???"));
    // 所有者的执行是混合的：点了就勾上，其他混合的位不动。
    click(cx, &fixture, perm(2));
    assert_eq!(value_of(cx, &fixture, "perm-octal").as_deref(), Some("7??"));
    click(cx, &fixture, "perm-commit");

    assert_eq!(
        wait_for_operations(cx, &fixture, 1),
        [RemoteOperation::SetPermissions {
            paths: vec![
                remote_path("/home/tester/目录"),
                remote_path("/home/tester/文件 乙.txt"),
                remote_path("/home/tester/文件 甲.txt"),
            ],
            edit: PermissionEdit::new(0o100, 0),
            recursive: false,
            add_x_to_dirs: false,
        }]
    );
}

#[gpui_kit::test]
fn a_link_alone_has_nothing_to_change(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "链接目录");
    press(cx, &fixture, "f9");
    assert!(visible(cx, &fixture, "file-properties"));
    click(cx, &fixture, "perm-commit");
    assert!(fixture.sftp.operations.lock().unwrap().is_empty());
}

#[gpui_kit::test]
fn properties_on_the_local_side_follow_what_the_system_can_do(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, false, "文件 甲.txt");
    press(cx, &fixture, "f9");
    if cfg!(windows) {
        // Windows 本地不能改权限：说一声，不开框。
        assert!(!visible(cx, &fixture, "file-properties"));
        let told = in_frame(cx, fixture.handle, |window, cx| {
            window.notifications(cx).len()
        });
        assert_eq!(told, 1);
    } else {
        assert!(visible(cx, &fixture, "file-properties"));
        // 本地没有所有者这一行。
        assert!(!visible(cx, &fixture, "file-properties-owner"));
        click(cx, &fixture, perm(2));
        click(cx, &fixture, "perm-commit");
        let paths = vec![in_downloads("文件 甲.txt")];
        local_call(
            cx,
            &fixture,
            format!("chmod {paths:?} +100 -0 recursive=false x=false"),
        );
    }
}

// ---------- 隐藏文件 ----------

fn is_listed(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, name: &str) -> bool {
    names(cx, fixture, remote).iter().any(|shown| shown == name)
}

#[gpui_kit::test]
fn hidden_files_are_always_shown(cx: &mut TestAppContext) {
    let fixture = open(cx);
    for remote in [true, false] {
        command(
            cx,
            &fixture,
            SftpTabCommand::Navigate {
                side: CardSide::from_remote(remote),
                path: "/dotfiles".into(),
            },
        );
        wait_for_path(cx, &fixture, remote, "/dotfiles");
    }
    // 本地远程都显示点文件，没有开关可以藏起来。
    assert!(is_listed(cx, &fixture, false, ".bashrc"));
    assert!(is_listed(cx, &fixture, true, ".bashrc"));
    // 状态栏不写隐藏了几项（26 号）。
    assert!(
        !label_of(cx, &fixture, "local-status")
            .unwrap()
            .contains("hidden")
    );
    // 隐藏文件能认出来：行和图标按它变淡。
    let tab = tab(cx, &fixture);
    cx.update(|cx| {
        let entries = tab.read(cx).local().read(cx).entries(cx);
        let hidden: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is_hidden())
            .map(|entry| entry.name.to_string())
            .collect();
        assert_eq!(hidden, [".ssh", ".bashrc"]);
    });
}

// ---------- 列和排序 ----------

/// 假服务器的 `/pictures`，三个大小不同的文件。按字符串排「900 B」会在「2.0 KB」后面。
fn open_sized_files(cx: &mut TestAppContext) -> Fixture {
    let fixture = open(cx);
    fixture.sftp.files.lock().unwrap().extend([
        ("/pictures/a.png".to_string(), (vec![0; 900], 300)),
        ("/pictures/b.png".to_string(), (vec![0; 2048], 200)),
        ("/pictures/c.png".to_string(), (vec![0; 1024 * 1024], 100)),
    ]);
    command(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            side: CardSide::Remote,
            path: "/pictures".into(),
        },
    );
    wait_for_path(cx, &fixture, true, "/pictures");
    fixture
}

fn click_column(cx: &mut TestAppContext, fixture: &Fixture, column: &'static str) {
    in_frame(cx, fixture.handle, move |window, cx| {
        window.within("remote-card").click(column, cx)
    });
}

#[gpui_kit::test]
fn sizes_are_short_and_sorting_by_size_uses_the_bytes(cx: &mut TestAppContext) {
    let fixture = open_sized_files(cx);
    let handle = fixture.handle;
    let size = |cx: &mut TestAppContext, name: &'static str| {
        in_frame(cx, handle, move |window, _| {
            window
                .within("remote-card")
                .find(format!("size:{name}"))
                .label()
                .map(str::to_owned)
        })
    };
    assert_eq!(size(cx, "a.png").as_deref(), Some("900 B"));
    assert_eq!(size(cx, "b.png").as_deref(), Some("2.0 KB"));
    assert_eq!(size(cx, "c.png").as_deref(), Some("1.0 MB"));

    // 点列标题：降序、升序、回到按名称。
    click_column(cx, &fixture, "column:size");
    assert_eq!(names(cx, &fixture, true), ["..", "c.png", "b.png", "a.png"]);
    click_column(cx, &fixture, "column:size");
    assert_eq!(names(cx, &fixture, true), ["..", "a.png", "b.png", "c.png"]);
    click_column(cx, &fixture, "column:size");
    assert_eq!(names(cx, &fixture, true), ["..", "a.png", "b.png", "c.png"]);
}

#[gpui_kit::test]
fn sorting_by_time_survives_a_refresh(cx: &mut TestAppContext) {
    let fixture = open_sized_files(cx);
    click_column(cx, &fixture, "column:modified");
    click_column(cx, &fixture, "column:modified");
    // 升序：最早的在前。
    assert_eq!(names(cx, &fixture, true), ["..", "c.png", "b.png", "a.png"]);

    let before = fixture.sftp.listed.lock().unwrap().len();
    command(
        cx,
        &fixture,
        SftpTabCommand::Refresh {
            side: CardSide::Remote,
        },
    );
    let reread = wait_until(cx, fixture.handle, |_| {
        fixture.sftp.listed.lock().unwrap().len() > before
    });
    assert!(reread);
    wait_not_busy(cx, &fixture, true);
    assert_eq!(names(cx, &fixture, true), ["..", "c.png", "b.png", "a.png"]);
}

#[gpui_kit::test]
fn folders_stay_ahead_of_files_whichever_way_it_sorts(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let folders = ["目录", "链接目录"];
    for _ in 0..2 {
        click_column(cx, &fixture, "column:size");
        let rows = names(cx, &fixture, true);
        assert!(
            rows[1..3]
                .iter()
                .all(|name| folders.contains(&name.as_str()))
        );
    }
}

// ---------- 菜单上写的按键 ----------

#[gpui_kit::test]
fn the_menu_commands_show_the_keys_of_the_prototype(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let tab = tab(cx, &fixture);
    let list = cx.update(|cx| tab.read(cx).remote().read(cx).focus_handle(cx));
    let remote = true;
    let cases = [
        (
            SftpTabCommand::Delete {
                side: CardSide::from_remote(remote),
            },
            "delete",
        ),
        (
            SftpTabCommand::Rename {
                side: CardSide::from_remote(remote),
            },
            "f2",
        ),
        (
            SftpTabCommand::Properties {
                side: CardSide::from_remote(remote),
            },
            "f9",
        ),
        (
            SftpTabCommand::Refresh {
                side: CardSide::from_remote(remote),
            },
            "r",
        ),
        (
            SftpTabCommand::Up {
                side: CardSide::from_remote(remote),
            },
            "backspace",
        ),
    ];
    in_frame(cx, fixture.handle, move |window, _| {
        for (command, key) in cases {
            let action = SftpShortcut(command);
            let binding = window
                .highest_precedence_binding_for_action_in(&action, &list)
                .unwrap_or_else(|| panic!("{action:?} 没有按键"));
            assert_eq!(binding.keystrokes()[0].as_keystroke().key, key);
        }
    });
}

// ---------- 点行、右键菜单（UI 重做工单 10，26 号「SFTP」） ----------

/// 菜单里文字是 `text` 的那一项。
fn menu_item(cx: &mut TestAppContext, fixture: &Fixture, text: &'static str) -> Option<usize> {
    in_frame(cx, fixture.handle, move |window, _| {
        let popup = window.within("popup-menu");
        (0..20_usize).find(|item| {
            popup
                .try_find(*item)
                .is_some_and(|item| item.label() == Some(text))
        })
    })
}

/// 整行是一个项目：点大小那一格也选中这一行。
#[gpui_kit::test]
fn a_click_anywhere_on_a_row_selects_it(cx: &mut TestAppContext) {
    let fixture = open(cx);
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").click("size:文件 甲.txt", cx)
    });
    assert_eq!(selected(cx, &fixture, true), ["文件 甲.txt"]);
}

/// 右键一行先选中它，开文件的菜单（编辑在最前，删除是红字的那项）；右键 `..` 当空白处，
/// 开空白处的菜单，「显示隐藏文件」是勾选项。
#[gpui_kit::test]
fn a_right_click_selects_the_row_and_blank_space_has_its_own_menu(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "目录");
    in_frame(cx, fixture.handle, |window, cx| {
        window
            .within("remote-card")
            .right_click("size:文件 甲.txt", cx)
    });
    assert_eq!(selected(cx, &fixture, true), ["文件 甲.txt"]);
    assert_eq!(menu_item(cx, &fixture, "Edit"), Some(0));
    assert!(menu_item(cx, &fixture, "Delete").is_some());
    assert_eq!(menu_item(cx, &fixture, "New Folder…"), None);
    press(cx, &fixture, "escape");
    assert!(!visible(cx, &fixture, "popup-menu"));

    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").right_click("name-cell:..", cx)
    });
    assert_eq!(menu_item(cx, &fixture, "New Folder…"), Some(0));
    // 隐藏文件一直显示，菜单里没有「显示隐藏文件」。
    assert_eq!(menu_item(cx, &fixture, "Show Hidden Files"), None);
}
