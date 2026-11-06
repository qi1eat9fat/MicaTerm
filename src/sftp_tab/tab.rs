// 参考 since2006/shell-rs @ b2a307a: src/explorer/explorer_panel.rs 改写
// 裁剪：去掉 Dock 面板、书签、查找文件、后退前进、单栏「文件」和目录跟随；
// 改造：本地从「本地默认打开目录」（默认「下载」）开始，远程从焦点卡片的目录开始，读不出来静默退回主目录。
//! SFTP 标签：一台主机的本地加远程两张文件卡片，有自己的一条连接，不能分屏。
//!
//! 每个标签一条工作线程（`SftpTransport`），事件经通道过来，这里每 16ms 取一次：
//! 工作线程不直接唤醒只在前台跑的 GPUI 任务。

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::base::{
    ResizableState, ResizeHandleContext, ResizeHandleRenderer, h_resizable, resizable_panel,
    v_resizable,
};
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::file_open::FileOpenState;
use super::operations::OperationState;
use super::transfers::TransferState;
use super::{CardSide, ConnectionState, FileCard, LoadIntent, SftpTabId};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{
    RemotePath, SftpCommand, SftpEvent, SharedLocalDirectoryProvider, SharedSftpTransportProvider,
    describe_sftp_error,
};
use crate::backend::{ConnectionPrompt, ConnectionPromptReply, HostLogin, Latency};
use crate::card::{StatusDot, host_badge};
use crate::host::{Host, HostId};
use crate::i18n::t;
use crate::programs::Program;
use crate::theme::GAP;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Choice, ChoiceButton, ChoiceTone, ConfirmLine, open_choice_dialog};

/// 每次连接一个新的代号：对话框比连接活得久时，它发的命令和问题的回答作废。
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// 一张卡片不能比这再窄。
const MIN_CARD_WIDTH: Pixels = px(240.);

/// 打开一个 SFTP 标签要的东西。
pub struct SftpTabOptions {
    pub id: SftpTabId,
    /// 打开时的主机：标签显示它，工作线程也按它登录、重连。改主机、删主机都不影响开着的标签。
    pub host: Host,
    pub login: HostLogin,
    pub transports: SharedSftpTransportProvider,
    pub local_files: SharedLocalDirectoryProvider,
    /// 设置里的「本地默认打开目录」；`None` 或者读不出来是「下载」。
    pub local_directory: Option<std::path::PathBuf>,
    /// 远程的初始目录：焦点卡片的目录；`None` 或者读不出来是主目录。
    pub start_directory: Option<String>,
    /// 窗口壳的焦点：命令经它派发。
    pub dispatch: FocusHandle,
}

/// SFTP 标签要告诉窗口壳的事。
#[derive(Clone, Debug)]
pub enum SftpTabEvent {
    /// 标签轨道上显示的东西变了（目录、连接状态、状态点）。
    Changed,
    /// 连接要问用户（主机密钥、密码、验证码）：进窗口壳的问答队列。
    Prompt {
        generation: u64,
        prompt: ConnectionPrompt,
    },
    /// 用户要编辑这个文件：窗口壳看它开着没有，没开就调 `read_for_editor`（工单 23）。
    EditFile(crate::editor::EditorLocation),
    /// `read_for_editor` 读好了（工单 23）。
    FileRead {
        location: crate::editor::EditorLocation,
        file: Rc<crate::backend::sftp::TextFile>,
    },
}

/// SFTP 标签。
pub struct SftpTab {
    id: SftpTabId,
    host: Host,
    login: HostLogin,
    generation: u64,
    local: Entity<FileCard>,
    remote: Entity<FileCard>,
    pub(super) local_files: SharedLocalDirectoryProvider,
    commands: async_channel::Sender<SftpCommand>,
    pub(super) state: ConnectionState,
    latency: Option<Latency>,
    /// 还没用掉的远程初始目录。
    start_directory: Option<String>,
    /// 归工单 21 的状态（`operations.rs`）。
    pub(super) operations: OperationState,
    /// 归工单 22 的状态（`transfers.rs`）。
    pub(super) transfers: TransferState,
    /// 归工单 23、24 的状态（`file_open.rs`）。
    pub(super) file_open: FileOpenState,
    /// 问要不要重新连接的那个框，开着时不再叠一个。
    reconnect_dialog: Option<DialogLayer>,
    pub(super) dispatch: FocusHandle,
    focus_handle: FocusHandle,
    /// 上次用的是远程那一侧：标签再到前面来时焦点回到那里。
    last_remote: bool,
    /// 用户把两张卡片之间的间隙拖到了哪、队列卡片上面的间隙拖到了哪。
    /// 存在实体里：元素状态在别的标签画出来的第一帧就没了，切回来比例就丢了。
    cards_split: Entity<ResizableState>,
    queue_split: Entity<ResizableState>,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
}

