//! 预览（工单 24）：图片、Markdown、下载、编辑和打开规则。连接和本地目录都是假的。
// 参考 since2006/shell-rs @ b2a307a: tests/ui/editor.rs 的预览部分

use std::time::Duration;

use gpui_kit::{
    Bounds, ElementId, InputEvent as _, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent,
    TouchPhase, point,
};
use qishell::app::{OpenHostSftp, SftpAction, SftpTabCommand};
use qishell::sftp_tab::{IMAGE_LIMIT, SftpTab};

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
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_ids().len()),
        1
    );
    assert_eq!(
        *fixture.sftp.reads.lock().unwrap(),
        vec!["/pictures/图.png".to_string()]
    );

    in_frame(cx, fixture.handle, |window, cx| window.press("escape", cx));
    assert!(!visible(cx, &fixture, "preview-dialog"));
}

/// 4×2000 的 PNG：很长的截图，比什么画框都高。
const LONG_PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x04\x00\x00\x07\xd0\x08\x06\x00\x00\x00\xe5\xf9\xfe\xc9\x00\x00\x00\x76\x49\x44\x41\x54\x78\xda\xed\xc8\x31\x0d\x00\x20\x0c\x00\xb0\x0d\x35\x68\x9a\x62\x5c\xb1\x77\xc1\x01\x49\x7b\x36\x77\x9d\x1b\xc3\x8a\x87\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\x42\x08\x21\x84\x10\xe2\x97\x68\xe4\x83\x11\xe9\xf4\xb8\x3d\xbb\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

