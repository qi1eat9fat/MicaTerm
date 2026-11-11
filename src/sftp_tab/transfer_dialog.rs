// 参考 since2006/shell-rs @ b2a307a: src/explorer/transfer_dialog.rs 改写
// 改造：走 `DialogLayer` 和 `open_choice_dialog`，问题框照全部界面原型 `sftpconf`（要传的 / 已有的两行对比）；
// 去掉英文和复数写法。
//! 传输的两种框：上传或下载的确认框（列出项目、来源、目标），和引擎问的问题（冲突、重试、续传）。

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{CardSide, FileIcon, FileIconKind, QueueEntry, format_changed, format_size};
use crate::backend::sftp::{
    FileFacts, TransferChoice, TransferDirection, TransferQuestion, TransferQuestionKind,
    describe_reason_link,
};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Choice, ChoiceButton, ChoiceTone, ConfirmLine};
use crate::ui::form::{FieldNote, dialog_footer, form_field};

/// 确认框的宽（原型 `.dlg.small`）。
const DIALOG_WIDTH: Pixels = px(480.);

/// 确认框里的一项。整条路径换行时一项看着像两项，所以共同的目录写一次，每项一行只写名字。
struct SourceItem {
    path: String,
    folder: String,
    name: String,
    is_dir: bool,
}

impl SourceItem {
    fn new(path: String, side: CardSide, is_dir: bool) -> Self {
        let (folder, name) = if side.is_remote() {
            match path.rsplit_once('/') {
                Some(("", name)) => ("/".to_string(), name.to_string()),
                Some((folder, name)) => (folder.to_string(), name.to_string()),
                None => (String::new(), path.clone()),
            }
        } else {
            let local = Path::new(&path);
            (
                local
                    .parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default(),
                local
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
            )
        };
        Self {
            path,
            folder,
            name,
            is_dir,
        }
    }
}

/// 一次上传或下载传的是什么。
pub(super) struct TransferSources {
    /// 全部项目共同的目录。
    folder: Option<String>,
    items: Vec<SourceItem>,
}

impl TransferSources {
    /// 每项是 `(路径, 是不是远程路径, 是不是文件夹)`。
    pub(super) fn new(items: Vec<(String, CardSide, bool)>) -> Self {
        let items: Vec<_> = items
            .into_iter()
            .map(|(path, side, is_dir)| SourceItem::new(path, side, is_dir))
            .collect();
        let folder = items
            .first()
            .map(|first| first.folder.clone())
            .filter(|folder| items.iter().all(|item| item.folder == *folder));
        Self { folder, items }
    }

    /// `2 个文件`、`1 个文件夹`、`3 个项目（1 个文件夹、2 个文件）`。
    fn count(&self) -> SharedString {
        let folders = self.items.iter().filter(|item| item.is_dir).count();
        let files = self.items.len() - folders;
        let of_files = || t!("sftp_transfer.confirm.files", count = files);
        let of_folders = || t!("sftp_transfer.confirm.folders", count = folders);
        match (folders, files) {
            (0, _) => of_files(),
            (_, 0) => of_folders(),
            _ => t!(
                "sftp_transfer.confirm.items",
                count = folders + files,
                folders = of_folders(),
                files = of_files()
            ),
        }
    }

    fn render(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let shared_folder = self.folder.is_some();
        let (line, faint) = (tokens.line, tokens.faint);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .when_some(self.folder.clone(), |this, folder| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(faint)
                        .child(t!("sftp_transfer.confirm.in_folder", folder = folder)),
                )
            })
            .child(
                div()
                    .id("transfer-source-list")
                    .flex()
                    .flex_col()
                    .max_h(px(128.))
                    .overflow_y_scroll()
                    .py(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(line)
                    .children(self.items.iter().map(|item| {
                        div()
                            .id(ElementId::Name(format!("source:{}", item.path).into()))
                            .test_support()
                            .aria_label(item.name.clone())
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .px(px(8.))
                            .py(px(2.))
                            .child(FileIcon::new(
                                item.name.clone(),
                                if item.is_dir {
                                    FileIconKind::Folder
                                } else {
                                    FileIconKind::File
                                },
                            ))
                            .child(div().min_w_0().child(item.name.clone()))
                            // 来自不同目录的项目各写各的目录。
                            .when(!shared_folder, |this| {
                                this.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(11.5))
                                        .text_color(faint)
                                        .child(item.folder.clone()),
                                )
                            })
                    })),
            )
    }
}

