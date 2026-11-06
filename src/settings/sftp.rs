// 参考 since2006/shell-rs @ b2a307a: src/settings/settings_panel.rs 的 SFTP 一节
// 改造：分组照原型「设置 › SFTP」；加「本地默认打开目录」。
//! 设置 › SFTP（工单 25，原型「设置 › SFTP」）：传输一组（最大同时传输数量），文件列表一组
//! （本地默认打开目录、图标风格、显示隐藏文件）。
//!
//! 模型在 `model::SftpSettings`，生效在 `apply::apply_sftp`，这里只做界面。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::Button,
    input::{Input, InputEvent, InputState},
    switch::Switch,
};
use gpui_kit::*;

use qishell_ssh::sftp::PARALLEL_TRANSFERS_RANGE;

use super::SettingsStore;
use super::controls::{dropdown, group, row, row_with_note, section_title, segmented};
use super::dialog::SettingsDialog;
use super::pages::write;
use crate::i18n::t;
use crate::sftp_tab::{CardSide, FileIcon, FileIconKind, FileIconStyle};
use crate::theme::Tokens;

/// 图标风格的两个选项，按分段按钮从左到右。
const ICON_STYLES: [FileIconStyle; 2] = [FileIconStyle::Color, FileIconStyle::Monochrome];

/// 图标风格一行的示例：文件夹，和几种常见的文件。名字是空的就是普通文件夹的图标。
const ICON_PREVIEW: [(&str, FileIconKind); 5] = [
    ("", FileIconKind::Folder),
    ("nginx.conf", FileIconKind::File),
    ("compose.yml", FileIconKind::File),
    ("photo.png", FileIconKind::File),
    ("backup.tar.gz", FileIconKind::File),
];

pub(super) fn sftp(
    dialog: &SettingsDialog,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let store = dialog.store.clone();
    let settings = store.read(cx).settings().sftp;

    let counts: Vec<usize> = PARALLEL_TRANSFERS_RANGE.collect();
    let parallel = {
        let store = store.clone();
        let chosen = counts.iter().position(|count| *count == settings.parallel);
        let counts_for_menu = counts.clone();
        dropdown(
            "sftp-parallel",
            settings.parallel.to_string().into(),
            counts
                .iter()
                .map(|count| count.to_string().into())
                .collect(),
            chosen,
            Rc::new(move |index, _, cx| {
                let count = counts_for_menu[index];
                write(&store, cx, |settings| settings.sftp.parallel = count)
            }),
            px(90.),
        )
    };

    let icon_style = {
        let store = store.clone();
        segmented(
            "sftp-icon-style",
            vec![
                t!("settings.sftp.icon_color"),
                t!("settings.sftp.icon_monochrome"),
            ],
            ICON_STYLES
                .iter()
                .position(|style| *style == settings.file_icon_style)
                .unwrap_or_default(),
            Rc::new(move |index, _, cx| {
                write(&store, cx, |settings| {
                    settings.sftp.file_icon_style = ICON_STYLES[index]
                })
            }),
            tokens,
        )
    };

    vec![
        section_title(t!("settings.sftp.transfers"), None, tokens).into_any_element(),
        group(
            [row(
                t!("settings.sftp.parallel"),
                Some(t!(
                    "settings.sftp.parallel_description",
                    min = *PARALLEL_TRANSFERS_RANGE.start(),
                    max = *PARALLEL_TRANSFERS_RANGE.end()
                )),
                parallel,
                tokens,
            )],
            tokens,
        )
        .into_any_element(),
        section_title(t!("settings.sftp.file_list"), None, tokens).into_any_element(),
        group(
            [
                row(
                    t!("settings.sftp.local_directory"),
                    Some(t!("settings.sftp.local_directory_description")),
                    dialog.local_directory.clone(),
                    tokens,
                ),
                row_with_note(
                    t!("settings.sftp.icon_style"),
                    Some(icon_preview().into_any_element()),
                    icon_style,
                ),
                row(
                    t!("settings.sftp.show_hidden_local"),
                    Some(t!("settings.sftp.show_hidden_description")),
                    hidden_switch(
                        "sftp-show-hidden-local",
                        t!("settings.sftp.show_hidden_local"),
                        CardSide::Local,
                        settings.show_hidden.local,
                        &store,
                    ),
                    tokens,
                ),
                row(
                    t!("settings.sftp.show_hidden_remote"),
                    None,
                    hidden_switch(
                        "sftp-show-hidden-remote",
                        t!("settings.sftp.show_hidden_remote"),
                        CardSide::Remote,
                        settings.show_hidden.remote,
                        &store,
                    ),
                    tokens,
                ),
            ],
            tokens,
        )
        .into_any_element(),
    ]
}

