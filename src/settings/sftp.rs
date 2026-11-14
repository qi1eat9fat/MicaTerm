// 参考 since2006/shell-rs @ b2a307a: src/settings/settings_panel.rs 的 SFTP 一节
// 改造：分组照原型「设置 › SFTP」；加「本地默认打开目录」。
//! 设置 › SFTP（26 号「设置 › SFTP」）：传输（最大同时传输数量），文件列表（本地默认打开目录、
//! 显示隐藏文件 本机 / 远程）。没有「图标风格」：文件图标只有单色一种（用户定）。
//!
//! 模型在 `model::SftpSettings`，生效在 `apply::apply_sftp`，这里只做界面。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use micaterm_ssh::sftp::PARALLEL_TRANSFERS_RANGE;

use super::controls::{dropdown, field_input, reset_if, row, section, setting_switch};
use super::pages::{WIDE_WIDTH, write};
use super::tab::SettingsTab;
use super::{AppSettings, SettingsStore};
use crate::i18n::t;
use crate::sftp_tab::CardSide;
use crate::theme::Tokens;
use crate::ui::modal::{ModalButtonTone, modal_button};

pub(super) fn sftp(
    tab: &SettingsTab,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let store = tab.store.clone();
    let settings = store.read(cx).settings().sftp;
    let default = AppSettings::default().sftp;

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
            px(96.),
            tokens,
        )
    };

    vec![
        section(t!("settings.sftp.transfers"), None, true, tokens),
        row(
            t!("settings.sftp.parallel"),
            Some(t!(
                "settings.sftp.parallel_description",
                min = *PARALLEL_TRANSFERS_RANGE.start(),
                max = *PARALLEL_TRANSFERS_RANGE.end()
            )),
            reset_if(
                settings.parallel != default.parallel,
                "sftp-parallel",
                &store,
                |settings| settings.sftp.parallel = AppSettings::default().sftp.parallel,
                tokens,
            ),
            parallel,
            tokens,
        ),
        section(t!("settings.sftp.file_list"), None, false, tokens),
        row(
            t!("settings.sftp.local_directory"),
            Some(t!("settings.sftp.local_directory_description")),
            reset_if(
                settings.local_directory.is_some(),
                "sftp-local-directory",
                &store,
                |settings| settings.sftp.local_directory = None,
                tokens,
            ),
            tab.local_directory.clone(),
            tokens,
        ),
        hidden_row(
            CardSide::Local,
            settings.show_hidden.local,
            default.show_hidden.local,
            &store,
            tokens,
        ),
        hidden_row(
            CardSide::Remote,
            settings.show_hidden.remote,
            default.show_hidden.remote,
            &store,
            tokens,
        ),
    ]
}

/// 一侧的「显示隐藏文件」一行：开关，和默认不同时有「恢复默认」。
fn hidden_row(
    side: CardSide,
    checked: bool,
    default: bool,
    store: &Entity<SettingsStore>,
    tokens: &Tokens,
) -> AnyElement {
    let (id, title, description) = match side {
        CardSide::Local => (
            "sftp-show-hidden-local",
            t!("settings.sftp.show_hidden_local"),
            t!("settings.sftp.show_hidden_description"),
        ),
        CardSide::Remote => (
            "sftp-show-hidden-remote",
            t!("settings.sftp.show_hidden_remote"),
            t!("settings.sftp.show_hidden_remote_description"),
        ),
    };
    let set = move |settings: &mut AppSettings, checked: bool| match side {
        CardSide::Local => settings.sftp.show_hidden.local = checked,
        CardSide::Remote => settings.sftp.show_hidden.remote = checked,
    };
    row(
        title.clone(),
        Some(description),
        reset_if(
            checked != default,
            id,
            store,
            move |settings| set(settings, default),
            tokens,
        ),
        setting_switch(id, title, checked, store, set),
        tokens,
    )
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        // 26 号 `.stg-ctl`：输入框 240，后面「选择…」描边按钮。
        let choose = cx.listener(|this, _, _, cx| this.choose(cx));
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div().w(WIDE_WIDTH).child(field_input(
                    Input::new(&self.input)
                        .id("sftp-local-directory")
                        .aria_label(t!(
                            "settings.sftp.local_directory_label",
                            path = self.downloads.as_ref()
                        )),
                    &tokens,
                )),
            )
            .child(modal_button(
                "sftp-local-directory-choose",
                t!("settings.sftp.choose"),
                None,
                ModalButtonTone::Line,
                Some(Box::new(choose)),
                &tokens,
                window,
                cx,
            ))
    }
}
