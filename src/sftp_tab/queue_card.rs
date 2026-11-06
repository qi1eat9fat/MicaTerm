// 参考 since2006/shell-rs @ b2a307a: src/explorer/queue_panel.rs 改写
// 改造：按全部界面原型的 `queueCard` 做成第三张卡片：头部、工具条（图标加文字）、按百分比摆的七列、5 高的进度条。
// 裁剪：去掉右侧栏用的窄版和大小格式。
//! 传输队列卡片：有传输才出现。每批一行，在跑的那批下面一行是正在传的文件，展开看每一项怎么结束的。

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{QueueEntry, QueueId, QueueState, SftpTab, format_size, percent};
use crate::app::{CatalogIcon, SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{TransferDirection, TransferOutcome, TransferPhase, describe_reason};
use crate::card::render_program_tile;
use crate::i18n::t;
use crate::programs::Program;
use crate::theme::{CARD_HEADER_HEIGHT, CARD_RADIUS, Tokens};

/// 工具条的高（原型 `.qtb`）。
const TOOLBAR_HEIGHT: Pixels = px(30.);
/// 列标题的高（原型 `.ftab th`）。
const COLUMN_HEADER_HEIGHT: Pixels = px(26.);
/// 一行的高（原型 `.queue table td`）。
const ROW_HEIGHT: Pixels = px(22.);

/// 七列的宽（原型的 `colgroup`）：操作、来源、目标、已传输、剩余、速度、进度。
const OPERATION: f32 = 0.09;
const SOURCE: f32 = 0.30;
const TARGET: f32 = 0.20;
const DONE: f32 = 0.09;
const REMAINING: f32 = 0.08;
const SPEED: f32 = 0.09;
const PROGRESS: f32 = 0.15;

/// 一个列格。
fn cell(width: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(relative(width))
        .min_w_0()
        .px(px(8.))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
}

/// 进度条的颜色（原型 `.bar`、`.bar.ok`、`.bar.stop`）。
fn bar(fraction: f32, color: Hsla, tokens: &Tokens) -> Div {
    div()
        .flex_1()
        .min_w(px(16.))
        .h(px(5.))
        .rounded(px(3.))
        .bg(tokens.pressed)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0., 1.)))
                .rounded(px(3.))
                .bg(color),
        )
}

/// `0:00:03`。
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// 头部的灰字：`1 批进行中 · 1 批等待`。都结束了就说结束了几批。
fn summary(entries: &[QueueEntry]) -> String {
    let count = |state: QueueState| entries.iter().filter(|e| e.state() == state).count();
    let parts: Vec<SharedString> = [
        (count(QueueState::Active), "running"),
        (count(QueueState::Stopped), "stopped"),
        (count(QueueState::Pending), "pending"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, kind)| match kind {
        "running" => t!("sftp_transfer.queue.running", count = count),
        "stopped" => t!("sftp_transfer.queue.stopped", count = count),
        _ => t!("sftp_transfer.queue.pending", count = count),
    })
    .collect();
    if parts.is_empty() {
        return t!("sftp_transfer.queue.finished", count = entries.len()).to_string();
    }
    parts.join(" \u{b7} ")
}

