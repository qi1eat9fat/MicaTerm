// 参考 since2006/shell-rs @ b2a307a: src/explorer/queue_panel.rs 改写
// 改造：照 26 号「SFTP」的传输队列（`.sf-q`）：标题行 + 一排文字按钮，下面和文件列表同一种表格（`ui::table`）。
// 裁剪：去掉右侧栏用的窄版和大小格式。
//! 传输队列：有传输才出现在两栏下面。每批一行，在跑的那批下面一行是正在传的文件，
//! 点折叠箭头展开看每一项怎么结束的。

use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{QueueEntry, QueueId, QueueState, SftpTab, format_size, percent};
use crate::app::{CatalogIcon, SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{TransferDirection, TransferOutcome, TransferPhase, describe_reason};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::icon_button::TextTooltip;
use crate::ui::table::{
    self, Align, ColumnWidth, RowState, TableColumn, cell, header_cell, header_row, text_cell,
};

/// 标题行的高（`.sf-qh`）、按钮的高（`.qb`）。
const TITLE_HEIGHT: Pixels = px(32.);
const BUTTON_HEIGHT: Pixels = px(24.);
/// 折叠箭头（`.sf-q .sf-r .op svg`）：12 见方，左边 13，和文件列表的图标在同一条竖线上。
const CHEVRON: Pixels = px(12.);
const CHEVRON_LEFT: Pixels = px(13.);
/// 12 的图标在 GPUI 里比字面中心高 1.5 个设备像素（150% 实测），往下挪 1。
const ICON_NUDGE: Pixels = px(1.);
/// 批次里的文件、展开的结果缩进一级（`.sub`）。
const SUB_INDENT: Pixels = px(22.);
/// 进度条（`.pg > i`）的高、状态文字的最小宽（`.pg > em`）。
const BAR_HEIGHT: Pixels = px(4.);
const STATUS_WIDTH: Pixels = px(44.);

/// 七列（26 号 `.sf-q` 的 `grid-template-columns`）：折叠箭头 32、来源、目标 210、文件大小 76、
/// 剩余 68、速度 80、进度 148。表头居中，来源、目标居左，文件大小居右（用户定）。
fn columns() -> Vec<TableColumn> {
    let fixed = |key: &'static str, title: SharedString, width: f32| {
        TableColumn::new(key, title, ColumnWidth::Fixed(px(width))).header(Align::Center)
    };
    vec![
        TableColumn::new("expand", "", ColumnWidth::Fixed(px(32.))).bare(),
        TableColumn::new(
            "source",
            t!("sftp_transfer.queue.column.source"),
            ColumnWidth::Fill,
        ),
        TableColumn::new(
            "target",
            t!("sftp_transfer.queue.column.target"),
            ColumnWidth::Fixed(px(210.)),
        ),
        fixed("size", t!("sftp_transfer.queue.column.size"), 76.).right(),
        fixed("remaining", t!("sftp_transfer.queue.column.remaining"), 68.)
            .right()
            .header(Align::Center),
        fixed("speed", t!("sftp_transfer.queue.column.speed"), 80.)
            .right()
            .header(Align::Center),
        fixed("progress", t!("sftp_transfer.queue.column.progress"), 148.),
    ]
}

/// 列的序号。
const EXPAND: usize = 0;
const SOURCE: usize = 1;
const TARGET: usize = 2;
const SIZE: usize = 3;
const REMAINING: usize = 4;
const SPEED: usize = 5;
const PROGRESS: usize = 6;

