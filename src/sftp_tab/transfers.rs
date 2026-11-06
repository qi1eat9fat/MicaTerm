// 参考 since2006/shell-rs @ b2a307a: src/explorer/explorer_panel.rs 改写
//! 传输：把确认框、问题框和队列串起来。框的内容在 `transfer_dialog`，队列卡片在 `queue_card`，
//! 排队的记账在 `transfer_queue`。
//!
//! 引擎一次跑一批（一批内的并发数是设置里的 `parallel`，`settings::apply` 设给引擎），标签把下一批交给它；
//! 引擎说 `Idle` 才能交下一批。停着的一批占着队头，等「继续」「移出队列」或「丢弃续传进度」。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use super::transfer_dialog::{
    TransferForm, TransferSources, choice_of, open_form_dialog, question_choice,
};
use super::{ConnectionState, QueueState, Removal, SftpTab, TransferJob, TransferQueue};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{
    DownloadRequest, RemotePath, SftpCommand, TransferAnswer, TransferDirection, TransferPhase,
    TransferProgress, TransferQuestion, UploadRequest, describe_sftp_error,
};
use crate::i18n::t;
use crate::ui::DialogLayer;
use crate::ui::confirm::open_choice_dialog;

/// 开着的问题框。
struct OpenQuestion {
    layer: DialogLayer,
    /// 问之前焦点在哪：从外面收起时还回去。
    focus: Option<FocusHandle>,
}

/// `SftpTab` 里归传输的状态。
#[derive(Default)]
pub struct TransferState {
    /// 排着的、在跑的和结束了的批次。引擎一次跑一批，标签把下一批交给它。
    pub(super) queue: TransferQueue,
    /// 引擎正在忙一条命令（一批、继续、丢弃、忘掉），`Idle` 清掉，那时才能交下一批。
    pub(super) engine_busy: bool,
    /// 开着的确认框，开着时不再叠一个。
    confirmation: Option<DialogLayer>,
    question: Option<OpenQuestion>,
}

impl SftpTab {
    /// 没结束的批次：等着的、在跑的、停着的。关闭标签和退出前要问。
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

    /// 「继续」现在能点：接着跑停下的一批；断开时还有等着的批次，就是连上去让它们走。
    pub fn can_resume_transfer(&self) -> bool {
        let waiting = self
            .transfers
            .queue
            .entries()
            .iter()
            .any(|entry| entry.state() == QueueState::Pending);
        !self.transfers.engine_busy
            && (self.head_stopped() || (self.state == ConnectionState::Disconnected && waiting))
    }

    /// 「移出队列」现在能点。停着的一批要等引擎说完 `Idle`，引擎才收得下「忘掉」。
    pub fn can_remove_transfer(&self) -> bool {
        self.transfers.queue.removable().is_some()
            && !(self.head_stopped() && self.transfers.engine_busy)
    }