impl SftpTab {
    /// 队列卡片（原型 `queueCard`）。队列空的时候这个位置收起，不占地方。
    pub(super) fn render_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::current(cx);
        let queue = &self.transfers.queue;
        let tile = Program {
            name: t!("sftp_transfer.queue.title").to_string(),
            glyph: "icons/upload.svg",
            tile_bg: 0x16a34a,
            tile_fg: 0xffffff,
            border: false,
            shell: false,
            letter: None,
            off: false,
        };
        let summary = summary(queue.entries());
        div()
            .id("transfer-queue")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .rounded(CARD_RADIUS)
            .bg(tokens.card)
            .border_1()
            .border_color(tokens.card_border)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(7.))
                    .h(CARD_HEADER_HEIGHT)
                    .pl(px(12.))
                    .pr(px(8.))
                    .child(render_program_tile(
                        "transfer-queue-tile".into(),
                        &tile,
                        &tokens,
                    ))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("sftp_transfer.queue.title")),
                    )
                    .child(
                        div()
                            .id("transfer-queue-summary")
                            .test_support()
                            .aria_label(summary.clone())
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tokens.muted)
                            .child(summary),
                    ),
            )
            .child(self.render_queue_toolbar(&tokens))
            .child(render_column_header(&tokens))
            .child(
                div()
                    .id("transfer-queue-rows")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px(px(4.))
                    .pb(px(4.))
                    .overflow_y_scroll()
                    .text_size(px(12.5))
                    .children(queue.entries().iter().map(|entry| {
                        let id = entry.id();
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .child(self.render_batch_row(entry, &tokens))
                            .children(render_file_row(entry, &tokens))
                            .when(queue.is_expanded(id), |this| {
                                this.child(render_item_results(entry, &tokens))
                            })
                    })),
            )
            .into_any_element()
    }

    /// 发给这个标签的命令。
    fn queue_command(
        &self,
        command: SftpTabCommand,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let (dispatch, tab) = (self.dispatch.clone(), self.id());
        move |_, window, cx| {
            dispatch.dispatch_sftp_action(&SftpAction::new(tab, command.clone()), window, cx);
        }
    }

    /// 工具条（原型 `.qtb`）：继续、停止、移出队列 ｜ 丢弃续传进度、清除已完成。
    fn render_queue_toolbar(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let queue = &self.transfers.queue;
        let tool = |id: &'static str,
                    icon: Option<Icon>,
                    label: SharedString,
                    enabled: bool,
                    command: SftpTabCommand| {
            Button::new(id)
                .ghost()
                .small()
                .when_some(icon, |button, icon| button.icon(icon))
                .label(label)
                .disabled(!enabled)
                .on_click(self.queue_command(command))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(TOOLBAR_HEIGHT)
            .px(px(8.))
            .child(tool(
                "resume-transfer",
                Some(Icon::new(CatalogIcon::Play)),
                t!("sftp_transfer.queue.resume"),
                self.can_resume_transfer(),
                SftpTabCommand::ResumeTransfer,
            ))
            .child(tool(
                "cancel-transfer",
                Some(Icon::new(CatalogIcon::Square)),
                t!("sftp_transfer.queue.stop"),
                self.transfer_running(),
                SftpTabCommand::CancelTransfer,
            ))
            .child(tool(
                "remove-transfer",
                Some(Icon::new(IconName::Close)),
                t!("sftp_transfer.queue.remove"),
                self.can_remove_transfer(),
                SftpTabCommand::RemoveQueueEntry,
            ))
            .child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h(px(14.))
                    .mx(px(4.))
                    .bg(tokens.separator),
            )
            .child(tool(
                "discard-transfer",
                None,
                t!("sftp_transfer.queue.discard"),
                self.can_discard_transfer(),
                SftpTabCommand::DiscardTransfer,
            ))
            .child(tool(
                "clear-finished-transfers",
                None,
                t!("sftp_transfer.queue.clear_finished"),
                queue.has_finished(),
                SftpTabCommand::ClearFinishedTransfers,
            ))
    }

    /// 一批：传什么到哪、传了多少、走到哪了。点一下选中，三角展开每一项的结果。
    fn render_batch_row(&self, entry: &QueueEntry, tokens: &Tokens) -> impl IntoElement + use<> {
        let queue = &self.transfers.queue;
        let QueueId(id) = entry.id();
        let id = id as usize;
        let is_head = queue.head().is_some_and(|head| head.id() == entry.id());
        let expanded = queue.is_expanded(entry.id());
        let selected = queue.is_selected(entry.id());
        let state = entry.state();
        let status = entry.status();
        let progress = entry.progress();
        let transferring = state == QueueState::Active
            && progress.is_some_and(|progress| progress.phase() == TransferPhase::Transferring);
        let direction = match entry.job().direction() {
            TransferDirection::Upload => t!("sftp_transfer.queue.upload"),
            TransferDirection::Download => t!("sftp_transfer.queue.download"),
        };
        // 测试等的是队头的状态，不管它是什么。
        let status_id: ElementId = if is_head {
            "transfer-status".into()
        } else {
            ("queue-status", id).into()
        };
        let source = source_text(entry);
        let target = entry.job().target().to_string();
        let done = progress.map(|progress| format_size(progress.completed_bytes()));
        let remaining = progress
            .filter(|_| transferring)
            .and_then(|progress| progress.remaining())
            .map(clock);
        let speed = progress
            .filter(|progress| transferring && progress.bytes_per_second() > 0)
            .map(|progress| {
                t!(
                    "sftp_transfer.queue.speed",
                    speed = format_size(progress.bytes_per_second())
                )
            });
        let fraction = match state {
            QueueState::Done => Some(1.0),
            QueueState::Stopped => progress.map(|progress| progress.fraction()),
            QueueState::Active => progress
                .filter(|progress| {
                    matches!(
                        progress.phase(),
                        TransferPhase::Transferring | TransferPhase::Waiting
                    )
                })
                .map(|progress| progress.fraction()),
            _ => None,
        };
        let bar_color = match state {
            QueueState::Done => tokens.busy,
            QueueState::Stopped => tokens.attention,
            _ => tokens.accent,
        };
        let (faint, hover, selection) = (tokens.faint, tokens.hover, tokens.selection_solid);
        let bar_tokens = tokens.clone();
        div()
            .id(("queue-entry", id))
            .test_support()
            .aria_selected(selected)
            .aria_label(status.clone())
            .flex()
            .flex_none()
            .items_center()
            .h(ROW_HEIGHT)
            .rounded(px(6.))
            .when(selected, |row| row.bg(selection))
            .when(!selected, |row| row.hover(|style| style.bg(hover)))
            .when(state.is_finished(), |row| row.text_color(faint))
            .on_click(self.queue_command(SftpTabCommand::SelectQueueEntry { id: id as u64 }))
            .child(
                cell(OPERATION)
                    .gap(px(2.))
                    .child(
                        Button::new(("queue-expand", id))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            }))
                            .on_click({
                                let toggle = self.queue_command(SftpTabCommand::ToggleQueueEntry {
                                    id: id as u64,
                                });
                                move |event, window, cx| {
                                    cx.stop_propagation();
                                    toggle(event, window, cx)
                                }
                            }),
                    )
                    .child(direction),
            )
            .child(cell(SOURCE).child(source))
            .child(
                cell(TARGET)
                    .text_size(px(11.5))
                    .text_color(faint)
                    .child(target),
            )
            .child(
                cell(DONE)
                    .id(("queue-done", id))
                    .test_support()
                    .aria_label(done.clone().unwrap_or_default())
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(faint)
                    .children(done),
            )
            .child(
                cell(REMAINING)
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(faint)
                    .children(remaining),
            )
            .child(
                cell(SPEED)
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(faint)
                    .children(speed),
            )
            .child(
                cell(PROGRESS)
                    .id(status_id)
                    .test_support()
                    .aria_label(status.clone())
                    .gap(px(6.))
                    .when_some(fraction, |this, fraction| {
                        this.child(bar(fraction, bar_color, &bar_tokens))
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(faint)
                            .child(status),
                    ),
            )
    }
}

