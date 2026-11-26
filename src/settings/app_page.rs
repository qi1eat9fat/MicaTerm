//! 设置 › 应用（26 号「设置 › 应用」）：
//! 窗口（记住窗口大小和位置）、数据（数据目录在哪、里面有什么，「打开文件夹」）、关于（版本、许可证）。

use gpui_kit::*;

use super::AppSettings;
use super::controls::{note, reset_if, row, row_with_note, section, setting_switch};
use super::tab::SettingsTab;
use crate::app::PRODUCT_NAME;
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale};
use crate::ui::modal::{ModalButtonTone, OnClick, modal_button};

/// MicaTerm 的版本，取自 `Cargo.toml`。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 「许可证」打开的页面：GPL-3.0 全文。
const LICENSE_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";

/// 数据目录就是 exe 旁边的 `data/`（没有用 `MICATERM_DATA_DIR` 另指）。
fn beside_exe(dir: &std::path::Path) -> bool {
    std::env::current_exe()
        .ok()
        .map(|exe| dunce::canonicalize(&exe).unwrap_or(exe))
        .is_some_and(|exe| crate::app::data_dir_of(&exe, None) == dir)
}

pub(super) fn app(
    tab: &SettingsTab,
    tokens: &Tokens,
    window: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let store = tab.store.clone();
    let settings = store.read(cx).settings();
    let default = AppSettings::default();

    let remember = setting_switch(
        "remember-window",
        t!("settings.app.remember_window"),
        settings.window.remember,
        &store,
        |settings, checked| settings.window.remember = checked,
    );

    // 数据目录：说明那一行写它在哪、里面有什么。在 exe 旁边时照 26 号写「exe 旁的 data/」，
    // 用环境变量另指的写完整路径；无障碍名总是完整路径（单独带 id，测试读它）。
    let (path, shown, open_folder) = match tab.data_dir.clone() {
        Some(dir) => {
            let path = dir.display().to_string();
            let shown = if beside_exe(&dir) {
                t!("settings.app.data_dir_beside_exe").to_string()
            } else {
                path.clone()
            };
            let open: OnClick = Box::new(move |_, _, cx| cx.open_with_system(&dir));
            let button = modal_button(
                "open-data-dir",
                t!("settings.app.open_data_dir"),
                None,
                ModalButtonTone::Line,
                Some(open),
                tokens,
                window,
                cx,
            )
            .into_any_element();
            (path, shown, Some(button))
        }
        // 没有数据目录：没有可以打开的文件夹。
        None => {
            let text = t!("settings.app.no_data_dir").to_string();
            (text.clone(), text, None)
        }
    };
    let contents = tab
        .data_dir
        .is_some()
        .then(|| t!("settings.app.data_dir_contents"));
    let data_note = note(tokens)
        .flex()
        .min_w_0()
        .child(
            div()
                .id("data-dir-path")
                .test_support()
                .aria_label(path)
                .min_w_0()
                .truncate()
                .child(shown),
        )
        .children(contents.map(|contents| div().flex_none().child(contents)))
        .into_any_element();

    let license: OnClick = Box::new(|_, _, cx| cx.open_url(LICENSE_URL));
    let license = modal_button(
        "about-license",
        t!("settings.app.license"),
        None,
        ModalButtonTone::Line,
        Some(license),
        tokens,
        window,
        cx,
    );
    let about_title: SharedString = format!("{PRODUCT_NAME} {VERSION}").into();

    vec![
        section(
            t!("settings.app.window"),
            None,
            true,
            tokens,
            TypeScale::current(cx),
        ),
        row(
            t!("settings.app.remember_window"),
            Some(t!("settings.app.remember_window_description")),
            reset_if(
                settings.window.remember != default.window.remember,
                "remember-window",
                &store,
                |settings| settings.window.remember = AppSettings::default().window.remember,
                tokens,
            ),
            remember,
            tokens,
        ),
        section(
            t!("settings.app.data"),
            None,
            false,
            tokens,
            TypeScale::current(cx),
        ),
        row_with_note(
            t!("settings.app.data_dir"),
            Some(data_note),
            None,
            div().children(open_folder),
            tokens,
        ),
        section(
            t!("settings.app.about"),
            None,
            false,
            tokens,
            TypeScale::current(cx),
        ),
        row(
            div()
                .id("about-version")
                .test_support()
                .aria_label(about_title.clone())
                .child(about_title),
            Some(t!("settings.app.about_description")),
            None,
            license,
            tokens,
        ),
    ]
}