/// 图标风格的示例，和文件列表用同一个图标件，所以跟着风格和深浅色变。
fn icon_preview() -> impl IntoElement {
    div()
        .flex()
        .gap(px(10.))
        .mt(px(4.))
        .children(
            ICON_PREVIEW
                .iter()
                .enumerate()
                .map(|(index, (name, kind))| {
                    div()
                        .id(("sftp-icon-preview", index))
                        .test_support()
                        .child(FileIcon::new(*name, *kind))
                }),
        )
}

/// 一侧的「显示隐藏文件」开关。
fn hidden_switch(
    id: &'static str,
    label: SharedString,
    side: CardSide,
    checked: bool,
    store: &Entity<SettingsStore>,
) -> impl IntoElement {
    let store = store.clone();
    Switch::new(id)
        .checked(checked)
        .accessibility_label(label)
        .on_click(move |checked: &bool, _, cx| {
            let checked = *checked;
            write(&store, cx, |settings| match side {
                CardSide::Local => settings.sftp.show_hidden.local = checked,
                CardSide::Remote => settings.sftp.show_hidden.remote = checked,
            })
        })
}

/// 本地默认打开目录：一个输入框（留空是「下载」文件夹，灰字显示它的路径），和「选择…」按钮。
/// 设置里只存用户选过的路径，默认的不存：数据目录能拷到别的电脑。
pub struct LocalDirectoryEditor {
    store: Entity<SettingsStore>,
    input: Entity<InputState>,
    /// 留空时用的目录，只用来显示。
    downloads: SharedString,
    _subscriptions: [Subscription; 2],
}

impl LocalDirectoryEditor {
    pub fn new(
        store: Entity<SettingsStore>,
        downloads: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let downloads: SharedString = downloads.to_string_lossy().into_owned().into();
        let shown = stored_text(&store, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(downloads.clone())
                .default_value(shown)
        });
        let subscriptions = [
            cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().trim().to_string();
                    if text != stored_text(&this.store, cx) {
                        let directory = (!text.is_empty()).then(|| PathBuf::from(text));
                        this.write(directory, cx);
                    }
                }
            }),
            // 输入框自己的改动回来时一样（只差两头的空白）；「选择…」选的显示到输入框里。
            cx.observe_in(&store, window, |this, store, window, cx| {
                let shown = stored_text(&store, cx);
                if this.input.read(cx).value().trim() != shown {
                    this.input
                        .update(cx, |input, cx| input.set_value(shown, window, cx));
                }
            }),
        ];
        Self {
            store,
            input,
            downloads,
            _subscriptions: subscriptions,
        }
    }

    fn write(&self, directory: Option<PathBuf>, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            store.update(|settings| settings.sftp.local_directory = directory, cx)
        });
    }

    /// 用系统的选择框挑一个目录。
    fn choose(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("settings.sftp.choose_prompt")),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(directory) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| this.write(Some(directory), cx))
                .ok();
        })
        .detach();
    }
}

/// 设置里存的目录，照输入框显示的样子；没设是空的。
fn stored_text(store: &Entity<SettingsStore>, cx: &App) -> String {
    store
        .read(cx)
        .settings()
        .sftp
        .local_directory
        .map(|directory| directory.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Render for LocalDirectoryEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .w(px(320.))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.input)
                        .id("sftp-local-directory")
                        .aria_label(t!(
                            "settings.sftp.local_directory_label",
                            path = self.downloads.as_ref()
                        ))
                        .small()
                        .font_family(cx.theme().mono_font_family.clone()),
                ),
            )
            .child(
                Button::new("sftp-local-directory-choose")
                    .label(t!("settings.sftp.choose"))
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
            )
    }
}