    /// 「丢弃续传进度」现在能点。
    pub(super) fn can_discard_transfer(&self) -> bool {
        self.head_stopped() && !self.transfers.engine_busy
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

    /// 把确认过的一批放进队列，轮到了就开始。框开着时连接断了：`execute` 已经问过要不要重新连接；
    /// 正在连接时先排着，连上就走。
    pub(super) fn enqueue(
        &mut self,
        job: TransferJob,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 现在就拒绝，别等轮到它。
        if let Err(error) = transfer_command(&job) {
            window.push_notification(Notification::error(describe_sftp_error(&error)), cx);
            return;
        }
        self.transfers.queue.push(job);
        self.start_next_if_idle(window, cx);
    }

    /// 引擎报告进度（`SftpEvent::Progress`）。
    pub(super) fn on_progress(
        &mut self,
        progress: TransferProgress,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 丢弃以后那条空的「结束」没有队头：不算一批结束。
        let finished = (progress.phase() == TransferPhase::Completed)
            .then(|| {
                self.transfers
                    .queue
                    .head()
                    .map(|head| head.job().direction())
            })
            .flatten();
        let reconnecting = progress.phase() == TransferPhase::Reconnecting;
        let summary = (progress.succeeded(), progress.skipped(), progress.failed());
        self.transfers.queue.on_progress(progress);
        // 问题不比批次活得久：批次停了、结束了或者重连去了，框上的问题就没人等了。
        if reconnecting || !self.transfer_running() {
            self.close_question(window, cx);
        }
        if let Some(direction) = finished {
            // 刷新传到的那一侧。
            self.reload(direction == TransferDirection::Upload, window, cx);
            let (done, skipped, failed) = summary;
            if skipped + failed > 0 {
                let message = t!(
                    "sftp_transfer.summary",
                    done = done,
                    skipped = skipped,
                    failed = failed
                );
                let note = if failed > 0 {
                    Notification::error(message)
                } else {
                    Notification::info(message)
                };
                window.push_notification(note, cx);
            }
        }
    }

    /// 引擎问一个问题（`SftpEvent::Question`）：覆盖、跳过……一定要回答，否则引擎一直等；
    /// 没回答就关掉（Esc）等于取消。叠在别的框上面，答完回到下面的框。
    pub(super) fn on_question(
        &mut self,
        question: TransferQuestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 引擎只等最新的问题。
        self.close_question(window, cx);
        let choice = question_choice(&question, self.transfers.queue.head(), &self.host().name);
        let (dispatch, tab, generation) = (self.dispatch.clone(), self.id(), self.generation());
        let request_id = question.id();
        let focus = window.focused(cx);
        let layer = DialogLayer::new();
        open_choice_dialog(
            choice,
            layer.clone(),
            Box::new({
                let focus = focus.clone();
                move |outcome, window, cx| {
                    if let Some(focus) = &focus {
                        window.focus(focus, cx);
                    }
                    let answer = TransferAnswer::new(choice_of(outcome.button), outcome.checked);
                    dispatch.dispatch_sftp_action(
                        &SftpAction::new(tab, SftpTabCommand::Answer { request_id, answer })
                            .with_generation(generation),
                        window,
                        cx,
                    );
                }
            }),
            window,
            cx,
        );
        self.transfers.question = Some(OpenQuestion { layer, focus });
    }

    /// 收起开着的问题框，不回答：引擎那边已经不等了。
    fn close_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(question) = self.transfers.question.take() else {
            return;
        };
        if question.layer.is_open(window, cx) {
            question.layer.close(window, cx);
            if let Some(focus) = &question.focus {
                window.focus(focus, cx);
            }
        }
    }

