// 参考 since2006/shell-rs @ b2a307a: src/explorer/transfer_dialog.rs 改写
// 改造：走 `open_choice_dialog`，问题框照全部界面原型 `sftpconf`（要传的 / 已有的两行对比）；
// 去掉英文和复数写法。
//! 上传和下载不再先确认：选好、拖好就直接进队列，目标是对面一侧现在的目录。
//! 引擎问的问题（冲突、重试、续传）写成共用的 `ui::modal` 弹窗，走 `open_choice_dialog`。

use gpui_kit::SharedString;

use super::{QueueEntry, format_changed, format_size};
use crate::backend::sftp::{
    FileFacts, TransferChoice, TransferDirection, TransferQuestion, TransferQuestionKind,
    describe_reason_link,
};
use crate::i18n::t;
use crate::ui::confirm::{Choice, ChoiceButton, ChoiceTone, ConfirmLine};

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
        TransferDirection::Upload => {
            if count == 1 {
                t!(
                    "sftp_transfer.question.upload_subtitle_one",
                    count = count,
                    target = target
                )
            } else {
                t!(
                    "sftp_transfer.question.upload_subtitle",
                    count = count,
                    target = target
                )
            }
        }
        TransferDirection::Download => {
            if count == 1 {
                t!(
                    "sftp_transfer.question.download_subtitle_one",
                    count = count,
                    target = target
                )
            } else {
                t!(
                    "sftp_transfer.question.download_subtitle",
                    count = count,
                    target = target
                )
            }
        }
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
    use super::choice_of;
    use crate::backend::sftp::TransferChoice;

    #[test]
    fn closing_a_question_without_a_button_cancels() {
        assert_eq!(choice_of(None), TransferChoice::Cancel);
        assert_eq!(choice_of(Some("cancel")), TransferChoice::Cancel);
        assert_eq!(choice_of(Some("overwrite")), TransferChoice::Overwrite);
        assert_eq!(choice_of(Some("restart")), TransferChoice::Restart);
    }
}