/// 2×3 的 JPEG。
const JPEG: &[u8] = b"\xff\xd8\xff\xe0\x00\x10\x4a\x46\x49\x46\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00\xff\xdb\x00\x43\x00\x10\x0b\x0c\x0e\x0c\x0a\x10\x0e\x0d\x0e\x12\x11\x10\x13\x18\x28\x1a\x18\x16\x16\x18\x31\x23\x25\x1d\x28\x3a\x33\x3d\x3c\x39\x33\x38\x37\x40\x48\x5c\x4e\x40\x44\x57\x45\x37\x38\x50\x6d\x51\x57\x5f\x62\x67\x68\x67\x3e\x4d\x71\x79\x70\x64\x78\x5c\x65\x67\x63\xff\xdb\x00\x43\x01\x11\x12\x12\x18\x15\x18\x2f\x1a\x1a\x2f\x63\x42\x38\x42\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\x63\xff\xc0\x00\x11\x08\x00\x03\x00\x02\x03\x01\x22\x00\x02\x11\x01\x03\x11\x01\xff\xc4\x00\x1f\x00\x00\x01\x05\x01\x01\x01\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\xff\xc4\x00\xb5\x10\x00\x02\x01\x03\x03\x02\x04\x03\x05\x05\x04\x04\x00\x00\x01\x7d\x01\x02\x03\x00\x04\x11\x05\x12\x21\x31\x41\x06\x13\x51\x61\x07\x22\x71\x14\x32\x81\x91\xa1\x08\x23\x42\xb1\xc1\x15\x52\xd1\xf0\x24\x33\x62\x72\x82\x09\x0a\x16\x17\x18\x19\x1a\x25\x26\x27\x28\x29\x2a\x34\x35\x36\x37\x38\x39\x3a\x43\x44\x45\x46\x47\x48\x49\x4a\x53\x54\x55\x56\x57\x58\x59\x5a\x63\x64\x65\x66\x67\x68\x69\x6a\x73\x74\x75\x76\x77\x78\x79\x7a\x83\x84\x85\x86\x87\x88\x89\x8a\x92\x93\x94\x95\x96\x97\x98\x99\x9a\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xff\xc4\x00\x1f\x01\x00\x03\x01\x01\x01\x01\x01\x01\x01\x01\x01\x00\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\xff\xc4\x00\xb5\x11\x00\x02\x01\x02\x04\x04\x03\x04\x07\x05\x04\x04\x00\x01\x02\x77\x00\x01\x02\x03\x11\x04\x05\x21\x31\x06\x12\x41\x51\x07\x61\x71\x13\x22\x32\x81\x08\x14\x42\x91\xa1\xb1\xc1\x09\x23\x33\x52\xf0\x15\x62\x72\xd1\x0a\x16\x24\x34\xe1\x25\xf1\x17\x18\x19\x1a\x26\x27\x28\x29\x2a\x35\x36\x37\x38\x39\x3a\x43\x44\x45\x46\x47\x48\x49\x4a\x53\x54\x55\x56\x57\x58\x59\x5a\x63\x64\x65\x66\x67\x68\x69\x6a\x73\x74\x75\x76\x77\x78\x79\x7a\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x92\x93\x94\x95\x96\x97\x98\x99\x9a\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xff\xda\x00\x0c\x03\x01\x00\x02\x11\x03\x11\x00\x3f\x00\xcb\xa2\x8a\x2b\x88\xfa\x63\xff\xd9";
/// 2×3 的 GIF。
const GIF: &[u8] = b"\x47\x49\x46\x38\x37\x61\x02\x00\x03\x00\x81\x00\x00\xc8\x28\x28\x00\x00\x00\x00\x00\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x02\x00\x03\x00\x00\x08\x06\x00\x01\x08\x1c\x18\x10\x00\x3b";
/// 2×3 的 WebP。
const WEBP: &[u8] = b"\x52\x49\x46\x46\x1e\x00\x00\x00\x57\x45\x42\x50\x56\x50\x38\x4c\x11\x00\x00\x00\x2f\x01\x80\x00\x00\x07\x50\x94\x22\x17\xa5\xff\x81\x88\xe8\x7f\x00\x00";
/// 2×3 的 BMP。
const BMP: &[u8] = b"\x42\x4d\x4e\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00\x28\x00\x00\x00\x02\x00\x00\x00\x03\x00\x00\x00\x01\x00\x18\x00\x00\x00\x00\x00\x18\x00\x00\x00\xc4\x0e\x00\x00\xc4\x0e\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x28\x28\xc8\x28\x28\xc8\x00\x00\x28\x28\xc8\x28\x28\xc8\x00\x00\x28\x28\xc8\x28\x28\xc8\x00\x00";
/// 2×3 的 TIFF。
const TIFF: &[u8] = b"\x49\x49\x2a\x00\x08\x00\x00\x00\x0b\x00\x00\x01\x04\x00\x01\x00\x00\x00\x02\x00\x00\x00\x01\x01\x04\x00\x01\x00\x00\x00\x03\x00\x00\x00\x02\x01\x03\x00\x04\x00\x00\x00\x92\x00\x00\x00\x03\x01\x03\x00\x01\x00\x00\x00\x01\x00\x00\x00\x06\x01\x03\x00\x01\x00\x00\x00\x02\x00\x00\x00\x11\x01\x04\x00\x01\x00\x00\x00\x9a\x00\x00\x00\x15\x01\x03\x00\x01\x00\x00\x00\x04\x00\x00\x00\x16\x01\x04\x00\x01\x00\x00\x00\x03\x00\x00\x00\x17\x01\x04\x00\x01\x00\x00\x00\x18\x00\x00\x00\x1c\x01\x03\x00\x01\x00\x00\x00\x01\x00\x00\x00\x52\x01\x03\x00\x01\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00\x08\x00\x08\x00\x08\x00\x08\x00\xc8\x28\x28\xff\xc8\x28\x28\xff\xc8\x28\x28\xff\xc8\x28\x28\xff\xc8\x28\x28\xff\xc8\x28\x28\xff";
/// 2×3 的 ICO（里面是 PNG）。
const ICO: &[u8] = b"\x00\x00\x01\x00\x01\x00\x02\x03\x00\x00\x01\x00\x20\x00\x4e\x00\x00\x00\x16\x00\x00\x00\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x15\x49\x44\x41\x54\x78\x9c\x63\x3c\xa1\xa1\xf1\x9f\x81\x81\x81\x81\x09\x44\xa0\x30\x00\x33\x4d\x02\x1d\xc3\xc8\xc2\x1a\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
/// 2×3 的 PNG。
const PNG_2X3: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x15\x49\x44\x41\x54\x78\x9c\x63\x3c\xa1\xa1\xf1\x9f\x81\x81\x81\x81\x09\x44\xa0\x30\x00\x33\x4d\x02\x1d\xc3\xc8\xc2\x1a\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