    /// 标签要关了：这个标签的框都收起来，引擎的连接跟着断。
    pub(super) fn close_transfer_dialogs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_question(window, cx);
        if let Some(layer) = self.transfers.confirmation.take() {
            layer.close(window, cx);
        }
    }

    /// 引擎这条命令做完了（`SftpEvent::Idle`）。
    pub(super) fn on_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transfers.engine_busy = false;
        self.start_next_if_idle(window, cx);
    }

    /// F5：问要不要上传本地选中的，或者下载远程选中的，目标是另一侧的当前目录。
    pub(super) fn open_transfer(
        &mut self,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.card(!remote).read(cx).path();
        if remote {
            let card = self.card(true).read(cx);
            let paths = card
                .selected_entries(cx)
                .iter()
                .filter(|entry| !entry.is_parent())
                .map(|entry| card.child_path_of(&entry.name))
                .collect();
            self.open_download(paths, target, window, cx);
        } else {
            let paths = self.card(false).read(cx).upload_sources(cx);
            self.open_upload(paths, target, window, cx);
        }
    }

    /// 问要不要把这些本地路径上传到 `target`。
    pub(super) fn open_upload(
        &mut self,
        paths: Vec<PathBuf>,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let sources = self.transfer_sources(
            false,
            paths.iter().map(|path| path.to_string_lossy().into_owned()),
            cx,
        );
        let job = move |target| TransferJob::Upload {
            paths: paths.clone(),
            target,
        };
        self.open_confirmation(TransferDirection::Upload, sources, target, job, window, cx);
    }

    /// 问要不要把这些远程路径下载到 `target`。
    pub(super) fn open_download(
        &mut self,
        paths: Vec<String>,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let sources = self.transfer_sources(true, paths.iter().cloned(), cx);
        let job = move |target| TransferJob::Download {
            paths: paths.clone(),
            target,
        };
        self.open_confirmation(
            TransferDirection::Download,
            sources,
            target,
            job,
            window,
            cx,
        );
    }

    /// 两个方向共用的确认框。`job` 用用户定下的目录做出这一批。
    fn open_confirmation(
        &mut self,
        direction: TransferDirection,
        sources: TransferSources,
        target: String,
        job: impl Fn(String) -> TransferJob + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state != ConnectionState::Connected
            || self
                .transfers
                .confirmation
                .as_ref()
                .is_some_and(|layer| layer.is_open(window, cx))
        {
            return;
        }
        let queued = self.unfinished_transfers() > 0;
        let endpoint = format!("{}@{}", self.login().user, self.login().host);
        let form = cx
            .new(|cx| TransferForm::new(direction, queued, sources, target, endpoint, window, cx));
        let (dispatch, tab) = (self.dispatch.clone(), self.id());
        let job = Rc::new(job);
        let layer = DialogLayer::new();
        open_form_dialog(
            form,
            layer.clone(),
            Rc::new(move |target, window, cx| {
                // 不带连接的代号：连接换过，这一批照样要传。
                dispatch.dispatch_sftp_action(
                    &SftpAction::new(tab, SftpTabCommand::Enqueue(job(target))),
                    window,
                    cx,
                );
            }),
            window,
            cx,
        );
        self.transfers.confirmation = Some(layer);
    }

    /// 确认框为 `paths` 列什么。文件夹和文件靠列着它们的卡片分；卡片没列的本地路径
    /// （从资源管理器拖来的、选择框挑的）看磁盘。
    fn transfer_sources(
        &self,
        remote: bool,
        paths: impl Iterator<Item = String>,
        cx: &App,
    ) -> TransferSources {
        let card = self.card(remote).read(cx);
        let listed: std::collections::HashMap<String, bool> = card
            .listed_entries(cx)
            .iter()
            .filter(|entry| !entry.is_parent())
            .map(|entry| (card.child_path_of(&entry.name), entry.is_dir()))
            .collect();
        TransferSources::new(
            paths
                .map(|path| {
                    let is_dir = listed
                        .get(&path)
                        .copied()
                        .unwrap_or_else(|| !remote && Path::new(&path).is_dir());
                    (path, remote, is_dir)
                })
                .collect(),
        )
    }

    /// 用原生选择框挑要上传的，再走同一个确认框。Windows 的选择框只能选文件或者只能选文件夹，
    /// 那里只选文件：文件夹从资源管理器拖进来。
    pub(super) fn choose_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choice = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: cx.can_select_mixed_files_and_dirs(),
            multiple: true,
            prompt: Some(t!("sftp_transfer.confirm.choose_files")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = choice.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    let target = this.card(true).read(cx).path();
                    this.open_upload(paths, target, window, cx);
                });
            }
        })
        .detach();
    }

    /// 回答传输的问题。
    pub(super) fn answer_question(&mut self, request_id: u64, answer: TransferAnswer) {
        self.transfers.question = None;
        self.send(SftpCommand::Answer { request_id, answer });
    }

    /// 停下正在跑的一批，进度留着。
    pub(super) fn cancel_transfer(&mut self) {
        if self.transfer_running() {
            self.send(SftpCommand::Cancel);
        }
    }

    /// 接着跑停下的一批。断开时引擎先连上去（等着的批次在 `Connected` 时开始）。
    pub(super) fn resume_transfer(&mut self) {
        if self.can_resume_transfer() {
            self.transfers.engine_busy = true;
            self.send(SftpCommand::Resume);
        }
    }

    /// 丢弃停下的那批的续传进度，队列接着走。
    pub(super) fn discard_transfer(&mut self) {
        if self.can_discard_transfer() {
            self.transfers.queue.abandon_head(Removal::Discarded);
            self.transfers.engine_busy = true;
            self.send(SftpCommand::Discard);
        }
    }

    /// 移出队列：等着的、结束了的直接拿掉；停着的队头留一行「已移出」，引擎忘掉它（`.filepart` 和续传记录都留着，
    /// 下次传同一个文件会问是否续传），`Idle` 后轮到下一批。
    pub(super) fn remove_queue_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_remove_transfer() {
            return;
        }
        let Some(id) = self.transfers.queue.removable() else {
            return;
        };
        let stopped_head = self
            .transfers
            .queue
            .head()
            .is_some_and(|head| head.id() == id && head.state() == QueueState::Stopped);
        if self.transfers.queue.remove(id) {
            if stopped_head {
                self.transfers.engine_busy = true;
                self.send(SftpCommand::Forget);
            } else {
                self.start_next_if_idle(window, cx);
            }
        }
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