impl EventEmitter<SftpTabEvent> for SftpTab {}

impl SftpTab {
    pub fn new(options: SftpTabOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let SftpTabOptions {
            id,
            host,
            login,
            transports,
            local_files,
            local_directory,
            start_directory,
            dispatch,
        } = options;
        let local_home = local_files.home().to_string_lossy().into_owned();
        let downloads = local_files.downloads().to_string_lossy().into_owned();
        let local = cx.new(|cx| {
            FileCard::new(
                CardSide::Local,
                id,
                t!("sftp_tab.local_name"),
                crate::programs::local_files_program(),
                local_home.clone(),
                dispatch.clone(),
                window,
                cx,
            )
        });
        let endpoint = endpoint_of(&login);
        let remote = cx.new(|cx| {
            let mut card = FileCard::new(
                CardSide::Remote,
                id,
                host.name.clone(),
                host_badge(&host, host.os),
                String::new(),
                dispatch.clone(),
                window,
                cx,
            );
            card.set_link(Some(endpoint), None, cx);
            card
        });
        // 本地：设置的目录 → 「下载」 → 主目录，读得出来的第一个。
        let (first, fallback) = start_chain(local_directory, downloads, local_home);
        {
            let provider = local_files.clone();
            local.update(cx, |card, cx| {
                card.load_local(first, LoadIntent::Start { fallback }, provider, window, cx)
            });
        }
        let (commands, receiver) = async_channel::unbounded();
        let (sender, events) = async_channel::unbounded();
        let transport = transports.create(&login);
        let failed = sender.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("qishell-sftp".into())
            .spawn(move || {
                if let Err(error) = transport.run(receiver, sender.clone()) {
                    let _ =
                        sender.send_blocking(SftpEvent::Disconnected(describe_sftp_error(&error)));
                }
            })
        {
            let _ = failed.try_send(SftpEvent::Disconnected(
                t!("sftp_tab.connection.start_failed", error = error).to_string(),
            ));
        }
        // 和终端引擎一样定时取。工作线程不直接唤醒只在前台跑的 GPUI 任务。
        let events_task = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let pending: Vec<_> = (0..128).map_while(|_| events.try_recv().ok()).collect();
                if !pending.is_empty()
                    && this
                        .update_in(cx, |this, window, cx| {
                            for event in pending {
                                this.on_event(event, window, cx);
                            }
                        })
                        .is_err()
                {
                    break;
                }
                if events.is_closed() && events.is_empty() {
                    break;
                }
            }
        });
        let local_focus = local.read(cx).focus_handle(cx);
        let remote_focus = remote.read(cx).focus_handle(cx);
        let subscriptions = vec![
            // 焦点监听在画的时候跑，那时视图的 `notify` 不要下一帧：点列表空白处换一侧就看不出来。
            // 推到画完以后。
            cx.on_focus_in(&local_focus, window, |_, window, cx| {
                cx.defer_in(window, |this, _, cx| this.set_current_card(false, cx))
            }),
            cx.on_focus_in(&remote_focus, window, |_, window, cx| {
                cx.defer_in(window, |this, _, cx| this.set_current_card(true, cx))
            }),
            cx.on_app_quit(|this, _| {
                this.send(SftpCommand::Shutdown);
                async {}
            }),
        ];
        Self {
            id,
            host,
            login,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            local,
            remote,
            local_files,
            commands,
            state: ConnectionState::Connecting,
            latency: None,
            start_directory,
            operations: OperationState::default(),
            transfers: TransferState::default(),
            file_open: FileOpenState::default(),
            reconnect_dialog: None,
            dispatch,
            focus_handle: cx.focus_handle(),
            last_remote: true,
            cards_split: cx.new(|_| ResizableState::default()),
            queue_split: cx.new(|_| ResizableState::default()),
            _subscriptions: subscriptions,
            _events: events_task,
        }
    }

    pub fn id(&self) -> SftpTabId {
        self.id
    }

    pub fn host_id(&self) -> HostId {
        self.host.id
    }

    /// 打开时的主机。
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// 打开时的登录。
    pub fn login(&self) -> &HostLogin {
        &self.login
    }

    /// 现在这条连接的代号。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn local(&self) -> &Entity<FileCard> {
        &self.local
    }

    pub fn remote(&self) -> &Entity<FileCard> {
        &self.remote
    }

    pub fn card(&self, remote: bool) -> &Entity<FileCard> {
        if remote { &self.remote } else { &self.local }
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.state
    }

    /// 给工作线程发命令。
    pub fn send(&self, command: SftpCommand) {
        let _ = self.commands.try_send(command);
    }

    /// 回答连接的问题。
    pub fn reply_to_prompt(&self, request_id: u64, reply: ConnectionPromptReply) {
        self.send(SftpCommand::PromptReply { request_id, reply });
    }

    /// 标签轨道上的名称：主机名。
    pub(crate) fn rail_name(&self) -> SharedString {
        self.host.name.clone()
    }

    /// 标签轨道上的灰字：当前远程目录。
    pub(crate) fn rail_directory(&self, cx: &App) -> SharedString {
        self.remote.read(cx).path().into()
    }

    /// 标签轨道上的图块：固定颜色的文件夹，没连着时变灰。
    pub(crate) fn rail_tile(&self) -> Program {
        let tile = crate::programs::sftp_program();
        if self.state == ConnectionState::Connected {
            tile
        } else {
            tile.offline()
        }
    }

    /// 标签轨道上的状态点：有传输在进行时是绿点。
    pub(crate) fn rail_dot(&self) -> StatusDot {
        if self.transfer_running() {
            StatusDot::Busy
        } else {
            StatusDot::None
        }
    }

    /// 标签到前面来时焦点给上次用的那一侧的文件列表，键盘马上能用。
    pub(crate) fn focus_target(&self, cx: &App) -> FocusHandle {
        self.card(self.last_remote).read(cx).focus_handle(cx)
    }

    /// 能不能重新连接：正在连接、批次在跑或者停着（它有自己的「继续」）时不行；
    /// 正在写回时也不行，编辑器会以为写断了，其实还在写。
    pub fn can_reconnect(&self) -> bool {
        self.state != ConnectionState::Connecting
            && !self.transfers.engine_busy
            && !self.head_stopped()
            && !self.writing_file()
    }

    /// 关掉标签：断开连接，工作线程退出。标签随后由窗口壳移走。
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(layer) = self.reconnect_dialog.take() {
            layer.close(window, cx);
        }
        self.close_transfer_dialogs(window, cx);
        self.close_operation_dialog(window, cx);
        self.close_preview(window, cx);
        self.send(SftpCommand::Disconnect);
        self.send(SftpCommand::Shutdown);
    }

    /// 上次用的那一侧：边框亮着，标签再到前面来时焦点回到这里。
    fn set_current_card(&mut self, remote: bool, cx: &mut Context<Self>) {
        self.last_remote = remote;
        self.local
            .update(cx, |card, cx| card.set_current(!remote, cx));
        self.remote
            .update(cx, |card, cx| card.set_current(remote, cx));
    }

    /// 两张卡片跟上现在的连接状态。
    fn sync_available(&mut self, cx: &mut Context<Self>) {
        let state = self.state;
        // 断开时传输照样能点，回答「要不要重新连接」；只有正在连接时只能等。
        let transfer = state != ConnectionState::Connecting;
        self.local.update(cx, |card, cx| {
            card.set_available(ConnectionState::Connected, transfer, cx)
        });
        let (endpoint, latency) = (endpoint_of(&self.login), self.latency);
        self.remote.update(cx, |card, cx| {
            card.set_available(state, transfer, cx);
            card.set_link(Some(endpoint), latency, cx);
        });
    }

    fn on_event(&mut self, event: SftpEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SftpEvent::Connecting => {
                self.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                self.state = ConnectionState::Connecting;
                self.latency = None;
            }
            SftpEvent::Connected { home } => {
                self.state = ConnectionState::Connected;
                let home = home.to_string();
                self.remote
                    .update(cx, |card, _| card.set_home(home.clone()));
                // 第一次连上去初始目录，读不出来静默退回主目录；重连就重读现在的目录。
                match self.start_directory.take() {
                    Some(directory) => self.navigate(
                        true,
                        directory,
                        LoadIntent::Start {
                            fallback: vec![home],
                        },
                        window,
                        cx,
                    ),
                    None => self.reload(true, window, cx),
                }
                self.start_next_if_idle(window, cx);
            }
            SftpEvent::Disconnected(message) => {
                self.state = ConnectionState::Disconnected;
                self.latency = None;
                self.abandon_remote_operations(cx);
                self.abandon_file_requests();
                self.remote
                    .update(cx, |card, cx| card.disconnected(message, cx));
            }
            SftpEvent::Prompt(prompt) => cx.emit(SftpTabEvent::Prompt {
                generation: self.generation,
                prompt,
            }),
            SftpEvent::Listed { request_id, result } => {
                let next = self.remote.update(cx, |card, cx| {
                    card.apply_listing(request_id, result, window, cx)
                });
                if let Some((path, intent)) = next {
                    self.navigate(true, path, intent, window, cx);
                }
            }
            SftpEvent::Operated { request_id, result } => {
                self.finish_operation(request_id, result, window, cx)
            }
            SftpEvent::FileRead { request_id, result } => {
                self.finish_file_read(request_id, result, window, cx)
            }
            SftpEvent::FileWritten { request_id, result } => {
                self.finish_file_write(request_id, result, window, cx)
            }
            SftpEvent::BytesRead { request_id, result } => {
                self.finish_bytes_read(request_id, result, window, cx)
            }
            SftpEvent::Progress(progress) => self.on_progress(progress, window, cx),
            SftpEvent::Question(question) => self.on_question(question, window, cx),
            SftpEvent::Idle => self.on_idle(window, cx),
            SftpEvent::Notice(message) => {
                window.push_notification(Notification::error(message), cx);
            }
            SftpEvent::Latency(latency) => self.latency = Some(latency),
        }
        self.sync_available(cx);
        cx.emit(SftpTabEvent::Changed);
        cx.notify();
    }

    /// 去 `path`。远程没连着时没人回答，什么都不做。
    pub fn navigate(
        &mut self,
        remote: bool,
        path: String,
        intent: LoadIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if remote && self.state != ConnectionState::Connected {
            return;
        }
        let card = self.card(remote).clone();
        let path = card.read(cx).expanded_path(&path);
        if remote {
            let id = card.update(cx, |card, cx| card.begin_load(intent, cx));
            match RemotePath::new(path) {
                Ok(path) => self.send(SftpCommand::List {
                    request_id: id,
                    path,
                }),
                Err(error) => {
                    let next = card.update(cx, |card, cx| {
                        card.apply_listing(id, Err(describe_sftp_error(&error)), window, cx)
                    });
                    if let Some((path, intent)) = next {
                        self.navigate(true, path, intent, window, cx);
                    }
                }
            }
        } else {
            let provider = self.local_files.clone();
            card.update(cx, |card, cx| {
                card.load_local(path, intent, provider, window, cx)
            });
        }
    }

    /// 重读这一侧当前的目录。
    pub fn reload(&mut self, remote: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.card(remote).read(cx).path();
        self.navigate(remote, path, LoadIntent::Visit, window, cx);
    }

    /// 做一条命令：菜单、拖放、对话框、按键都到这里。
    ///
    /// 断开时要用连接的命令先说明为什么做不了、问要不要重新连接，而不是什么都不做。
    pub fn execute(
        &mut self,
        command: &SftpTabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state == ConnectionState::Disconnected && needs_connection(command) {
            self.offer_reconnect(window, cx);
            return;
        }
        use SftpTabCommand as C;
        match command {
            C::Navigate { remote, path } => {
                self.navigate(*remote, path.clone(), LoadIntent::Visit, window, cx)
            }
            C::EditPath { remote } => self
                .card(*remote)
                .clone()
                .update(cx, |card, cx| card.begin_path_edit(window, cx)),
            C::SubmitPath { remote, text } => {
                self.navigate(*remote, text.clone(), LoadIntent::PathBar, window, cx)
            }
            C::Up { remote } => {
                let path = self.card(*remote).read(cx).parent_path();
                self.navigate(*remote, path, LoadIntent::Visit, window, cx);
            }
            C::Refresh { remote } => self.reload(*remote, window, cx),
            C::Open { remote } => self.open_cursor_entry(*remote, window, cx),
            C::Edit { remote, path } => self.open_for_editing(*remote, path.clone(), window, cx),
            C::Preview { remote, path } => self.open_for_preview(*remote, path.clone(), window, cx),
            C::CopyPath { remote } => {
                let path = self.card(*remote).read(cx).path();
                if !path.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(path));
                }
            }
            C::CopySelectedPaths { remote } => {
                let card = self.card(*remote).read(cx);
                let paths: Vec<String> = card
                    .selected_entries(cx)
                    .iter()
                    .map(|entry| {
                        if entry.is_parent() {
                            card.parent_path()
                        } else {
                            card.child_path_of(&entry.name)
                        }
                    })
                    .collect();
                if !paths.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                }
            }
            C::FocusCard { remote } => {
                let focus = self.card(*remote).read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            C::MoveCursor {
                remote,
                motion,
                extend,
            } => self
                .card(*remote)
                .clone()
                .update(cx, |card, cx| card.move_cursor(*motion, *extend, cx)),
            C::ToggleSelection { remote } => self
                .card(*remote)
                .clone()
                .update(cx, |card, cx| card.toggle_selection(cx)),
            C::SelectAll { remote } => self
                .card(*remote)
                .clone()
                .update(cx, |card, cx| card.select_all(cx)),
            // 文件操作：工单 21（`operations.rs`）。
            C::Delete { remote } => self.confirm_delete(*remote, window, cx),
            C::Rename { remote } => self.open_rename(*remote, window, cx),
            C::New { remote, kind } => self.open_new(*remote, *kind, window, cx),
            C::Properties { remote } => self.open_properties(*remote, window, cx),
            C::Operate { remote, operation } => {
                self.start_operation(*remote, operation.clone(), window, cx)
            }
            // 传输：工单 22（`transfers.rs`）。
            C::Transfer { remote } => self.open_transfer(*remote, window, cx),
            C::ChooseFiles => self.choose_files(window, cx),
            C::UploadPaths { paths, target } => {
                self.open_upload(paths.clone(), target.clone(), window, cx)
            }
            C::DownloadPaths { paths, target } => {
                self.open_download(paths.clone(), target.clone(), window, cx)
            }
            C::Enqueue(job) => self.enqueue(job.clone(), window, cx),
            C::Answer { request_id, answer } => self.answer_question(*request_id, *answer),
            C::CancelTransfer => self.cancel_transfer(),
            C::ResumeTransfer => self.resume_transfer(),
            C::DiscardTransfer => self.discard_transfer(),
            C::SelectQueueEntry { id } => {
                self.transfers.queue.select(super::QueueId(*id));
            }
            C::ToggleQueueEntry { id } => {
                self.transfers.queue.toggle_expanded(super::QueueId(*id));
            }
            C::RemoveQueueEntry => self.remove_queue_entry(window, cx),
            C::ClearFinishedTransfers => self.transfers.queue.clear_finished(),
            C::Reconnect => self.reconnect(cx),
            C::DoubleClick { remote } => self.open_double_clicked(*remote, window, cx),
        }
        self.sync_available(cx);
        cx.emit(SftpTabEvent::Changed);
        cx.notify();
    }

    /// 回车：进光标所在的目录；文件交给编辑器。
    pub(super) fn open_cursor_entry(
        &mut self,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = self.card(remote).clone();
        let Some(entry) = card.read(cx).cursor_entry(cx) else {
            return;
        };
        if entry.is_dir() {
            let path = if entry.is_parent() {
                card.read(cx).parent_path()
            } else {
                card.read(cx).child_path_of(&entry.name)
            };
            self.navigate(remote, path, LoadIntent::Visit, window, cx);
        } else {
            let path = Some(card.read(cx).child_path_of(&entry.name));
            self.dispatch.dispatch_sftp_action(
                &SftpAction::new(self.id, SftpTabCommand::Edit { remote, path }),
                window,
                cx,
            );
        }
    }

    /// 一条新的 SFTP 连接：引擎的 `Resume` 在没有停着的批次时只重连。
    fn reconnect(&mut self, cx: &mut Context<Self>) {
        if !self.can_reconnect() {
            return;
        }
        if self.state == ConnectionState::Connected {
            self.send(SftpCommand::Disconnect);
            self.state = ConnectionState::Disconnected;
            self.abandon_file_requests();
            let message = t!("sftp_tab.connection.lost").to_string();
            self.remote
                .update(cx, |card, cx| card.disconnected(message, cx));
        }
        self.transfers.engine_busy = true;
        self.send(SftpCommand::Resume);
    }

    /// 告诉用户远程断开了和原因，「重新连接」就在那里。队列自己管重连时（停着的批次等「继续」，
    /// 或者传输正在自己重连）就这么说。这是没有工具栏以后唯一的重连入口。
    pub(super) fn offer_reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .reconnect_dialog
            .as_ref()
            .is_some_and(|layer| layer.is_open(window, cx))
        {
            return;
        }
        let reason: SharedString = self
            .remote
            .read(cx)
            .problem()
            .map(|problem| SharedString::from(problem.to_string()))
            .unwrap_or_else(|| t!("sftp_tab.connection.lost"));
        let can_reconnect = self.can_reconnect();
        let description = if can_reconnect {
            t!("sftp_tab.connection.lost_reconnect", reason = reason)
        } else if self.head_stopped() {
            t!("sftp_tab.connection.lost_stopped_batch", reason = reason)
        } else {
            t!("sftp_tab.connection.lost_reconnecting", reason = reason)
        };
        let buttons = if can_reconnect {
            vec![
                ChoiceButton::new("cancel", t!("sftp_tab.connection.cancel")),
                ChoiceButton::new("reconnect", t!("sftp_tab.connection.reconnect"))
                    .tone(ChoiceTone::Primary),
            ]
        } else {
            vec![ChoiceButton::new("ok", t!("sftp_tab.connection.ok")).tone(ChoiceTone::Primary)]
        };
        let (dispatch, tab, generation) = (self.dispatch.clone(), self.id, self.generation);
        let focus = window.focused(cx);
        let layer = DialogLayer::new();
        open_choice_dialog(
            Choice {
                id: "sftp-reconnect",
                title: t!("sftp_tab.connection.lost_title", name = self.host.name),
                subtitle: None,
                lines: vec![ConfirmLine::Text(description)],
                check: None,
                buttons,
            },
            layer.clone(),
            Box::new(move |outcome, window, cx| {
                if let Some(focus) = &focus {
                    window.focus(focus, cx);
                }
                if outcome.button == Some("reconnect") {
                    dispatch.dispatch_sftp_action(
                        &SftpAction::new(tab, SftpTabCommand::Reconnect)
                            .with_generation(generation),
                        window,
                        cx,
                    );
                }
            }),
            window,
            cx,
        );
        self.reconnect_dialog = Some(layer);
    }

    /// 两张卡片，中间是间隙（可以拖）。两边一开始都从 0 起按 flex 平分，量过以后由 resizable 组件按像素摆。
    fn render_cards(&self) -> impl IntoElement + use<> {
        let half = GAP / 2.;
        h_resizable(("sftp-cards", self.id.0 as usize))
            .with_state(&self.cards_split)
            .with_handle_appearance(invisible_handle("sftp-cards-gap"))
            .child(
                resizable_panel()
                    .size(px(0.))
                    .size_range(MIN_CARD_WIDTH..Pixels::MAX)
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .min_w_0()
                            .min_h_0()
                            .pr(half)
                            .child(self.local.clone()),
                    ),
            )
            .child(
                resizable_panel()
                    .size(px(0.))
                    .size_range(MIN_CARD_WIDTH..Pixels::MAX)
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .min_w_0()
                            .min_h_0()
                            .pl(half)
                            .child(self.remote.clone()),
                    ),
            )
    }
}