/// 表格里的一行。
#[derive(Clone, Copy, Debug)]
enum QueueRow {
    /// 一批，按在队列里的序号。
    Batch(usize),
    /// 那批正在传的文件。
    File(usize),
    /// 展开的一批里第几项的结果（最新的在上面）。
    Detail(usize, usize),
    /// 展开的一批还没有结果。
    NoDetail(usize),
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

/// 标题后面的淡字：`1 批进行中 · 1 批等待`。都结束了就说结束了几批。
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

/// 进度格（`.pg`）：一条 4 高的进度条和后面的状态字。`fraction` 是 `None` 时条不画但占着位置。
fn progress_cell(
    columns: &[TableColumn],
    fraction: Option<f32>,
    color: Hsla,
    status: SharedString,
    tokens: &Tokens,
) -> Div {
    cell(columns, PROGRESS)
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .h(BAR_HEIGHT)
                .rounded(px(2.))
                .overflow_hidden()
                .when(fraction.is_some(), |bar| bar.bg(tokens.line))
                .children(fraction.map(|fraction| {
                    div()
                        .h_full()
                        .w(relative(fraction.clamp(0., 1.)))
                        .rounded(px(2.))
                        .bg(color)
                })),
        )
        .child(
            div()
                .flex_none()
                .min_w(STATUS_WIDTH)
                .max_w(relative(0.7))
                .truncate()
                .text_color(tokens.text2)
                .child(status),
        )
}