/// 上传或下载的确认框的内容。
pub(super) struct TransferForm {
    direction: TransferDirection,
    /// 有别的批次没结束，这一批要排队。
    queued: bool,
    sources: TransferSources,
    target: Entity<InputState>,
    endpoint: String,
    error: Option<SharedString>,
}

impl TransferForm {
    pub(super) fn new(
        direction: TransferDirection,
        queued: bool,
        sources: TransferSources,
        target: String,
        endpoint: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            direction,
            queued,
            sources,
            target: cx.new(|cx| InputState::new(window, cx).default_value(target)),
            endpoint,
            error: None,
        }
    }

    /// 目标过得去就返回它，否则写上原因。下载到本机要完整路径，引擎也只收完整路径。
    fn commit(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let target = self.target.read(cx).value().trim().to_string();
        let problem = match self.direction {
            TransferDirection::Upload if target.is_empty() => {
                Some(t!("sftp_transfer.confirm.target_missing"))
            }
            TransferDirection::Download if !Path::new(&target).is_absolute() => {
                Some(t!("sftp_transfer.confirm.local_target_relative"))
            }
            _ => None,
        };
        self.error = problem;
        cx.notify();
        self.error.is_none().then_some(target)
    }

    /// 选下载到的本机目录。
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choice = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("sftp_transfer.confirm.choose_download_folder")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = choice.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.target.update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    });
                    this.error = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

impl Render for TransferForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let count = self.sources.count();
        let upload = self.direction == TransferDirection::Upload;
        let (summary, label, field, target) = if upload {
            (
                t!(
                    "sftp_transfer.confirm.upload_summary",
                    items = count,
                    endpoint = self.endpoint
                ),
                t!("sftp_transfer.confirm.target"),
                "upload-target",
                Input::new(&self.target)
                    .id("upload-target")
                    .small()
                    .into_any_element(),
            )
        } else {
            (
                t!(
                    "sftp_transfer.confirm.download_summary",
                    items = count,
                    endpoint = self.endpoint
                ),
                t!("sftp_transfer.confirm.download_to"),
                "download-target",
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.target).id("download-target").small()),
                    )
                    .child(
                        Button::new("download-browse")
                            .small()
                            .label(t!("sftp_transfer.confirm.browse"))
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                    )
                    .into_any_element(),
            )
        };
        let notes = self
            .error
            .clone()
            .map(|message| FieldNote::Error { field, message })
            .into_iter()
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.5))
            .child(
                div()
                    .id("transfer-summary")
                    .test_support()
                    .aria_label(summary.clone())
                    .child(summary),
            )
            .child(self.sources.render(&tokens))
            .when(self.queued, |this| {
                this.child(
                    div()
                        .id("transfer-queued-note")
                        .test_support()
                        .text_size(px(11.5))
                        .text_color(tokens.faint)
                        .child(t!("sftp_transfer.confirm.queued_note")),
                )
            })
            .child(form_field(Some(label), target, notes, &tokens))
    }
}

