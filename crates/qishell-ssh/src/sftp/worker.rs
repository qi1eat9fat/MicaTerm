// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/sftp/worker.rs
// 裁剪：`SftpRoute` 换成 `HostLogin`；去掉外部 CLI 的 `sync`（`sync_target`、删除多余文件）。
//! One SFTP connection's worker: it connects, serves listings, operations
//! and the editor's files alongside one transfer batch at a time, and
//! reconnects a batch whose connection dropped.

use super::{
    Disconnect, EngineEvent, EntryKind, EventSink, Reason, RemotePath, SftpCommand, SftpError,
    TransferDetail, TransferDirection, TransferPhase, TransferProgress,
    client::RemoteFs as _,
    client::{SftpClient, is_network_error},
    control::{Cancelled, TransferControl},
    download::DownloadBatch,
    edit::{self, ReadFailure, SaveFailure},
    journal::{DownloadJournal, Journal},
    meter::TransferMeter,
    model::{scp_local_target, scp_remote_target},
    operations,
    upload::UploadBatch,
};
use crate::{HostLogin, LATENCY_INTERVAL, SshConnectionConfig, SshConnector, SshPrompts};
use anyhow::{Result, anyhow};
use async_channel::Receiver;
use std::{
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc, watch};

/// The answer to a request that needs the connection while there is none.
fn not_connected() -> Reason {
    Reason::new(SftpError::NotConnected)
}

/// How often the latency task looks for a connection to measure, and for
/// the one it is measuring to be dropped.
const LATENCY_POLL: Duration = Duration::from_millis(250);

/// 断线后第几次重连之前等几秒。
const RECONNECT_DELAYS: [u64; 3] = [1, 3, 10];

/// One worker lifetime, run on the caller's runtime; see
/// [`run`](Self::run).
pub struct SftpWorker {
    connector: SshConnector,
    login: HostLogin,
    endpoint: String,
    journal: Journal,
    download_journal: DownloadJournal,
}

impl SftpWorker {
    /// Upload and download resume records live in separate directories.
    pub fn new(
        connector: SshConnector,
        login: HostLogin,
        resume_dir: PathBuf,
        download_resume_dir: PathBuf,
    ) -> Self {
        Self {
            connector,
            endpoint: login.endpoint(),
            login,
            journal: Journal::new(resume_dir),
            download_journal: DownloadJournal::new(download_resume_dir),
        }
    }
}

/// The host's one transfer batch, whichever way it goes.
enum TransferBatch {
    Upload(UploadBatch),
    Download(DownloadBatch),
}
impl TransferBatch {
    fn verify_host(&self, fingerprint: &str) -> Result<()> {
        match self {
            Self::Upload(batch) => batch.verify_host(fingerprint),
            Self::Download(batch) => batch.verify_host(fingerprint),
        }
    }
    fn is_complete(&self) -> bool {
        match self {
            Self::Upload(batch) => batch.is_complete(),
            Self::Download(batch) => batch.is_complete(),
        }
    }
    fn meter(&mut self) -> &mut TransferMeter {
        match self {
            Self::Upload(batch) => &mut batch.meter,
            Self::Download(batch) => &mut batch.meter,
        }
    }
    fn phase(&mut self, phase: TransferPhase, control: &TransferControl) {
        self.meter().phase(phase, control);
    }
    /// Say what a reconnecting batch is waiting for; the next phase
    /// other than reconnecting clears it.
    fn set_note(&mut self, note: SftpError) {
        self.meter().progress.note = Some(Reason::new(note));
    }
    async fn run(&mut self, client: &SftpClient, control: &TransferControl) -> Result<()> {
        match self {
            Self::Upload(batch) => batch.run(client, control).await,
            Self::Download(batch) => batch.run(client, control).await,
        }
    }
}
impl SftpWorker {
    async fn connect(
        &self,
        prompts: Arc<SshPrompts>,
        events: &EventSink,
        shutdown: &watch::Receiver<bool>,
        shared: &RwLock<Option<Arc<SftpClient>>>,
    ) -> Result<Arc<SftpClient>> {
        events.send(EngineEvent::Connecting).await?;
        let mut stop = shutdown.clone();
        let config = SshConnectionConfig::from(&self.login);
        let client = SftpClient::connect(&self.connector, &config, prompts);
        let client = tokio::select! {
            result = client => Arc::new(result?),
            _ = stop.changed() => return Err(Cancelled.into()),
        };
        let home = client.canonicalize(&RemotePath::new(".")?).await?;
        *shared.write().await = Some(client.clone());
        events.send(EngineEvent::Connected { home }).await?;
        Ok(client)
    }

