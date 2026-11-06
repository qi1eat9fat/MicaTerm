//! 传输：确认框、问题、队列卡片（工单 22 填）。
//!
//! 工单 20 已经接好的：队列本身（`TransferQueue`）、把一批交给引擎、进度和 `Idle` 的记账，
//! 所以 `unfinished_transfers`（关闭确认看它）和标签上的绿点是真的。
//! 留给 22 的是下面的空函数。`TransferState` 是 `SftpTab` 里归这个工单的状态。
//! 上游参考 shell-rs 的 `src/explorer/transfer_dialog.rs`、`queue_panel.rs`、`explorer_panel.rs`。

use std::path::PathBuf;

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use super::{ConnectionState, QueueState, Removal, SftpTab, TransferJob, TransferQueue};
use crate::backend::sftp::{
    DownloadRequest, RemotePath, SftpCommand, TransferAnswer, TransferPhase, TransferProgress,
    TransferQuestion, UploadRequest, describe_sftp_error,
};

/// `SftpTab` 里归传输的状态。
#[derive(Default)]
pub struct TransferState {
    /// 排着的、在跑的和结束了的批次。引擎一次跑一批，标签把下一批交给它。
    pub(super) queue: TransferQueue,
    /// 引擎正在忙一条命令（一批、继续、丢弃），`Idle` 清掉，那时才能交下一批。
    pub(super) engine_busy: bool,
}

impl SftpTab {
    /// 没结束的批次：等着的、在跑的、停着的。关闭标签前要问。
    pub fn unfinished_transfers(&self) -> usize {
        self.transfers.queue.unfinished_count()
    }

    /// 传输队列，测试和队列卡片用。
    pub fn transfer_queue(&self) -> &TransferQueue {
        &self.transfers.queue
    }

    /// 有一批正在传：标签上的绿点。
    pub(super) fn transfer_running(&self) -> bool {
        self.transfers
            .queue
            .head()
            .is_some_and(|head| head.state() == QueueState::Active)
    }

    /// 停着的一批占着队头，等「继续」。
    pub(super) fn head_stopped(&self) -> bool {
        self.transfers
            .queue
            .head()
            .is_some_and(|head| head.state() == QueueState::Stopped)
    }

    /// 引擎闲着就把下一批交给它。变不成请求的一批就此结束，轮到再下一批。
    pub(super) fn start_next_if_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while !self.transfers.engine_busy && self.state == ConnectionState::Connected {
            let Some((_, job)) = self.transfers.queue.start_next() else {
                return;
            };
            match transfer_command(&job) {
                Ok(command) => {
                    self.transfers.engine_busy = true;
                    self.send(command);
                }
                Err(error) => {
                    let message = describe_sftp_error(&error);
                    self.transfers
                        .queue
                        .abandon_head(Removal::NotStarted(message.clone()));
                    window.push_notification(Notification::error(message), cx);
                }
            }
        }
    }

    /// 把确认过的一批放进队列，没有别的在跑就开始。
    pub(super) fn enqueue(
        &mut self,
        job: TransferJob,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state != ConnectionState::Connected {
            return;
        }
        // 现在就拒绝，别等轮到它。
        if let Err(error) = transfer_command(&job) {
            window.push_notification(Notification::error(describe_sftp_error(&error)), cx);
            return;
        }
        self.transfers.queue.push(job);
        self.start_next_if_idle(window, cx);
    }

    /// 引擎报告进度（`SftpEvent::Progress`）。22 在这里接问题框的收起和完成后刷新另一侧。
    pub(super) fn on_progress(
        &mut self,
        progress: TransferProgress,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let _complete = progress.phase() == TransferPhase::Completed;
        self.transfers.queue.on_progress(progress);
    }

    /// 引擎问一个问题（`SftpEvent::Question`）：覆盖、跳过……一定要回答，否则引擎一直等。
    pub(super) fn on_question(
        &mut self,
        _question: TransferQuestion,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 引擎这条命令做完了（`SftpEvent::Idle`）。
    pub(super) fn on_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transfers.engine_busy = false;
        self.start_next_if_idle(window, cx);
    }

    /// F5：问要不要上传本地选中的，或者下载远程选中的。
    pub(super) fn open_transfer(
        &mut self,
        _remote: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 问要不要把这些本地路径上传到 `target`。
    pub(super) fn open_upload(
        &mut self,
        _paths: Vec<PathBuf>,
        _target: String,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 问要不要把这些远程路径下载到 `target`。
    pub(super) fn open_download(
        &mut self,
        _paths: Vec<String>,
        _target: String,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 用原生选择框挑要上传的。
    pub(super) fn choose_files(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// 回答传输的问题。
    pub(super) fn answer_question(&mut self, _request_id: u64, _answer: TransferAnswer) {}

    /// 停下正在跑的一批，进度留着。
    pub(super) fn cancel_transfer(&mut self) {
        self.send(SftpCommand::Cancel);
    }

    /// 接着跑停下的一批。
    pub(super) fn resume_transfer(&mut self) {
        if !self.transfers.engine_busy {
            self.transfers.engine_busy = true;
            self.send(SftpCommand::Resume);
        }
    }

    /// 丢弃停下的那批的续传进度，队列接着走。
    pub(super) fn discard_transfer(&mut self) {
        if !self.transfers.engine_busy && self.head_stopped() {
            self.transfers.queue.abandon_head(Removal::Discarded);
            self.transfers.engine_busy = true;
            self.send(SftpCommand::Discard);
        }
    }

    /// 队列卡片（原型 `queueCard`）。队列空的时候这个位置收起，不占地方。
    pub(super) fn render_queue(&self, _cx: &mut Context<Self>) -> AnyElement {
        div().size_full().into_any_element()
    }
}

/// 队列里一批对应的引擎命令；路径引擎不收时失败。
fn transfer_command(job: &TransferJob) -> anyhow::Result<SftpCommand> {
    match job {
        TransferJob::Upload { paths, target } => RemotePath::new(target.clone())
            .and_then(|target| UploadRequest::new(paths.clone(), target))
            .map(SftpCommand::Upload),
        TransferJob::Download { paths, target } => paths
            .iter()
            .map(|path| RemotePath::new(path.clone()))
            .collect::<anyhow::Result<Vec<_>>>()
            .and_then(|paths| DownloadRequest::new(paths, target.into()))
            .map(SftpCommand::Download),
    }
}