/// 确认了要做的事：参数是用户定下的目录。
pub(super) type OnConfirmed = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// 叠一个确认框，是 `layer` 那一层。回车等于确认，Esc 等于取消，关了焦点回到原来的地方。
pub(super) fn open_form_dialog(
    form: Entity<TransferForm>,
    layer: DialogLayer,
    on_confirmed: OnConfirmed,
    window: &mut Window,
    cx: &mut App,
) {
    let upload = form.read(cx).direction == TransferDirection::Upload;
    let (title, confirm_id, confirm) = if upload {
        (
            t!("sftp_transfer.confirm.upload_title"),
            "upload-confirm",
            t!("sftp_transfer.confirm.upload"),
        )
    } else {
        (
            t!("sftp_transfer.confirm.download_title"),
            "download-confirm",
            t!("sftp_transfer.confirm.download"),
        )
    };
    let focus = window.focused(cx);
    let input = form.read(cx).target.clone();
    layer.open_dismissable(
        move |dialog, _, _| {
            let (commit, on_confirmed) = (form.clone(), on_confirmed.clone());
            dialog
                .title(title.clone())
                .w(DIALOG_WIDTH)
                .rounded(px(14.))
                .overlay_closable(false)
                .child(form.clone())
                .footer(dialog_footer(
                    confirm_id,
                    confirm.clone(),
                    t!("sftp_transfer.confirm.cancel"),
                ))
                .on_ok(
                    move |_, window, cx| match commit.update(cx, |form, cx| form.commit(cx)) {
                        Some(target) => {
                            on_confirmed(target, window, cx);
                            true
                        }
                        None => false,
                    },
                )
        },
        move |window, cx| {
            if let Some(focus) = &focus {
                window.focus(focus, cx);
            }
        },
        window,
        cx,
    );
    // 打开就能改：要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

/// 引擎问的问题写成框（原型 `sftpconf`）。
pub(super) fn question_choice(
    question: &TransferQuestion,
    head: Option<&QueueEntry>,
    host: &str,
) -> Choice {
    let direction = head.map_or(TransferDirection::Upload, |head| head.job().direction());
    let upload = direction == TransferDirection::Upload;
    let either = |upload_text: SharedString, download_text: SharedString| {
        if upload { upload_text } else { download_text }
    };
    let restart = || {
        either(
            t!("sftp_transfer.question.restart_upload"),
            t!("sftp_transfer.question.restart_download"),
        )
    };
    let (title, id, label) = match question.kind() {
        TransferQuestionKind::Conflict => (
            t!("sftp_transfer.question.conflict_title"),
            "overwrite",
            t!("sftp_transfer.question.overwrite"),
        ),
        TransferQuestionKind::Resume => (
            either(
                t!("sftp_transfer.question.resume_upload_title"),
                t!("sftp_transfer.question.resume_download_title"),
            ),
            "resume",
            either(
                t!("sftp_transfer.question.resume_upload"),
                t!("sftp_transfer.question.resume_download"),
            ),
        ),
        TransferQuestionKind::InvalidResume => (
            t!("sftp_transfer.question.invalid_resume_title"),
            "restart",
            restart(),
        ),
        TransferQuestionKind::Error => (
            either(
                t!("sftp_transfer.question.upload_failed_title"),
                t!("sftp_transfer.question.download_failed_title"),
            ),
            "retry",
            t!("sftp_transfer.question.retry"),
        ),
    };
    let mut buttons = vec![
        ChoiceButton::new("cancel", t!("sftp_transfer.question.cancel")).left(),
        ChoiceButton::new("skip", t!("sftp_transfer.question.skip")),
    ];
    // 续传的框多一个「重新开始」，原型没画，shell-rs 有。
    if question.kind() == TransferQuestionKind::Resume {
        buttons.push(ChoiceButton::new("restart", restart()));
    }
    buttons.push(ChoiceButton::new(id, label).tone(ChoiceTone::Primary));
    let conflict = question.conflict();
    // 目标在传输期间变了的那次不许「都这样处理」：引擎也只记没变的。
    let check = (question.kind() == TransferQuestionKind::Conflict
        && !conflict.is_some_and(|conflict| conflict.changed()))
    .then(|| t!("sftp_transfer.question.apply_to_all"));
    Choice {
        id: "transfer-question",
        title,
        subtitle: head.map(|head| batch_subtitle(head, direction)),
        lines: question_lines(question, direction, host),
        check,
        buttons,
    }
}

/// `上传 3 项到 /etc/nginx/`。
fn batch_subtitle(head: &QueueEntry, direction: TransferDirection) -> SharedString {
    let (count, target) = (head.job().count(), head.job().target());
    match direction {
        TransferDirection::Upload => t!(
            "sftp_transfer.question.upload_subtitle",
            count = count,
            target = target
        ),
        TransferDirection::Download => t!(
            "sftp_transfer.question.download_subtitle",
            count = count,
            target = target
        ),
    }
}

fn question_lines(
    question: &TransferQuestion,
    direction: TransferDirection,
    host: &str,
) -> Vec<ConfirmLine> {
    let reason: SharedString = describe_reason_link(question.message()).into();
    let Some(conflict) = question.conflict() else {
        return vec![
            ConfirmLine::Text(question.path().to_string().into()),
            ConfirmLine::Text(reason),
        ];
    };
    let local: SharedString = t!("sftp_tab.local_name");
    let host: SharedString = host.to_string().into();
    let (incoming_side, existing_side) = match direction {
        TransferDirection::Upload => (local, host),
        TransferDirection::Download => (host, local),
    };
    let name = question
        .path()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(question.path());
    let mut lines = vec![ConfirmLine::Text(match conflict.existing() {
        None => reason,
        Some(_) if conflict.changed() => t!("sftp_transfer.question.changed", name = name),
        Some(_) => t!("sftp_transfer.question.exists", name = name),
    })];
    lines.push(ConfirmLine::Detail {
        label: t!("sftp_transfer.question.incoming"),
        value: facts_line(conflict.incoming(), &incoming_side),
    });
    if let Some(existing) = conflict.existing() {
        lines.push(ConfirmLine::Detail {
            label: t!("sftp_transfer.question.existing"),
            value: facts_line(existing, &existing_side),
        });
    }
    lines
}

/// `2.4 KB · 2026/11/3 10:02（本机）`。
fn facts_line(facts: FileFacts, side: &SharedString) -> SharedString {
    let size = format_size(facts.size());
    match facts.modified() {
        Some(modified) => t!(
            "sftp_transfer.question.facts_with_time",
            size = size,
            time = format_changed(modified as i64),
            side = side
        ),
        None => t!("sftp_transfer.question.facts", size = size, side = side),
    }
}

/// 点的按钮是什么回答。没点就关了（Esc）等于取消：不回答的话引擎一直等。
pub(super) fn choice_of(button: Option<&str>) -> TransferChoice {
    match button {
        Some("overwrite") => TransferChoice::Overwrite,
        Some("skip") => TransferChoice::Skip,
        Some("retry") => TransferChoice::Retry,
        Some("resume") => TransferChoice::Resume,
        Some("restart") => TransferChoice::Restart,
        _ => TransferChoice::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::{CardSide, TransferSources, choice_of};
    use crate::backend::sftp::TransferChoice;

    fn sources(items: &[(&str, bool, bool)]) -> TransferSources {
        TransferSources::new(
            items
                .iter()
                .map(|&(path, remote, is_dir)| {
                    (path.to_string(), CardSide::from_remote(remote), is_dir)
                })
                .collect(),
        )
    }

    #[test]
    fn a_selection_is_listed_by_name_under_its_folder_once() {
        let remote = sources(&[("/srv/app", true, true), ("/etc/hosts", true, false)]);
        assert_eq!(remote.folder, None);
        assert_eq!(remote.items[0].folder, "/srv");
        assert_eq!(remote.count(), "2 个项目（1 个文件夹、1 个文件）");

        let root = sources(&[("/boot", true, true)]);
        assert_eq!(root.folder.as_deref(), Some("/"));
        assert_eq!(root.items[0].name, "boot");
        assert_eq!(root.count(), "1 个文件夹");

        let shared = sources(&[("/a/甲", true, false), ("/a/乙", true, false)]);
        assert_eq!(shared.folder.as_deref(), Some("/a"));
        assert_eq!(shared.count(), "2 个文件");
    }

    #[test]
    fn closing_a_question_without_a_button_cancels() {
        assert_eq!(choice_of(None), TransferChoice::Cancel);
        assert_eq!(choice_of(Some("cancel")), TransferChoice::Cancel);
        assert_eq!(choice_of(Some("overwrite")), TransferChoice::Overwrite);
        assert_eq!(choice_of(Some("restart")), TransferChoice::Restart);
    }
}
