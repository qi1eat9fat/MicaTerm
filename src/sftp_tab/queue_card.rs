//! SFTP transfer list panel (redesigned after Muxy's docked panels, `agents_panel`, and meter bars):
//! appears beneath the two file panes whenever the queue is non-empty.
//!
//! - Header (`28px` with bottom hairline): `ArrowUpDown` icon + `Semibold` title + rounded count
//!   pill + status summary on the left; compact `22px` icon+label action buttons on the right.
//! - Streamlined activity rows (`26px` tall, `4px` inset, `6px` rounded hover/selection):
//!   expand chevron + state-tinted `Upload`/`Download` badge + `Source [count] → Target` flow +
//!   live `speed · remaining · size` telemetry + right-aligned `232px` Muxy brand/emerald/amber/pink
//!   progress bar or status indicator.
//! - Active sub-row shows the file currently in flight; expanding a batch reveals per-item outcomes.

use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{QueueEntry, QueueId, QueueState, SftpTab, format_size, percent};
use crate::app::{CatalogIcon, SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{TransferDirection, TransferOutcome, TransferPhase, describe_reason};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, WEIGHT_SEMIBOLD, size, space};
use crate::ui::count::count_badge;
use crate::ui::icon_button::TextTooltip;
use crate::ui::table::{self, RowState};

/// Panel header height and compact toolbar button height.
const HEADER_HEIGHT: Pixels = px(28.);
const ACTION_HEIGHT: Pixels = size::SEGMENT_ITEM;
/// Fixed width of the right-hand progress & status column (`232px`).
const PROGRESS_WIDTH: Pixels = px(232.);
/// Fixed width of the batch progress bar: the status label never squeezes it, so bars line up.
const BAR_WIDTH: Pixels = px(88.);
/// Fixed width of the status label zone: statuses of different lengths share one centre line,
/// and `已完成（跳过 12 项）` still fits without truncating.
const STATUS_WIDTH: Pixels = px(120.);
/// Progress bar heights (batch vs active sub-file).
const BAR_HEIGHT: Pixels = px(4.);
const FILE_BAR_HEIGHT: Pixels = px(3.);
/// Left indent for sub-rows (active file and expanded outcome details) so they align under Source.
const SUB_INDENT: Pixels = px(48.);

/// Virtualized row kind in the transfer list.
#[derive(Clone, Copy, Debug)]
enum QueueRow {
    /// Batch summary row (`entries[ix]`).
    Batch(usize),
    /// Currently transferring file sub-row for active batch `ix`.
    File(usize),
    /// Completed/skipped/failed item detail row (`entries[ix]`, reverse index `detail`).
    Detail(usize, usize),
    /// Placeholder sub-row when an expanded batch has no completed items yet.
    NoDetail(usize),
}

/// Formats remaining duration as `H:MM:SS` (`0:00:03`).
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Header status summary (`1 running · 1 waiting`, or `N finished` when all batches have ended).
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

/// Right-aligned `232px` progress bar + status/percentage cell.
#[allow(clippy::too_many_arguments)]
fn progress_cell(
    fraction: Option<f32>,
    bar_height: Pixels,
    bar_color: Hsla,
    dot_color: Option<Hsla>,
    status_color: Hsla,
    status: SharedString,
    scale: TypeScale,
    tokens: &Tokens,
) -> Div {
    let base = div()
        .flex()
        .flex_none()
        .w(PROGRESS_WIDTH)
        .h_full()
        .items_center()
        .pl(space::SPACE_8)
        .pr(space::SPACE_8)
        .gap(space::SPACE_8)
        .overflow_hidden()
        .whitespace_nowrap();
    match fraction {
        Some(fraction) => base
            .child(
                // 进度条定宽：状态文字再长也只压缩文字，不会把条挤短。
                div()
                    .flex_none()
                    .w(BAR_WIDTH)
                    .h(bar_height)
                    .rounded_full()
                    .overflow_hidden()
                    .bg(tokens.sel)
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction.clamp(0., 1.)))
                            .rounded_full()
                            .bg(bar_color),
                    ),
            )
            .child(
                // 状态区定宽居中：长短状态共用一条中线，和进度条的间距不再忽大忽小；
                // 放不下时由 truncate 从末尾出省略号，不再把开头硬裁掉。
                div()
                    .w(STATUS_WIDTH)
                    .flex_none()
                    .truncate()
                    .text_center()
                    .text_size(scale.sm)
                    .font_weight(WEIGHT_NORMAL)
                    .text_color(status_color)
                    .child(status),
            ),
        None => base.justify_end().gap(space::SPACE_5).children(dot_color.map(|color| {
            crate::ui::dot(color, size::DOT)
        })).child(
            div()
                .w(STATUS_WIDTH)
                .flex_none()
                .truncate()
                .text_center()
                .text_size(scale.sm)
                .font_weight(WEIGHT_NORMAL)
                .text_color(status_color)
                .child(status),
        ),
    }
}