fn bounds_of(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Bounds<Pixels> {
    in_frame(cx, fixture.handle, move |window, _| {
        window.find(id).bounds()
    })
}

fn click(cx: &mut TestAppContext, fixture: &Fixture, id: impl Into<ElementId>) {
    in_frame(cx, fixture.handle, |window, cx| window.click(id, cx));
}

fn press(cx: &mut TestAppContext, fixture: &Fixture, key: &str) {
    in_frame(cx, fixture.handle, |window, cx| window.press(key, cx));
}

/// 等图片解码好、画框量好，返回现在的倍数。
fn wait_for_scale(cx: &mut TestAppContext, fixture: &Fixture) -> String {
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            visible(cx, fixture, "preview-picture")
                && !label(cx, fixture, "preview-zoom").is_empty()
        }),
        "图片没有显示出来：picture={} zoom={:?} info={:?} note={}",
        visible(cx, fixture, "preview-picture"),
        label(cx, fixture, "preview-zoom"),
        label(cx, fixture, "preview-info"),
        visible(cx, fixture, "preview-note"),
    );
    // 量到新的画框要多画一帧。
    in_frame(cx, fixture.handle, |_, _| {});
    label(cx, fixture, "preview-zoom")
}

fn wait_for_zoom(cx: &mut TestAppContext, fixture: &Fixture, zoom: &str) {
    assert!(
        wait_until(cx, fixture.handle, |cx| label(cx, fixture, "preview-zoom")
            == zoom),
        "倍数没有变成 {zoom}，现在是 {}",
        label(cx, fixture, "preview-zoom")
    );
}

fn mode_selected(cx: &mut TestAppContext, fixture: &Fixture, mode: usize) -> Option<bool> {
    in_frame(cx, fixture.handle, move |window, _| {
        window.find(("preview-zoom-mode", mode)).selected()
    })
}

fn scroll_picture(cx: &mut TestAppContext, fixture: &Fixture, by: f32, secondary: bool) {
    in_frame(cx, fixture.handle, |window, cx| {
        let position = window.find("preview-image").bounds().center();
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(by))),
                modifiers: if secondary {
                    Modifiers::secondary_key()
                } else {
                    Modifiers::default()
                },
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    });
}

#[gpui_kit::test]
fn the_preview_takes_most_of_the_window(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    double_click_row(cx, &fixture, "图.png");
    wait_visible(cx, &fixture, "preview-dialog");
    // 原型 `.pv`：窗口 1280×800，四周留白。
    let dialog = bounds_of(cx, &fixture, "preview-dialog");
    assert!(dialog.size.width >= px(1150.), "{dialog:?}");
    assert!(dialog.size.height >= px(700.), "{dialog:?}");
    let frame = bounds_of(cx, &fixture, "preview-image");
    assert!(frame.size.height >= px(650.), "{frame:?}");
    assert!(frame.size.width >= px(1150.), "{frame:?}");
}

#[gpui_kit::test]
fn an_image_fits_its_frame_in_the_middle_and_a_small_one_keeps_its_size(cx: &mut TestAppContext) {
    let fixture = open_pictures(
        cx,
        &[("/pictures/长图.png", LONG_PNG), ("/pictures/点.png", PNG)],
    );
    double_click_row(cx, &fixture, "长图.png");
    wait_for_scale(cx, &fixture);
    let frame = bounds_of(cx, &fixture, "preview-image");
    let picture = bounds_of(cx, &fixture, "preview-picture");
    assert!(
        (picture.size.height - frame.size.height).abs() < px(1.),
        "{picture:?} in {frame:?}"
    );
    assert!(
        (picture.center().x - frame.center().x).abs() < px(1.),
        "{picture:?} in {frame:?}"
    );
    let info = label(cx, &fixture, "preview-info");
    assert!(info.contains("4×2000"), "{info}");
    assert_eq!(mode_selected(cx, &fixture, 0), Some(true));
    press(cx, &fixture, "escape");

    double_click_row(cx, &fixture, "点.png");
    wait_for_zoom(cx, &fixture, "100%");
    let frame = bounds_of(cx, &fixture, "preview-image");
    let picture = bounds_of(cx, &fixture, "preview-picture");
    assert_eq!(picture.size, size(px(1.), px(1.)));
    assert!((picture.center().x - frame.center().x).abs() < px(1.));
    assert!((picture.center().y - frame.center().y).abs() < px(1.));
}