/// 来源列：一项写它自己，多项写共同的目录和项数（原型 `/etc/nginx/（3 项）`）。
fn source_text(entry: &QueueEntry) -> String {
    let label = entry.job().source_label();
    match entry.job().count() {
        1 => label,
        count => t!(
            "sftp_transfer.queue.source_items",
            source = label,
            count = count
        )
        .to_string(),
    }
}

fn render_column_header(tokens: &Tokens) -> impl IntoElement + use<> {
    let column = |width: f32, title: SharedString, right: bool| {
        cell(width)
            .when(right, |cell| cell.justify_end())
            .child(title)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(COLUMN_HEADER_HEIGHT)
        .mx(px(4.))
        .border_b_1()
        .border_color(tokens.line)
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(tokens.faint)
        .child(column(
            OPERATION,
            t!("sftp_transfer.queue.column.operation"),
            false,
        ))
        .child(column(
            SOURCE,
            t!("sftp_transfer.queue.column.source"),
            false,
        ))
        .child(column(
            TARGET,
            t!("sftp_transfer.queue.column.target"),
            false,
        ))
        .child(column(DONE, t!("sftp_transfer.queue.column.done"), true))
        .child(column(
            REMAINING,
            t!("sftp_transfer.queue.column.remaining"),
            true,
        ))
        .child(column(SPEED, t!("sftp_transfer.queue.column.speed"), true))
        .child(column(
            PROGRESS,
            t!("sftp_transfer.queue.column.progress"),
            false,
        ))
}

/// 正在传的文件，在它那批下面，有自己的进度条。
fn render_file_row(entry: &QueueEntry, tokens: &Tokens) -> Option<impl IntoElement + use<>> {
    let progress = entry.progress()?;
    if entry.state() != QueueState::Active || progress.current_source().is_empty() {
        return None;
    }
    let QueueId(id) = entry.id();
    let id = id as usize;
    let source = progress.current_source().to_string();
    let name = source
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source.as_str())
        .to_string();
    let fraction = progress.current_fraction();
    let text = percent(fraction);
    Some(
        div()
            .id(("queue-file", id))
            .test_support()
            .aria_label(source)
            .flex()
            .flex_none()
            .items_center()
            .h(ROW_HEIGHT)
            .text_color(tokens.faint)
            .child(cell(OPERATION))
            .child(cell(SOURCE).pl(px(22.)).child(name))
            .child(cell(TARGET))
            .child(
                cell(DONE)
                    .justify_end()
                    .text_size(px(11.5))
                    .child(format_size(progress.current_bytes())),
            )
            .child(cell(REMAINING))
            .child(cell(SPEED))
            .child(
                cell(PROGRESS)
                    .id(("queue-file-status", id))
                    .test_support()
                    .aria_label(text.clone())
                    .gap(px(6.))
                    .child(bar(fraction, tokens.accent, tokens))
                    .child(div().flex_none().text_size(px(11.)).child(text)),
            ),
    )
}