impl SftpTab {
    /// 传输队列（26 号 `.sf-q`）。队列空的时候这个位置收起，不占地方。
    pub(super) fn render_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let queue = &self.transfers.queue;
        let summary = summary(queue.entries());
        let columns = columns();
        let rows = self.queue_rows().len();
        div()
            .id("transfer-queue")
            .test_support()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(tokens.content)
            .border_t(size::LINE)
            .border_color(tokens.line)
            .text_size(scale.ui)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .h(TITLE_HEIGHT)
                    .pl(px(12.))
                    .pr(px(8.))
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .text_color(tokens.text)
                            .child(t!("sftp_transfer.queue.title")),
                    )
                    .child(
                        div()
                            .id("transfer-queue-summary")
                            .test_support()
                            .aria_label(summary.clone())
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.st_dim)
                            .child(summary),
                    )
                    .child(div().flex_1())
                    .child(self.render_queue_buttons(&tokens)),
            )
            .child(
                header_row(&tokens).children(
                    (0..columns.len())
                        .map(|ix| header_cell(&columns, ix, None, &tokens).test_support()),
                ),
            )
            .child(table::body(
                "transfer-queue-rows",
                rows,
                &self.transfers.scroll,
                cx,
                Self::render_queue_rows,
            ))
            .child(table::scrollbar(&self.transfers.scroll))
            .into_any_element()
    }

    /// 表格里的行，从上到下：每批一行，在跑的那批下面是正在传的文件，展开的批下面是每一项的结果。
    fn queue_rows(&self) -> Vec<QueueRow> {
        let queue = &self.transfers.queue;
        let mut rows = Vec::new();
        for (ix, entry) in queue.entries().iter().enumerate() {
            rows.push(QueueRow::Batch(ix));
            if file_progress(entry).is_some() {
                rows.push(QueueRow::File(ix));
            }
            if queue.is_expanded(entry.id()) {
                let details = entry
                    .progress()
                    .map_or(0, |progress| progress.details().len());
                if details == 0 {
                    rows.push(QueueRow::NoDetail(ix));
                }
                rows.extend((0..details).map(|detail| QueueRow::Detail(ix, detail)));
            }
        }
        rows
    }

    fn render_queue_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::current(cx);
        let columns = columns();
        let rows = self.queue_rows();
        let entries = self.transfers.queue.entries();
        rows.get(range)
            .unwrap_or_default()
            .iter()
            .map(|row| match *row {
                QueueRow::Batch(ix) => self.render_batch_row(&entries[ix], &columns, &tokens),
                QueueRow::File(ix) => render_file_row(&entries[ix], &columns, &tokens),
                QueueRow::Detail(ix, detail) => render_detail(&entries[ix], detail, &tokens),
                QueueRow::NoDetail(ix) => render_no_detail(&entries[ix], &tokens),
            })
            .collect()
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

    /// 标题行右边的按钮（`.qb`，只有文字）：继续、停止、移出队列 ｜ 丢弃续传进度、清除已完成。
    fn render_queue_buttons(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let queue = &self.transfers.queue;
        let (label, faint, hover) = (tokens.label, tokens.faint, tokens.hover);
        let button = |id: &'static str, text: SharedString, enabled: bool, command| {
            div()
                .id(id)
                .test_support()
                .aria_label(text.clone())
                .flex()
                .flex_none()
                .items_center()
                .h(BUTTON_HEIGHT)
                .px(px(8.))
                .rounded(size::RADIUS)
                .text_color(if enabled { label } else { faint })
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover))
                        .on_click(self.queue_command(command))
                })
                .child(text)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(button(
                "resume-transfer",
                t!("sftp_transfer.queue.resume"),
                self.can_resume_transfer(),
                SftpTabCommand::ResumeTransfer,
            ))
            .child(button(
                "cancel-transfer",
                t!("sftp_transfer.queue.stop"),
                self.transfer_running(),
                SftpTabCommand::CancelTransfer,
            ))
            .child(button(
                "remove-transfer",
                t!("sftp_transfer.queue.remove"),
                self.can_remove_transfer(),
                SftpTabCommand::RemoveQueueEntry,
            ))
            .child(div().flex_none().w(size::LINE).h(px(14.)).bg(tokens.line))
            .child(button(
                "discard-transfer",
                t!("sftp_transfer.queue.discard"),
                self.can_discard_transfer(),
                SftpTabCommand::DiscardTransfer,
            ))
            .child(button(
                "clear-finished-transfers",
                t!("sftp_transfer.queue.clear_finished"),
                queue.has_finished(),
                SftpTabCommand::ClearFinishedTransfers,
            ))
    }

    /// 一批：传什么到哪、多大、走到哪了。点一下选中，折叠箭头展开每一项的结果；
    /// 悬停箭头看是上传还是下载（「操作」一列只留箭头，用户定）。
    fn render_batch_row(
        &self,
        entry: &QueueEntry,
        columns: &[TableColumn],
        tokens: &Tokens,
    ) -> AnyElement {
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
        let size = progress
            .map(|progress| progress.total_bytes())
            .filter(|bytes| *bytes > 0)
            .map(format_size);
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
            QueueState::Done => tokens.ok,
            QueueState::Stopped => tokens.warn,
            _ => tokens.icon,
        };
        let dim = tokens.text2;
        table::row(
            ("queue-entry", id),
            RowState {
                selected,
                focused: true,
            },
            tokens,
        )
        .aria_selected(selected)
        .aria_label(status.clone())
        .on_click(self.queue_command(SftpTabCommand::SelectQueueEntry { id: id as u64 }))
        .child(
            cell(columns, EXPAND)
                .id(("queue-expand", id))
                .test_support()
                .aria_label(direction.clone())
                .aria_expanded(expanded)
                .pl(CHEVRON_LEFT)
                .text_color(tokens.st_dim)
                .tooltip(move |_, cx| TextTooltip::view(direction.clone(), cx))
                .on_click({
                    let toggle =
                        self.queue_command(SftpTabCommand::ToggleQueueEntry { id: id as u64 });
                    move |event, window, cx| {
                        cx.stop_propagation();
                        toggle(event, window, cx)
                    }
                })
                .child(
                    div().relative().top(ICON_NUDGE).child(
                        Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(CHEVRON),
                    ),
                ),
        )
        .child(text_cell(columns, SOURCE, source_text(entry)))
        .child(text_cell(columns, TARGET, entry.job().target().to_string()).text_color(dim))
        .child(
            cell(columns, SIZE)
                .id(("queue-size", id))
                .test_support()
                .aria_label(size.clone().unwrap_or_default())
                .text_color(dim)
                .children(size),
        )
        .child(cell(columns, REMAINING).text_color(dim).children(remaining))
        .child(cell(columns, SPEED).text_color(dim).children(speed))
        .child(
            progress_cell(columns, fraction, bar_color, status.clone().into(), tokens)
                .id(status_id)
                .aria_label(status)
                .test_support(),
        )
        .test_support()
        .into_any_element()
    }
}