#[gpui_kit::test]
fn an_image_switches_between_fit_and_actual_size_and_zooms(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/长图.png", LONG_PNG)]);
    double_click_row(cx, &fixture, "长图.png");
    let fit = wait_for_scale(cx, &fixture);

    // 「原图」：自己的大小，比画框大，从正中开始。
    click(cx, &fixture, ("preview-zoom-mode", 1usize));
    wait_for_zoom(cx, &fixture, "100%");
    assert_eq!(mode_selected(cx, &fixture, 0), Some(false));
    assert_eq!(mode_selected(cx, &fixture, 1), Some(true));
    let frame = bounds_of(cx, &fixture, "preview-image");
    let picture = bounds_of(cx, &fixture, "preview-picture");
    assert_eq!(picture.size.height, px(2000.));
    assert!((picture.center().y - frame.center().y).abs() < px(1.));

    // 能滚动。
    scroll_picture(cx, &fixture, -300., false);
    let scrolled = bounds_of(cx, &fixture, "preview-picture");
    assert_eq!(scrolled.top(), picture.top() - px(300.));

    // 按钮和按键一档一档地缩放；别的倍数两个分段都不选。
    click(cx, &fixture, "preview-zoom-in");
    wait_for_zoom(cx, &fixture, "125%");
    assert_eq!(mode_selected(cx, &fixture, 1), Some(false));
    press(cx, &fixture, "ctrl--");
    wait_for_zoom(cx, &fixture, "100%");
    press(cx, &fixture, "ctrl-=");
    wait_for_zoom(cx, &fixture, "125%");
    press(cx, &fixture, "ctrl-9");
    wait_for_zoom(cx, &fixture, &fit);
    assert_eq!(mode_selected(cx, &fixture, 0), Some(true));
    click(cx, &fixture, "preview-zoom-out");
    wait_for_zoom(cx, &fixture, "33%");
    press(cx, &fixture, "ctrl-0");
    wait_for_zoom(cx, &fixture, "100%");
    click(cx, &fixture, ("preview-zoom-mode", 0usize));
    wait_for_zoom(cx, &fixture, &fit);

    // 双击在放进画框和 100% 之间切。
    in_frame(cx, fixture.handle, |window, cx| {
        window.double_click("preview-picture", cx)
    });
    wait_for_zoom(cx, &fixture, "100%");
    in_frame(cx, fixture.handle, |window, cx| {
        window.double_click("preview-picture", cx)
    });
    wait_for_zoom(cx, &fixture, &fit);

    // Ctrl 加滚轮是缩放，不是滚动。
    let before = bounds_of(cx, &fixture, "preview-picture");
    scroll_picture(cx, &fixture, 100., true);
    wait_until(cx, fixture.handle, |cx| {
        label(cx, &fixture, "preview-zoom") != fit
    });
    let after = bounds_of(cx, &fixture, "preview-picture");
    assert!(after.size.height > before.size.height, "{after:?}");
}

#[gpui_kit::test]
fn every_image_format_is_shown(cx: &mut TestAppContext) {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="3"><rect width="2" height="3" fill="red"/></svg>"#;
    let files: [(&str, &[u8]); 9] = [
        ("/pictures/a.png", PNG_2X3),
        ("/pictures/a.jpg", JPEG),
        ("/pictures/a.jpeg", JPEG),
        ("/pictures/a.gif", GIF),
        ("/pictures/a.webp", WEBP),
        ("/pictures/a.svg", svg),
        ("/pictures/a.bmp", BMP),
        ("/pictures/a.ico", ICO),
        ("/pictures/a.tiff", TIFF),
    ];
    let fixture = open_pictures(cx, &files);
    for (path, _) in files {
        command(
            cx,
            &fixture,
            SftpTabCommand::Preview {
                remote: true,
                path: Some(path.to_string()),
            },
        );
        assert!(
            wait_until(cx, fixture.handle, |cx| {
                visible(cx, &fixture, "preview-dialog")
                    && label(cx, &fixture, "preview-info").contains("2×3")
            }),
            "{path} 没有显示出来：{}",
            label(cx, &fixture, "preview-info")
        );
        press(cx, &fixture, "escape");
        assert!(!visible(cx, &fixture, "preview-dialog"), "{path}");
    }
}

#[gpui_kit::test]
fn an_image_that_cannot_be_decoded_says_so(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/坏.png", b"not a png at all")]);
    double_click_row(cx, &fixture, "坏.png");
    assert!(
        wait_until(cx, fixture.handle, |cx| visible(
            cx,
            &fixture,
            "preview-note"
        ) && label(
            cx,
            &fixture,
            "preview-note"
        ) == "无法显示这张图片"),
        "没有说明"
    );
    assert!(!visible(cx, &fixture, "preview-picture"));
}