    /// Serve `commands` until `Shutdown`, or until they close, telling
    /// `events` what happens.
    ///
    /// 返回之前等写回都结束：写回是截断重写，调用方接着销毁运行时的话会中止它，文件就只剩一半。
    /// 连接卡死时写回一直等到连接自己超时，这条线程就留到那时，不碍事。
    pub async fn run(self, commands: Receiver<SftpCommand>, events: EventSink) -> Result<()> {
        // 每个写回拿着一个 sender，全放下了 `recv` 才返回。
        let (writes, mut writes_done) = mpsc::channel::<()>(1);
        let result = self.serve(commands, events, writes).await;
        let _ = writes_done.recv().await;
        result
    }

    async fn serve(
        self,
        commands: Receiver<SftpCommand>,
        events: EventSink,
        writes: mpsc::Sender<()>,
    ) -> Result<()> {
        let (shutdown_tx, shutdown) = watch::channel(false);
        let (cancel_tx, cancel) = watch::channel(false);
        let (answers_tx, answers) = async_channel::unbounded();
        let (operations_tx, operations) = async_channel::bounded(1);
        let prompt_events = events.clone();
        let prompts = Arc::new(SshPrompts::new(
            Arc::new(move |p| prompt_events.try_send(EngineEvent::Prompt(p))),
            shutdown.clone(),
        ));
        let client: Arc<RwLock<Option<Arc<SftpClient>>>> = Arc::new(RwLock::new(None));
        let busy = Arc::new(AtomicBool::new(false));
        let _router = {
            let prompts = prompts.clone();
            let events = events.clone();
            let client = client.clone();
            let busy = busy.clone();
            let cancel_tx = cancel_tx.clone();
            AbortOnDrop(tokio::spawn(async move {
                while let Ok(command) = commands.recv().await {
                    match command {
                        SftpCommand::PromptReply { request_id, reply } => {
                            prompts.respond(request_id, reply)
                        }
                        SftpCommand::Answer { request_id, answer } => {
                            let _ = answers_tx.send((request_id, answer)).await;
                        }
                        SftpCommand::Cancel => {
                            cancel_tx.send_replace(true);
                            prompts.cancel_all();
                        }
                        SftpCommand::Disconnect => {
                            cancel_tx.send_replace(true);
                            prompts.cancel_all();
                            *client.write().await = None;
                            let _ = events
                                .send(EngineEvent::Disconnected(Disconnect::ByUser))
                                .await;
                        }
                        SftpCommand::Shutdown => break,
                        SftpCommand::List { request_id, path } => {
                            let shared = client.clone();
                            let connected = client.read().await.clone();
                            let events = events.clone();
                            tokio::spawn(async move {
                                let result = match connected {
                                    Some(connected) => match connected.list(&path).await {
                                        Ok(listing) => Ok(listing),
                                        Err(error) => {
                                            drop_broken(&shared, &connected, &events, &error).await;
                                            Err(Reason::from(error).under(SftpError::ListFailed))
                                        }
                                    },
                                    None => Err(not_connected()),
                                };
                                let _ = events
                                    .send(EngineEvent::Listed { request_id, result })
                                    .await;
                            });
                        }
                        SftpCommand::Operate {
                            request_id,
                            operation,
                        } => {
                            let shared = client.clone();
                            let connected = client.read().await.clone();
                            let events = events.clone();
                            tokio::spawn(async move {
                                let result = match connected {
                                    Some(connected) => {
                                        match operations::run(connected.as_ref(), &operation).await
                                        {
                                            Ok(()) => Ok(()),
                                            Err(error) => {
                                                drop_broken(&shared, &connected, &events, &error)
                                                    .await;
                                                Err(error.into())
                                            }
                                        }
                                    }
                                    None => Err(not_connected()),
                                };
                                let _ = events
                                    .send(EngineEvent::Operated { request_id, result })
                                    .await;
                            });
                        }
                        SftpCommand::ReadFile { request_id, path } => {
                            let shared = client.clone();
                            let connected = client.read().await.clone();
                            let events = events.clone();
                            tokio::spawn(async move {
                                let result = match connected {
                                    Some(connected) => {
                                        match edit::read_text(connected.as_ref(), &path).await {
                                            Ok(file) => Ok(file),
                                            Err(error) => {
                                                drop_broken(&shared, &connected, &events, &error)
                                                    .await;
                                                Err(ReadFailure::from_error(error, |why| why))
                                            }
                                        }
                                    }
                                    None => Err(ReadFailure::Failed(not_connected())),
                                };
                                let _ = events
                                    .send(EngineEvent::FileRead { request_id, result })
                                    .await;
                            });
                        }
                        SftpCommand::ReadBytes {
                            request_id,
                            path,
                            limit,
                        } => {
                            let shared = client.clone();
                            let connected = client.read().await.clone();
                            let events = events.clone();
                            tokio::spawn(async move {
                                let result = match connected {
                                    Some(connected) => {
                                        match edit::read_whole(connected.as_ref(), &path, limit)
                                            .await
                                        {
                                            Ok(bytes) => Ok(bytes),
                                            Err(error) => {
                                                drop_broken(&shared, &connected, &events, &error)
                                                    .await;
                                                Err(ReadFailure::from_error(error, |why| why))
                                            }
                                        }
                                    }
                                    None => Err(ReadFailure::Failed(not_connected())),
                                };
                                let _ = events
                                    .send(EngineEvent::BytesRead { request_id, result })
                                    .await;
                            });
                        }
                        SftpCommand::WriteFile {
                            request_id,
                            path,
                            bytes,
                            expected,
                        } => {
                            let shared = client.clone();
                            let connected = client.read().await.clone();
                            let events = events.clone();
                            let write = writes.clone();
                            tokio::spawn(async move {
                                let _write = write;
                                let result = match connected {
                                    Some(connected) => match edit::write_in_place(
                                        connected.as_ref(),
                                        &path,
                                        bytes,
                                        expected,
                                    )
                                    .await
                                    {
                                        Ok(stamp) => Ok(stamp),
                                        Err(error) => {
                                            drop_broken(&shared, &connected, &events, &error).await;
                                            Err(SaveFailure::from_error(error, |why| why))
                                        }
                                    },
                                    None => Err(SaveFailure::Failed(not_connected())),
                                };
                                let _ = events
                                    .send(EngineEvent::FileWritten { request_id, result })
                                    .await;
                            });
                        }
                        command => {
                            if busy.swap(true, Ordering::AcqRel) {
                                let _ = events
                                    .send(EngineEvent::Notice(Reason::new(SftpError::Busy)))
                                    .await;
                            } else {
                                cancel_tx.send_replace(false);
                                if operations_tx.send(command).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                shutdown_tx.send_replace(true);
                cancel_tx.send_replace(true);
                prompts.cancel_all();
            }))
        };
        let _latency = AbortOnDrop(tokio::spawn(measure_latency(
            client.clone(),
            events.clone(),
        )));
        let control = TransferControl {
            events: events.clone(),
            cancel,
            answers,
        };
        let link = Link {
            prompts: &prompts,
            events: &events,
            shutdown: &shutdown,
            client: &client,
            control: &control,
        };
        let mut batch: Option<TransferBatch> = None;
        // 扫描没做完就停了或者断了的那一批：还没有 `batch`，「继续」按原来的请求重新扫描。
        let mut unscanned: Option<SftpCommand> = None;
        match control
            .run(self.connect(prompts.clone(), &events, &shutdown, &client))
            .await
        {
            Ok(connected) => *client.write().await = Some(connected),
            Err(error) => {
                let _ = events
                    .send(EngineEvent::Disconnected(Disconnect::LoginFailed(
                        error.into(),
                    )))
                    .await;
            }
        }
        let mut stop = shutdown.clone();
        while !*stop.borrow() {
            // Weakly: a client dropped on 断开 must still close its connection.
            let watched = client
                .read()
                .await
                .as_ref()
                .map(|connected| (Arc::downgrade(connected), connected.closed()));
            let (watched, closed) = watched.unzip();
            let operation = tokio::select! {
                operation = operations.recv() => match operation {
                    Ok(operation) => operation,
                    Err(_) => break,
                },
                _ = stop.changed() => break,
                // An idle connection that drops says so at once, as a
                // terminal on the same host does, not at the next request.
                _ = async {
                    match closed {
                        Some(closed) => closed.await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(connected) = watched.and_then(|weak| weak.upgrade()) {
                        forget(&client, &connected, &events).await;
                    }
                    continue;
                }
            };
            // 不用连接：断开时移出队列也不该连上去。
            if matches!(operation, SftpCommand::Forget) {
                batch = None;
                unscanned = None;
                busy.store(false, Ordering::Release);
                events.send(EngineEvent::Idle).await?;
                continue;
            }
            // 引擎这就不再留着这一批，不管清理成不成：否则之后的 `Resume` 会把丢弃的那批接着传。
            if matches!(operation, SftpCommand::Discard) {
                unscanned = None;
                self.discard(batch.take(), &link).await?;
                busy.store(false, Ordering::Release);
                events.send(EngineEvent::Idle).await?;
                continue;
            }
            if client.read().await.is_none() {
                match control
                    .run(self.connect(prompts.clone(), &events, &shutdown, &client))
                    .await
                {
                    Ok(connected) => *client.write().await = Some(connected),
                    Err(error) => {
                        events
                            .send(EngineEvent::Disconnected(Disconnect::LoginFailed(
                                error.into(),
                            )))
                            .await?;
                        // 新的一批连扫描都没开始：停着等「继续」，别在队列里一直显示在跑。
                        if let Some(direction) = scan_direction(&operation) {
                            batch = None;
                            unscanned = Some(operation);
                            events
                                .send(EngineEvent::Progress(TransferProgress {
                                    direction,
                                    phase: TransferPhase::Stopped,
                                    ..Default::default()
                                }))
                                .await?;
                        }
                        events.send(EngineEvent::Idle).await?;
                        busy.store(false, Ordering::Release);
                        continue;
                    }
                }
            }
            let operation = match (operation, unscanned.take()) {
                (SftpCommand::Resume, Some(request)) => request,
                (operation, _) => operation,
            };
            let direction = scan_direction(&operation).unwrap_or_else(|| {
                batch
                    .as_ref()
                    .map(|batch| match batch {
                        TransferBatch::Upload(_) => TransferDirection::Upload,
                        TransferBatch::Download(_) => TransferDirection::Download,
                    })
                    .unwrap_or_default()
            });
            let mut reconnects = 0;
            let mut gave_up = false;
            let prepared: Result<()> = loop {
                let connected = client.read().await.clone();
                let attempt = match connected {
                    Some(connected) => {
                        self.prepare(&operation, &mut batch, &connected, &link)
                            .await
                    }
                    None => Err(no_connection(&control)),
                };
                match attempt {
                    // 扫描时断线和传的时候一样自己重连。
                    Err(error)
                        if scan_direction(&operation).is_some() && is_network_error(&error) =>
                    {
                        *client.write().await = None;
                        events
                            .send(EngineEvent::Disconnected(Disconnect::DroppedKept))
                            .await?;
                        if !self
                            .reconnect(&mut reconnects, None, direction, &link)
                            .await?
                        {
                            gave_up = true;
                            break Err(error);
                        }
                    }
                    other => break other,
                }
            };
            if let Err(error) = prepared {
                let cancelled = error.is::<Cancelled>();
                let reason = Reason::from(error);
                // 用户自己停的不是出错；重连放弃了，`Disconnected` 已经说过了。
                if !cancelled && !gave_up {
                    events.send(EngineEvent::Notice(reason.clone())).await?;
                }
                if let Some(batch) = &mut batch {
                    batch.phase(TransferPhase::Stopped, &control);
                } else {
                    if scan_direction(&operation).is_some() {
                        unscanned = Some(operation);
                    }
                    events
                        .send(EngineEvent::Progress(TransferProgress {
                            direction,
                            phase: TransferPhase::Stopped,
                            details: if cancelled {
                                Vec::new()
                            } else {
                                vec![TransferDetail::failed("", reason)]
                            },
                            ..Default::default()
                        }))
                        .await?;
                }
                // Done, as far as whoever sent the batch is concerned.
                events.send(EngineEvent::Idle).await?;
                busy.store(false, Ordering::Release);
                continue;
            }
            if let Some(batch) = &mut batch {
                let mut reconnects = 0;
                loop {
                    let connected = client.read().await.clone();
                    let result = match connected {
                        Some(connected) => batch.run(connected.as_ref(), &control).await,
                        None => Err(no_connection(&control)),
                    };
                    match result {
                        Ok(()) => break,
                        Err(error) if is_network_error(&error) => {
                            *client.write().await = None;
                            events
                                .send(EngineEvent::Disconnected(Disconnect::DroppedKept))
                                .await?;
                            if self
                                .reconnect(&mut reconnects, Some(batch), direction, &link)
                                .await?
                            {
                                continue;
                            }
                            batch.phase(TransferPhase::Stopped, &control);
                            break;
                        }
                        Err(error) => {
                            if !error.is::<Cancelled>() {
                                events.send(EngineEvent::Notice(error.into())).await?;
                            }
                            batch.phase(TransferPhase::Stopped, &control);
                            break;
                        }
                    }
                }
                if batch.is_complete() {
                    batch.phase(TransferPhase::Completed, &control);
                }
            }
            busy.store(false, Ordering::Release);
            events.send(EngineEvent::Idle).await?;
        }
        *client.write().await = None;
        Ok(())
    }

    /// 上传、下载的准备：解析目标、扫描，做出 `batch`；别的命令只核对主机。
    async fn prepare(
        &self,
        operation: &SftpCommand,
        batch: &mut Option<TransferBatch>,
        connected: &SftpClient,
        link: &Link<'_>,
    ) -> Result<()> {
        let (events, control) = (link.events, link.control);
        if let SftpCommand::Upload(request) = operation {
            *batch = None;
            events
                .send(EngineEvent::Progress(TransferProgress::default()))
                .await?;
            let request = if request.is_scp() {
                let destination = request.destination();
                let is_directory = destination.is_root()
                    || control
                        .run(connected.stat(destination))
                        .await?
                        .is_some_and(|metadata| metadata.kind() == EntryKind::Directory);
                let (directory, name) = scp_remote_target(destination, is_directory);
                // Some servers resolve a path that is not there, so
                // ask whether it is a directory too.
                let missing = || {
                    anyhow!(SftpError::RemoteFolderMissing {
                        path: directory.to_string()
                    })
                };
                let resolved = match control.run(connected.canonicalize(&directory)).await {
                    Ok(resolved) => resolved,
                    // 断线和停下不是「目录不存在」。
                    Err(error) if error.is::<Cancelled>() || is_network_error(&error) => {
                        return Err(error);
                    }
                    Err(_) => return Err(missing()),
                };
                if !control
                    .run(connected.stat(&resolved))
                    .await?
                    .is_some_and(|metadata| metadata.kind() == EntryKind::Directory)
                {
                    return Err(missing());
                }
                request.clone().resolved(resolved, name)
            } else {
                let target = control
                    .run(connected.canonicalize(request.destination()))
                    .await?;
                super::UploadRequest::new(request.sources().to_vec(), target)?
            };
            let upload = control
                .run(UploadBatch::scan(
                    &request,
                    &self.endpoint,
                    connected.fingerprint(),
                    self.journal.clone(),
                    control,
                ))
                .await?;
            *batch = Some(TransferBatch::Upload(upload));
        }
        if let SftpCommand::Download(request) = operation {
            *batch = None;
            let request = &if request.is_scp() {
                let destination = request.destination();
                let is_directory = tokio::fs::metadata(destination)
                    .await
                    .is_ok_and(|metadata| metadata.is_dir());
                let (directory, name) = scp_local_target(destination, is_directory);
                if !tokio::fs::metadata(&directory)
                    .await
                    .is_ok_and(|metadata| metadata.is_dir())
                {
                    anyhow::bail!(SftpError::LocalFolderMissing { path: directory });
                }
                request.clone().resolved(directory, name)
            } else {
                request.clone()
            };
            events
                .send(EngineEvent::Progress(TransferProgress {
                    direction: TransferDirection::Download,
                    ..TransferProgress::default()
                }))
                .await?;
            *batch = Some(TransferBatch::Download(
                DownloadBatch::scan(
                    request,
                    &self.endpoint,
                    connected.fingerprint(),
                    self.download_journal.clone(),
                    connected,
                    control,
                )
                .await?,
            ));
        }
        if let Some(batch) = batch {
            batch.verify_host(connected.fingerprint())?;
        }
        Ok(())
    }

    /// 丢弃续传进度。下载的半截在本机，不用连接；上传的在远程，连不上就留着（和移出队列一样，
    /// 下次传同一个文件会问是否续传），告诉用户。答一条空的「结束」。
    async fn discard(&self, batch: Option<TransferBatch>, link: &Link<'_>) -> Result<()> {
        let direction = match &batch {
            Some(TransferBatch::Download(_)) => TransferDirection::Download,
            _ => TransferDirection::Upload,
        };
        let outcome = match &batch {
            None => Ok(()),
            Some(TransferBatch::Download(current)) => current.discard().await,
            Some(TransferBatch::Upload(current)) => match self.connected(link).await? {
                Some(connected) => current.discard(connected.as_ref()).await,
                None => Err(SftpError::DiscardOffline.into()),
            },
        };
        if let Err(error) = outcome {
            link.events.send(EngineEvent::Notice(error.into())).await?;
        }
        link.events
            .send(EngineEvent::Progress(TransferProgress {
                direction,
                phase: TransferPhase::Completed,
                ..Default::default()
            }))
            .await
    }

    /// 现在的连接，没有就连一条；连不上是 `None`，已经说过 `Disconnected` 了。
    async fn connected(&self, link: &Link<'_>) -> Result<Option<Arc<SftpClient>>> {
        if let Some(connected) = link.client.read().await.clone() {
            return Ok(Some(connected));
        }
        match link
            .control
            .run(self.connect(
                link.prompts.clone(),
                link.events,
                link.shutdown,
                link.client,
            ))
            .await
        {
            Ok(connected) => {
                *link.client.write().await = Some(connected.clone());
                Ok(Some(connected))
            }
            Err(error) => {
                link.events
                    .send(EngineEvent::Disconnected(Disconnect::LoginFailed(
                        error.into(),
                    )))
                    .await?;
                Ok(None)
            }
        }
    }

    /// 断线后重连，这一批最多试 3 次（等 1、3、10 秒）。`batch` 是已经扫描好的那批：进度记在它上面，
    /// 新连接要是原来那台主机；还在扫描时没有批次，直接报进度。连上了返回 true；
    /// 放弃了（试完了、用户停了、认证失败）返回 false。
    async fn reconnect(
        &self,
        reconnects: &mut usize,
        mut batch: Option<&mut TransferBatch>,
        direction: TransferDirection,
        link: &Link<'_>,
    ) -> Result<bool> {
        let (events, control) = (link.events, link.control);
        while *reconnects < 3 {
            let note = SftpError::ReconnectIn {
                seconds: RECONNECT_DELAYS[*reconnects],
                attempt: *reconnects + 1,
            };
            match batch.as_deref_mut() {
                Some(batch) => {
                    batch.set_note(note);
                    batch.phase(TransferPhase::Reconnecting, control);
                }
                None => {
                    events
                        .send(EngineEvent::Progress(TransferProgress {
                            direction,
                            phase: TransferPhase::Reconnecting,
                            note: Some(Reason::new(note)),
                            ..Default::default()
                        }))
                        .await?;
                }
            }
            let delay = Duration::from_secs(RECONNECT_DELAYS[*reconnects]);
            *reconnects += 1;
            if control
                .run(async {
                    tokio::time::sleep(delay).await;
                    Ok(())
                })
                .await
                .is_err()
            {
                return Ok(false);
            }
            match control
                .run(self.connect(link.prompts.clone(), events, link.shutdown, link.client))
                .await
            {
                Ok(next) => {
                    let same_host = match batch.as_deref() {
                        Some(batch) => batch.verify_host(next.fingerprint()),
                        None => Ok(()),
                    };
                    return match same_host {
                        Ok(()) => {
                            *link.client.write().await = Some(next);
                            Ok(true)
                        }
                        Err(error) => {
                            events.send(EngineEvent::Notice(error.into())).await?;
                            Ok(false)
                        }
                    };
                }
                Err(error) => {
                    // Authentication/trust failures must require user action.
                    let give_up = !is_network_error(&error);
                    events
                        .send(EngineEvent::Disconnected(Disconnect::Failed(error.into())))
                        .await?;
                    if give_up {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(false)
    }
}

/// 工作循环里到处要用的东西：连接和它的问答、事件、停止。
struct Link<'a> {
    prompts: &'a Arc<SshPrompts>,
    events: &'a EventSink,
    shutdown: &'a watch::Receiver<bool>,
    client: &'a RwLock<Option<Arc<SftpClient>>>,
    control: &'a TransferControl,
}

/// 离开作用域就中止任务。工作循环可能半路返回（没人听事件了）：路由任务要跟着走，
/// 它手里的 `writes` 放下了 `run` 才等得到写回结束。
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// 这条命令要扫描（新的一批）：是哪个方向。
fn scan_direction(operation: &SftpCommand) -> Option<TransferDirection> {
    match operation {
        SftpCommand::Upload(_) => Some(TransferDirection::Upload),
        SftpCommand::Download(_) => Some(TransferDirection::Download),
        _ => None,
    }
}

/// 要用连接的时候没有连接：用户停了，或者网络断了。
fn no_connection(control: &TransferControl) -> anyhow::Error {
    if control.check().is_err() {
        Cancelled.into()
    } else {
        std::io::Error::new(std::io::ErrorKind::NotConnected, SftpError::NoConnection).into()
    }
}

/// Measure the connection in use every [`LATENCY_INTERVAL`], and a new one
/// at once, transfers or not. The client is held only for a ping, and let go
/// as soon as it is no longer the one in use, so 断开 still closes the
/// connection at once.
async fn measure_latency(shared: Arc<RwLock<Option<Arc<SftpClient>>>>, events: EventSink) {
    let mut measured = Weak::new();
    let mut last = Instant::now();
    loop {
        tokio::time::sleep(LATENCY_POLL).await;
        let Some(connected) = shared.read().await.clone() else {
            continue;
        };
        let fresh = !Weak::ptr_eq(&measured, &Arc::downgrade(&connected));
        if !fresh && last.elapsed() < LATENCY_INTERVAL {
            continue;
        }
        measured = Arc::downgrade(&connected);
        last = Instant::now();
        let latency = tokio::select! {
            latency = connected.round_trip() => latency,
            () = replaced(&shared, &connected) => None,
        };
        drop(connected);
        if let Some(latency) = latency
            && events.send(EngineEvent::Latency(latency)).await.is_err()
        {
            break;
        }
    }
}

/// Resolves once `connected` is no longer the client in use.
async fn replaced(shared: &RwLock<Option<Arc<SftpClient>>>, connected: &Arc<SftpClient>) {
    loop {
        tokio::time::sleep(LATENCY_POLL).await;
        let current = shared.read().await;
        if !current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, connected))
        {
            return;
        }
    }
}

/// A request failed: if the connection broke, `forget` it.
async fn drop_broken(
    shared: &RwLock<Option<Arc<SftpClient>>>,
    connected: &Arc<SftpClient>,
    events: &EventSink,
    error: &anyhow::Error,
) {
    if is_network_error(error) {
        forget(shared, connected, events).await;
    }
}

/// The connection is gone: forget that client (unless a newer one already
/// replaced it) and tell the panel once.
async fn forget(
    shared: &RwLock<Option<Arc<SftpClient>>>,
    connected: &Arc<SftpClient>,
    events: &EventSink,
) {
    let mut current = shared.write().await;
    if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, connected)) {
        *current = None;
        let _ = events
            .send(EngineEvent::Disconnected(Disconnect::Dropped))
            .await;
    }
}