impl Drop for SftpTab {
    fn drop(&mut self) {
        self.send(SftpCommand::Shutdown);
    }
}

impl Render for SftpTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_queue = !self.transfers.queue.is_empty();
        let half = GAP / 2.;
        div()
            .id("sftp-tab")
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .when(!has_queue, |tab| tab.child(self.render_cards()))
            // 队列卡片有自己的位置：有东西才显示，两张卡片的比例不因为它来去而变。
            .when(has_queue, |tab| {
                tab.child(
                    v_resizable(("sftp-queue", self.id.0 as usize))
                        .with_state(&self.queue_split)
                        .with_handle_appearance(invisible_handle("sftp-queue-gap"))
                        .child(
                            resizable_panel().child(
                                div()
                                    .flex()
                                    .size_full()
                                    .min_h_0()
                                    .pb(half)
                                    .child(self.render_cards()),
                            ),
                        )
                        .child(
                            resizable_panel()
                                .size(px(180.))
                                .size_range(px(96.)..px(480.))
                                .child(
                                    div()
                                        .flex()
                                        .size_full()
                                        .min_h_0()
                                        .pt(half)
                                        .child(self.render_queue(cx)),
                                ),
                        ),
                )
            })
    }
}

/// 间隙就是拖动柄（同卡片分屏）：柄里什么都不画，命中区和光标还是库的。
fn invisible_handle(id: &'static str) -> ResizeHandleRenderer {
    Rc::new(move |_: &ResizeHandleContext, _, _| {
        Some(
            div()
                .id(id)
                .test_support()
                .absolute()
                .inset_0()
                .into_any_element(),
        )
    })
}