// ---------- 打开规则 ----------

fn editor_name(cx: &mut TestAppContext, fixture: &Fixture) -> Option<String> {
    cx.update(|cx| {
        fixture
            .workspace
            .read(cx)
            .active_editor_tab()
            .map(|editor| editor.read(cx).file_name().to_string())
    })
}

fn wait_for_editor(cx: &mut TestAppContext, fixture: &Fixture, name: &str) {
    assert!(
        wait_until(cx, fixture.handle, |cx| editor_name(cx, fixture).as_deref()
            == Some(name)),
        "编辑器没有打开 {name}"
    );
}

fn tab_count(cx: &mut TestAppContext, fixture: &Fixture) -> usize {
    cx.update(|cx| fixture.workspace.read(cx).tab_ids().len())
}

fn preview_of(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, path: &str) {
    command(
        cx,
        fixture,
        SftpTabCommand::Preview {
            remote,
            path: Some(path.to_string()),
        },
    );
}

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"><rect width="10" height="20" fill="red"/></svg>"#;
const MARKDOWN: &[u8] = "# 标题\n\n正文\n".as_bytes();

#[gpui_kit::test]
fn markdown_and_svg_are_edited_on_a_double_click(cx: &mut TestAppContext) {
    let fixture = open_pictures(
        cx,
        &[
            ("/pictures/README.md", MARKDOWN),
            ("/pictures/logo.svg", SVG),
        ],
    );
    for name in ["README.md", "logo.svg"] {
        double_click_row(cx, &fixture, name);
        wait_for_editor(cx, &fixture, name);
        assert!(!visible(cx, &fixture, "preview-dialog"));
        dispatch(cx, fixture.handle, qishell::app::PreviousTab);
    }
}

/// 回车和 F4 是编辑（spec 第 6 节），图片上也是：编辑器说明打不开，不会变成预览。
#[gpui_kit::test]
fn enter_and_f4_edit_even_on_an_image(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    let row = row("图.png");
    for key in ["enter", "f4"] {
        in_frame(cx, fixture.handle, |window, cx| {
            window.within("remote-card").click(row.clone(), cx);
            window.press(key, cx);
        });
        wait_visible(cx, &fixture, "editor-refused");
        assert!(!visible(cx, &fixture, "preview-dialog"), "{key}");
        click(cx, &fixture, "editor-refused-close");
    }
}

/// 菜单的「预览」派发的是这条命令；不写路径时对着只选中的那个文件。
#[gpui_kit::test]
fn preview_without_a_path_opens_the_selected_file(cx: &mut TestAppContext) {
    let fixture = open_pictures(
        cx,
        &[
            ("/pictures/README.md", MARKDOWN),
            ("/pictures/notes.txt", b"text\n"),
        ],
    );
    let row = row("README.md");
    in_frame(cx, fixture.handle, |window, cx| {
        window.within("remote-card").click(row, cx)
    });
    dispatch(
        cx,
        fixture.handle,
        qishell::app::SftpShortcut(SftpTabCommand::Preview {
            remote: true,
            path: None,
        }),
    );
    wait_visible(cx, &fixture, "preview-markdown");
    assert_eq!(label(cx, &fixture, "preview-dialog"), "README.md");
    press(cx, &fixture, "escape");

    // 预览不了的文件什么都不做。
    let reads = fixture.sftp.reads.lock().unwrap().len();
    preview_of(cx, &fixture, true, "/pictures/notes.txt");
    assert!(!visible(cx, &fixture, "preview-dialog"));
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), reads);
}

#[gpui_kit::test]
fn closing_a_preview_gives_the_focus_back_to_the_file_list(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    let tab = sftp_tab(cx, &fixture);
    let list = cx.update(|cx| tab.read(cx).remote().read(cx).focus_handle(cx));
    let list_focused = |cx: &mut TestAppContext| {
        let list = list.clone();
        in_frame(cx, fixture.handle, move |window, _| list.is_focused(window))
    };
    for close in ["escape", "button"] {
        double_click_row(cx, &fixture, "图.png");
        wait_visible(cx, &fixture, "preview-dialog");
        assert!(!list_focused(cx), "{close}：焦点该在预览里");
        if close == "escape" {
            press(cx, &fixture, "escape");
        } else {
            click(cx, &fixture, "preview-close");
        }
        assert!(!visible(cx, &fixture, "preview-dialog"), "{close}");
        assert!(list_focused(cx), "{close}：焦点没有回到文件列表");
    }
}

