//! 设置 › 应用与 Mica（工单 26）：记住窗口大小和位置、数据目录、关于；外观里的 Mica 背景开关。
//! 最后核对六类设置都在左栏里（spec 第 9 节，Agent CLI 工单 01 加了「外部 CLI」，2026-11-16 改名「Agent」）。

use std::path::Path;
use std::time::Duration;

use micaterm::app::{OpenSettings, WINDOW_STATE_FILE};
use micaterm::settings::{WindowBackdrop, WindowSettings, mica_supported};
use micaterm::workspace::{WindowState, default_window_size, window_options};

use crate::support::*;

/// 设置里「应用」在左栏的位置。
const APP_CATEGORY: usize = 5;

fn open_with_data_dir(
    cx: &mut TestAppContext,
    data_dir: Option<&Path>,
) -> (WindowHandle<Root>, Entity<Workspace>) {
    let services = WorkspaceServices {
        data_dir: data_dir.map(Path::to_path_buf),
        ..fake_services()
    };
    open_workspace_with(cx, settings_store(), services)
}

fn open_category(cx: &mut TestAppContext, handle: WindowHandle<Root>, category: usize) {
    dispatch(cx, handle, OpenSettings);
    in_frame(cx, handle, |window, cx| {
        window.click(("settings-category", category), cx)
    });
}

fn stored(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> AppSettings {
    cx.update(|cx| workspace.read(cx).settings().read(cx).settings())
}

#[gpui_kit::test]
fn the_settings_have_all_six_categories_in_order(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    dispatch(cx, handle, OpenSettings);
    in_frame(cx, handle, |window, _| {
        let names: Vec<String> = (0..6_usize)
            .map(|index| {
                window
                    .find(("settings-category", index))
                    .label()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            ["Appearance", "Terminal", "SFTP", "Security", "Agent", "App"]
        );
        assert!(window.try_find(("settings-category", 6_usize)).is_none());
    });
}

#[gpui_kit::test]
fn the_app_category_shows_the_window_data_folder_and_about(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, _) = open_with_data_dir(cx, Some(data.path()));
    open_category(cx, handle, APP_CATEGORY);

    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("settings-category", APP_CATEGORY)).selected(),
            Some(true)
        );
        // 记住窗口默认开。
        assert_eq!(window.find("remember-window").checked(), Some(true));
        // 数据目录显示完整路径，可以打开。
        let shown = data.path().display().to_string();
        assert_eq!(window.find("data-dir-path").label(), Some(shown.as_str()));
        assert!(window.find("open-data-dir").visible());
        // 关于：产品名和版本，许可证按钮。
        let about = format!("MicaTerm {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(window.find("about-version").label(), Some(about.as_str()));
        assert!(window.find("about-license").visible());
    });
}

#[gpui_kit::test]
fn without_a_data_folder_the_app_category_says_so(cx: &mut TestAppContext) {
    let (handle, _) = open_with_data_dir(cx, None);
    open_category(cx, handle, APP_CATEGORY);
    in_frame(cx, handle, |window, _| {
        let note = window.find("data-dir-path");
        assert!(
            note.label()
                .is_some_and(|label| label.contains("No data folder"))
        );
        assert!(window.try_find("open-data-dir").is_none());
    });
}

/// 拨测试时钟，让窗口停稳后的那次写文件跑完。
fn let_the_window_settle(cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_window_size_is_remembered_for_the_next_launch(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, workspace) = open_with_data_dir(cx, Some(data.path()));
    let path = data.path().join(WINDOW_STATE_FILE);

    cx.simulate_window_resize(handle.into(), size(px(1000.), px(640.)));
    let_the_window_settle(cx);
    let saved = WindowState::load(&path).expect("the window's place is written");
    assert_eq!(saved.bounds().size, size(px(1000.), px(640.)));
    assert!(!saved.maximized());

    // 下次启动：记住时开成上次的大小，在上次的位置。
    let remembered = stored(cx, &workspace).window;
    assert!(remembered.remember);
    let options = cx.update(|cx| window_options(Some(&saved), remembered, cx));
    match options.window_bounds {
        Some(gpui_kit::WindowBounds::Windowed(bounds)) => {
            assert_eq!(bounds, saved.bounds())
        }
        other => panic!("expected a windowed window, got {other:?}"),
    }

    // 关掉「记住窗口大小和位置」：窗口照样记，但下次启动像新窗口一样。
    open_category(cx, handle, APP_CATEGORY);
    in_frame(cx, handle, |window, cx| window.click("remember-window", cx));
    let forgotten = stored(cx, &workspace).window;
    assert_eq!(forgotten, WindowSettings { remember: false });
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("remember-window").checked(), Some(false))
    });
    let options = cx.update(|cx| window_options(Some(&saved), forgotten, cx));
    match options.window_bounds {
        Some(gpui_kit::WindowBounds::Windowed(bounds)) => {
            assert_eq!(bounds.size, default_window_size())
        }
        other => panic!("expected a windowed window, got {other:?}"),
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    gpui_kit::Bounds::new(gpui_kit::point(px(x), px(y)), size(px(width), px(height)))
}

/// 上次退出时记下的 `window.json`：在测试的那块屏幕上，不最大化时是 `bounds`。
fn write_window_file(
    cx: &mut TestAppContext,
    path: &Path,
    bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
    maximized: bool,
) {
    let display = cx.update(|cx| {
        let display = cx.primary_display().expect("the test has a screen");
        display.uuid().unwrap().to_string()
    });
    let state = serde_json::json!({
        "display": display,
        "bounds": {
            "origin": { "x": bounds.origin.x.as_f32(), "y": bounds.origin.y.as_f32() },
            "size": { "width": bounds.size.width.as_f32(), "height": bounds.size.height.as_f32() },
        },
        "maximized": maximized,
    });
    std::fs::write(path, state.to_string()).unwrap();
}

