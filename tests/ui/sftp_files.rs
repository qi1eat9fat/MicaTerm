//! SFTP 文件操作（工单 21）：新建、重命名、删除、属性、隐藏文件、排序。
//! 连接和本地目录都是假的（`FakeSftpProvider`、`FakeLocalDirectory`），真协议在 backend 的测试里。

use std::path::{Path, PathBuf};
use std::time::Duration;

use qishell::app::{OpenHostSftp, SftpAction, SftpTabCommand, ToggleHiddenFiles};
use qishell::backend::sftp::{PermissionEdit, RemoteOperation, RemotePath, SftpEvent};
use qishell::sftp_tab::{CardSide, NewEntryKind, SftpTab};

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

fn tab(cx: &mut TestAppContext, fixture: &Fixture) -> Entity<SftpTab> {
    cx.update(|cx| fixture.workspace.read(cx).active_sftp_tab())
        .expect("an SFTP tab in front")
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
            let card = tab.read(cx).card(remote).read(cx);
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
            .card(remote)
            .read(cx)
            .entries(cx)
            .iter()
            .map(|entry| entry.name.to_string())
            .collect()
    })
}

fn selected(cx: &mut TestAppContext, fixture: &Fixture, remote: bool) -> Vec<String> {
    let tab = tab(cx, fixture);
    cx.update(|cx| tab.read(cx).card(remote).read(cx).selected_names(cx))
}

fn card_id(remote: bool) -> &'static str {
    if remote { "remote-card" } else { "local-card" }
}

/// 焦点放进这一侧的列表，只选中 `name`。
fn select(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, name: &str) {
    command(cx, fixture, SftpTabCommand::FocusCard { remote });
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
            let card = tab.read(cx).card(remote).read(cx);
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
            remote: true,
            kind: NewEntryKind::Folder,
        },
    );
    // 名字已经填好、选中：打字就替换。
    assert_eq!(
        value_of(cx, &fixture, "entry-name").as_deref(),
        Some("新建文件夹")
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