#[gpui_kit::test]
fn closing_the_sftp_tab_closes_its_preview(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    double_click_row(cx, &fixture, "图.png");
    wait_visible(cx, &fixture, "preview-dialog");
    let tab = cx.update(|cx| fixture.workspace.read(cx).active_tab().unwrap());
    dispatch(cx, fixture.handle, qishell::app::CloseTab(tab));
    assert!(!visible(cx, &fixture, "preview-dialog"));
}

// ---------- Markdown ----------

#[gpui_kit::test]
fn markdown_is_laid_out_and_links_open_in_the_browser(cx: &mut TestAppContext) {
    let fixture = open_pictures(
        cx,
        &[(
            "/pictures/README.md",
            b"[example](https://example.com/docs)\n",
        )],
    );
    preview_of(cx, &fixture, true, "/pictures/README.md");
    wait_visible(cx, &fixture, "preview-markdown");
    let info = label(cx, &fixture, "preview-info");
    assert!(info.contains("/pictures"), "{info}");
    assert!(info.contains("36 B"), "{info}");
    // Markdown 没有缩放的工具。
    assert!(!visible(cx, &fixture, "preview-zoom"));
    assert!(visible(cx, &fixture, "preview-edit"));
    assert!(visible(cx, &fixture, "preview-download"));
    assert_eq!(cx.opened_url(), None);
    // 链接在正文左上角：留白 60、22 里面。
    in_frame(cx, fixture.handle, |window, cx| {
        window.click_at("preview-markdown", point(px(70.), px(30.)), cx)
    });
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs"));
    // 点链接不关预览。
    assert!(visible(cx, &fixture, "preview-markdown"));
}

#[gpui_kit::test]
fn a_preview_shows_what_is_on_the_server_not_unsaved_edits(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/README.md", b"# server\n")]);
    double_click_row(cx, &fixture, "README.md");
    wait_for_editor(cx, &fixture, "README.md");
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("unsaved text", cx)
    });
    let editor = cx
        .update(|cx| fixture.workspace.read(cx).active_editor_tab())
        .expect("an editor");
    assert!(cx.update(|cx| editor.read(cx).has_unsaved_changes()));
    let reads = fixture.sftp.reads.lock().unwrap().len();

    preview_of(cx, &fixture, true, "/pictures/README.md");
    wait_visible(cx, &fixture, "preview-markdown");
    // 又读了一次服务器，大小是服务器上的 9 字节；编辑器里的修改还在。
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), reads + 1);
    let info = label(cx, &fixture, "preview-info");
    assert!(info.contains("9 B"), "{info}");
    assert!(cx.update(|cx| editor.read(cx).has_unsaved_changes()));
    assert!(fixture.sftp.writes.lock().unwrap().is_empty());
}

// ---------- 编辑 ----------

#[gpui_kit::test]
fn edit_closes_the_preview_and_opens_the_editor(cx: &mut TestAppContext) {
    let fixture = open_pictures(
        cx,
        &[
            ("/pictures/README.md", MARKDOWN),
            ("/pictures/logo.svg", SVG),
        ],
    );
    for (name, path, shown) in [
        ("README.md", "/pictures/README.md", "preview-markdown"),
        ("logo.svg", "/pictures/logo.svg", "preview-image"),
    ] {
        preview_of(cx, &fixture, true, path);
        wait_visible(cx, &fixture, shown);
        click(cx, &fixture, "preview-edit");
        assert!(!visible(cx, &fixture, "preview-dialog"), "{name}");
        wait_for_editor(cx, &fixture, name);
        dispatch(cx, fixture.handle, qishell::app::PreviousTab);
    }
    assert_eq!(tab_count(cx, &fixture), 3);
    // SVG 在编辑器里是文本。
    dispatch(cx, fixture.handle, qishell::app::SwitchToTab(3));
    let editor = cx
        .update(|cx| fixture.workspace.read(cx).active_editor_tab())
        .expect("an editor");
    let text = cx.update(|cx| editor.read(cx).text(cx));
    assert_eq!(text.as_bytes(), SVG);
}