impl SftpTab {
    /// Bottom transfer list panel (`id = "transfer-queue"`). Hidden when the queue is empty.
    pub(super) fn render_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let queue = &self.transfers.queue;
        let total = queue.entries().len();
        let summary = summary(queue.entries());
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
            .text_size(scale.body)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(space::SPACE_6)
                    .h(HEADER_HEIGHT)
                    .pl(space::SPACE_10)
                    .pr(space::SPACE_6)
                    .border_b(size::LINE)
                    .border_color(tokens.line)
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .text_color(tokens.icon)
                            .child(Icon::new(CatalogIcon::ArrowUpDown).size(size::ICON_SM)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(scale.sm)
                            .font_weight(WEIGHT_SEMIBOLD)
                            .text_color(tokens.text)
                            .child(t!("sftp_transfer.queue.title")),
                    )
                    .child(count_badge(total, &tokens, scale))
                    .child(
                        div()
                            .id("transfer-queue-summary")
                            .test_support()
                            .aria_label(summary.clone())
                            .min_w_0()
                            .ml(space::SPACE_2)
                            .truncate()
                            .text_size(scale.sm)
                            .text_color(tokens.st_dim)
                            .child(summary),
                    )
                    .child(div().flex_1())
                    .child(self.render_queue_buttons(&tokens, scale)),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(table::body(
                        "transfer-queue-rows",
                        rows,
                        &self.transfers.scroll,
                        cx,
                        Self::render_queue_rows,
                    ))
                    .child(table::scrollbar_from(px(0.), &self.transfers.scroll)),
            )
            .into_any_element()
    }

    /// Flattens batches, active file sub-rows, and expanded outcome rows in display order.
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
        let scale = TypeScale::current(cx);
        let rows = self.queue_rows();
        let entries = self.transfers.queue.entries();
        rows.get(range)
            .unwrap_or_default()
            .iter()
            .map(|row| match *row {
                QueueRow::Batch(ix) => self.render_batch_row(&entries[ix], scale, &tokens),
                QueueRow::File(ix) => render_file_row(&entries[ix], scale, &tokens),
                QueueRow::Detail(ix, detail) => render_detail(&entries[ix], detail, scale, &tokens),
                QueueRow::NoDetail(ix) => render_no_detail(&entries[ix], scale, &tokens),
            })
            .collect()
    }

    fn queue_command(
        &self,
        command: SftpTabCommand,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let (dispatch, tab) = (self.dispatch.clone(), self.id());
        move |_, window, cx| {
            dispatch.dispatch_sftp_action(&SftpAction::new(tab, command.clone()), window, cx);
        }
    }

    /// Compact Muxy icon+label toolbar buttons on the right of the transfer panel header.
    fn render_queue_buttons(&self, tokens: &Tokens, scale: TypeScale) -> impl IntoElement + use<> {
        let queue = &self.transfers.queue;
        let (text, text2, faint, hover) = (tokens.text, tokens.text2, tokens.faint, tokens.hover);
        let button = |id: &'static str,
                      icon: Icon,
                      label: SharedString,
                      enabled: bool,
                      command: SftpTabCommand| {
            div()
                .id(id)
                .test_support()
                .aria_label(label.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(space::SPACE_4)
                .h(ACTION_HEIGHT)
                .px(space::SPACE_6)
                .rounded(size::RADIUS_SM)
                .text_size(scale.sm)
                .font_weight(WEIGHT_NORMAL)
                .text_color(if enabled { text2 } else { faint })
                .when(!enabled, |button| button.opacity(0.45))
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover).text_color(text))
                        .on_click(self.queue_command(command))
                })
                .child(icon.size(px(11.)))
                .child(label)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_2)
            .child(button(
                "resume-transfer",
                Icon::new(CatalogIcon::Play),
                t!("sftp_transfer.queue.resume"),
                self.can_resume_transfer(),
                SftpTabCommand::ResumeTransfer,
            ))
            .child(button(
                "cancel-transfer",
                Icon::new(CatalogIcon::Square),
                t!("sftp_transfer.queue.stop"),
                self.transfer_running(),
                SftpTabCommand::CancelTransfer,
            ))
            .child(button(
                "remove-transfer",
                Icon::new(IconName::Close),
                t!("sftp_transfer.queue.remove"),
                self.can_remove_transfer(),
                SftpTabCommand::RemoveQueueEntry,
            ))
            .child(
                div()
                    .flex_none()
                    .w(size::LINE)
                    .h(px(12.))
                    .mx(px(3.))
                    .bg(tokens.line),
            )
            .child(button(
                "discard-transfer",
                Icon::new(CatalogIcon::Undo2),
                t!("sftp_transfer.queue.discard"),
                self.can_discard_transfer(),
                SftpTabCommand::DiscardTransfer,
            ))
            .child(button(
                "clear-finished-transfers",
                Icon::new(CatalogIcon::Eraser),
                t!("sftp_transfer.queue.clear_finished"),
                queue.has_finished(),
                SftpTabCommand::ClearFinishedTransfers,
            ))
    }

    /// Renders a transfer batch row: expand toggle + direction badge + `Source [count] → Target` +
    /// telemetry (`speed · remaining · size`) + `148px` progress/status column.
    fn render_batch_row(
        &self,
        entry: &QueueEntry,
        scale: TypeScale,
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
        let failed_count = progress.map_or(0, |p| p.failed());
        let transferring = state == QueueState::Active
            && progress.is_some_and(|progress| progress.phase() == TransferPhase::Transferring);
        let is_upload = entry.job().direction() == TransferDirection::Upload;
        let direction = if is_upload {
            t!("sftp_transfer.queue.upload")
        } else {
            t!("sftp_transfer.queue.download")
        };
        let status_id: ElementId = if is_head {
            "transfer-status".into()
        } else {
            ("queue-status", id).into()
        };
        let size_str = progress
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
            QueueState::Done if failed_count == 0 => Some(1.0),
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
        let (badge_bg, badge_fg, bar_color, dot_color, status_color) = match state {
            QueueState::Active => (
                tokens.brand_soft,
                tokens.brand,
                tokens.brand,
                Some(tokens.brand),
                tokens.text,
            ),
            QueueState::Done if failed_count > 0 => (
                tokens.err_soft,
                tokens.err,
                tokens.err,
                Some(tokens.err),
                tokens.err,
            ),
            QueueState::Done => (
                tokens.ok_soft,
                tokens.ok,
                tokens.ok,
                Some(tokens.ok),
                tokens.ok_text,
            ),
            QueueState::Stopped => (
                tokens.warn_soft,
                tokens.warn,
                tokens.warn,
                Some(tokens.warn),
                tokens.warn,
            ),
            QueueState::Pending => (
                tokens.sel,
                tokens.icon,
                tokens.brand,
                Some(tokens.faint),
                tokens.st_dim,
            ),
            QueueState::Removed => (tokens.sel, tokens.st_dim, tokens.st_dim, None, tokens.st_dim),
        };
        let source_label = entry.job().source_label();
        let item_count = entry.job().count();
        let full_source = source_text(entry);
        let expand_hover = tokens.hover;
        let expand_text = tokens.text;
        let direction_tip = direction;
        let expand_tip = if expanded {
            t!("sftp_transfer.queue.collapse_details")
        } else {
            t!("sftp_transfer.queue.expand_details")
        };

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
        .items_center()
        .gap(space::SPACE_6)
        .on_click(self.queue_command(SftpTabCommand::SelectQueueEntry { id: id as u64 }))
        .child(
            div()
                .id(("queue-expand", id))
                .test_support()
                .aria_label(expand_tip.clone())
                .aria_expanded(expanded)
                .ml(space::SPACE_4)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::CONTROL_SM)
                .rounded(size::RADIUS_SM)
                .text_color(tokens.st_dim)
                .cursor_pointer()
                .hover(move |style| style.bg(expand_hover).text_color(expand_text))
                .tooltip(move |_, cx| TextTooltip::view(expand_tip.clone(), cx))
                .on_click({
                    let toggle =
                        self.queue_command(SftpTabCommand::ToggleQueueEntry { id: id as u64 });
                    move |event, window, cx| {
                        cx.stop_propagation();
                        toggle(event, window, cx)
                    }
                })
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(11.)),
                ),
        )
        .child(
            div()
                .id(("queue-direction", id))
                .test_support()
                .aria_label(direction_tip.clone())
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded(size::RADIUS_SM)
                .bg(badge_bg)
                .text_color(badge_fg)
                .tooltip(move |_, cx| TextTooltip::view(direction_tip.clone(), cx))
                .child(
                    Icon::new(if is_upload {
                        CatalogIcon::Upload
                    } else {
                        CatalogIcon::Download
                    })
                    .size(px(11.)),
                ),
        )
        .child(
            div()
                .id(("queue-flow", id))
                .test_support()
                .aria_label(SharedString::from(format!(
                    "{full_source} \u{2192} {}",
                    entry.job().target()
                )))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(space::SPACE_6)
                .tooltip(move |_, cx| {
                    TextTooltip::view(SharedString::from(full_source.clone()), cx)
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(if state == QueueState::Removed {
                            tokens.st_dim
                        } else {
                            tokens.text
                        })
                        .child(source_label),
                )
                .when(item_count > 1, |flow| {
                    flow.child(count_badge(item_count, &tokens, scale))
                })
                .child(
                    Icon::new(IconName::ArrowRight)
                        .size(px(11.))
                        .flex_none()
                        .text_color(tokens.faint),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink(1.)
                        .truncate()
                        .text_size(scale.sm)
                        .text_color(tokens.st_dim)
                        .child(entry.job().target().to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(space::SPACE_10)
                .pl(space::SPACE_6)
                .text_size(scale.sm)
                .whitespace_nowrap()
                .when_some(speed, |meta, speed| {
                    meta.child(div().text_color(tokens.text2).child(speed))
                })
                .when_some(remaining, |meta, remaining| {
                    meta.child(div().text_color(tokens.st_dim).child(remaining))
                })
                .child(
                    div()
                        .id(("queue-size", id))
                        .test_support()
                        .aria_label(size_str.clone().unwrap_or_default())
                        .flex()
                        .justify_end()
                        .min_w(px(48.))
                        .text_color(tokens.text2)
                        .children(size_str),
                ),
        )
        .child(
            progress_cell(
                fraction,
                BAR_HEIGHT,
                bar_color,
                dot_color,
                status_color,
                status.clone().into(),
                scale,
                tokens,
            )
            .id(status_id)
            // 悬停看完整状态：像「无法开始：<原因>」这种放不下的文案也能读全。
            .tooltip({
                let tip = status.clone();
                move |_, cx| TextTooltip::view(tip.clone().into(), cx)
            })
            .aria_label(status)
            .test_support(),
        )
        .test_support()
        .into_any_element()
    }
}

/// Active per-file progress when the batch is currently transferring a known file.
fn file_progress(entry: &QueueEntry) -> Option<&crate::backend::sftp::TransferProgress> {
    let progress = entry.progress()?;
    (entry.state() == QueueState::Active && !progress.current_source().is_empty())
        .then_some(progress)
}

/// Full source description including item count for tooltips.
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

/// Indented sub-row showing the individual file currently being transferred.
fn render_file_row(entry: &QueueEntry, scale: TypeScale, tokens: &Tokens) -> AnyElement {
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
        .items_center()
        .gap(space::SPACE_6)
        .pl(SUB_INDENT)
        .text_size(scale.sm)
        .text_color(tokens.text2)
        .child(
            Icon::new(IconName::File)
                .size(px(11.))
                .flex_none()
                .text_color(tokens.faint),
        )
        .child(div().flex_1().min_w_0().truncate().child(name))
        .child(
            div()
                .flex_none()
                .pl(space::SPACE_6)
                .text_color(tokens.st_dim)
                .child(format_size(progress.current_total())),
        )
        .child(
            progress_cell(
                Some(fraction),
                FILE_BAR_HEIGHT,
                tokens.brand_progress,
                None,
                tokens.st_dim,
                text.clone().into(),
                scale,
                tokens,
            )
            .id(("queue-file-status", id))
            .test_support()
            .aria_label(text),
        )
        .into_any_element()
}

/// Indented sub-row showing how an item ended (newest first), with an error pill on failure.
fn render_detail(
    entry: &QueueEntry,
    index: usize,
    scale: TypeScale,
    tokens: &Tokens,
) -> AnyElement {
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
    .gap(space::SPACE_6)
    .pl(SUB_INDENT)
    .pr(space::SPACE_8)
    .text_size(scale.sm)
    .text_color(tokens.text2)
    .child(icon.size(size::ICON_SM).flex_none().text_color(color))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(detail.path().to_string()),
    )
    .when_some(detail.reason(), |row, reason| {
        row.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .max_w(relative(0.45))
                .px(space::SPACE_6)
                .h(px(16.))
                .rounded(size::RADIUS_SM)
                .bg(tokens.err_soft)
                .text_color(tokens.err)
                .child(div().min_w_0().truncate().child(describe_reason(reason))),
        )
    })
    .test_support()
    .into_any_element()
}

/// Indented sub-row when an expanded batch has no completed items yet.
fn render_no_detail(entry: &QueueEntry, scale: TypeScale, tokens: &Tokens) -> AnyElement {
    let upload = entry.job().direction() == TransferDirection::Upload;
    table::row("transfer-detail-empty", RowState::default(), tokens)
        .test_support()
        .items_center()
        .pl(SUB_INDENT)
        .pr(space::SPACE_8)
        .text_size(scale.sm)
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
