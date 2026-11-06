//! 预览（工单 24）：图片、Markdown、下载、编辑和打开规则。连接和本地目录都是假的。
// 参考 since2006/shell-rs @ b2a307a: tests/ui/editor.rs 的预览部分

use std::time::Duration;

use gpui_kit::ElementId;
use qishell::app::{OpenHostSftp, SftpAction, SftpTabCommand};
use qishell::sftp_tab::SftpTab;

use crate::support::*;

/// 1×1 的 PNG，70 字节。
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0d\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\x89\x99\x3d\x1d\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

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

/// 服务器的 `/pictures` 里有这些文件：打开主机 1 的 SFTP 标签，远程一侧进这个目录。
fn open_pictures(cx: &mut TestAppContext, files: &[(&str, &[u8])]) -> Fixture {
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
    let fixture = Fixture {
        handle,
        workspace,
        sftp,
        local,
    };
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
    command(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            remote: true,
            path: "/pictures".into(),
        },
    );
    assert!(
        wait_until(cx, handle, |cx| shown(cx, "remote-path").as_deref()
            == Some("/pictures")),
        "没有进 /pictures"
    );
    fixture
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

fn command(cx: &mut TestAppContext, fixture: &Fixture, command: SftpTabCommand) {
    let tab = sftp_tab(cx, fixture);
    let id = cx.update(|cx| tab.read(cx).id());
    dispatch(cx, fixture.handle, SftpAction::new(id, command));
}

fn row(name: &str) -> ElementId {
    ElementId::Name(format!("name:{name}").into())
}

fn double_click_row(cx: &mut TestAppContext, fixture: &Fixture, name: &str) {
    let row = row(name);
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").double_click(row, cx)
    });
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

// ---------- 打开规则和图片 ----------

#[gpui_kit::test]
fn double_clicking_an_image_previews_it(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    double_click_row(cx, &fixture, "图.png");
    wait_visible(cx, &fixture, "preview-dialog");
    assert_eq!(label(cx, &fixture, "preview-dialog"), "图.png");
    assert!(visible(cx, &fixture, "preview-image"));
    // 写着它在哪、多大。
    let info = label(cx, &fixture, "preview-info");
    assert!(info.contains("/pictures"), "{info}");
    assert!(info.contains("70 B"), "{info}");
    // 远程的图片能下载，不能编辑。
    assert!(visible(cx, &fixture, "preview-download"));
    assert!(!visible(cx, &fixture, "preview-edit"));
    // 没有开编辑器，也没有「无法用编辑器打开」。
    assert!(!visible(cx, &fixture, "editor-refused"));
    assert_eq!(cx.update(|cx| fixture.workspace.read(cx).tab_ids().len()), 1);
    assert_eq!(
        *fixture.sftp.reads.lock().unwrap(),
        vec!["/pictures/图.png".to_string()]
    );

    in_frame(cx, fixture.handle, |window, cx| window.press("escape", cx));
    assert!(!visible(cx, &fixture, "preview-dialog"));
}