#[gpui_kit::test]
fn edit_switches_to_the_editor_already_open(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/README.md", MARKDOWN)]);
    double_click_row(cx, &fixture, "README.md");
    wait_for_editor(cx, &fixture, "README.md");
    let opened = cx.update(|cx| fixture.workspace.read(cx).active_tab());
    dispatch(cx, fixture.handle, qishell::app::PreviousTab);

    preview_of(cx, &fixture, true, "/pictures/README.md");
    wait_visible(cx, &fixture, "preview-markdown");
    let reads = fixture.sftp.reads.lock().unwrap().len();
    click(cx, &fixture, "preview-edit");
    wait_for_editor(cx, &fixture, "README.md");
    assert!(!visible(cx, &fixture, "preview-dialog"));
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).active_tab()),
        opened
    );
    assert_eq!(tab_count(cx, &fixture), 2);
    // 开着就切过去，不再读一遍。
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), reads);
}

// ---------- 下载 ----------

#[gpui_kit::test]
fn download_asks_over_the_preview_and_cancel_goes_back_to_it(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    double_click_row(cx, &fixture, "图.png");
    wait_for_scale(cx, &fixture);

    click(cx, &fixture, "preview-download");
    wait_visible(cx, &fixture, "download-confirm");
    // 确认框在预览上面，预览还开着；目标默认是本地一侧的当前目录。
    assert!(visible(cx, &fixture, "preview-dialog"));
    let target = in_frame(cx, fixture.handle, |window, _| {
        window.find("download-target").value().map(str::to_owned)
    });
    assert_eq!(target.as_deref(), Some(LOCAL_DOWNLOADS));

    // 取消：回到预览，焦点也回来了，缩放的按键能用。
    press(cx, &fixture, "escape");
    assert!(!visible(cx, &fixture, "download-confirm"));
    assert!(visible(cx, &fixture, "preview-dialog"));
    assert!(fixture.sftp.downloads.lock().unwrap().is_empty());
    press(cx, &fixture, "ctrl-0");
    wait_for_zoom(cx, &fixture, "100%");
}

/// 本地一侧的目录，两个系统上都是绝对路径：确认框只收绝对路径。
fn local_folder() -> &'static str {
    if cfg!(windows) {
        r"C:\收到的"
    } else {
        "/收到的"
    }
}

#[gpui_kit::test]
fn a_confirmed_download_goes_into_the_queue_of_this_tab(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    command(
        cx,
        &fixture,
        SftpTabCommand::Navigate {
            remote: false,
            path: local_folder().into(),
        },
    );
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            in_frame(cx, fixture.handle, |window, _| {
                window.find("local-path").value().map(str::to_owned)
            })
            .as_deref()
                == Some(local_folder())
        }),
        "本地一侧没有换目录"
    );
    double_click_row(cx, &fixture, "图.png");
    wait_visible(cx, &fixture, "preview-download");
    click(cx, &fixture, "preview-download");
    wait_visible(cx, &fixture, "download-confirm");
    click(cx, &fixture, "download-confirm");

    let downloads = fixture.sftp.downloads.clone();
    assert!(
        wait_until(cx, fixture.handle, |_| !downloads
            .lock()
            .unwrap()
            .is_empty()),
        "工作线程没有收到下载"
    );
    let request = format!("{:?}", downloads.lock().unwrap()[0]);
    assert!(request.contains("图.png"), "{request}");
    assert!(request.contains("收到的"), "{request}");
    let tab = sftp_tab(cx, &fixture);
    assert_eq!(
        cx.update(|cx| tab.read(cx).transfer_queue().entries().len()),
        1
    );
    // 框关了，预览还开着。
    assert!(!visible(cx, &fixture, "download-confirm"));
    assert!(visible(cx, &fixture, "preview-dialog"));
}

#[gpui_kit::test]
fn a_local_file_has_no_download(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[]);
    fixture.local.files.lock().unwrap().extend([
        (format!("{LOCAL_DOWNLOADS}/图.png").into(), PNG.to_vec()),
        (
            format!("{LOCAL_DOWNLOADS}/README.md").into(),
            MARKDOWN.to_vec(),
        ),
    ]);
    preview_of(cx, &fixture, false, &format!("{LOCAL_DOWNLOADS}/图.png"));
    wait_visible(cx, &fixture, "preview-image");
    assert!(!visible(cx, &fixture, "preview-download"));
    assert!(!visible(cx, &fixture, "preview-edit"));
    let info = label(cx, &fixture, "preview-info");
    assert!(info.contains("本机"), "{info}");
    press(cx, &fixture, "escape");

    preview_of(cx, &fixture, false, &format!("{LOCAL_DOWNLOADS}/README.md"));
    wait_visible(cx, &fixture, "preview-markdown");
    assert!(!visible(cx, &fixture, "preview-download"));
    assert!(visible(cx, &fixture, "preview-edit"));
}

