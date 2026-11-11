//! 设置 › 应用（工单 26，原型「设置 › 应用」）：窗口（记住窗口大小和位置）、
//! 数据（数据目录的路径和「打开文件夹」）、关于（版本、许可证）。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::*;

use super::controls::{group, note, row, row_with_note, section_title, setting_switch};
use super::dialog::SettingsDialog;
use crate::app::PRODUCT_NAME;
use crate::i18n::t;
use crate::theme::Tokens;

/// QiShell 的版本，取自 `Cargo.toml`。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 「许可证」打开的页面：GPL-3.0 全文。
const LICENSE_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";

pub(super) fn app(
    dialog: &SettingsDialog,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let store = dialog.store.clone();
    let settings = store.read(cx).settings();

    let remember = setting_switch(
        "remember-window",
        t!("settings.app.remember_window"),
        settings.window.remember,
        &store,
        |settings, checked| settings.window.remember = checked,
    );

    let data_dir = dialog.data_dir.clone();
    let (data_note, open_folder) = match data_dir {
        Some(dir) => {
            let shown = dir.display().to_string();
            let button = Button::new("open-data-dir")
                .label(t!("settings.app.open_data_dir"))
                .outline()
                .small()
                .on_click(move |_, _, cx| cx.open_with_system(&dir));
            (shown, Some(button))
        }
        // 没有数据目录：没有可以打开的文件夹。
        None => (t!("settings.app.no_data_dir").to_string(), None),
    };
    // 路径单独放一个带 id 的元素：测试读它的无障碍名。
    let data_path = note(tokens)
        .id("data-dir-path")
        .test_support()
        .aria_label(data_note.clone())
        .child(data_note)
        .into_any_element();
    let data_contents = note(tokens)
        .child(t!("settings.app.data_dir_contents"))
        .into_any_element();

    let license = Button::new("about-license")
        .label(t!("settings.app.license"))
        .outline()
        .small()
        .on_click(|_, _, cx| cx.open_url(LICENSE_URL));
    let about_title: SharedString = format!("{PRODUCT_NAME} {VERSION}").into();

    vec![
        section_title(t!("settings.app.window"), None, tokens).into_any_element(),
        group(
            [row(
                t!("settings.app.remember_window"),
                Some(t!("settings.app.remember_window_description")),
                remember,
                tokens,
            )],
            tokens,
        )
        .into_any_element(),
        section_title(t!("settings.app.data"), None, tokens).into_any_element(),
        group(
            [row_with_note(
                t!("settings.app.data_dir"),
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .child(data_path)
                        .child(data_contents)
                        .into_any_element(),
                ),
                div().children(open_folder),
            )],
            tokens,
        )
        .into_any_element(),
        section_title(t!("settings.app.about"), None, tokens).into_any_element(),
        group(
            [row(
                div()
                    .id("about-version")
                    .test_support()
                    .aria_label(about_title.clone())
                    .child(about_title),
                Some(t!("settings.app.about_description")),
                license,
                tokens,
            )],
            tokens,
        )
        .into_any_element(),
    ]
}