/// 一批正在传的文件的进度：只有在跑、知道是哪个文件时有。
fn file_progress(entry: &QueueEntry) -> Option<&crate::backend::sftp::TransferProgress> {
    let progress = entry.progress()?;
    (entry.state() == QueueState::Active && !progress.current_source().is_empty())
        .then_some(progress)
}

/// 来源列：一项写它自己，多项写共同的目录和项数（`/etc/nginx/（3 项）`）。
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

/// 正在传的文件，在它那批下面缩进一级，有自己的大小和进度条。
fn render_file_row(entry: &QueueEntry, columns: &[TableColumn], tokens: &Tokens) -> AnyElement {
    let Some(progress) = file_progress(entry) else {
        return div().into_any_element();
    };
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
    table::row(("queue-file", id), RowState::default(), tokens)
        .test_support()
        .aria_label(source)
        .text_color(tokens.st_dim)
        .child(cell(columns, EXPAND))
        .child(text_cell(columns, SOURCE, name).pl(SUB_INDENT))
        .child(cell(columns, TARGET))
        .child(
            cell(columns, SIZE)
                .text_color(tokens.text2)
                .child(format_size(progress.current_total())),
        )
        .child(cell(columns, REMAINING))
        .child(cell(columns, SPEED))
        .child(
            progress_cell(
                columns,
                Some(fraction),
                tokens.icon,
                text.clone().into(),
                tokens,
            )
            .id(("queue-file-status", id))
            .test_support()
            .aria_label(text),
        )
        .into_any_element()
}

/// 展开的一批里一项怎么结束的（最新的在上面）：图标、路径，失败的后面红字写原因。
fn render_detail(entry: &QueueEntry, index: usize, tokens: &Tokens) -> AnyElement {
    let details = entry
        .progress()
        .map(|progress| progress.details())
        .unwrap_or_default();
    let Some(detail) = details.iter().rev().nth(index) else {
        return div().into_any_element();
    };
    let (icon, color, outcome) = match detail.outcome() {
        TransferOutcome::Done => (
            Icon::new(IconName::CircleCheck),
            tokens.ok,
            t!("sftp_transfer.queue.outcome.done"),
        ),
        TransferOutcome::Skipped => (
            Icon::new(CatalogIcon::CircleMinus),
            tokens.st_dim,
            t!("sftp_transfer.queue.outcome.skipped"),
        ),
        TransferOutcome::Failed => (
            Icon::new(IconName::CircleX),
            tokens.err,
            t!("sftp_transfer.queue.outcome.failed"),
        ),
    };
    table::row(
        ElementId::Name(format!("transfer-detail:{}", detail.path()).into()),
        RowState::default(),
        tokens,
    )
    .aria_label(format!("{outcome} {}", detail.path()))
    .items_center()
    .gap(px(6.))
    .pl(px(32.) + px(8.) + SUB_INDENT)
    .pr(px(8.))
    .text_color(tokens.text2)
    .child(
        div()
            .flex_none()
            .relative()
            .top(ICON_NUDGE)
            .child(icon.size(CHEVRON).text_color(color)),
    )
    .child(div().min_w_0().truncate().child(detail.path().to_string()))
    .when_some(detail.reason(), |row, reason| {
        row.child(
            div()
                .min_w_0()
                .truncate()
                .text_color(tokens.err)
                .child(describe_reason(reason)),
        )
    })
    .test_support()
    .into_any_element()
}

/// 展开的一批还没有结果：还没开始，或者什么都没传。
fn render_no_detail(entry: &QueueEntry, tokens: &Tokens) -> AnyElement {
    let upload = entry.job().direction() == TransferDirection::Upload;
    table::row("transfer-detail-empty", RowState::default(), tokens)
        .test_support()
        .items_center()
        .pl(px(32.) + px(8.) + SUB_INDENT)
        .text_color(tokens.st_dim)
        .child(if entry.state() == QueueState::Pending {
            t!("sftp_transfer.queue.not_started")
        } else if upload {
            t!("sftp_transfer.queue.nothing_uploaded")
        } else {
            t!("sftp_transfer.queue.nothing_downloaded")
        })
        .into_any_element()
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