/// 像 `main` 那样启动：按数据目录里记下的样子和设置算窗口选项，再打开窗口壳。
fn launch(
    cx: &mut TestAppContext,
    data_dir: &Path,
    settings: WindowSettings,
) -> (WindowHandle<Root>, Entity<Workspace>) {
    init_app(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let saved = WindowState::load(&data_dir.join(WINDOW_STATE_FILE));
    let options = cx.update(|cx| window_options(saved.as_ref(), settings, cx));
    let services = WorkspaceServices {
        data_dir: Some(data_dir.to_path_buf()),
        ..fake_services()
    };
    let mut workspace = None;
    let handle = cx
        .update(|cx| {
            cx.open_window(options, |window, cx| {
                let settings = cx.new(|_| settings_store());
                let view =
                    cx.new(|cx| Workspace::new_with_services(settings, services, window, cx));
                workspace = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            })
        })
        .unwrap();
    cx.run_until_parked();
    (handle, workspace.expect("workspace created"))
}

fn window_bounds(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.update_window(handle.into(), |_, window, _| window.bounds())
        .unwrap()
}

#[gpui_kit::test]
fn the_window_position_is_remembered_for_the_next_launch(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join(WINDOW_STATE_FILE);
    let was = rect(240., 160., 1000., 640.);
    write_window_file(cx, &path, was, false);

    // 记住时开在上次的位置，是上次的大小。
    let (handle, _) = launch(cx, data.path(), WindowSettings { remember: true });
    assert_eq!(window_bounds(cx, handle), was);

    // 再改大小：记下的位置还是窗口所在的位置。
    cx.simulate_window_resize(handle.into(), size(px(1100.), px(700.)));
    let_the_window_settle(cx);
    let saved = WindowState::load(&path).expect("the window's place is written");
    assert_eq!(saved.bounds(), rect(240., 160., 1100., 700.));
    assert!(!saved.maximized());
}

#[gpui_kit::test]
fn a_window_left_maximized_opens_maximized_over_its_restored_place(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join(WINDOW_STATE_FILE);
    let restored = rect(240., 160., 1000., 640.);
    write_window_file(cx, &path, restored, true);
    let saved = WindowState::load(&path).unwrap();
    assert!(saved.maximized());

    let primary = cx.update(|cx| cx.primary_display().unwrap().id());
    let options =
        cx.update(|cx| window_options(Some(&saved), WindowSettings { remember: true }, cx));
    assert_eq!(options.display_id, Some(primary));
    let expected = if cfg!(target_os = "macos") {
        // macOS 上最大化是填满屏幕可见部分的窗口。
        let visible = cx.update(|cx| cx.primary_display().unwrap().visible_bounds());
        gpui_kit::WindowBounds::Windowed(visible)
    } else {
        // 还原时回到上次的大小和位置。
        gpui_kit::WindowBounds::Maximized(restored)
    };
    assert_eq!(options.window_bounds, Some(expected));

    // 不记住：像新窗口一样，不最大化。
    let options =
        cx.update(|cx| window_options(Some(&saved), WindowSettings { remember: false }, cx));
    assert!(matches!(
        options.window_bounds,
        Some(gpui_kit::WindowBounds::Windowed(bounds)) if bounds.size == default_window_size()
    ));
}

#[gpui_kit::test]
fn without_a_data_folder_the_window_is_not_remembered(cx: &mut TestAppContext) {
    let (handle, _) = open_with_data_dir(cx, None);
    cx.simulate_window_resize(handle.into(), size(px(1000.), px(640.)));
    let_the_window_settle(cx);
    // 不写到当前目录或别的地方。
    assert!(!Path::new(WINDOW_STATE_FILE).exists());
}

#[gpui_kit::test]
fn the_mica_switch_toggles_from_the_keyboard(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_category(cx, handle, 0);

    // 点一下先把焦点交给开关，再按空格、回车各切换一次。
    in_frame(cx, handle, |window, cx| window.click("mica", cx));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("mica").checked(), Some(true));
        assert_eq!(window.find("mica").focused(), Some(true));
    });
    in_frame(cx, handle, |window, cx| window.press("space", cx));
    assert!(!stored(cx, &workspace).interface.mica);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("mica").checked(), Some(false));
    });
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("mica").checked(), Some(true));
    });
    assert!(stored(cx, &workspace).interface.mica);
}

#[gpui_kit::test]
fn the_mica_switch_is_off_by_default_and_changes_the_backdrop(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_category(cx, handle, 0);
    let backdrop = |cx: &mut TestAppContext| cx.update(|cx| WindowBackdrop::current(cx));

    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("mica").checked(), Some(false));
    });
    assert_eq!(backdrop(cx), WindowBackdrop::Opaque);

    in_frame(cx, handle, |window, cx| window.click("mica", cx));
    assert!(stored(cx, &workspace).interface.mica);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("mica").checked(), Some(true));
    });
    // Windows 11 22H2 及以上换成 Mica；别处开关开着也保持不透明。
    let expected = if mica_supported() {
        WindowBackdrop::Mica
    } else {
        WindowBackdrop::Opaque
    };
    assert_eq!(backdrop(cx), expected);

    in_frame(cx, handle, |window, cx| window.click("mica", cx));
    assert!(!stored(cx, &workspace).interface.mica);
    assert_eq!(backdrop(cx), WindowBackdrop::Opaque);
}