// ---------- 预览不了的文件 ----------

#[gpui_kit::test]
fn an_image_over_the_limit_is_refused_and_offers_the_download(cx: &mut TestAppContext) {
    let large = vec![0; IMAGE_LIMIT as usize + 1];
    let fixture = open_pictures(cx, &[("/pictures/图.png", large.as_slice())]);
    double_click_row(cx, &fixture, "图.png");
    wait_visible(cx, &fixture, "preview-refused");
    assert!(!visible(cx, &fixture, "preview-dialog"));
    let text = label(cx, &fixture, "preview-refused-text");
    assert!(text.contains("20.0 MB"), "{text}");
    click(cx, &fixture, "preview-refused-download");
    wait_visible(cx, &fixture, "download-confirm");
}

#[gpui_kit::test]
fn markdown_over_the_limit_or_not_in_utf8_is_refused(cx: &mut TestAppContext) {
    let large = vec![b'a'; qishell::backend::sftp::EDIT_LIMIT as usize + 1];
    let fixture = open_pictures(
        cx,
        &[
            ("/pictures/大.md", large.as_slice()),
            ("/pictures/gbk.md", b"\xd6\xd0\xce\xc4\n"),
        ],
    );
    preview_of(cx, &fixture, true, "/pictures/大.md");
    wait_visible(cx, &fixture, "preview-refused");
    let text = label(cx, &fixture, "preview-refused-text");
    assert!(text.contains("5.0 MB"), "{text}");
    click(cx, &fixture, "preview-refused-close");

    preview_of(cx, &fixture, true, "/pictures/gbk.md");
    wait_visible(cx, &fixture, "preview-refused");
    let text = label(cx, &fixture, "preview-refused-text");
    assert!(text.contains("UTF-8"), "{text}");
    assert!(!visible(cx, &fixture, "preview-dialog"));
}

#[gpui_kit::test]
fn a_local_file_over_the_limit_has_no_download(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[]);
    let path = format!("{LOCAL_DOWNLOADS}/图.png");
    fixture
        .local
        .files
        .lock()
        .unwrap()
        .insert(path.clone().into(), vec![0; IMAGE_LIMIT as usize + 1]);
    preview_of(cx, &fixture, false, &path);
    wait_visible(cx, &fixture, "preview-refused");
    assert!(!visible(cx, &fixture, "preview-refused-download"));
}

#[gpui_kit::test]
fn a_file_that_cannot_be_read_only_notifies(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[]);
    preview_of(cx, &fixture, true, "/pictures/没有.png");
    assert!(
        wait_until(cx, fixture.handle, |cx| in_frame(
            cx,
            fixture.handle,
            |window, cx| window.notifications(cx).len() == 1
        )),
        "没有通知"
    );
    assert!(!visible(cx, &fixture, "preview-dialog"));
    assert!(!visible(cx, &fixture, "preview-refused"));
}

#[gpui_kit::test]
fn previewing_while_disconnected_offers_to_reconnect(cx: &mut TestAppContext) {
    let fixture = open_pictures(cx, &[("/pictures/图.png", PNG)]);
    fixture
        .sftp
        .emit(qishell::backend::sftp::SftpEvent::Disconnected(
            "连接断开了".into(),
        ));
    let tab = sftp_tab(cx, &fixture);
    assert!(
        wait_until(cx, fixture.handle, |cx| cx.update(|cx| {
            tab.read(cx).connection_state() == qishell::sftp_tab::ConnectionState::Disconnected
        })),
        "没有断开"
    );
    let reads = fixture.sftp.reads.lock().unwrap().len();
    preview_of(cx, &fixture, true, "/pictures/图.png");
    wait_visible(cx, &fixture, "sftp-reconnect");
    assert!(!visible(cx, &fixture, "preview-dialog"));
    assert_eq!(fixture.sftp.reads.lock().unwrap().len(), reads);
}
