// Windows 的 release 版是 GUI 程序，打开时不带控制台窗口；debug 版保留控制台看日志。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use gpui_kit::component::{Root, WindowExt as _, notification::Notification};
use gpui_kit::*;

use micaterm::settings::{SettingsProblem, SettingsStore};

fn main() {
    let app = gpui_kit::application().with_assets(micaterm::app::AppAssets);
    app.run(|cx| {
        // 先读设置：外观决定窗口用浅色还是深色。
        let (settings, problem) = open_settings();
        micaterm::init(cx);
        cx.activate(true);

        // 窗口开在上次的地方（设置 › 应用 › 记住窗口大小和位置）。
        let saved = micaterm::app::data_dir().ok().and_then(|dir| {
            micaterm::workspace::WindowState::load(&micaterm::app::window_state_path(&dir))
        });
        let options =
            micaterm::workspace::window_options(saved.as_ref(), settings.settings().window, cx);
        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                window.activate_window();
                window.set_window_title(micaterm::app::PRODUCT_NAME);
                let mode = settings
                    .settings()
                    .appearance
                    .theme_mode(window.appearance());
                gpui_kit::component::Theme::change(mode, Some(window), cx);
                let settings = cx.new(|_| settings);
                let workspace =
                    cx.new(|cx| micaterm::workspace::Workspace::new(settings, window, cx));
                if let Some(problem) = problem {
                    notify_once_open(Notification::error(problem.message()), window, cx);
                }
                // `Root` 自己不画底色：窗口壳的根节点画背景，开了 Mica 时要透出去（工单 26）。
                cx.new(|cx| Root::new(workspace, window, cx).bg(transparent_black()))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}

/// 读设置文件。没有数据目录时设置照样能用，只是只在本次运行有效；返回的问题会告诉用户。
fn open_settings() -> (SettingsStore, Option<SettingsProblem>) {
    match micaterm::app::settings_path() {
        Ok(path) => SettingsStore::load(path),
        Err(error) => (
            SettingsStore::in_memory(),
            Some(SettingsProblem::Unsaved(error.to_string())),
        ),
    }
}

/// 窗口还没挂上 `Root` 时不能弹通知：等下一帧再弹。
fn notify_once_open(notification: Notification, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        window.push_notification(notification, cx)
    });
}