/// 状态栏上的 `user@地址:端口`。
fn endpoint_of(login: &HostLogin) -> SharedString {
    format!("{}@{}:{}", login.user, login.host, login.port).into()
}

/// 本地一侧从哪里开始：设置的目录 → 「下载」 → 主目录，去掉重复的。返回第一个和后面的退路。
fn start_chain(
    configured: Option<std::path::PathBuf>,
    downloads: String,
    home: String,
) -> (String, Vec<String>) {
    let mut chain: Vec<String> = Vec::new();
    for path in configured
        .map(|path| path.to_string_lossy().into_owned())
        .into_iter()
        .chain([downloads, home])
    {
        if !chain.contains(&path) {
            chain.push(path);
        }
    }
    let first = chain.remove(0);
    (first, chain)
}

/// 这条命令要不要 SFTP 连接：读或改远程的，和所有传输。选择、复制路径、队列自己的按钮不要。
fn needs_connection(command: &SftpTabCommand) -> bool {
    use SftpTabCommand as C;
    match command {
        C::Navigate { remote, .. }
        | C::EditPath { remote }
        | C::SubmitPath { remote, .. }
        | C::Up { remote }
        | C::Refresh { remote }
        | C::Open { remote }
        | C::DoubleClick { remote }
        | C::Edit { remote, .. }
        | C::Preview { remote, .. }
        | C::Delete { remote }
        | C::Rename { remote }
        | C::New { remote, .. }
        | C::Properties { remote }
        | C::Operate { remote, .. } => *remote,
        C::Transfer { .. }
        | C::ChooseFiles
        | C::UploadPaths { .. }
        | C::DownloadPaths { .. }
        | C::Enqueue(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_connection, start_chain};
    use crate::app::SftpTabCommand;

    #[test]
    fn the_remote_side_and_transfers_need_the_connection() {
        assert!(needs_connection(&SftpTabCommand::Refresh { remote: true }));
        assert!(!needs_connection(&SftpTabCommand::Refresh {
            remote: false
        }));
        assert!(needs_connection(&SftpTabCommand::Delete { remote: true }));
        assert!(!needs_connection(&SftpTabCommand::Delete { remote: false }));
        // 不管哪个方向都要走连接。
        assert!(needs_connection(&SftpTabCommand::Transfer {
            remote: false
        }));
        assert!(needs_connection(&SftpTabCommand::ChooseFiles));
        // 选择、复制路径、重新连接本身不要。
        assert!(!needs_connection(&SftpTabCommand::SelectAll {
            remote: true
        }));
        assert!(!needs_connection(&SftpTabCommand::CopyPath {
            remote: true
        }));
        assert!(!needs_connection(&SftpTabCommand::Reconnect));
    }

    #[test]
    fn the_local_side_starts_at_the_setting_then_downloads_then_home() {
        assert_eq!(
            start_chain(None, "/d".into(), "/h".into()),
            ("/d".to_string(), vec!["/h".to_string()])
        );
        assert_eq!(
            start_chain(Some("/x".into()), "/d".into(), "/h".into()),
            ("/x".to_string(), vec!["/d".to_string(), "/h".to_string()])
        );
        // 没有「下载」文件夹时 `downloads` 就是主目录。
        assert_eq!(
            start_chain(None, "/h".into(), "/h".into()),
            ("/h".to_string(), vec![])
        );
    }
}