/// 展开的一批：每一项怎么结束的，最新的在上面。这是批次结束后的汇总，失败的写着原因。
fn render_item_results(entry: &QueueEntry, tokens: &Tokens) -> impl IntoElement + use<> {
    let (faint, busy, bad) = (tokens.faint, tokens.busy, tokens.bad);
    let details = entry
        .progress()
        .map(|progress| progress.details())
        .unwrap_or_default();
    let upload = entry.job().direction() == TransferDirection::Upload;
    let empty = details.is_empty().then(|| {
        div()
            .id("transfer-detail-empty")
            .test_support()
            .py(px(2.))
            .text_color(faint)
            .child(if entry.state() == QueueState::Pending {
                t!("sftp_transfer.queue.not_started")
            } else if upload {
                t!("sftp_transfer.queue.nothing_uploaded")
            } else {
                t!("sftp_transfer.queue.nothing_downloaded")
            })
    });
    div()
        .id(("transfer-detail-list", entry.id().0 as usize))
        .test_support()
        .flex()
        .flex_col()
        .flex_none()
        .pl(relative(OPERATION))
        .pb(px(4.))
        .text_size(px(11.5))
        .children(empty)
        .children(details.iter().rev().map(|detail| {
            let (icon, color, outcome) = match detail.outcome() {
                TransferOutcome::Done => (
                    Icon::new(IconName::CircleCheck),
                    busy,
                    t!("sftp_transfer.queue.outcome.done"),
                ),
                TransferOutcome::Skipped => (
                    Icon::new(CatalogIcon::CircleMinus),
                    faint,
                    t!("sftp_transfer.queue.outcome.skipped"),
                ),
                TransferOutcome::Failed => (
                    Icon::new(IconName::CircleX),
                    bad,
                    t!("sftp_transfer.queue.outcome.failed"),
                ),
            };
            div()
                .id(ElementId::Name(
                    format!("transfer-detail:{}", detail.path()).into(),
                ))
                .test_support()
                .aria_label(format!("{outcome} {}", detail.path()))
                .flex()
                .flex_none()
                .items_start()
                .gap(px(6.))
                .py(px(2.))
                .child(icon.xsmall().text_color(color))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(detail.path().to_string())
                        .when_some(detail.reason(), |this, reason| {
                            this.child(div().text_color(bad).child(describe_reason(reason)))
                        }),
                )
        }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::clock;

    #[test]
    fn times_read_as_hours_minutes_seconds() {
        assert_eq!(clock(Duration::from_secs(3)), "0:00:03");
        assert_eq!(clock(Duration::from_millis(3_725_900)), "1:02:05");
    }
}
